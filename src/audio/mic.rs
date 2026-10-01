use crate::audio::frame::AudioFrame;
use std::sync::atomic::{AtomicU8, Ordering};

pub use crate::config::NOISE_DETECTION_THRESHOLD_PERCENT;

/// Global runtime noise detection threshold percentage (0..=100) initialized from config.
/// Audio exceeding this percentage is recognized as acoustic sound activity.
pub static NOISE_THRESHOLD_PERCENT: AtomicU8 = AtomicU8::new(NOISE_DETECTION_THRESHOLD_PERCENT);

/// Returns the current noise detection threshold percentage (0..=100).
pub fn get_noise_threshold() -> u8 {
    NOISE_THRESHOLD_PERCENT.load(Ordering::Relaxed)
}

/// Dynamically sets the noise detection threshold percentage (0..=100).
pub fn set_noise_threshold(val: u8) {
    NOISE_THRESHOLD_PERCENT.store(val.min(100), Ordering::Relaxed);
}

#[cfg(target_arch = "xtensa")]
use esp_idf_svc::hal::gpio::{AnyIOPin, Gpio14, Gpio15, Gpio16};
#[cfg(target_arch = "xtensa")]
use esp_idf_svc::hal::i2s::{
    config::{DataBitWidth, StdConfig},
    I2sDriver, I2sRx, I2S0,
};

/// Pure domain helper to evaluate whether raw sample bytes contain audio energy.
pub fn detect_signal(samples: &[u8]) -> bool {
    samples.iter().any(|&b| b != 0)
}

/// Converts raw 32-bit I2S bytes (from INMP441) to 16-bit PCM samples.
/// INMP441 produces 24-bit MSB-aligned data inside a 32-bit slot.
/// In standard Philips format with stereo slots (8 bytes per sample pair),
/// the INMP441 (with L/R tied to GND) asserts data on the Left slot.
pub fn convert_i2s_bytes_to_pcm16(raw: &[u8]) -> Vec<i16> {
    convert_i2s_bytes_to_pcm16_with_gain(raw, 1.0)
}

/// Converts raw I2S bytes to 16-bit PCM with a linear gain multiplier.
/// A gain of 0.0 (or negative/NaN) produces complete silence (all zeros).
/// A gain of 1.0 produces standard MSB-aligned 16-bit PCM.
/// Higher values scale the amplitude linearly, clamping to i16 range.
pub fn convert_i2s_bytes_to_pcm16_with_gain(raw: &[u8], gain: f32) -> Vec<i16> {
    if gain <= 0.0 || gain.is_nan() {
        let sample_count = if raw.len() >= 8 && raw.len() % 8 == 0 {
            raw.len() / 8
        } else {
            raw.len() / 4
        };
        return vec![0i16; sample_count];
    }

    if raw.len() >= 8 && raw.len() % 8 == 0 {
        // Standard 32-bit slot stereo pair (8 bytes total)
        raw.chunks_exact(8)
            .map(|chunk| {
                let left = i32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                let right = i32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]);
                // Pick active channel (INMP441 on Left channel, fallback to Right if needed)
                let active = if left != 0 { left } else { right };
                let base_sample = (active >> 16) as f32;
                let scaled = (base_sample * gain).round();
                scaled.clamp(i16::MIN as f32, i16::MAX as f32) as i16
            })
            .collect()
    } else {
        // Fallback for 4 bytes per sample (mono 32-bit slot)
        raw.chunks_exact(4)
            .map(|chunk| {
                let val = i32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                let base_sample = (val >> 16) as f32;
                let scaled = (base_sample * gain).round();
                scaled.clamp(i16::MIN as f32, i16::MAX as f32) as i16
            })
            .collect()
    }
}

pub struct Microphone<'a> {
    #[cfg(target_arch = "xtensa")]
    driver: I2sDriver<'a, I2sRx>,
    last_signal_detected: bool,
    current_gain: f32,
    #[cfg(target_arch = "xtensa")]
    raw_buffer: Vec<u8>,
    #[cfg(not(target_arch = "xtensa"))]
    _phantom: std::marker::PhantomData<&'a ()>,
    #[cfg(not(target_arch = "xtensa"))]
    mock_frame_index: usize,
}

