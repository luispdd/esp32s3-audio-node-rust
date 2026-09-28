use super::frame::AudioFrame;

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
        let endpoint = "/live-stream".to_string();
        let browser_page = format!(
            "<!doctype html><html><body><h1>Audio Node</h1><audio controls autoplay src=\"{endpoint}\"></audio><p>Live stream endpoint: <code>{endpoint}</code></p></body></html>"
        );

        Self {
            sample_rate: 16_000,
            channels: 1,
            frame_size: 320,
            endpoint,
            browser_page,
            frame_index: 0,
        }
    }

    pub fn capture_frame(&mut self) -> AudioFrame {
        let samples = (0..self.frame_size)
            .map(|index| {
                let phase = (self.frame_index + index) as f32 / self.sample_rate as f32;
                let tone = (phase * 440.0).sin() * 12000.0;
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
}

impl Default for LiveAudioStream {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn live_audio_stream_generates_browser_ready_frames() {
        let mut stream = LiveAudioStream::new();
        let first_frame = stream.capture_frame();

        assert_eq!(first_frame.sample_rate, 16_000);
        assert_eq!(first_frame.channels, 1);
        assert!(!first_frame.samples.is_empty());
        assert!(stream.browser_page().contains("/live-stream"));
        assert!(stream.browser_page().contains("audio"));
    }
}
