use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use super::stream::SharedAudioBuffer;
use crate::time::UtcDateTime;

/// Builds a 44-byte RIFF WAV header for standard PCM16 audio.
pub fn create_wav_header(channels: u16, sample_rate: u32, data_len: u32) -> [u8; 44] {
    let mut header = [0u8; 44];
    // RIFF chunk descriptor
    header[0..4].copy_from_slice(b"RIFF");
    let riff_size = data_len.saturating_add(36);
    header[4..8].copy_from_slice(&riff_size.to_le_bytes());
    header[8..12].copy_from_slice(b"WAVE");

    // "fmt " sub-chunk
    header[12..16].copy_from_slice(b"fmt ");
    header[16..20].copy_from_slice(&16u32.to_le_bytes()); // Subchunk1Size = 16 for PCM
    header[20..22].copy_from_slice(&1u16.to_le_bytes()); // AudioFormat = 1 (linear PCM)
    header[22..24].copy_from_slice(&channels.to_le_bytes());
    header[24..28].copy_from_slice(&sample_rate.to_le_bytes());

    let byte_rate = sample_rate * channels as u32 * 2; // sample_rate * channels * bits_per_sample / 8
    header[28..32].copy_from_slice(&byte_rate.to_le_bytes());

    let block_align = channels * 2; // channels * bits_per_sample / 8
    header[32..34].copy_from_slice(&block_align.to_le_bytes());
    header[34..36].copy_from_slice(&16u16.to_le_bytes()); // BitsPerSample = 16

    // "data" sub-chunk
    header[36..40].copy_from_slice(b"data");
    header[40..44].copy_from_slice(&data_len.to_le_bytes());

    header
}

/// Streams PCM audio samples into a standard RIFF WAV container.
pub struct WavWriter<W: Write + Seek> {
    writer: W,
    channels: u16,
    sample_rate: u32,
    total_data_bytes: u32,
}

