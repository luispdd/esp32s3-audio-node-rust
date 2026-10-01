use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use super::frame::AudioFrame;
use super::stream::SharedAudioBuffer;

/// Information about an ongoing on-device audio playback session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivePlaybackInfo {
    pub filename: String,
    pub duration_secs: u32,
    pub total_secs: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaybackState {
    Idle,
    Playing(ActivePlaybackInfo),
}

struct PlaybackShared {
    state: Mutex<PlaybackState>,
    stop_flag: AtomicBool,
}

/// Parses the 44-byte WAV header and returns (sample_rate, channels, data_len_bytes).
pub fn parse_wav_header(header: &[u8; 44]) -> Result<(u32, u16, u32), String> {
    if &header[0..4] != b"RIFF" || &header[8..12] != b"WAVE" {
        return Err("Invalid WAV container: missing RIFF/WAVE signature".to_string());
    }
    if &header[12..16] != b"fmt " {
        return Err("Invalid WAV container: missing fmt subchunk".to_string());
    }
    let format = u16::from_le_bytes([header[20], header[21]]);
    if format != 1 {
        return Err(format!("Unsupported audio format: {format} (expected 1 for linear PCM)"));
    }
    let channels = u16::from_le_bytes([header[22], header[23]]);
    let sample_rate = u32::from_le_bytes([header[24], header[25], header[26], header[27]]);
    let data_len = u32::from_le_bytes([header[40], header[41], header[42], header[43]]);

    Ok((sample_rate, channels, data_len))
}

/// Controller managing on-device audio playback of recorded WAV files.
#[derive(Clone)]
pub struct PlaybackController {
    shared: Arc<PlaybackShared>,
}

impl PlaybackController {
    pub fn new() -> Self {
        Self {
            shared: Arc::new(PlaybackShared {
                state: Mutex::new(PlaybackState::Idle),
                stop_flag: AtomicBool::new(false),
            }),
        }
    }

    /// Check if playback is currently active.
    pub fn is_playing(&self) -> bool {
        let state = self.shared.state.lock().unwrap();
        matches!(*state, PlaybackState::Playing(_))
    }

    /// Return details of the current playback session if active.
    pub fn current_playback(&self) -> Option<ActivePlaybackInfo> {
        let state = self.shared.state.lock().unwrap();
        match &*state {
            PlaybackState::Playing(info) => Some(info.clone()),
            PlaybackState::Idle => None,
        }
    }

