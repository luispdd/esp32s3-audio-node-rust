use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use super::frame::AudioFrame;

#[cfg(target_arch = "xtensa")]
use esp_idf_svc::http::server::{Configuration as HttpServerConfig, EspHttpServer};
#[cfg(target_arch = "xtensa")]
use esp_idf_svc::http::Method;
#[cfg(target_arch = "xtensa")]
use esp_idf_svc::io::{EspIOError, Write};

const INDEX_HTML_TEMPLATE: &str = include_str!("../../web/index.html");

/// Generates a standard 44-byte RIFF/WAVE header.
/// If `data_len` is 0x7fff_ffff or 0xffff_ffff, it indicates an indefinite live stream.
pub fn create_wav_header(
    sample_rate: u32,
    channels: u16,
    bits_per_sample: u16,
    data_len: u32,
) -> [u8; 44] {
    let byte_rate = sample_rate * (channels as u32) * (bits_per_sample as u32 / 8);
    let block_align = channels * (bits_per_sample / 8);
    let riff_chunk_size = if data_len <= u32::MAX - 36 {
        data_len + 36
    } else {
        0x7fff_ffff
    };

    let mut header = [0u8; 44];
    // RIFF chunk descriptor
    header[0..4].copy_from_slice(b"RIFF");
    header[4..8].copy_from_slice(&riff_chunk_size.to_le_bytes());
    header[8..12].copy_from_slice(b"WAVE");

    // fmt subchunk
    header[12..16].copy_from_slice(b"fmt ");
    header[16..20].copy_from_slice(&16u32.to_le_bytes()); // Subchunk1Size = 16 for PCM
    header[20..22].copy_from_slice(&1u16.to_le_bytes());  // AudioFormat = 1 (PCM)
    header[22..24].copy_from_slice(&channels.to_le_bytes());
    header[24..28].copy_from_slice(&sample_rate.to_le_bytes());
    header[28..32].copy_from_slice(&byte_rate.to_le_bytes());
    header[32..34].copy_from_slice(&block_align.to_le_bytes());
    header[34..36].copy_from_slice(&bits_per_sample.to_le_bytes());

    // data subchunk
    header[36..40].copy_from_slice(b"data");
    header[40..44].copy_from_slice(&data_len.to_le_bytes());

    header
}

/// Thread-safe RAII guard that tracks the count of active streaming clients.
pub struct ListenerGuard {
    counter: Arc<AtomicUsize>,
}