#[cfg(target_arch = "xtensa")]
impl<'a> Microphone<'a> {
    pub fn new(
        i2s: I2S0<'a>,
        sck: Gpio14<'a>,
        ws: Gpio15<'a>,
        sd: Gpio16<'a>,
    ) -> Result<Self, String> {
        let config = StdConfig::philips(16_000, DataBitWidth::Bits32);
        let mut driver = I2sDriver::new_std_rx(i2s, &config, sck, sd, AnyIOPin::none(), ws)
            .map_err(|err| format!("I2S driver init failed: {err}"))?;

        driver
            .rx_enable()
            .map_err(|err| format!("I2S rx enable failed: {err}"))?;

        log::info!("INMP441 I2S microphone initialized (SCK: GPIO14, WS: GPIO15, SD: GPIO16, 16kHz)");

        let mut mic = Self {
            driver,
            last_signal_detected: false,
            current_gain: 1.0,
            raw_buffer: Vec::with_capacity(320 * 8),
        };

        // Perform initial hardware read to verify communication
        let _ = mic.probe_signal();

        Ok(mic)
    }

    pub fn set_gain(&mut self, gain: f32) {
        self.current_gain = gain;
    }

    pub fn current_gain(&self) -> f32 {
        self.current_gain
    }

    pub fn read_samples(&mut self, buffer: &mut [u8], timeout_ticks: u32) -> Result<usize, String> {
        self.driver
            .read(buffer, timeout_ticks)
            .map_err(|err| format!("I2S read failed: {err}"))
    }

    /// Reads a continuous AudioFrame consisting of `samples_count` 16-bit PCM samples with dynamic gain.
    pub fn read_frame_with_gain(
        &mut self,
        samples_count: usize,
        timeout_ticks: u32,
        gain: f32,
    ) -> Result<AudioFrame, String> {
        // In 32-bit stereo mode, each sample needs 8 bytes from I2S
        let raw_len = samples_count * 8;
        if self.raw_buffer.len() < raw_len {
            self.raw_buffer.resize(raw_len, 0);
        }

        let bytes_read = self
            .driver
            .read(&mut self.raw_buffer[..raw_len], timeout_ticks)
            .map_err(|err| format!("I2S read failed: {err}"))?;
        let pcm_samples = convert_i2s_bytes_to_pcm16_with_gain(&self.raw_buffer[..bytes_read], gain);

        let signal_present = pcm_samples.iter().any(|&s| s.abs() > 300);
        self.last_signal_detected = signal_present;
        self.current_gain = gain;

        Ok(AudioFrame::new(16_000, 1, pcm_samples))
    }

    /// Reads a continuous AudioFrame consisting of `samples_count` 16-bit PCM samples using the configured gain.
    pub fn read_frame(&mut self, samples_count: usize, timeout_ticks: u32) -> Result<AudioFrame, String> {
        self.read_frame_with_gain(samples_count, timeout_ticks, self.current_gain)
    }

    pub fn probe_signal(&mut self) -> bool {
        let mut buf = [0u8; 128];
        match self.read_samples(&mut buf, 20) {
            Ok(n) if n > 0 => {
                let has_data = detect_signal(&buf[..n]);
                self.last_signal_detected = has_data;
                has_data
            }
            Ok(_) => self.last_signal_detected,
            Err(err) => {
                static mut ERR_COUNTER: u32 = 0;
                unsafe {
                    ERR_COUNTER += 1;
                    if ERR_COUNTER % 50 == 0 {
                        log::warn!("Microphone read probe error: {err}");
                    }
                }
                false
            }
        }
    }

    pub fn is_signal_present(&self) -> bool {
        self.last_signal_detected
    }
}

