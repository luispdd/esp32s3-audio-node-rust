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

pub struct Microphone<'a> {
    #[cfg(target_arch = "xtensa")]
    driver: I2sDriver<'a, I2sRx>,
    last_signal_detected: bool,
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
        };

        // Perform initial hardware read to verify communication
        let _ = mic.probe_signal();

        Ok(mic)
    }

    pub fn read_samples(&mut self, buffer: &mut [u8], timeout_ticks: u32) -> Result<usize, String> {
        self.driver
            .read(buffer, timeout_ticks)
            .map_err(|err| format!("I2S read failed: {err}"))
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
        })
    }

    pub fn read_samples(&mut self, buffer: &mut [u8], _timeout_ticks: u32) -> Result<usize, String> {
        buffer.fill(0x12);
        Ok(buffer.len())
    }

    pub fn probe_signal(&mut self) -> bool {
        self.last_signal_detected
    }

    pub fn is_signal_present(&self) -> bool {
        self.last_signal_detected
    }
}

#[cfg(test)]
#[allow(dead_code, unused_imports)]
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
}
