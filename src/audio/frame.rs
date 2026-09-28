#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioFrame {
    pub sample_rate: u32,
    pub channels: u8,
    pub samples: Vec<i16>,
}

impl AudioFrame {
    pub fn new(sample_rate: u32, channels: u8, samples: Vec<i16>) -> Self {
        Self {
            sample_rate,
            channels,
            samples,
        }
    }

    /// Converts 16-bit PCM samples into a little-endian byte vector.
    pub fn to_le_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.samples.len() * 2);
        for &sample in &self.samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        bytes
    }

    /// Calculates the duration in milliseconds of this audio frame.
    pub fn duration_ms(&self) -> u32 {
        if self.sample_rate == 0 || self.channels == 0 {
            return 0;
        }
        ((self.samples.len() as u64 * 1000) / (self.sample_rate as u64 * self.channels as u64)) as u32
    }

    /// Calculates Root Mean Square (RMS) amplitude of the audio samples.
    pub fn rms_amplitude(&self) -> u16 {
        if self.samples.is_empty() {
            return 0;
        }
        let sum_sq: u64 = self
            .samples
            .iter()
            .map(|&s| (s as i64 * s as i64) as u64)
            .sum();
        let mean = sum_sq / (self.samples.len() as u64);
        (mean as f64).sqrt() as u16
    }

    /// Evaluates if the audio frame consists solely of silence below the amplitude threshold.
    pub fn is_silent(&self, threshold: i16) -> bool {
        self.samples.iter().all(|&s| s.abs() <= threshold)
    }
}

#[cfg(test)]
#[allow(unused_imports)]
mod tests {
    use super::*;

    #[test]
    fn audio_frame_conversion_to_le_bytes() {
        let frame = AudioFrame::new(16_000, 1, vec![0x1234, -1]);
        let bytes = frame.to_le_bytes();
        assert_eq!(bytes, vec![0x34, 0x12, 0xff, 0xff]);
    }

    #[test]
    fn audio_frame_duration_calculation() {
        // 320 samples at 16,000 Hz mono = 20 ms
        let frame = AudioFrame::new(16_000, 1, vec![0; 320]);
        assert_eq!(frame.duration_ms(), 20);

        // 1600 samples at 16,000 Hz mono = 100 ms
        let frame_100ms = AudioFrame::new(16_000, 1, vec![0; 1600]);
        assert_eq!(frame_100ms.duration_ms(), 100);
    }

    #[test]
    fn audio_frame_silence_and_rms() {
        let silence = AudioFrame::new(16_000, 1, vec![0; 100]);
        assert!(silence.is_silent(10));
        assert_eq!(silence.rms_amplitude(), 0);

        let sound = AudioFrame::new(16_000, 1, vec![1000; 100]);
        assert!(!sound.is_silent(100));
        assert_eq!(sound.rms_amplitude(), 1000);
    }
}
