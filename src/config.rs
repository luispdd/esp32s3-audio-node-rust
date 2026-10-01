use std::collections::BTreeSet;

/// Configurable noise level detection threshold percentage (0..=100) for microphone acoustic activity (Task 6.5).
/// When live audio noise level meets or exceeds this percentage during PIR monitoring / recording,
/// sound activity is detected and the recording timer is extended.
/// Update this variable easily in code to adjust microphone acoustic sensitivity:
/// - Lower value (e.g. 15): more sensitive to quieter sounds
/// - Higher value (e.g. 40): requires louder sounds to trigger
pub const NOISE_DETECTION_THRESHOLD_PERCENT: u8 = 25;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WifiCredentials {
    pub ssid: String,
    pub password: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HardwareBaseline {
    pub inmp441_sck: u32,
    pub inmp441_ws: u32,
    pub inmp441_sd: u32,
    pub micro_sd_sck: u32,
    pub micro_sd_mosi: u32,
    pub micro_sd_miso: u32,
    pub micro_sd_cs: u32,
    pub oled_sda: u32,
    pub oled_scl: u32,
    pub pir_out: u32,
    pub adc_input: u32,
    pub button_mode: u32,
    pub button_record: u32,
    pub button_display: u32,
}

impl HardwareBaseline {
    pub fn project_spec() -> Self {
        Self {
            inmp441_sck: 14,
            inmp441_ws: 15,
            inmp441_sd: 16,
            micro_sd_sck: 12,
            micro_sd_mosi: 11,
            micro_sd_miso: 13,
            micro_sd_cs: 10,
            oled_sda: 8,
            oled_scl: 9,
            pir_out: 3,
            adc_input: 4,
            button_mode: 5,
            button_record: 6,
            button_display: 7,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        let pins = [
            ("INMP441 SCK", self.inmp441_sck),
            ("INMP441 WS", self.inmp441_ws),
            ("INMP441 SD", self.inmp441_sd),
            ("MicroSD SCK", self.micro_sd_sck),
            ("MicroSD MOSI", self.micro_sd_mosi),
            ("MicroSD MISO", self.micro_sd_miso),
            ("MicroSD CS", self.micro_sd_cs),
            ("OLED SDA", self.oled_sda),
            ("OLED SCL", self.oled_scl),
            ("PIR OUT", self.pir_out),
            ("ADC input", self.adc_input),
            ("Mode button", self.button_mode),
            ("Record button", self.button_record),
            ("Display button", self.button_display),
        ];

        let mut seen = BTreeSet::new();
        for (name, pin) in pins {
            if (33..=37).contains(&pin) {
                return Err(format!(
                    "{} uses reserved GPIO {} which is part of the PSRAM bus and must remain unused",
                    name, pin
                ));
            }
            if !seen.insert(pin) {
                return Err(format!("GPIO {} is assigned to multiple hardware functions", pin));
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeArchitecture {
    pub core_1_tasks: [&'static str; 4],
    pub core_0_tasks: [&'static str; 5],
    pub psram_buffers: &'static str,
}

impl RuntimeArchitecture {
    pub fn project_spec() -> Self {
        Self {
            core_1_tasks: [
                "I2S capture",
                "ADC polling",
                "button debouncing",
                "OLED rendering",
            ],
            core_0_tasks: [
                "Wi-Fi networking",
                "stream serving",
                "WAV audio recording",
                "SD file writes",
                "buffer management",
            ],
            psram_buffers: "PSRAM-backed audio buffers absorb SD write stalls without dropping samples",
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.core_1_tasks.is_empty() || self.core_0_tasks.is_empty() {
            return Err("core task split is incomplete".to_string());
        }
        if !self.psram_buffers.contains("PSRAM") {
            return Err("PSRAM-backed buffer policy is missing from the runtime architecture".to_string());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub wifi: WifiCredentials,
    pub hardware: HardwareBaseline,
    pub architecture: RuntimeArchitecture,
}

impl Config {
    pub fn validate(&self) -> Result<(), String> {
        self.hardware.validate()?;
        self.architecture.validate()?;

        if self.wifi.ssid.trim().is_empty() {
            return Err("WiFi SSID is empty".to_string());
        }

        if self.wifi.password.trim().is_empty() {
            return Err("WiFi password is empty".to_string());
        }

        Ok(())
    }
}
