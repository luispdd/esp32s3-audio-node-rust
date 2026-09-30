#[cfg(target_arch = "xtensa")]
use core::time::Duration;
#[cfg(target_arch = "xtensa")]
use esp_idf_svc::hal::gpio::OutputPin;
#[cfg(target_arch = "xtensa")]
use esp_idf_svc::hal::rmt::config::{TransmitConfig, TxChannelConfig};
#[cfg(target_arch = "xtensa")]
use esp_idf_svc::hal::rmt::encoder::{BytesEncoder, BytesEncoderConfig};
#[cfg(target_arch = "xtensa")]
use esp_idf_svc::hal::rmt::{PinState, Pulse, PulseTicks, Symbol, TxChannelDriver};
#[cfg(target_arch = "xtensa")]
use esp_idf_svc::hal::units::*;

pub struct RgbLed<'d> {
    #[cfg(target_arch = "xtensa")]
    driver: TxChannelDriver<'d>,
    #[cfg(target_arch = "xtensa")]
    encoder: BytesEncoder,
    #[cfg(not(target_arch = "xtensa"))]
    _phantom: std::marker::PhantomData<&'d ()>,
}

impl<'d> RgbLed<'d> {
    #[cfg(target_arch = "xtensa")]
    pub fn new(pin: impl OutputPin + 'd) -> Result<Self, String> {
        let config = TxChannelConfig {
            resolution: 20_u32.MHz().into(),
            ..Default::default()
        };
        let driver = TxChannelDriver::new(pin, &config)
            .map_err(|err| format!("Failed to create RMT TxChannelDriver for RGB LED: {err}"))?;

        // WS2812 timing at 20 MHz (50 ns tick):
        // Bit 0: 350 ns high (7 ticks), 900 ns low (18 ticks)
        // Bit 1: 900 ns high (18 ticks), 350 ns low (7 ticks)
        let high_0 = Pulse::new(PinState::High, PulseTicks::new(7).map_err(|e| format!("{e}"))?);
        let low_0 = Pulse::new(PinState::Low, PulseTicks::new(18).map_err(|e| format!("{e}"))?);
        let bit0 = Symbol::new(high_0, low_0);

        let high_1 = Pulse::new(PinState::High, PulseTicks::new(18).map_err(|e| format!("{e}"))?);
        let low_1 = Pulse::new(PinState::Low, PulseTicks::new(7).map_err(|e| format!("{e}"))?);
        let bit1 = Symbol::new(high_1, low_1);

        let encoder_config = BytesEncoderConfig {
            bit0,
            bit1,
            msb_first: true,
            ..Default::default()
        };
        let encoder = BytesEncoder::with_config(&encoder_config)
            .map_err(|err| format!("Failed to create BytesEncoder for WS2812: {err}"))?;

        let mut led = Self { driver, encoder };
        // Immediately set to blue during boot
        let _ = led.set_booting();

        Ok(led)
    }

    #[cfg(not(target_arch = "xtensa"))]
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            _phantom: std::marker::PhantomData,
        })
    }

    /// Sets the RGB color (r, g, b) where each value is 0..=255.
    /// WS2812 expects colors in GRB order.
    pub fn set_rgb(&mut self, r: u8, g: u8, b: u8) -> Result<(), String> {
        #[cfg(target_arch = "xtensa")]
        {
            // WS2812 protocol requires [Green, Red, Blue]
            let grb = [g, r, b];
            self.driver
                .send_and_wait(&mut self.encoder, &grb, &TransmitConfig::default())
                .map_err(|err| format!("Failed to transmit WS2812 RGB LED data: {err}"))?;
            // A short delay (>50us) to ensure the reset latch completes
            std::thread::sleep(Duration::from_micros(100));
        }
        Ok(())
    }

    /// Sets the LED to blue during the boot process.
    pub fn set_booting(&mut self) -> Result<(), String> {
        // Distinct blue: R=0, G=0, B=64
        self.set_rgb(0, 0, 64)
    }

    /// Turns the LED off.
    pub fn turn_off(&mut self) -> Result<(), String> {
        self.set_rgb(0, 0, 0)
    }
}

#[cfg(test)]
#[allow(unused_imports)]
mod tests {
    use super::*;

    #[test]
    fn rgb_led_lifecycle() {
        let mut led = RgbLed::new().unwrap();
        assert!(led.set_booting().is_ok());
        assert!(led.turn_off().is_ok());
        assert!(led.set_rgb(255, 0, 0).is_ok());
    }
}
