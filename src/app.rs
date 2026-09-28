use std::thread;
use std::time::Duration;

use crate::config::Config;
use crate::network::{wifi_credentials, WifiConnection};
use embedded_graphics::{
    draw_target::DrawTarget,
    mono_font::{ascii::FONT_6X10, MonoTextStyleBuilder},
    pixelcolor::BinaryColor,
    prelude::Point,
    text::Text,
    Drawable,
};
use esp_idf_svc::fs::fatfs::Fatfs;
use esp_idf_svc::hal::delay::BLOCK;
use esp_idf_svc::hal::gpio::{AnyIOPin, PinDriver, Pull};
use esp_idf_svc::hal::i2c::{I2cConfig, I2cDriver};
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::hal::sd::{spi::SdSpiHostDriver, SdCardConfiguration, SdCardDriver};
use esp_idf_svc::hal::spi::{config::DriverConfig, Dma, SpiDriver};
use esp_idf_svc::hal::units::*;
use esp_idf_svc::io::vfs::MountedFatfs;
use ssd1306::{prelude::*, I2CDisplayInterface, Ssd1306};

fn probe_oled_display(i2c: &mut I2cDriver<'_>) -> Result<(), String> {
    for address in [0x3c_u8, 0x3d_u8] {
        let probe = [0x00, 0xAE];
        match i2c.write(address, &probe, BLOCK) {
            Ok(()) => {
                log::info!("SSD1306 I2C probe succeeded at address 0x{:02x}", address);
                return Ok(());
            }
            Err(err) => {
                log::warn!("SSD1306 probe at 0x{:02x} failed: {}", address, err);
            }
        }
    }

    Err("SSD1306 display not responding on I2C bus at 0x3C or 0x3D".to_string())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioFrame {
    pub sample_rate: u32,
    pub channels: u8,
    pub samples: Vec<i16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceMode {
    Status,
    Live,
    Sd,
}

impl DeviceMode {
    pub fn next(self) -> Self {
        match self {
            Self::Status => Self::Live,
            Self::Live => Self::Sd,
            Self::Sd => Self::Status,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::Status => Self::Sd,
            Self::Live => Self::Status,
            Self::Sd => Self::Live,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Status => "STATUS_MODE",
            Self::Live => "LIVE_MODE",
            Self::Sd => "SD_MODE",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SystemStatus {
    pub wifi: bool,
    pub mic: bool,
    pub pir: bool,
    pub sd: bool,
}

impl SystemStatus {
    pub fn from_runtime(wifi_connected: bool) -> Self {
        Self {
            wifi: wifi_connected,
            mic: true,
            pir: false,
            sd: check_sd_card_is_writable(),
        }
    }

    pub fn summary_lines(&self) -> Vec<String> {
        vec![
            format!("wifi {} mic {}", status_marker(self.wifi), status_marker(self.mic)),
            format!("pir {} sd {}", status_marker(self.pir), status_marker(self.sd)),
        ]
    }
}

fn status_marker(value: bool) -> &'static str {
    if value { "OK" } else { "KO" }
}

fn check_sd_card_is_writable() -> bool {
    #[cfg(not(target_arch = "xtensa"))]
    {
        return false;
    }

    #[cfg(target_arch = "xtensa")]
    {
        let peripherals = match Peripherals::take() {
            Ok(peripherals) => peripherals,
            Err(err) => {
                log::warn!("SD card probe skipped: unable to access ESP peripherals: {err}");
                return false;
            }
        };

        let pins = peripherals.pins;

        let spi_driver = match SpiDriver::new(
            peripherals.spi3,
            pins.gpio12,
            pins.gpio11,
            Some(pins.gpio13),
            &DriverConfig::default().dma(Dma::Auto(4096)),
        ) {
            Ok(driver) => driver,
            Err(err) => {
                log::warn!("SD card probe skipped: SPI bus init failed: {err}");
                return false;
            }
        };

        let sd_host = match SdSpiHostDriver::new(
            spi_driver,
            Some(pins.gpio10),
            AnyIOPin::none(),
            AnyIOPin::none(),
            AnyIOPin::none(),
            None,
        ) {
            Ok(host) => host,
            Err(err) => {
                log::warn!("SD card probe skipped: SD host init failed: {err}");
                return false;
            }
        };

        let sd_card_driver = match SdCardDriver::new_spi(sd_host, &SdCardConfiguration::new()) {
            Ok(card) => card,
            Err(err) => {
                log::warn!("SD card probe skipped: SD card init failed: {err}");
                return false;
            }
        };

        let mounted = match Fatfs::new_sdcard(0, sd_card_driver) {
            Ok(fatfs) => match MountedFatfs::mount(fatfs, "/sdcard", 4) {
                Ok(mounted) => mounted,
                Err(err) => {
                    log::warn!("SD card probe skipped: SD FATFS mount failed: {err}");
                    return false;
                }
            },
            Err(err) => {
                log::warn!("SD card probe skipped: FATFS driver init failed: {err}");
                return false;
            }
        };

        drop(mounted);
        log::info!("Real SD card mount probe succeeded on /sdcard");
        true
    }
}

fn render_mode_screen<D>(
    display: &mut D,
    mode: DeviceMode,
    connection: &crate::network::WifiConnection,
    status: &SystemStatus,
    text_style: embedded_graphics::mono_font::MonoTextStyle<'_, BinaryColor>,
) -> Result<(), String>
where
    D: DrawTarget<Color = BinaryColor>,
    D::Error: core::fmt::Debug,
{
    display
        .clear(BinaryColor::Off)
        .map_err(|_| "failed to clear OLED display".to_string())?;

    match mode {
        DeviceMode::Status => {
            Text::new(mode.as_str(), Point::new(2, 5), text_style)
                .draw(display)
                .map_err(|_| "failed to draw STATUS_MODE label".to_string())?;

            let lines = status.summary_lines();
            Text::new(lines[0].as_str(), Point::new(2, 17), text_style)
                .draw(display)
                .map_err(|_| "failed to draw first status row".to_string())?;
            Text::new(lines[1].as_str(), Point::new(2, 27), text_style)
                .draw(display)
                .map_err(|_| "failed to draw second status row".to_string())?;
        }
        DeviceMode::Live => {
            Text::new(mode.as_str(), Point::new(2, 5), text_style)
                .draw(display)
                .map_err(|_| "failed to draw LIVE_MODE label".to_string())?;

            let ip_line = format!("IP {}", connection.ip);
            Text::new(ip_line.as_str(), Point::new(2, 19), text_style)
                .draw(display)
                .map_err(|_| "failed to draw live IP line".to_string())?;
        }
        DeviceMode::Sd => {
            Text::new(mode.as_str(), Point::new(2, 5), text_style)
                .draw(display)
                .map_err(|_| "failed to draw SD_MODE label".to_string())?;

            let sd_line = if status.sd { "SD writable" } else { "SD unavailable" };
            Text::new(sd_line, Point::new(2, 19), text_style)
                .draw(display)
                .map_err(|_| "failed to draw SD_MODE detail".to_string())?;
        }
    }

    Ok(())
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

pub struct App;

impl App {
    pub fn new() -> Self {
        Self
    }

    pub fn run(&self) -> Result<(), String> {
        let wifi = wifi_credentials().map_err(|error| {
            log::error!("WiFi credentials are not configured: {}", error);
            format!("configuration error: {}", error)
        })?;

        let config = Config {
            wifi,
            hardware: crate::config::HardwareBaseline::project_spec(),
            architecture: crate::config::RuntimeArchitecture::project_spec(),
        };

        config.validate().map_err(|error| {
            log::error!("Configuration validation failed: {}", error);
            format!("configuration validation error: {}", error)
        })?;

        let peripherals = Peripherals::take().map_err(|err| format!("failed to take hardware peripherals: {err}"))?;
        let Peripherals { modem, pins, i2c0, .. } = peripherals;
        let sys_loop = esp_idf_svc::eventloop::EspSystemEventLoop::take()
            .map_err(|err| format!("failed to take system event loop: {err}"))?;
        let nvs = esp_idf_svc::nvs::EspDefaultNvsPartition::take()
            .map_err(|err| format!("failed to take NVS partition: {err}"))?;

        let connection = WifiConnection::connect_with_modem(modem, &config.wifi.ssid, &config.wifi.password, sys_loop.clone(), nvs)
            .map_err(|error| {
                log::error!("WiFi connection failed: {}", error);
                error
            })?;

        let mut i2c = I2cDriver::new(i2c0, pins.gpio8, pins.gpio9, &I2cConfig::new().baudrate(100_u32.kHz().into()))
            .map_err(|err| format!("failed to configure I2C bus for SSD1306 display: {err}"))?;

        probe_oled_display(&mut i2c).map_err(|err| format!("OLED hardware validation failed: {err}"))?;

        let interface = I2CDisplayInterface::new(i2c);
        let mut display = Ssd1306::new(interface, DisplaySize128x32, DisplayRotation::Rotate0)
            .into_buffered_graphics_mode();
        display
            .init()
            .map_err(|err| format!("failed to initialize SSD1306 OLED display: {:?}", err))?;

        let mode_button = PinDriver::input(pins.gpio5, Pull::Up)
            .map_err(|err| format!("failed to configure mode button on GPIO 5: {err}"))?;

        let mut current_mode = DeviceMode::Status;
        let mut last_pressed = false;

        let text_style = MonoTextStyleBuilder::new()
            .font(&FONT_6X10)
            .text_color(BinaryColor::On)
            .build();

        let mut system_status = SystemStatus::from_runtime(connection.connected);
        render_mode_screen(&mut display, current_mode, &connection, &system_status, text_style)?;
        display
            .flush()
            .map_err(|err| format!("failed to flush initial OLED screen: {:?}", err))?;

        log::info!("WiFi credentials loaded from the embedded credential module.");
        log::info!("SSID configured: {}", config.wifi.ssid);
        log::info!("WiFi connection established: {}", connection.status_line);
        log::info!("DHCP lease assigned to this node: {}", connection.ip);
        log::info!("Initial device mode: {}", current_mode.as_str());
        log::info!("Display status: {}", connection.screen_status());
        log::info!("Mode button state machine: {} -> {} -> {}", DeviceMode::Status.as_str(), DeviceMode::Live.as_str(), DeviceMode::Sd.as_str());
        log::info!("Mode button is active-low and configured on GPIO 5 with internal pull-up enabled.");

        loop {
            let pressed = mode_button.is_low();
            if pressed && !last_pressed {
                let previous_mode = current_mode;
                current_mode = current_mode.next();
                log::info!(
                    "Mode switch on GPIO 5: {} -> {}",
                    previous_mode.as_str(),
                    current_mode.as_str()
                );
            }
            last_pressed = pressed;

            system_status = SystemStatus::from_runtime(connection.connected);
            render_mode_screen(&mut display, current_mode, &connection, &system_status, text_style)?;
            display
                .flush()
                .map_err(|err| format!("failed to flush OLED mode update: {:?}", err))?;

            if !pressed {
                log::info!(
                    "Mode {} status: WiFi connected to {} via DHCP {}",
                    current_mode.as_str(),
                    connection.ssid,
                    connection.ip
                );
            }

            thread::sleep(Duration::from_millis(50));
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn live_audio_stream_generates_browser_ready_frames() {
        let mut stream = super::LiveAudioStream::new();
        let first_frame = stream.capture_frame();

        assert_eq!(first_frame.sample_rate, 16_000);
        assert_eq!(first_frame.channels, 1);
        assert!(!first_frame.samples.is_empty());
        assert!(stream.browser_page().contains("/live-stream"));
        assert!(stream.browser_page().contains("audio"));
    }

    #[test]
    fn status_health_summary_reports_each_device_state() {
        let status = super::SystemStatus {
            wifi: true,
            mic: true,
            pir: false,
            sd: true,
        };

        let summary = status.summary_lines();

        assert!(summary[0].contains("wifi OK"));
        assert!(summary[0].contains("mic OK"));
        assert!(summary[1].contains("pir KO"));
        assert!(summary[1].contains("sd OK"));
    }

    #[test]
    fn wifi_connection_reports_connection_and_dhcp_ip() {
        let connection = super::WifiConnection::connect("AudioNodeLab", "secret-pass").unwrap();

        assert_eq!(connection.ssid, "AudioNodeLab");
        assert!(connection.connected);
        assert!(connection.ip.contains('.'));
        assert!(connection.status_line.contains("AudioNodeLab"));
        assert!(connection.status_line.contains(connection.ip.as_str()));
    }

    #[test]
    fn mode_button_cycles_through_status_live_and_sd() {
        let mut mode = super::DeviceMode::Status;

        mode = mode.next();
        assert_eq!(mode, super::DeviceMode::Live);

        mode = mode.next();
        assert_eq!(mode, super::DeviceMode::Sd);

        mode = mode.next();
        assert_eq!(mode, super::DeviceMode::Status);
    }
}
