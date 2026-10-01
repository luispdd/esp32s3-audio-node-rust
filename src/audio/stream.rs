use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, AtomicUsize, Ordering};
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

/// Parses gain override parameters from either a URI query string or a JSON body string.
/// Returns (override_active, value_percent_0_to_100).
pub fn parse_gain_override_params(uri: &str, body: &str) -> (Option<bool>, Option<u8>) {
    let mut override_opt = None;
    let mut value_opt = None;

    // 1. Check URI query parameters (e.g. ?override=true&value=75)
    if let Some(query_idx) = uri.find('?') {
        let query = &uri[query_idx + 1..];
        for param in query.split('&') {
            if let Some((k, v)) = param.split_once('=') {
                if k.eq_ignore_ascii_case("override") {
                    override_opt = Some(v.eq_ignore_ascii_case("true") || v == "1");
                } else if k.eq_ignore_ascii_case("value") {
                    if let Ok(val) = v.parse::<u8>() {
                        value_opt = Some(val.clamp(0, 100));
                    }
                }
            }
        }
    }

    // 2. Fall back to body parameters if not present in query string
    if override_opt.is_none() {
        if body.contains("\"override\":true") || body.contains("\"override\": true") {
            override_opt = Some(true);
        } else if body.contains("\"override\":false") || body.contains("\"override\": false") {
            override_opt = Some(false);
        }
    }

    if value_opt.is_none() {
        if let Some(idx) = body.find("\"value\":") {
            let after = &body[idx + 8..];
            let trimmed = after.trim_start();
            let end = trimmed
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(trimmed.len());
            if let Ok(val) = trimmed[..end].parse::<u8>() {
                value_opt = Some(val.clamp(0, 100));
            }
        }
    }

    (override_opt, value_opt)
}

/// Parses the noise threshold parameter from either a URI query string or a JSON body string.
/// Returns Option<threshold_percent_0_to_100>.
pub fn parse_threshold_param(uri: &str, body: &str) -> Option<u8> {
    if let Some(query_idx) = uri.find('?') {
        let query = &uri[query_idx + 1..];
        for param in query.split('&') {
            if let Some((k, v)) = param.split_once('=') {
                if k.eq_ignore_ascii_case("value") || k.eq_ignore_ascii_case("threshold") {
                    if let Ok(val) = v.parse::<u8>() {
                        return Some(val.clamp(0, 100));
                    }
                }
            }
        }
    }

    for key in &["\"value\":", "\"threshold\":"] {
        if let Some(idx) = body.find(key) {
            let after = &body[idx + key.len()..];
            let trimmed = after.trim_start();
            let end = trimmed
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(trimmed.len());
            if let Ok(val) = trimmed[..end].parse::<u8>() {
                return Some(val.clamp(0, 100));
            }
        }
    }

    None
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
    gain_bits: AtomicU32,
    gain_percent: AtomicU32,
    /// When true, the web-override gain is used instead of the physical potentiometer.
    gain_override_active: AtomicBool,
    /// The web-override gain multiplier stored as f32 bits.
    gain_override_bits: AtomicU32,
    /// The web-override gain as a percentage (0..=100).
    gain_override_percent: AtomicU32,
    /// Active device mode: 0: STATUS_MODE, 1: LIVE_MODE, 2: SD_MODE, 3: PIR_MODE
    device_mode: AtomicU8,
    /// Operational PIR state: 0: idle, 1: arming, 2: armed
    pir_state: AtomicU8,
    /// Arming countdown remaining seconds (0 if None, else 1..=10)
    pir_arming_countdown: AtomicU8,
    /// Live digital PIR motion sensor detection flag
    pir_motion_detected: AtomicBool,
    /// PIR recording remaining seconds (0 if None, else 1..=20)
    pir_recording_remaining_secs: AtomicU8,
    /// Latest normalized noise level percentage (0..=100)
    noise_level: AtomicU8,
    /// Pending command from web interface:
    /// 0: None, 1: Start PIR_MODE (switch mode to PIR_MODE and start 10s arming delay), 2: Stop PIR_MODE (stop/save active recording if any, disarm, exit to STATUS_MODE)
    pir_command: AtomicU8,
}

struct AudioBufferState {
    seq: u64,
    frames: VecDeque<(u64, AudioFrame)>,
    capacity: usize,
    signal_present: bool,
}

/// Snapshot summary of device mode and PIR operational state for status reporting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PirStatusSnapshot {
    pub mode: &'static str,
    pub state: &'static str,
    pub armed: bool,
    pub arming_countdown: Option<u8>,
    pub motion_detected: bool,
    pub recording_remaining_secs: Option<u8>,
    pub noise_level: u8,
    pub sound_detected: bool,
}

/// Default buffer capacity in frames (150 frames * 20ms = 3,000ms = 3 seconds).
/// Absorbs MicroSD cluster allocation latency and SPI bus contention without audio drops.
pub const DEFAULT_BUFFER_CAPACITY: usize = 150;