impl<W: Write + Seek> WavWriter<W> {
    pub fn channels(&self) -> u16 {
        self.channels
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn total_data_bytes(&self) -> u32 {
        self.total_data_bytes
    }

    /// Initializes a new WavWriter, writing the initial 44-byte header placeholder.
    pub fn new(mut inner: W, sample_rate: u32, channels: u16) -> Result<Self, String> {
        let initial_header = create_wav_header(channels, sample_rate, 0);
        inner
            .write_all(&initial_header)
            .map_err(|e| format!("Failed to write initial WAV header: {e}"))?;

        Ok(Self {
            writer: inner,
            channels,
            sample_rate,
            total_data_bytes: 0,
        })
    }

    /// Writes a slice of 16-bit signed PCM audio samples in little-endian byte order.
    pub fn write_samples(&mut self, samples: &[i16]) -> Result<usize, String> {
        if samples.is_empty() {
            return Ok(0);
        }

        let mut byte_buf = Vec::with_capacity(samples.len() * 2);
        for &sample in samples {
            byte_buf.extend_from_slice(&sample.to_le_bytes());
        }

        self.writer
            .write_all(&byte_buf)
            .map_err(|e| format!("Failed to write PCM samples to WAV: {e}"))?;

        let bytes_written = byte_buf.len();
        self.total_data_bytes = self.total_data_bytes.saturating_add(bytes_written as u32);
        Ok(bytes_written)
    }

    /// Finalizes the WAV file by updating the RIFF and data chunk length fields.
    pub fn finish(mut self) -> Result<W, String> {
        // 0. Flush any buffered sample data before seeking
        self.writer
            .flush()
            .map_err(|e| format!("Failed to flush audio samples before header seek: {e}"))?;

        // 1. Update RIFF chunk size at offset 4
        self.writer
            .seek(SeekFrom::Start(4))
            .map_err(|e| format!("Failed to seek to RIFF size offset: {e}"))?;
        let riff_size = self.total_data_bytes.saturating_add(36);
        self.writer
            .write_all(&riff_size.to_le_bytes())
            .map_err(|e| format!("Failed to update RIFF size: {e}"))?;

        // 2. Update data chunk size at offset 40
        self.writer
            .seek(SeekFrom::Start(40))
            .map_err(|e| format!("Failed to seek to data chunk size offset: {e}"))?;
        self.writer
            .write_all(&self.total_data_bytes.to_le_bytes())
            .map_err(|e| format!("Failed to update data chunk size: {e}"))?;

        // 3. Flush buffered bytes
        self.writer
            .flush()
            .map_err(|e| format!("Failed to flush WAV writer: {e}"))?;

        Ok(self.writer)
    }
}

/// Active recording metadata
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveRecordingInfo {
    pub filename: String,
    pub path: String,
    pub duration_secs: u32,
    pub frames_recorded: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordingState {
    Idle,
    Recording(ActiveRecordingInfo),
}

struct RecorderShared {
    state: Mutex<RecordingState>,
    stop_flag: AtomicBool,
    cancel_flag: AtomicBool,
}

/// Controller managing the WAV recording lifecycle.
#[derive(Clone)]
pub struct RecordingController {
    shared: Arc<RecorderShared>,
}

impl RecordingController {
    pub fn new() -> Self {
        Self {
            shared: Arc::new(RecorderShared {
                state: Mutex::new(RecordingState::Idle),
                stop_flag: AtomicBool::new(false),
                cancel_flag: AtomicBool::new(false),
            }),
        }
    }

    /// Check if currently recording.
    pub fn is_recording(&self) -> bool {
        let state = self.shared.state.lock().unwrap();
        matches!(*state, RecordingState::Recording(_))
    }

    /// Get details of current recording if active.
    pub fn current_recording(&self) -> Option<ActiveRecordingInfo> {
        let state = self.shared.state.lock().unwrap();
        match &*state {
            RecordingState::Recording(info) => Some(info.clone()),
            RecordingState::Idle => None,
        }
    }

    /// Request cancellation and deletion of the in-progress recording.
    pub fn cancel(&self) -> Result<(), String> {
        if !self.is_recording() {
            return Err("No active recording to cancel".to_string());
        }
        log::warn!("Cancelling active recording and discarding file");
        self.shared.cancel_flag.store(true, Ordering::SeqCst);

        // Wait up to 500ms for worker thread to finish cleaning up
        let deadline = Instant::now() + Duration::from_millis(500);
        while self.is_recording() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }

        Ok(())
    }

    /// Request normal stop and finalization of the in-progress recording.
    pub fn stop(&self) -> Result<String, String> {
        let info = self
            .current_recording()
            .ok_or_else(|| "No active recording to stop".to_string())?;
        log::info!("Stopping active recording: {}", info.filename);
        self.shared.stop_flag.store(true, Ordering::SeqCst);

        // Wait up to 500ms for worker thread to finalize file and return to Idle
        let deadline = Instant::now() + Duration::from_millis(500);
        while self.is_recording() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }

        Ok(info.filename)
    }