    /// Stop any in-progress playback session.
    pub fn stop(&self) -> Result<(), String> {
        if !self.is_playing() {
            return Ok(());
        }
        log::info!("Requesting playback stop");
        self.shared.stop_flag.store(true, Ordering::SeqCst);

        // Wait briefly for worker thread to exit
        for _ in 0..20 {
            if !self.is_playing() {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }

        let mut state = self.shared.state.lock().unwrap();
        *state = PlaybackState::Idle;
        self.shared.stop_flag.store(false, Ordering::SeqCst);
        Ok(())
    }

    /// Starts playback of the given WAV file from the audio directory.
    /// Reads PCM samples and pushes them into `buffer` so that live streams and visualizers reflect the playback.
    pub fn start(
        &self,
        audio_dir: &str,
        filename: &str,
        buffer: SharedAudioBuffer,
    ) -> Result<ActivePlaybackInfo, String> {
        if self.is_playing() {
            let _ = self.stop();
        }

        let file_path = Path::new(audio_dir).join(filename);
        if !file_path.exists() || !file_path.is_file() {
            return Err(format!("File does not exist: {}", file_path.display()));
        }

        let file_size = std::fs::metadata(&file_path)
            .map(|m| m.len())
            .unwrap_or(0);

        let mut file = File::open(&file_path)
            .map_err(|e| format!("Failed to open {filename}: {e}"))?;

        let mut header = [0u8; 44];
        if file.read_exact(&mut header).is_err() {
            return Err("Failed to read 44-byte WAV header".to_string());
        }

        let (sample_rate, channels, mut data_len) = parse_wav_header(&header)?;
        if data_len == 0 || data_len as u64 > file_size {
            data_len = file_size.saturating_sub(44) as u32;
        }

        let bytes_per_sec = sample_rate * (channels as u32) * 2;
        let total_secs = if bytes_per_sec > 0 {
            data_len / bytes_per_sec
        } else {
            0
        };

        let info = ActivePlaybackInfo {
            filename: filename.to_string(),
            duration_secs: 0,
            total_secs,
        };

        {
            let mut state = self.shared.state.lock().unwrap();
            *state = PlaybackState::Playing(info.clone());
        }
        self.shared.stop_flag.store(false, Ordering::SeqCst);

        let shared_clone = self.shared.clone();
        let fname_clone = filename.to_string();

        let worker = move || {
            log::info!("Audio playback worker started for {fname_clone} ({total_secs}s)");

            // Buffer reads using 4096 bytes to align with SPI DMA and minimize SD bus traffic
            let mut reader = BufReader::with_capacity(4096, file);

            // Buffer for 20ms of 16 kHz 16-bit mono PCM: 320 samples * 2 bytes = 640 bytes
            let frame_sample_count = 320usize;
            let frame_byte_count = frame_sample_count * 2;
            let mut raw_buf = vec![0u8; frame_byte_count];
            let mut bytes_played: u64 = 0;

            let frame_duration = Duration::from_millis(20);
            let mut next_tick = Instant::now();

            loop {
                if shared_clone.stop_flag.load(Ordering::SeqCst) {
                    log::info!("Playback stop flag received");
                    break;
                }

                let bytes_read = match reader.read(&mut raw_buf) {
                    Ok(0) => {
                        log::info!("Playback reached end of file: {fname_clone}");
                        break;
                    }
                    Ok(n) => n,
                    Err(e) => {
                        log::warn!("Playback read error: {e}");
                        break;
                    }
                };

                let sample_count = bytes_read / 2;
                let mut samples = Vec::with_capacity(sample_count);
                for chunk in raw_buf[..sample_count * 2].chunks_exact(2) {
                    samples.push(i16::from_le_bytes([chunk[0], chunk[1]]));
                }

                // Push to live audio buffer so streaming listeners and visualizers receive the playback
                let frame = AudioFrame::new(16000, 1, samples);
                buffer.push_frame(frame);

                bytes_played = bytes_played.saturating_add(bytes_read as u64);
                let cur_secs = if bytes_per_sec > 0 {
                    (bytes_played / bytes_per_sec as u64) as u32
                } else {
                    0
                };

                // Update progress in state
                {
                    let mut state = shared_clone.state.lock().unwrap();
                    if let PlaybackState::Playing(ref mut inf) = *state {
                        inf.duration_secs = cur_secs;
                    }
                }

                next_tick += frame_duration;
                let now = Instant::now();
                if next_tick > now {
                    thread::sleep(next_tick - now);
                } else {
                    next_tick = now;
                }
            }

            {
                let mut state = shared_clone.state.lock().unwrap();
                *state = PlaybackState::Idle;
            }
            shared_clone.stop_flag.store(false, Ordering::SeqCst);
            log::info!("Audio playback worker finished for {fname_clone}");
        };

        #[cfg(target_arch = "xtensa")]
        {
            esp_idf_svc::hal::task::thread::ThreadSpawnConfiguration {
                name: Some(c"audio_playback"),
                stack_size: 8192,
                priority: 5,
                pin_to_core: Some(esp_idf_svc::hal::cpu::Core::Core0),
                ..Default::default()
            }
            .set()
            .map_err(|e| format!("Failed to configure playback thread: {e}"))?;

            thread::Builder::new()
                .name("audio_playback".to_string())
                .spawn(worker)
                .map_err(|e| format!("Failed to spawn playback thread: {e}"))?;
        }

        #[cfg(not(target_arch = "xtensa"))]
        {
            thread::Builder::new()
                .name("audio_playback".to_string())
                .spawn(worker)
                .map_err(|e| format!("Failed to spawn playback thread: {e}"))?;
        }

        Ok(info)
    }
}

impl Default for PlaybackController {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(unused_imports)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_wav_header() {
        let header = crate::audio::create_wav_header(1, 16000, 32000);
        let parsed = parse_wav_header(&header);
        assert!(parsed.is_ok());
        let (rate, channels, data_len) = parsed.unwrap();
        assert_eq!(rate, 16000);
        assert_eq!(channels, 1);
        assert_eq!(data_len, 32000);
    }

    #[test]
    fn parse_invalid_wav_header() {
        let mut header = [0u8; 44];
        header[0..4].copy_from_slice(b"NOPE");
        assert!(parse_wav_header(&header).is_err());
    }

    #[test]
    fn playback_controller_lifecycle() {
        let ctrl = PlaybackController::new();
        assert!(!ctrl.is_playing());
        assert_eq!(ctrl.current_playback(), None);
        assert!(ctrl.stop().is_ok());
    }
}