impl Drop for ListenerGuard {
    fn drop(&mut self) {
        self.counter.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Thread-safe ring buffer for sharing live audio frames between Core 1 and Core 0.
/// Backed by heap/PSRAM allocations, keeping real-time acquisition decoupled from network I/O.
#[derive(Clone)]
pub struct SharedAudioBuffer {
    inner: Arc<AudioBufferInner>,
}

struct AudioBufferInner {
    state: Mutex<AudioBufferState>,
    available: Condvar,
    listener_count: Arc<AtomicUsize>,
}

struct AudioBufferState {
    seq: u64,
    frames: VecDeque<(u64, AudioFrame)>,
    capacity: usize,
    signal_present: bool,
}

impl SharedAudioBuffer {
    pub fn new(capacity: usize) -> Self {
        let cap = if capacity == 0 { 50 } else { capacity };
        Self {
            inner: Arc::new(AudioBufferInner {
                state: Mutex::new(AudioBufferState {
                    seq: 0,
                    frames: VecDeque::with_capacity(cap),
                    capacity: cap,
                    signal_present: false,
                }),
                available: Condvar::new(),
                listener_count: Arc::new(AtomicUsize::new(0)),
            }),
        }
    }

    /// Pushes a newly captured audio frame into the buffer.
    pub fn push_frame(&self, frame: AudioFrame) {
        let has_energy = frame.samples.iter().any(|&s| s.abs() > 300);
        let mut state = self.inner.state.lock().unwrap();
        state.seq = state.seq.wrapping_add(1);
        state.signal_present = has_energy;

        if state.frames.len() >= state.capacity {
            state.frames.pop_front();
        }
        let seq = state.seq;
        state.frames.push_back((seq, frame));
        self.inner.available.notify_all();
    }

    /// Fetches all audio frames produced after `after_seq`.
    /// Waits up to `timeout` if no newer frames are immediately available.
    pub fn fetch_frames(&self, after_seq: u64, timeout: Duration) -> (Vec<AudioFrame>, u64) {
        let mut state = self.inner.state.lock().unwrap();

        if state.seq <= after_seq {
            let (new_state, _) = self.inner.available.wait_timeout(state, timeout).unwrap();
            state = new_state;
        }

        if state.frames.is_empty() {
            return (Vec::new(), state.seq);
        }

        let newest_seq = state.seq;
        let mut result = Vec::new();

        for &(seq, ref frame) in &state.frames {
            if after_seq == 0 || seq > after_seq {
                result.push(frame.clone());
            }
        }

        (result, newest_seq)
    }

    /// Checks if recent audio frames contained detectable sound energy.
    pub fn is_signal_present(&self) -> bool {
        self.inner.state.lock().unwrap().signal_present
    }

    /// Returns the number of clients currently streaming audio.
    pub fn active_listeners(&self) -> usize {
        self.inner.listener_count.load(Ordering::SeqCst)
    }

    /// Increments the active listener counter and returns an RAII guard to decrement on disconnect.
    pub fn listener_guard(&self) -> ListenerGuard {
        self.inner.listener_count.fetch_add(1, Ordering::SeqCst);
        ListenerGuard {
            counter: self.inner.listener_count.clone(),
        }
    }
}

impl Default for SharedAudioBuffer {
    fn default() -> Self {
        Self::new(50)
    }
}

#[derive(Debug, Clone)]
pub struct LiveAudioStream {
    sample_rate: u32,
    channels: u8,
    frame_size: usize,
    endpoint: String,
    browser_page: String,
    frame_index: usize,
}

impl LiveAudioStream {
    pub fn new() -> Self {
        let endpoint = "/stream.wav".to_string();
        let browser_page = Self::build_browser_page(&endpoint);

        Self {
            sample_rate: 16_000,
            channels: 1,
            frame_size: 320,
            endpoint,
            browser_page,
            frame_index: 0,
        }
    }

    /// Builds a rich, responsive, completely self-contained HTML page for browser playback.
    /// The template is stored independently in `web/index.html` and included at compile time.
    pub fn build_browser_page(endpoint: &str) -> String {
        INDEX_HTML_TEMPLATE.replace("{{ENDPOINT}}", endpoint)
    }

    /// Synthesizes an AudioFrame for simulation or host unit tests.
    pub fn capture_frame(&mut self) -> AudioFrame {
        let samples = (0..self.frame_size)
            .map(|index| {
                let phase = (self.frame_index + index) as f32 / self.sample_rate as f32;
                let tone = (phase * 440.0 * 2.0 * std::f32::consts::PI).sin() * 8000.0;
                let envelope = 0.35 + ((index % 64) as f32 / 64.0) * 0.65;
                (tone * envelope) as i16
            })
            .collect();

        self.frame_index += self.frame_size;

        AudioFrame {
            sample_rate: self.sample_rate,
            channels: self.channels,
            samples,
        }
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    pub fn browser_page(&self) -> &str {
        &self.browser_page
    }

    #[cfg(target_arch = "xtensa")]
    pub fn start_server(
        shared_buffer: SharedAudioBuffer,
        port: u16,
    ) -> Result<EspHttpServer<'static>, String> {
        let config = HttpServerConfig {
            http_port: port,
            max_open_sockets: 4,
            stack_size: 8192,
            ..Default::default()
        };

        let mut server = EspHttpServer::new(&config)
            .map_err(|err| format!("Failed to start EspHttpServer: {err}"))?;

        // 1. Root page handler: serves the embedded, self-contained HTML player page
        let page_html = Self::build_browser_page("/stream.wav");
        server
            .fn_handler("/", Method::Get, move |req| -> Result<(), EspIOError> {
                let headers = [
                    ("Content-Type", "text/html; charset=utf-8"),
                    ("Cache-Control", "no-cache, no-store, must-revalidate"),
                ];
                let mut resp = req.into_response(200, Some("OK"), &headers)?;
                resp.write_all(page_html.as_bytes())?;
                Ok(())
            })
            .map_err(|err| format!("Failed to register root handler: {err}"))?;

        // 2. Status handler: returns device streaming health in JSON
        let status_buffer = shared_buffer.clone();
        server
            .fn_handler("/status", Method::Get, move |req| -> Result<(), EspIOError> {
                let listeners = status_buffer.active_listeners();
                let signal = status_buffer.is_signal_present();
                let body = format!(
                    r#"{{"status":"ok","sample_rate":16000,"channels":1,"format":"pcm16","signal_detected":{},"listeners":{}}}"#,
                    signal, listeners
                );
                let headers = [
                    ("Content-Type", "application/json"),
                    ("Cache-Control", "no-cache"),
                ];
                let mut resp = req.into_response(200, Some("OK"), &headers)?;
                resp.write_all(body.as_bytes())?;
                Ok(())
            })
            .map_err(|err| format!("Failed to register status handler: {err}"))?;

        // 3. Audio stream handler: continuous WAV streaming over HTTP chunked response
        let stream_buffer = shared_buffer.clone();
        let stream_handler = move |req: esp_idf_svc::http::server::Request<&mut esp_idf_svc::http::server::EspHttpConnection<'_>>| -> Result<(), EspIOError> {
            let _guard = stream_buffer.listener_guard();
            log::info!("Live audio client connected to stream");

            let headers = [
                ("Content-Type", "audio/wav"),
                ("Cache-Control", "no-cache, no-store, must-revalidate"),
                ("Pragma", "no-cache"),
                ("Connection", "close"),
            ];

            let mut resp = req.into_response(200, Some("OK"), &headers)?;

            // Send 44-byte WAV header for streaming (0x7fff_ffff indicates streaming data chunk)
            let wav_header = create_wav_header(16_000, 1, 16, 0x7fff_ffff);
            resp.write_all(&wav_header)?;

            let mut last_seq = 0u64;

            loop {
                let (frames, new_seq) = stream_buffer.fetch_frames(last_seq, Duration::from_millis(200));
                last_seq = new_seq;

                for frame in frames {
                    let bytes = frame.to_le_bytes();
                    if let Err(err) = resp.write_all(&bytes) {
                        log::info!("Audio stream client disconnected: {err}");
                        return Ok(());
                    }
                }
            }
        };

        server
            .fn_handler("/stream.wav", Method::Get, stream_handler)
            .map_err(|err| format!("Failed to register /stream.wav handler: {err}"))?;

        log::info!("EspHttpServer live streaming endpoints registered on port {port}: / and /stream.wav");

        Ok(server)
    }

    #[cfg(not(target_arch = "xtensa"))]
    pub fn start_server(
        _shared_buffer: SharedAudioBuffer,
        _port: u16,
    ) -> Result<(), String> {
        log::info!("Mock HTTP server started (non-xtensa architecture)");
        Ok(())
    }
}

impl Default for LiveAudioStream {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(unused_imports)]
mod tests {
    use super::*;