    /// Spawns the background recording task on Core 0 that drains frames from `SharedAudioBuffer`.
    pub fn start(
        &self,
        audio_dir: &str,
        buffer: SharedAudioBuffer,
    ) -> Result<ActiveRecordingInfo, String> {
        if self.is_recording() {
            return Err("A recording is already in progress".to_string());
        }

        // 1. Ensure directory exists
        crate::sd::ensure_audio_folder(audio_dir)?;
        let dir_path = Path::new(audio_dir);

        // 2. Generate filename based on current UTC time (e.g. YYYYMMDD_HHMMSS.wav)
        let now = UtcDateTime::now();
        let filename = now.format_recording_filename();
        let file_path = dir_path.join(&filename);

        log::info!("Starting WAV audio recording to {}", file_path.display());

        let info = ActiveRecordingInfo {
            filename: filename.clone(),
            path: file_path.to_string_lossy().into_owned(),
            duration_secs: 0,
            frames_recorded: 0,
        };

        {
            let mut state = self.shared.state.lock().unwrap();
            *state = RecordingState::Recording(info.clone());
        }

        self.shared.stop_flag.store(false, Ordering::SeqCst);
        self.shared.cancel_flag.store(false, Ordering::SeqCst);

        let shared = self.shared.clone();
        let file_path_clone = file_path.clone();

        // Spawn recording worker on Core 0 with standard internal SRAM stack.
        // Standard WAV writes have very low stack footprint, preventing stack overflow
        // and avoiding MMU/PSRAM bus cache conflicts on Xtensa.
        #[cfg(target_arch = "xtensa")]
        {
            use enumset::enum_set;
            use esp_idf_svc::hal::cpu::Core;
            use esp_idf_svc::hal::task::thread::MallocCap;
            use esp_idf_svc::hal::task::thread::ThreadSpawnConfiguration;

            let thread_config = ThreadSpawnConfiguration {
                name: Some(c"wav_recorder"),
                stack_size: 8 * 1024,
                priority: 5,
                pin_to_core: Some(Core::Core0),
                stack_alloc_caps: enum_set!(
                    MallocCap::Internal | MallocCap::Cap32bit | MallocCap::Cap8bit
                ),
                ..Default::default()
            };
            let _ = thread_config.set();
        }

        let handle = thread::Builder::new()
            .name("wav_recorder".to_string())
            .stack_size(8 * 1024)
            .spawn(move || {
                run_recorder_worker(file_path_clone, buffer, shared);
            });

        match handle {
            Ok(_) => Ok(info),
            Err(e) => {
                let mut state = self.shared.state.lock().unwrap();
                *state = RecordingState::Idle;
                Err(format!("Failed to spawn recording worker: {e}"))
            }
        }
    }
}

fn run_recorder_worker(
    file_path: PathBuf,
    buffer: SharedAudioBuffer,
    shared: Arc<RecorderShared>,
) {
    log::info!("Recording worker started for {:?}", file_path);

    let file = match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&file_path)
    {
        Ok(f) => f,
        Err(e) => {
            log::error!("Failed to create recording file {:?}: {e}", file_path);
            let mut state = shared.state.lock().unwrap();
            *state = RecordingState::Idle;
            return;
        }
    };

    // Buffer writes to optimize SD card sector throughput
    let buffered_file = BufWriter::with_capacity(1024, file);
    let mut writer = match WavWriter::new(buffered_file, 16000, 1) {
        Ok(w) => w,
        Err(e) => {
            log::error!("Failed to initialize WavWriter: {e}");
            let _ = std::fs::remove_file(&file_path);
            let mut state = shared.state.lock().unwrap();
            *state = RecordingState::Idle;
            return;
        }
    };

    let start_instant = Instant::now();
    let mut last_seq: u64 = 0;
    let mut total_frames: u32 = 0;

    loop {
        if shared.cancel_flag.load(Ordering::SeqCst) {
            log::warn!("Recording cancelled! Aborting and deleting {:?}", file_path);
            drop(writer);
            if let Err(e) = std::fs::remove_file(&file_path) {
                log::warn!("Failed to delete cancelled recording {:?}: {e}", file_path);
            } else {
                log::info!("Discarded cancelled recording file {:?}", file_path);
            }
            break;
        }

        let is_stopping = shared.stop_flag.load(Ordering::SeqCst);

        // Fetch captured audio frames from buffer
        let (frames, newest_seq) = buffer.fetch_frames(last_seq, Duration::from_millis(40));
        last_seq = newest_seq;

        for frame in frames {
            if let Err(e) = writer.write_samples(&frame.samples) {
                log::error!("Error writing audio samples to WAV: {e}");
            } else {
                total_frames += 1;
            }
        }

        // Update progress in state
        let elapsed_secs = start_instant.elapsed().as_secs() as u32;
        {
            let mut state = shared.state.lock().unwrap();
            if let RecordingState::Recording(ref mut info) = *state {
                info.duration_secs = elapsed_secs;
                info.frames_recorded = total_frames;
            }
        }

        if is_stopping {
            log::info!(
                "Finalizing WAV recording {:?} ({} frames, {}s)",
                file_path,
                total_frames,
                elapsed_secs
            );
            match writer.finish() {
                Ok(mut buffered) => {
                    let _ = buffered.flush();
                    log::info!("Successfully finalized recording file {:?}", file_path);
                }
                Err(e) => {
                    log::error!("Failed to finalize WAV header: {e}");
                }
            }
            break;
        }
    }

    let mut state = shared.state.lock().unwrap();
    *state = RecordingState::Idle;
    shared.stop_flag.store(false, Ordering::SeqCst);
    shared.cancel_flag.store(false, Ordering::SeqCst);
    log::info!("Recording worker thread finished");
}