impl SharedAudioBuffer {
    pub fn new(capacity: usize) -> Self {
        let cap = if capacity == 0 { DEFAULT_BUFFER_CAPACITY } else { capacity };
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
                gain_bits: AtomicU32::new(1.0_f32.to_bits()),
                gain_percent: AtomicU32::new(25),
                gain_override_active: AtomicBool::new(false),
                gain_override_bits: AtomicU32::new(1.0_f32.to_bits()),
                gain_override_percent: AtomicU32::new(25),
                device_mode: AtomicU8::new(0),
                pir_state: AtomicU8::new(0),
                pir_arming_countdown: AtomicU8::new(0),
                pir_motion_detected: AtomicBool::new(false),
                pir_recording_remaining_secs: AtomicU8::new(0),
                noise_level: AtomicU8::new(0),
                pir_command: AtomicU8::new(0),
            }),
        }
    }

    /// Returns the latest monotonic sequence ID produced in the buffer.
    pub fn current_seq(&self) -> u64 {
        self.inner.state.lock().unwrap().seq
    }

    /// Returns the capacity of the buffer in frames.
    pub fn capacity(&self) -> usize {
        self.inner.state.lock().unwrap().capacity
    }

    /// Returns the current number of frames stored in the ring buffer.
    pub fn len(&self) -> usize {
        self.inner.state.lock().unwrap().frames.len()
    }

    /// Checks if the buffer contains no frames.
    pub fn is_empty(&self) -> bool {
        self.inner.state.lock().unwrap().frames.is_empty()
    }

    /// Pushes a newly captured audio frame into the buffer.
    pub fn push_frame(&self, frame: AudioFrame) {
        let has_energy = frame.samples.iter().any(|&s| s.abs() > 300);
        let frame_noise = frame.noise_level_percent();
        let prev_noise = self.inner.noise_level.load(Ordering::Relaxed);
        let smoothed_noise = if frame_noise >= prev_noise {
            frame_noise
        } else {
            // Smooth decay: 7/8 previous + 1/8 current
            ((prev_noise as u16 * 7 + frame_noise as u16) / 8) as u8
        };
        self.inner.noise_level.store(smoothed_noise, Ordering::Relaxed);

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

    /// Updates the measured gain level from the hardware potentiometer.
    pub fn set_gain(&self, gain: f32, percent: u8) {
        self.inner.gain_bits.store(gain.to_bits(), Ordering::Relaxed);
        self.inner.gain_percent.store(percent as u32, Ordering::Relaxed);
    }

    /// Returns the active software gain multiplier (override if active, else potentiometer).
    pub fn current_gain(&self) -> f32 {
        if self.is_gain_override_active() {
            self.gain_override()
        } else {
            f32::from_bits(self.inner.gain_bits.load(Ordering::Relaxed))
        }
    }

    /// Returns the active software gain percentage 0..=100 (override if active, else potentiometer).
    pub fn current_gain_percent(&self) -> u8 {
        if self.is_gain_override_active() {
            self.gain_override_percent()
        } else {
            self.inner.gain_percent.load(Ordering::Relaxed) as u8
        }
    }

    /// Returns the physical potentiometer gain multiplier regardless of override state.
    pub fn potentiometer_gain(&self) -> f32 {
        f32::from_bits(self.inner.gain_bits.load(Ordering::Relaxed))
    }

    /// Returns the physical potentiometer gain percentage regardless of override state.
    pub fn potentiometer_gain_percent(&self) -> u8 {
        self.inner.gain_percent.load(Ordering::Relaxed) as u8
    }

    /// Activates or deactivates the web-based gain override.
    /// When active, the audio capture loop uses `gain` and `percent` instead of the potentiometer.
    pub fn set_gain_override(&self, active: bool, gain: f32, percent: u8) {
        self.inner.gain_override_active.store(active, Ordering::Relaxed);
        self.inner.gain_override_bits.store(gain.to_bits(), Ordering::Relaxed);
        self.inner.gain_override_percent.store(percent as u32, Ordering::Relaxed);
    }

    /// Returns true if the web-based gain override is currently active.
    pub fn is_gain_override_active(&self) -> bool {
        self.inner.gain_override_active.load(Ordering::Relaxed)
    }

    /// Returns the web-override gain multiplier.
    pub fn gain_override(&self) -> f32 {
        f32::from_bits(self.inner.gain_override_bits.load(Ordering::Relaxed))
    }

    /// Returns the web-override gain as a percentage (0..=100).
    pub fn gain_override_percent(&self) -> u8 {
        self.inner.gain_override_percent.load(Ordering::Relaxed) as u8
    }

    /// Returns the current smoothed microphone noise level percentage (0..=100).
    pub fn current_noise_level(&self) -> u8 {
        self.inner.noise_level.load(Ordering::Relaxed)
    }

    /// Evaluates if the current noise level meets or exceeds the given threshold percentage.
    pub fn is_noise_above(&self, threshold_percent: u8) -> bool {
        self.current_noise_level() >= threshold_percent
    }

    /// Requests entering PIR_MODE and initiating the 10-second arming delay from web interface.
    pub fn request_pir_start(&self) {
        self.inner.pir_command.store(1, Ordering::SeqCst);
    }

    /// Requests stopping PIR_MODE (saving active recording if any, disarming, and exiting) from web interface.
    pub fn request_pir_stop(&self) {
        self.inner.pir_command.store(2, Ordering::SeqCst);
    }

    /// Retrieves and clears any pending PIR web command.
    pub fn take_pir_command(&self) -> u8 {
        self.inner.pir_command.swap(0, Ordering::SeqCst)
    }

    /// Updates the atomic PIR and device mode status for status reporting and web endpoints.
    pub fn update_pir_status(
        &self,
        mode: crate::modes::DeviceMode,
        is_armed: bool,
        arming_countdown: Option<u8>,
        motion_detected: bool,
        recording_remaining_secs: Option<u8>,
    ) {
        let mode_val = match mode {
            crate::modes::DeviceMode::Status => 0,
            crate::modes::DeviceMode::Live => 1,
            crate::modes::DeviceMode::Sd => 2,
            crate::modes::DeviceMode::Pir => 3,
        };
        let state_val = if is_armed {
            2 // armed
        } else if arming_countdown.is_some() {
            1 // arming
        } else {
            0 // idle
        };
        self.inner.device_mode.store(mode_val, Ordering::Relaxed);
        self.inner.pir_state.store(state_val, Ordering::Relaxed);
        self.inner.pir_arming_countdown.store(arming_countdown.unwrap_or(0), Ordering::Relaxed);
        self.inner.pir_motion_detected.store(motion_detected, Ordering::Relaxed);
        self.inner.pir_recording_remaining_secs.store(recording_remaining_secs.unwrap_or(0), Ordering::Relaxed);
    }

    /// Returns a summary snapshot of the current PIR and device mode status.
    pub fn pir_status_summary(&self) -> PirStatusSnapshot {
        let mode_str = match self.inner.device_mode.load(Ordering::Relaxed) {
            1 => "LIVE_MODE",
            2 => "SD_MODE",
            3 => "PIR_MODE",
            _ => "STATUS_MODE",
        };
        let (state_str, armed) = match self.inner.pir_state.load(Ordering::Relaxed) {
            1 => ("arming", false),
            2 => ("armed", true),
            _ => ("idle", false),
        };
        let arming_cd = self.inner.pir_arming_countdown.load(Ordering::Relaxed);
        let motion = self.inner.pir_motion_detected.load(Ordering::Relaxed);
        let rec_rem = self.inner.pir_recording_remaining_secs.load(Ordering::Relaxed);
        let noise_lvl = self.inner.noise_level.load(Ordering::Relaxed);
        let snd_det = noise_lvl >= crate::audio::get_noise_threshold();

        PirStatusSnapshot {
            mode: mode_str,
            state: state_str,
            armed,
            arming_countdown: if arming_cd > 0 { Some(arming_cd) } else { None },
            motion_detected: motion,
            recording_remaining_secs: if rec_rem > 0 { Some(rec_rem) } else { None },
            noise_level: noise_lvl,
            sound_detected: snd_det,
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
        recording_controller: crate::audio::RecordingController,
        playback_controller: crate::audio::PlaybackController,
        port: u16,
    ) -> Result<ServerHandle, String> {
        let stream_port = 8080;

        // ── 1. Main HTTP Server (Port 80) ───────────────────────────────────
        // Serves HTML page, /status, and /gain control endpoints.
        // Never executes any blocking loops, remaining 100% responsive.
        let main_config = HttpServerConfig {
            http_port: port,
            ctrl_port: 32768,
            max_open_sockets: 4,
            stack_size: 8192,
            uri_match_wildcard: true,
            ..Default::default()
        };

        let mut main_server = EspHttpServer::new(&main_config)
            .map_err(|err| format!("Failed to start main EspHttpServer on port {port}: {err}"))?;

        let page_html = Self::build_browser_page("/stream.wav");
        let root_html = page_html.clone();
        main_server
            .fn_handler("/", Method::Get, move |req| -> Result<(), EspIOError> {
                let headers = [
                    ("Content-Type", "text/html; charset=utf-8"),
                    ("Cache-Control", "no-cache, no-store, must-revalidate"),
                ];
                let mut resp = req.into_response(200, Some("OK"), &headers)?;
                resp.write_all(root_html.as_bytes())?;
                Ok(())
            })
            .map_err(|err| format!("Failed to register root handler: {err}"))?;

        let index_html = page_html;
        main_server
            .fn_handler("/index.html*", Method::Get, move |req| -> Result<(), EspIOError> {
                let headers = [
                    ("Content-Type", "text/html; charset=utf-8"),
                    ("Cache-Control", "no-cache, no-store, must-revalidate"),
                ];
                let mut resp = req.into_response(200, Some("OK"), &headers)?;
                resp.write_all(index_html.as_bytes())?;
                Ok(())
            })
            .map_err(|err| format!("Failed to register /index.html handler: {err}"))?;

        let status_buffer = shared_buffer.clone();
        let status_recorder = recording_controller.clone();
        let status_playback = playback_controller.clone();
        main_server
            .fn_handler("/status*", Method::Get, move |req| -> Result<(), EspIOError> {
                let listeners = status_buffer.active_listeners();
                let signal = status_buffer.is_signal_present();
                let gain_percent = status_buffer.current_gain_percent();
                let override_active = status_buffer.is_gain_override_active();
                let override_percent = status_buffer.gain_override_percent();
                let pir = status_buffer.pir_status_summary();
                let pir_arming_cd_str = match pir.arming_countdown {
                    Some(cd) => cd.to_string(),
                    None => "null".to_string(),
                };
                let pir_rec_rem_str = match pir.recording_remaining_secs {
                    Some(rem) => rem.to_string(),
                    None => "null".to_string(),
                };
                let rec_info = status_recorder.current_recording();
                let (is_rec, rec_fn, rec_dur, rec_frames) = match rec_info {
                    Some(info) => (true, info.filename, info.duration_secs, info.frames_recorded),
                    None => (false, String::new(), 0, 0),
                };
                let pb_info = status_playback.current_playback();
                let (is_play, pb_fn, pb_dur, pb_total) = match pb_info {
                    Some(info) => (true, info.filename, info.duration_secs, info.total_secs),
                    None => (false, String::new(), 0, 0),
                };
                let noise_level = status_buffer.current_noise_level();
                let noise_threshold = crate::audio::get_noise_threshold();
                let sound_detected = status_buffer.is_noise_above(noise_threshold);
                let body = format!(
                    r#"{{"status":"ok","sample_rate":16000,"channels":1,"format":"pcm16","signal_detected":{},"listeners":{},"gain_percent":{},"gain_override_active":{},"gain_override_percent":{},"noise_level":{},"noise_threshold":{},"sound_detected":{},"device_mode":"{}","pir_state":"{}","pir_armed":{},"pir_arming_countdown":{},"pir_motion_detected":{},"pir_recording_remaining_secs":{},"recording":{},"recording_filename":"{}","recording_duration":{},"recording_frames":{},"playback":{},"playback_filename":"{}","playback_duration":{},"playback_total":{}}}"#,
                    signal, listeners, gain_percent, override_active, override_percent,
                    noise_level, noise_threshold, sound_detected,
                    pir.mode, pir.state, pir.armed, pir_arming_cd_str, pir.motion_detected, pir_rec_rem_str,
                    is_rec, rec_fn, rec_dur, rec_frames, is_play, pb_fn, pb_dur, pb_total
                );
                let headers = [
                    ("Content-Type", "application/json"),
                    ("Cache-Control", "no-cache"),
                    ("Access-Control-Allow-Origin", "*"),
                ];
                let mut resp = req.into_response(200, Some("OK"), &headers)?;
                resp.write_all(body.as_bytes())?;
                Ok(())
            })
            .map_err(|err| format!("Failed to register status handler: {err}"))?;

        let gain_post_buffer = shared_buffer.clone();
        main_server
            .fn_handler("/gain*", Method::Post, move |mut req| -> Result<(), EspIOError> {
                let uri = req.uri();
                let (mut override_opt, mut value_opt) = parse_gain_override_params(uri, "");

                if override_opt.is_none() || value_opt.is_none() {
                    let content_len = req
                        .header("Content-Length")
                        .and_then(|v| v.parse::<usize>().ok())
                        .unwrap_or(0);

                    if content_len > 0 {
                        let mut body_buf = [0u8; 128];
                        let to_read = content_len.min(body_buf.len());
                        let mut read_bytes = 0;
                        while read_bytes < to_read {
                            match req.read(&mut body_buf[read_bytes..to_read]) {
                                Ok(0) => break,
                                Ok(n) => read_bytes += n,
                                Err(err) => {
                                    log::warn!("Error reading /gain body: {:?}", err);
                                    break;
                                }
                            }
                        }
                        let body_str = core::str::from_utf8(&body_buf[..read_bytes]).unwrap_or("");
                        let (body_override, body_value) = parse_gain_override_params("", body_str);
                        if override_opt.is_none() {
                            override_opt = body_override;
                        }
                        if value_opt.is_none() {
                            value_opt = body_value;
                        }
                    }
                }

                let override_active = override_opt.unwrap_or(false);
                let value_percent = value_opt.unwrap_or_else(|| gain_post_buffer.gain_override_percent());

                let gain_multiplier = (value_percent as f32 / 100.0) * crate::potentiometer::DEFAULT_MAX_GAIN;
                gain_post_buffer.set_gain_override(override_active, gain_multiplier, value_percent);

                log::info!(
                    "Web gain override updated (POST): active={}, value={}%, multiplier={:.2}x",
                    override_active, value_percent, gain_multiplier
                );

                let resp_body = format!(
                    r#"{{"ok":true,"gain_override_active":{},"gain_override_percent":{}}}"#,
                    override_active, value_percent
                );
                let headers = [
                    ("Content-Type", "application/json"),
                    ("Cache-Control", "no-cache, no-store, must-revalidate"),
                    ("Access-Control-Allow-Origin", "*"),
                    ("Access-Control-Allow-Methods", "POST, GET, OPTIONS"),
                    ("Access-Control-Allow-Headers", "*"),
                ];
                let mut resp = req.into_response(200, Some("OK"), &headers)?;
                resp.write_all(resp_body.as_bytes())?;
                Ok(())
            })
            .map_err(|err| format!("Failed to register /gain POST handler: {err}"))?;

        let gain_get_buffer = shared_buffer.clone();
        main_server
            .fn_handler("/gain*", Method::Get, move |req| -> Result<(), EspIOError> {
                let (override_opt, value_opt) = parse_gain_override_params(req.uri(), "");
                if let Some(active) = override_opt {
                    let value_percent = value_opt.unwrap_or_else(|| gain_get_buffer.gain_override_percent());
                    let gain_multiplier = (value_percent as f32 / 100.0) * crate::potentiometer::DEFAULT_MAX_GAIN;
                    gain_get_buffer.set_gain_override(active, gain_multiplier, value_percent);
                }
                let override_active = gain_get_buffer.is_gain_override_active();
                let value_percent = gain_get_buffer.gain_override_percent();

                log::info!(
                    "Web gain override updated (GET): active={}, value={}%",
                    override_active, value_percent
                );

                let resp_body = format!(
                    r#"{{"ok":true,"gain_override_active":{},"gain_override_percent":{}}}"#,
                    override_active, value_percent
                );
                let headers = [
                    ("Content-Type", "application/json"),
                    ("Cache-Control", "no-cache, no-store, must-revalidate"),
                    ("Access-Control-Allow-Origin", "*"),
                    ("Access-Control-Allow-Methods", "POST, GET, OPTIONS"),
                    ("Access-Control-Allow-Headers", "*"),
                ];
                let mut resp = req.into_response(200, Some("OK"), &headers)?;
                resp.write_all(resp_body.as_bytes())?;
                Ok(())
            })
            .map_err(|err| format!("Failed to register /gain GET handler: {err}"))?;

        main_server
            .fn_handler("/gain*", Method::Options, |req| -> Result<(), EspIOError> {
                let headers = [
                    ("Access-Control-Allow-Origin", "*"),
                    ("Access-Control-Allow-Methods", "POST, GET, OPTIONS"),
                    ("Access-Control-Allow-Headers", "*"),
                    ("Content-Length", "0"),
                ];
                let _resp = req.into_response(200, Some("OK"), &headers)?;
                Ok(())
            })
            .map_err(|err| format!("Failed to register /gain OPTIONS handler: {err}"))?;

        // ── 1c. Recording API endpoints ─────────────────────────────────────
        main_server
            .fn_handler("/api/recordings*", Method::Get, |_req| -> Result<(), EspIOError> {
                let files = crate::sd::list_audio_files();
                let audio_dir = crate::sd::get_audio_dir();
                let mut items = Vec::new();
                for f in files {
                    let path = std::path::Path::new(audio_dir).join(&f);
                    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                    let date_str = if f.len() >= 15 && f.contains('_') {
                        let base = f.trim_end_matches(".wav").trim_end_matches(".opus");
                        if base.len() == 15 && base.as_bytes()[8] == b'_' {
                            format!("{}-{}-{} {}:{}:{}", &base[0..4], &base[4..6], &base[6..8], &base[9..11], &base[11..13], &base[13..15])
                        } else {
                            String::new()
                        }
                    } else {
                        String::new()
                    };
                    items.push(format!(r#"{{"filename":"{}","size":{},"date":"{}"}}"#, f, size, date_str));
                }
                let json = format!(r#"{{"status":"ok","recordings":[{}]}}"#, items.join(","));
                let headers = [
                    ("Content-Type", "application/json"),
                    ("Access-Control-Allow-Origin", "*"),
                    ("Cache-Control", "no-cache"),
                ];
                let mut resp = _req.into_response(200, Some("OK"), &headers)?;
                resp.write_all(json.as_bytes())?;
                Ok(())
            })
            .map_err(|err| format!("Failed to register /api/recordings GET handler: {err}"))?;

        main_server
            .fn_handler("/api/recordings*", Method::Delete, |req| -> Result<(), EspIOError> {
                let filename: String = {
                    let uri = req.uri();
                    if let Some(idx) = uri.find("filename=") {
                        uri[idx + 9..].split('&').next().unwrap_or("")
                    } else {
                        uri.trim_start_matches("/api/recordings/").split('?').next().unwrap_or("")
                    }
                    .trim_matches('/')
                    .to_string()
                };
                let (code, msg) = if filename.is_empty()
                    || filename.contains("..")
                    || filename.contains('/')
                    || filename.contains('\\')
                {
                    (400, r#"{"status":"error","message":"invalid filename"}"#.to_string())
                } else {
                    let base_dir = crate::sd::get_audio_dir();
                    let file_path = std::path::Path::new(base_dir).join(&filename);
                    if file_path.exists() && file_path.is_file() {
                        match std::fs::remove_file(&file_path) {
                            Ok(()) => (200, format!(r#"{{"status":"ok","deleted":"{}"}}"#, filename)),
                            Err(e) => (500, format!(r#"{{"status":"error","message":"{}"}}"#, e)),
                        }
                    } else {
                        (404, r#"{"status":"error","message":"file not found"}"#.to_string())
                    }
                };

                let headers = [
                    ("Content-Type", "application/json"),
                    ("Access-Control-Allow-Origin", "*"),
                ];
                let mut resp = req.into_response(code, Some("OK"), &headers)?;
                resp.write_all(msg.as_bytes())?;
                Ok(())
            })
            .map_err(|err| format!("Failed to register /api/recordings DELETE handler: {err}"))?;

        main_server
            .fn_handler("/api/recordings*", Method::Options, |req| -> Result<(), EspIOError> {
                let headers = [
                    ("Access-Control-Allow-Origin", "*"),
                    ("Access-Control-Allow-Methods", "GET, DELETE, OPTIONS"),
                    ("Access-Control-Allow-Headers", "*"),
                    ("Content-Length", "0"),
                ];
                let _resp = req.into_response(200, Some("OK"), &headers)?;
                Ok(())
            })
            .map_err(|err| format!("Failed to register /api/recordings OPTIONS handler: {err}"))?;

        let rec_start = recording_controller.clone();
        let buf_start = shared_buffer.clone();
        main_server
            .fn_handler("/api/recording/start*", Method::Post, move |req| -> Result<(), EspIOError> {
                let audio_dir = crate::sd::get_audio_dir();
                let (code, msg) = match rec_start.start(audio_dir, buf_start.clone()) {
                    Ok(info) => (200, format!(r#"{{"status":"ok","filename":"{}"}}"#, info.filename)),
                    Err(e) => (400, format!(r#"{{"status":"error","message":"{}"}}"#, e)),
                };
                let headers = [
                    ("Content-Type", "application/json"),
                    ("Access-Control-Allow-Origin", "*"),
                ];
                let mut resp = req.into_response(code, Some("OK"), &headers)?;
                resp.write_all(msg.as_bytes())?;
                Ok(())
            })
            .map_err(|err| format!("Failed to register /api/recording/start handler: {err}"))?;

        let rec_stop = recording_controller.clone();
        main_server
            .fn_handler("/api/recording/stop*", Method::Post, move |req| -> Result<(), EspIOError> {
                let (code, msg) = match rec_stop.stop() {
                    Ok(filename) => (200, format!(r#"{{"status":"ok","filename":"{}"}}"#, filename)),
                    Err(e) => (400, format!(r#"{{"status":"error","message":"{}"}}"#, e)),
                };
                let headers = [
                    ("Content-Type", "application/json"),
                    ("Access-Control-Allow-Origin", "*"),
                ];
                let mut resp = req.into_response(code, Some("OK"), &headers)?;
                resp.write_all(msg.as_bytes())?;
                Ok(())
            })
            .map_err(|err| format!("Failed to register /api/recording/stop handler: {err}"))?;

        let rec_cancel = recording_controller.clone();
        main_server
            .fn_handler("/api/recording/cancel*", Method::Post, move |req| -> Result<(), EspIOError> {
                let (code, msg) = match rec_cancel.cancel() {
                    Ok(()) => (200, r#"{"status":"ok"}"#.to_string()),
                    Err(e) => (400, format!(r#"{{"status":"error","message":"{}"}}"#, e)),
                };
                let headers = [
                    ("Content-Type", "application/json"),
                    ("Access-Control-Allow-Origin", "*"),
                ];
                let mut resp = req.into_response(code, Some("OK"), &headers)?;
                resp.write_all(msg.as_bytes())?;
                Ok(())
            })
            .map_err(|err| format!("Failed to register /api/recording/cancel handler: {err}"))?;

        main_server
            .fn_handler("/api/recording/*", Method::Options, |req| -> Result<(), EspIOError> {
                let headers = [
                    ("Access-Control-Allow-Origin", "*"),
                    ("Access-Control-Allow-Methods", "POST, OPTIONS"),
                    ("Access-Control-Allow-Headers", "*"),
                    ("Content-Length", "0"),
                ];
                let _resp = req.into_response(200, Some("OK"), &headers)?;
                Ok(())
            })
            .map_err(|err| format!("Failed to register /api/recording OPTIONS handler: {err}"))?;

        main_server
            .fn_handler("/recordings/*", Method::Get, |req| -> Result<(), EspIOError> {
                let filename: String = {
                    let uri = req.uri();
                    uri.trim_start_matches("/recordings/")
                        .split('?')
                        .next()
                        .unwrap_or("")
                        .trim_matches('/')
                        .to_string()
                };
                if filename.is_empty()
                    || filename.contains("..")
                    || filename.contains('/')
                    || filename.contains('\\')
                {
                    let mut resp = req.into_response(400, Some("Bad Request"), &[("Content-Type", "text/plain")])?;
                    resp.write_all(b"Invalid filename")?;
                    return Ok(());
                }

                let base_dir = crate::sd::get_audio_dir();
                let file_path = std::path::Path::new(base_dir).join(&filename);

                if !file_path.exists() || !file_path.is_file() {
                    let mut resp = req.into_response(404, Some("Not Found"), &[("Content-Type", "text/plain")])?;
                    resp.write_all(b"File not found")?;
                    return Ok(());
                }

                let mut file = match std::fs::File::open(&file_path) {
                    Ok(f) => f,
                    Err(e) => {
                        let mut resp = req.into_response(500, Some("Internal Error"), &[("Content-Type", "text/plain")])?;
                        resp.write_all(format!("Error opening file: {e}").as_bytes())?;
                        return Ok(());
                    }
                };

                let file_size = file.metadata().map(|m| m.len()).unwrap_or(0);
                let content_len_str = file_size.to_string();
                let mime_type = if filename.ends_with(".wav") {
                    "audio/wav"
                } else if filename.ends_with(".opus") {
                    "audio/ogg"
                } else {
                    "application/octet-stream"
                };
                let headers = [
                    ("Content-Type", mime_type),
                    ("Content-Length", content_len_str.as_str()),
                    ("Accept-Ranges", "bytes"),
                    ("Access-Control-Allow-Origin", "*"),
                    ("Cache-Control", "public, max-age=3600"),
                ];

                let mut resp = req.into_response(200, Some("OK"), &headers)?;
                let mut buf = vec![0u8; 4096];
                loop {
                    use std::io::Read;
                    match file.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            if let Err(e) = resp.write_all(&buf[..n]) {
                                log::warn!("Client disconnected while downloading {filename}: {e}");
                                break;
                            }
                        }
                        Err(e) => {
                            log::warn!("Error reading file {filename}: {e}");
                            break;
                        }
                    }
                }
                Ok(())
            })
            .map_err(|err| format!("Failed to register /recordings handler: {err}"))?;

        main_server
            .fn_handler("/recordings/*", Method::Options, |req| -> Result<(), EspIOError> {
                let headers = [
                    ("Access-Control-Allow-Origin", "*"),
                    ("Access-Control-Allow-Methods", "GET, OPTIONS"),
                    ("Access-Control-Allow-Headers", "*"),
                    ("Content-Length", "0"),
                ];
                let _resp = req.into_response(200, Some("OK"), &headers)?;
                Ok(())
            })
            .map_err(|err| format!("Failed to register /recordings OPTIONS handler: {err}"))?;

        // ── 1d. Playback API endpoints ──────────────────────────────────────
        let pb_start = playback_controller.clone();
        let pb_buf = shared_buffer.clone();
        main_server
            .fn_handler("/api/playback/start*", Method::Post, move |req| -> Result<(), EspIOError> {
                let uri = req.uri();
                let filename = if let Some(idx) = uri.find("filename=") {
                    uri[idx + 9..].split('&').next().unwrap_or("").to_string()
                } else {
                    String::new()
                };
                let audio_dir = crate::sd::get_audio_dir();
                let (code, msg) = match pb_start.start(audio_dir, &filename, pb_buf.clone()) {
                    Ok(info) => (200, format!(r#"{{"status":"ok","filename":"{}","total_secs":{}}}"#, info.filename, info.total_secs)),
                    Err(e) => (400, format!(r#"{{"status":"error","message":"{}"}}"#, e)),
                };
                let headers = [
                    ("Content-Type", "application/json"),
                    ("Access-Control-Allow-Origin", "*"),
                ];
                let mut resp = req.into_response(code, Some("OK"), &headers)?;
                resp.write_all(msg.as_bytes())?;
                Ok(())
            })
            .map_err(|err| format!("Failed to register /api/playback/start handler: {err}"))?;

        let pb_stop = playback_controller.clone();
        main_server
            .fn_handler("/api/playback/stop*", Method::Post, move |req| -> Result<(), EspIOError> {
                let _ = pb_stop.stop();
                let headers = [
                    ("Content-Type", "application/json"),
                    ("Access-Control-Allow-Origin", "*"),
                ];
                let mut resp = req.into_response(200, Some("OK"), &headers)?;
                resp.write_all(br#"{"status":"ok"}"#)?;
                Ok(())
            })
            .map_err(|err| format!("Failed to register /api/playback/stop handler: {err}"))?;

        // ── 1e. PIR Mode API endpoints ──────────────────────────────────────
        let pir_api_buffer = shared_buffer.clone();
        main_server
            .fn_handler("/api/pir*", Method::Get, move |req| -> Result<(), EspIOError> {
                let pir = pir_api_buffer.pir_status_summary();
                let pir_arming_cd_str = match pir.arming_countdown {
                    Some(cd) => cd.to_string(),
                    None => "null".to_string(),
                };
                let pir_rec_rem_str = match pir.recording_remaining_secs {
                    Some(rem) => rem.to_string(),
                    None => "null".to_string(),
                };
                let noise_threshold = crate::audio::get_noise_threshold();
                let body = format!(
                    r#"{{"status":"ok","device_mode":"{}","pir_state":"{}","armed":{},"arming_countdown":{},"motion_detected":{},"recording_remaining_secs":{},"noise_level":{},"noise_threshold":{},"sound_detected":{}}}"#,
                    pir.mode, pir.state, pir.armed, pir_arming_cd_str, pir.motion_detected, pir_rec_rem_str,
                    pir.noise_level, noise_threshold, pir.sound_detected
                );
                let headers = [
                    ("Content-Type", "application/json"),
                    ("Cache-Control", "no-cache"),
                    ("Access-Control-Allow-Origin", "*"),
                ];
                let mut resp = req.into_response(200, Some("OK"), &headers)?;
                resp.write_all(body.as_bytes())?;
                Ok(())
            })
            .map_err(|err| format!("Failed to register /api/pir GET handler: {err}"))?;

        main_server
            .fn_handler("/api/pir*", Method::Options, |req| -> Result<(), EspIOError> {
                let headers = [
                    ("Access-Control-Allow-Origin", "*"),
                    ("Access-Control-Allow-Methods", "GET, POST, OPTIONS"),
                    ("Access-Control-Allow-Headers", "*"),
                    ("Content-Length", "0"),
                ];
                let _resp = req.into_response(200, Some("OK"), &headers)?;
                Ok(())
            })
            .map_err(|err| format!("Failed to register /api/pir OPTIONS handler: {err}"))?;

        let pir_start_buf = shared_buffer.clone();
        main_server
            .fn_handler("/api/pir/start*", Method::Post, move |req| -> Result<(), EspIOError> {
                pir_start_buf.request_pir_start();
                let headers = [
                    ("Content-Type", "application/json"),
                    ("Access-Control-Allow-Origin", "*"),
                ];
                let mut resp = req.into_response(200, Some("OK"), &headers)?;
                resp.write_all(br#"{"status":"ok","action":"start_pir_mode"}"#)?;
                Ok(())
            })
            .map_err(|err| format!("Failed to register /api/pir/start handler: {err}"))?;

        let pir_stop_buf = shared_buffer.clone();
        main_server
            .fn_handler("/api/pir/stop*", Method::Post, move |req| -> Result<(), EspIOError> {
                pir_stop_buf.request_pir_stop();
                let headers = [
                    ("Content-Type", "application/json"),
                    ("Access-Control-Allow-Origin", "*"),
                ];
                let mut resp = req.into_response(200, Some("OK"), &headers)?;
                resp.write_all(br#"{"status":"ok","action":"stop_pir_mode"}"#)?;
                Ok(())
            })
            .map_err(|err| format!("Failed to register /api/pir/stop handler: {err}"))?;

        main_server
            .fn_handler("/api/pir/threshold*", Method::Post, |mut req| -> Result<(), EspIOError> {
                let uri = req.uri();
                let mut val_opt = parse_threshold_param(uri, "");
                if val_opt.is_none() {
                    let content_len = req
                        .header("Content-Length")
                        .and_then(|v| v.parse::<usize>().ok())
                        .unwrap_or(0);
                    if content_len > 0 {
                        let mut body_buf = [0u8; 128];
                        let to_read = content_len.min(body_buf.len());
                        let mut read_bytes = 0;
                        while read_bytes < to_read {
                            match req.read(&mut body_buf[read_bytes..to_read]) {
                                Ok(0) => break,
                                Ok(n) => read_bytes += n,
                                Err(_) => break,
                            }
                        }
                        if let Ok(body_str) = std::str::from_utf8(&body_buf[..read_bytes]) {
                            val_opt = parse_threshold_param("", body_str);
                        }
                    }
                }

                if let Some(val) = val_opt {
                    crate::audio::set_noise_threshold(val);
                    log::info!("Web: noise detection threshold updated to {val}%");
                }

                let current = crate::audio::get_noise_threshold();
                let body = format!(r#"{{"status":"ok","noise_threshold":{}}}"#, current);
                let headers = [
                    ("Content-Type", "application/json"),
                    ("Access-Control-Allow-Origin", "*"),
                ];
                let mut resp = req.into_response(200, Some("OK"), &headers)?;
                resp.write_all(body.as_bytes())?;
                Ok(())
            })
            .map_err(|err| format!("Failed to register /api/pir/threshold handler: {err}"))?;

        // ── 2. Dedicated Streaming Server (Port 8080) ────────────────────────
        // Pinned to its own FreeRTOS task so that the continuous audio streaming
        // loop never blocks the main HTTP server from handling /gain, /status, etc.
        let stream_config = HttpServerConfig {
            http_port: stream_port,
            ctrl_port: 32769,
            max_open_sockets: 4,
            stack_size: 8192,
            uri_match_wildcard: true,
            ..Default::default()
        };

        let mut stream_server = EspHttpServer::new(&stream_config)
            .map_err(|err| format!("Failed to start streaming EspHttpServer on port {stream_port}: {err}"))?;

        let stream_buffer = shared_buffer.clone();
        let make_stream_handler = move |req: esp_idf_svc::http::server::Request<&mut esp_idf_svc::http::server::EspHttpConnection<'_>>| -> Result<(), EspIOError> {
            let _guard = stream_buffer.listener_guard();
            log::info!("Live audio client connected to stream");

            let headers = [
                ("Content-Type", "audio/wav"),
                ("Cache-Control", "no-cache, no-store, must-revalidate"),
                ("Access-Control-Allow-Origin", "*"),
                ("Pragma", "no-cache"),
                ("Connection", "close"),
            ];

            let mut resp = req.into_response(200, Some("OK"), &headers)?;

            let wav_header = create_wav_header(16_000, 1, 16, 0x7fff_ffff);
            resp.write_all(&wav_header)?;

            // Start stream with a small pre-roll (10 frames = 200ms) to prime
            // the client's audio buffer while keeping live latency low (~200ms).
            let mut last_seq = stream_buffer.current_seq().saturating_sub(10);

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

        let main_fallback_buffer = shared_buffer.clone();
        let main_stream_handler = move |req: esp_idf_svc::http::server::Request<&mut esp_idf_svc::http::server::EspHttpConnection<'_>>| -> Result<(), EspIOError> {
            let _guard = main_fallback_buffer.listener_guard();
            log::info!("Live audio client connected to main server stream fallback");

            let headers = [
                ("Content-Type", "audio/wav"),
                ("Cache-Control", "no-cache, no-store, must-revalidate"),
                ("Access-Control-Allow-Origin", "*"),
                ("Pragma", "no-cache"),
                ("Connection", "close"),
            ];

            let mut resp = req.into_response(200, Some("OK"), &headers)?;
            let wav_header = create_wav_header(16_000, 1, 16, 0x7fff_ffff);
            resp.write_all(&wav_header)?;

            // Start stream with a small pre-roll (10 frames = 200ms)
            let mut last_seq = main_fallback_buffer.current_seq().saturating_sub(10);

            loop {
                let (frames, new_seq) = main_fallback_buffer.fetch_frames(last_seq, Duration::from_millis(200));
                last_seq = new_seq;

                for frame in frames {
                    let bytes = frame.to_le_bytes();
                    if let Err(err) = resp.write_all(&bytes) {
                        log::info!("Audio stream fallback client disconnected: {err}");
                        return Ok(());
                    }
                }
            }
        };

        stream_server
            .fn_handler("/stream.wav*", Method::Get, make_stream_handler)
            .map_err(|err| format!("Failed to register /stream.wav handler on streaming server: {err}"))?;

        stream_server
            .fn_handler("/stream.wav*", Method::Options, |req| -> Result<(), EspIOError> {
                let headers = [
                    ("Access-Control-Allow-Origin", "*"),
                    ("Access-Control-Allow-Methods", "GET, OPTIONS"),
                    ("Access-Control-Allow-Headers", "*"),
                    ("Content-Length", "0"),
                ];
                let _resp = req.into_response(200, Some("OK"), &headers)?;
                Ok(())
            })
            .map_err(|err| format!("Failed to register /stream.wav OPTIONS on streaming server: {err}"))?;

        // Fallback on main server
        main_server
            .fn_handler("/stream.wav*", Method::Get, main_stream_handler)
            .map_err(|err| format!("Failed to register fallback /stream.wav handler on main server: {err}"))?;

        log::info!("Servers started: Main server on port {port} (UI & controls), Streaming server on port {stream_port} (/stream.wav*)");

        Ok(ServerHandle {
            _main: main_server,
            _stream: stream_server,
        })
    }

    #[cfg(not(target_arch = "xtensa"))]
    pub fn start_server(
        _shared_buffer: SharedAudioBuffer,
        _recording_controller: crate::audio::RecordingController,
        _playback_controller: crate::audio::PlaybackController,
        _port: u16,
    ) -> Result<ServerHandle, String> {
        log::info!("Mock HTTP server started (non-xtensa architecture)");
        Ok(ServerHandle)
    }
}

pub struct ServerHandle {
    #[cfg(target_arch = "xtensa")]
    _main: EspHttpServer<'static>,
    #[cfg(target_arch = "xtensa")]
    _stream: EspHttpServer<'static>,
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

    #[test]
    fn shared_audio_buffer_gain_tracking() {
        let buffer = SharedAudioBuffer::new(5);
        buffer.set_gain(3.5, 88);
        assert!((buffer.current_gain() - 3.5).abs() < 0.001);
        assert_eq!(buffer.current_gain_percent(), 88);

        // Web override activates
        buffer.set_gain_override(true, 1.5, 37);
        assert!(buffer.is_gain_override_active());
        assert!((buffer.current_gain() - 1.5).abs() < 0.001);
        assert_eq!(buffer.current_gain_percent(), 37);
        // Potentiometer values are preserved in the background
        assert!((buffer.potentiometer_gain() - 3.5).abs() < 0.001);
        assert_eq!(buffer.potentiometer_gain_percent(), 88);

        // Potentiometer can update in background while override is active
        buffer.set_gain(2.0, 50);
        assert!((buffer.potentiometer_gain() - 2.0).abs() < 0.001);
        assert_eq!(buffer.potentiometer_gain_percent(), 50);
        // But active gain is still the override value
        assert!((buffer.current_gain() - 1.5).abs() < 0.001);
        assert_eq!(buffer.current_gain_percent(), 37);

        // Deactivating override reverts back to potentiometer reading
        buffer.set_gain_override(false, 1.5, 37);
        assert!(!buffer.is_gain_override_active());
        assert!((buffer.current_gain() - 2.0).abs() < 0.001);
        assert_eq!(buffer.current_gain_percent(), 50);
    }

    #[test]
    fn parse_gain_override_from_uri_query() {
        let (active, val) = parse_gain_override_params("/gain?override=true&value=75", "");
        assert_eq!(active, Some(true));
        assert_eq!(val, Some(75));

        let (active2, val2) = parse_gain_override_params("/gain?override=false&value=0", "");
        assert_eq!(active2, Some(false));
        assert_eq!(val2, Some(0));

        let (active3, val3) = parse_gain_override_params("/gain?override=1&value=100", "");
        assert_eq!(active3, Some(true));
        assert_eq!(val3, Some(100));
    }

    #[test]
    fn parse_gain_override_from_json_body() {
        let (active, val) = parse_gain_override_params("/gain", r#"{"override":true,"value":60}"#);
        assert_eq!(active, Some(true));
        assert_eq!(val, Some(60));

        let (active2, val2) = parse_gain_override_params("/gain", r#"{"override": false, "value": 25}"#);
        assert_eq!(active2, Some(false));
        assert_eq!(val2, Some(25));
    }

    #[test]
    fn shared_audio_buffer_pir_status_snapshot_tracking() {
        let buffer = SharedAudioBuffer::new(5);
        let initial = buffer.pir_status_summary();
        assert_eq!(initial.mode, "STATUS_MODE");
        assert_eq!(initial.state, "idle");
        assert!(!initial.armed);
        assert_eq!(initial.arming_countdown, None);
        assert!(!initial.motion_detected);
        assert_eq!(initial.recording_remaining_secs, None);

        // Update to PIR mode arming
        buffer.update_pir_status(
            crate::modes::DeviceMode::Pir,
            false,
            Some(8),
            false,
            None,
        );
        let arming = buffer.pir_status_summary();
        assert_eq!(arming.mode, "PIR_MODE");
        assert_eq!(arming.state, "arming");
        assert!(!arming.armed);
        assert_eq!(arming.arming_countdown, Some(8));

        // Update to PIR mode armed with motion and recording
        buffer.update_pir_status(
            crate::modes::DeviceMode::Pir,
            true,
            None,
            true,
            Some(19),
        );
        let armed_rec = buffer.pir_status_summary();
        assert_eq!(armed_rec.mode, "PIR_MODE");
        assert_eq!(armed_rec.state, "armed");
        assert!(armed_rec.armed);
        assert_eq!(armed_rec.arming_countdown, None);
        assert!(armed_rec.motion_detected);
        assert_eq!(armed_rec.recording_remaining_secs, Some(19));
    }

    #[test]
    fn shared_audio_buffer_sequence_and_capacity_tracking() {
        let buffer = SharedAudioBuffer::new(0); // 0 defaults to DEFAULT_BUFFER_CAPACITY
        assert_eq!(buffer.capacity(), DEFAULT_BUFFER_CAPACITY);
        assert_eq!(buffer.current_seq(), 0);
        assert!(buffer.is_empty());
        assert_eq!(buffer.len(), 0);

        let frame = AudioFrame::new(16_000, 1, vec![500; 320]);
        buffer.push_frame(frame);

        assert_eq!(buffer.current_seq(), 1);
        assert_eq!(buffer.len(), 1);
        assert!(!buffer.is_empty());
    }

    #[test]
    fn shared_audio_buffer_tracks_noise_level() {
        let buffer = SharedAudioBuffer::new(10);
        assert_eq!(buffer.current_noise_level(), 0);
        assert!(!buffer.is_noise_above(20));

        // Push frame with 5,000 amplitude -> RMS = 5,000 -> 50% noise level
        let sound_frame = AudioFrame::new(16_000, 1, vec![5_000; 320]);
        buffer.push_frame(sound_frame);

        assert_eq!(buffer.current_noise_level(), 50);
        assert!(buffer.is_noise_above(25));
        assert!(buffer.is_noise_above(50));
        assert!(!buffer.is_noise_above(51));

        let summary = buffer.pir_status_summary();
        assert_eq!(summary.noise_level, 50);
        assert!(summary.sound_detected);
    }

    #[test]
    fn parse_threshold_param_from_query_and_body() {
        assert_eq!(parse_threshold_param("/api/pir/threshold?value=35", ""), Some(35));
        assert_eq!(parse_threshold_param("/api/pir/threshold?threshold=50", ""), Some(50));
        assert_eq!(parse_threshold_param("/api/pir/threshold?value=150", ""), Some(100)); // clamped
        assert_eq!(parse_threshold_param("/api/pir/threshold", r#"{"value": 40}"#), Some(40));
        assert_eq!(parse_threshold_param("/api/pir/threshold", r#"{"threshold": 15}"#), Some(15));
        assert_eq!(parse_threshold_param("/api/pir/threshold", ""), None);
    }

    #[test]
    fn shared_audio_buffer_pir_commands() {
        let buffer = SharedAudioBuffer::new(10);
        assert_eq!(buffer.take_pir_command(), 0);

        buffer.request_pir_start();
        assert_eq!(buffer.take_pir_command(), 1);
        assert_eq!(buffer.take_pir_command(), 0);

        buffer.request_pir_stop();
        assert_eq!(buffer.take_pir_command(), 2);
        assert_eq!(buffer.take_pir_command(), 0);
    }
}