    #[test]
    fn wav_header_generation_format_and_fields() {
        let header = create_wav_header(16_000, 1, 16, 0x7fff_ffff);
        assert_eq!(&header[0..4], b"RIFF");
        assert_eq!(&header[8..12], b"WAVE");
        assert_eq!(&header[12..16], b"fmt ");
        assert_eq!(&header[20..22], &1u16.to_le_bytes()); // PCM
        assert_eq!(&header[22..24], &1u16.to_le_bytes()); // Mono
        assert_eq!(&header[24..28], &16_000u32.to_le_bytes()); // 16kHz
        assert_eq!(&header[28..32], &32_000u32.to_le_bytes()); // Byte rate = 16000 * 2
        assert_eq!(&header[32..34], &2u16.to_le_bytes()); // Block align = 2
        assert_eq!(&header[34..36], &16u16.to_le_bytes()); // 16-bit
        assert_eq!(&header[36..40], b"data");
        assert_eq!(&header[40..44], &0x7fff_ffffu32.to_le_bytes());
    }

    #[test]
    fn shared_audio_buffer_push_and_fetch() {
        let buffer = SharedAudioBuffer::new(10);
        assert_eq!(buffer.active_listeners(), 0);

        let frame1 = AudioFrame::new(16_000, 1, vec![1000; 320]);
        let frame2 = AudioFrame::new(16_000, 1, vec![2000; 320]);

        buffer.push_frame(frame1);
        buffer.push_frame(frame2);

        assert!(buffer.is_signal_present());

        let (frames, seq) = buffer.fetch_frames(0, Duration::from_millis(10));
        assert_eq!(frames.len(), 2);
        assert_eq!(seq, 2);

        // Fetch again with last seq should return empty
        let (frames2, seq2) = buffer.fetch_frames(seq, Duration::from_millis(10));
        assert!(frames2.is_empty());
        assert_eq!(seq2, 2);
    }

    #[test]
    fn shared_audio_buffer_capacity_overrun() {
        let buffer = SharedAudioBuffer::new(3);

        for i in 1..=5 {
            buffer.push_frame(AudioFrame::new(16_000, 1, vec![i as i16; 10]));
        }

        let (frames, seq) = buffer.fetch_frames(0, Duration::from_millis(10));
        assert_eq!(frames.len(), 3); // Capped at 3 frames
        assert_eq!(seq, 5);
        assert_eq!(frames[0].samples[0], 3);
        assert_eq!(frames[2].samples[0], 5);
    }

    #[test]
    fn shared_audio_buffer_listener_tracking() {
        let buffer = SharedAudioBuffer::new(10);
        assert_eq!(buffer.active_listeners(), 0);

        {
            let _guard1 = buffer.listener_guard();
            assert_eq!(buffer.active_listeners(), 1);

            {
                let _guard2 = buffer.listener_guard();
                assert_eq!(buffer.active_listeners(), 2);
            }
            assert_eq!(buffer.active_listeners(), 1);
        }
        assert_eq!(buffer.active_listeners(), 0);
    }

    #[test]
    fn live_audio_stream_generates_browser_ready_frames() {
        let mut stream = LiveAudioStream::new();
        let first_frame = stream.capture_frame();

        assert_eq!(first_frame.sample_rate, 16_000);
        assert_eq!(first_frame.channels, 1);
        assert!(!first_frame.samples.is_empty());
        assert!(stream.browser_page().contains("/stream.wav"));
        assert!(stream.browser_page().contains("audio"));
        assert!(stream.browser_page().contains("Audio Node"));
    }
}
