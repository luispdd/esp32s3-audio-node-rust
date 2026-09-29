#[cfg(target_arch = "xtensa")]
use esp_idf_svc::hal::adc::attenuation::DB_12;
#[cfg(target_arch = "xtensa")]
use esp_idf_svc::hal::adc::oneshot::config::AdcChannelConfig;
#[cfg(target_arch = "xtensa")]
use esp_idf_svc::hal::adc::oneshot::{AdcChannelDriver, AdcDriver};
#[cfg(target_arch = "xtensa")]
use esp_idf_svc::hal::adc::ADC1;
#[cfg(target_arch = "xtensa")]
use esp_idf_svc::hal::gpio::Gpio4;

pub const DEFAULT_MAX_GAIN: f32 = 4.0;
pub const ADC_MAX_RAW: u16 = 4095;
pub const ADC_MIN_DEADBAND: u16 = 40;

/// Pure linear mapping from raw ADC reading (0..4095) to a gain multiplier.
/// At or below DEADBAND (wiper near minimum), returns 0.0 (complete silence).
/// At full scale (4095), returns `max_gain`.
/// Intermediate values scale linearly between 0.0 and `max_gain`.
pub fn raw_to_gain(raw: u16, max_gain: f32) -> f32 {
    if raw <= ADC_MIN_DEADBAND {
        0.0
    } else {
        let span = (ADC_MAX_RAW - ADC_MIN_DEADBAND) as f32;
        let normalized = (raw - ADC_MIN_DEADBAND) as f32 / span;
        normalized.clamp(0.0, 1.0) * max_gain
    }
}

/// Maps raw ADC reading (0..4095) to an integer percentage 0..=100.
pub fn raw_to_gain_percent(raw: u16) -> u8 {
    if raw <= ADC_MIN_DEADBAND {
        0
    } else {
        let span = (ADC_MAX_RAW - ADC_MIN_DEADBAND) as f32;
        let normalized = (raw - ADC_MIN_DEADBAND) as f32 / span;
        (normalized.clamp(0.0, 1.0) * 100.0).round() as u8
    }
}

pub struct Potentiometer<'a> {
    #[cfg(target_arch = "xtensa")]
    driver: AdcChannelDriver<
        'a,
        esp_idf_svc::hal::adc::ADCCH3<esp_idf_svc::hal::adc::ADCU1>,
        AdcDriver<'a, esp_idf_svc::hal::adc::ADCU1>,
    >,
    max_gain: f32,
    last_raw: u16,
    #[cfg(not(target_arch = "xtensa"))]
    _phantom: std::marker::PhantomData<&'a ()>,
}

#[cfg(target_arch = "xtensa")]
impl<'a> Potentiometer<'a> {
    pub fn new(adc1: ADC1<'a>, pin: Gpio4<'a>) -> Result<Self, String> {
        let adc = AdcDriver::new(adc1).map_err(|err| format!("ADC1 driver init failed: {err}"))?;
        let config = AdcChannelConfig {
            attenuation: DB_12,
            ..Default::default()
        };
        let driver = AdcChannelDriver::new(adc, pin, &config)
            .map_err(|err| format!("AdcChannelDriver for GPIO4 failed: {err}"))?;

        log::info!("Potentiometer initialized on GPIO4 (ADC1_CH3, 12-bit, max gain: {:.1}x)", DEFAULT_MAX_GAIN);

        let mut pot = Self {
            driver,
            max_gain: DEFAULT_MAX_GAIN,
            last_raw: 0,
        };

        // Initial reading
        let _ = pot.read_raw();

        Ok(pot)
    }

    pub fn read_raw(&mut self) -> Result<u16, String> {
        let raw = self.driver.read_raw().map_err(|err| format!("ADC read failed: {err}"))?;
        self.last_raw = raw;
        Ok(raw)
    }

    pub fn last_gain(&self) -> f32 {
        raw_to_gain(self.last_raw, self.max_gain)
    }

    pub fn last_gain_percent(&self) -> u8 {
        raw_to_gain_percent(self.last_raw)
    }

    pub fn read_gain(&mut self) -> Result<f32, String> {
        let raw = self.read_raw()?;
        Ok(raw_to_gain(raw, self.max_gain))
    }

    pub fn read_gain_percent(&mut self) -> Result<u8, String> {
        let raw = self.read_raw()?;
        Ok(raw_to_gain_percent(raw))
    }
}

#[cfg(not(target_arch = "xtensa"))]
impl<'a> Potentiometer<'a> {
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            max_gain: DEFAULT_MAX_GAIN,
            last_raw: ADC_MAX_RAW,
            _phantom: std::marker::PhantomData,
        })
    }

    pub fn with_raw(raw: u16) -> Result<Self, String> {
        Ok(Self {
            max_gain: DEFAULT_MAX_GAIN,
            last_raw: raw,
            _phantom: std::marker::PhantomData,
        })
    }

    pub fn read_raw(&mut self) -> Result<u16, String> {
        Ok(self.last_raw)
    }

    pub fn set_mock_raw(&mut self, raw: u16) {
        self.last_raw = raw;
    }

    pub fn last_gain(&self) -> f32 {
        raw_to_gain(self.last_raw, self.max_gain)
    }

    pub fn last_gain_percent(&self) -> u8 {
        raw_to_gain_percent(self.last_raw)
    }

    pub fn read_gain(&mut self) -> Result<f32, String> {
        Ok(raw_to_gain(self.last_raw, self.max_gain))
    }

    pub fn read_gain_percent(&mut self) -> Result<u8, String> {
        Ok(raw_to_gain_percent(self.last_raw))
    }
}

#[cfg(test)]
#[allow(unused_imports)]
mod tests {
    use super::*;

    #[test]
    fn raw_to_gain_minimum_produces_silence() {
        assert_eq!(raw_to_gain(0, 4.0), 0.0);
        assert_eq!(raw_to_gain(ADC_MIN_DEADBAND, 4.0), 0.0);
        assert_eq!(raw_to_gain_percent(0), 0);
        assert_eq!(raw_to_gain_percent(ADC_MIN_DEADBAND), 0);
    }

    #[test]
    fn raw_to_gain_maximum_produces_max_gain() {
        assert_eq!(raw_to_gain(ADC_MAX_RAW, 4.0), 4.0);
        assert_eq!(raw_to_gain_percent(ADC_MAX_RAW), 100);
    }

    #[test]
    fn raw_to_gain_intermediate_is_linear() {
        let mid_raw = (ADC_MAX_RAW + ADC_MIN_DEADBAND) / 2;
        let gain = raw_to_gain(mid_raw, 4.0);
        assert!((gain - 2.0).abs() < 0.01);
        let pct = raw_to_gain_percent(mid_raw);
        assert_eq!(pct, 50);
    }
}