#[cfg(test)]
#[allow(unused_imports)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_wav_header_structure() {
        let header = create_wav_header(1, 16000, 32000);
        assert_eq!(&header[0..4], b"RIFF");
        let riff_size = u32::from_le_bytes(header[4..8].try_into().unwrap());
        assert_eq!(riff_size, 32000 + 36);
        assert_eq!(&header[8..12], b"WAVE");
        assert_eq!(&header[12..16], b"fmt ");
        assert_eq!(&header[16..20], &16u32.to_le_bytes()); // Subchunk1Size
        assert_eq!(&header[20..22], &1u16.to_le_bytes()); // AudioFormat (PCM)
        assert_eq!(&header[22..24], &1u16.to_le_bytes()); // Channels = 1
        assert_eq!(&header[24..28], &16000u32.to_le_bytes()); // Sample rate
        assert_eq!(&header[28..32], &32000u32.to_le_bytes()); // Byte rate = 16000 * 1 * 2
        assert_eq!(&header[32..34], &2u16.to_le_bytes()); // Block align = 2
        assert_eq!(&header[34..36], &16u16.to_le_bytes()); // Bits per sample = 16
        assert_eq!(&header[36..40], b"data");
        assert_eq!(&header[40..44], &32000u32.to_le_bytes()); // Data chunk size
    }

    #[test]
    fn test_wav_writer_full_cycle() {
        let buffer = Cursor::new(Vec::new());
        let mut writer = WavWriter::new(buffer, 16000, 1).unwrap();

        // 1 second of 440 Hz test tone = 16,000 samples = 32,000 bytes
        let mut tone = Vec::with_capacity(16000);
        for i in 0..16000 {
            let t = i as f32 / 16000.0;
            let sample = ((t * 440.0 * 2.0 * std::f32::consts::PI).sin() * 10000.0) as i16;
            tone.push(sample);
        }

        let written = writer.write_samples(&tone).unwrap();
        assert_eq!(written, 32000);
        assert_eq!(writer.total_data_bytes(), 32000);

        let finished_cursor = writer.finish().unwrap();
        let bytes = finished_cursor.into_inner();

        // 44-byte header + 32,000 bytes PCM = 32,044 bytes total
        assert_eq!(bytes.len(), 44 + 32000);

        // Verify header chunk sizes were properly updated on finish
        assert_eq!(&bytes[0..4], b"RIFF");
        let riff_size = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
        assert_eq!(riff_size, 32000 + 36);

        assert_eq!(&bytes[36..40], b"data");
        let data_size = u32::from_le_bytes(bytes[40..44].try_into().unwrap());
        assert_eq!(data_size, 32000);
    }

    #[test]
    fn test_recording_controller_state_transitions() {
        let ctrl = RecordingController::new();
        assert!(!ctrl.is_recording());
        assert!(ctrl.current_recording().is_none());

        // Stop or cancel when not recording returns error
        assert!(ctrl.stop().is_err());
        assert!(ctrl.cancel().is_err());
    }
}