#[cfg(not(target_arch = "xtensa"))]
impl<'a> Microphone<'a> {
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            last_signal_detected: true,
            current_gain: 1.0,
            _phantom: std::marker::PhantomData,
            mock_frame_index: 0,
        })
    }

    pub fn set_gain(&mut self, gain: f32) {
        self.current_gain = gain;
    }

    pub fn current_gain(&self) -> f32 {
        self.current_gain
    }

    pub fn read_samples(&mut self, buffer: &mut [u8], _timeout_ticks: u32) -> Result<usize, String> {
        buffer.fill(0x12);
        Ok(buffer.len())
    }

    pub fn read_frame_with_gain(
        &mut self,
        samples_count: usize,
        _timeout_ticks: u32,
        gain: f32,
    ) -> Result<AudioFrame, String> {
        let samples: Vec<i16> = (0..samples_count)
            .map(|i| {
                if gain <= 0.0 {
                    0
                } else {
                    let phase = (self.mock_frame_index + i) as f32 / 16_000.0;
                    let tone = (phase * 440.0 * 2.0 * std::f32::consts::PI).sin() * 8000.0 * gain;
                    tone.clamp(i16::MIN as f32, i16::MAX as f32) as i16
                }
            })
            .collect();
        self.mock_frame_index += samples_count;
        self.last_signal_detected = gain > 0.0;
        self.current_gain = gain;
        Ok(AudioFrame::new(16_000, 1, samples))
    }

    pub fn read_frame(&mut self, samples_count: usize, timeout_ticks: u32) -> Result<AudioFrame, String> {
        self.read_frame_with_gain(samples_count, timeout_ticks, self.current_gain)
    }

    pub fn probe_signal(&mut self) -> bool {
        self.last_signal_detected
    }

    pub fn is_signal_present(&self) -> bool {
        self.last_signal_detected
    }
}

#[cfg(test)]
#[allow(unused_imports)]
mod tests {
    use super::*;

    #[test]
    fn detect_signal_identifies_sound_and_silence() {
        let silence = [0u8; 64];
        assert!(!detect_signal(&silence));

        let mut signal = [0u8; 64];
        signal[10] = 0x42;
        assert!(detect_signal(&signal));
    }

    #[test]
    fn convert_stereo_i2s_to_pcm16_extracts_samples() {
        // Slot 0 (Left): 0x1234_0000 (i32) -> shift 16 = 0x1234
        // Slot 1 (Right): 0x0000_0000
        let mut raw = [0u8; 8];
        let sample: i32 = 0x1234_0000;
        raw[0..4].copy_from_slice(&sample.to_le_bytes());

        let pcm = convert_i2s_bytes_to_pcm16(&raw);
        assert_eq!(pcm.len(), 1);
        assert_eq!(pcm[0], 0x1234);
    }

    #[test]
    fn convert_negative_sample_preserves_sign() {
        let mut raw = [0u8; 8];
        let sample: i32 = -1000 << 16;
        raw[0..4].copy_from_slice(&sample.to_le_bytes());

        let pcm = convert_i2s_bytes_to_pcm16(&raw);
        assert_eq!(pcm.len(), 1);
        assert_eq!(pcm[0], -1000);
    }

    #[test]
    fn convert_with_gain_amplifies_signal() {
        let mut raw = [0u8; 8];
        // 100 shifted into position
        let sample: i32 = 100 << 16;
        raw[0..4].copy_from_slice(&sample.to_le_bytes());

        // Gain 0.0 -> complete silence
        let pcm0 = convert_i2s_bytes_to_pcm16_with_gain(&raw, 0.0);
        assert_eq!(pcm0[0], 0);

        // Gain 1.0 -> 1x amplitude (100)
        let pcm1 = convert_i2s_bytes_to_pcm16_with_gain(&raw, 1.0);
        assert_eq!(pcm1[0], 100);

        // Gain 2.0 -> 2x amplitude (200)
        let pcm2 = convert_i2s_bytes_to_pcm16_with_gain(&raw, 2.0);
        assert_eq!(pcm2[0], 200);

        // Gain 4.0 -> 4x amplitude (400)
        let pcm4 = convert_i2s_bytes_to_pcm16_with_gain(&raw, 4.0);
        assert_eq!(pcm4[0], 400);
    }

    #[test]
    fn microphone_read_frame_non_xtensa_produces_frames() {
        let mut mic = Microphone::new().unwrap();
        let frame = mic.read_frame(320, 20).unwrap();
        assert_eq!(frame.samples.len(), 320);
        assert_eq!(frame.sample_rate, 16_000);
        assert_eq!(frame.channels, 1);
        assert!(mic.is_signal_present());
    }

    #[test]
    fn microphone_read_frame_with_silence_produces_zeroes() {
        let mut mic = Microphone::new().unwrap();
        let frame = mic.read_frame_with_gain(320, 20, 0.0).unwrap();
        assert_eq!(frame.samples.len(), 320);
        assert!(frame.samples.iter().all(|&s| s == 0));
        assert!(!mic.is_signal_present());
    }
}
