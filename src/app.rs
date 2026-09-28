use std::thread;
use std::time::Duration;

use embedded_graphics::{
    mono_font::{ascii::FONT_6X10, MonoTextStyleBuilder},
    pixelcolor::BinaryColor,
};
use esp_idf_svc::hal::gpio::{PinDriver, Pull};
use esp_idf_svc::hal::i2c::{I2cConfig, I2cDriver};
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::hal::units::*;
use ssd1306::{prelude::*, I2CDisplayInterface, Ssd1306};

use crate::config::Config;
use crate::display::{probe_oled_display, render_mode_screen};
use crate::modes::DeviceMode;
use crate::network::{wifi_credentials, WifiConnection};
use crate::status::SystemStatus;

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

        let peripherals =
            Peripherals::take().map_err(|err| format!("failed to take hardware peripherals: {err}"))?;
        let Peripherals {
            modem,
            pins,
            i2c0,
            ..
        } = peripherals;
        let sys_loop = esp_idf_svc::eventloop::EspSystemEventLoop::take()
            .map_err(|err| format!("failed to take system event loop: {err}"))?;
        let nvs = esp_idf_svc::nvs::EspDefaultNvsPartition::take()
            .map_err(|err| format!("failed to take NVS partition: {err}"))?;

        let connection = WifiConnection::connect_with_modem(
            modem,
            &config.wifi.ssid,
            &config.wifi.password,
            sys_loop.clone(),
            nvs,
        )
        .map_err(|error| {
            log::error!("WiFi connection failed: {}", error);
            error
        })?;

        let mut i2c = I2cDriver::new(
            i2c0,
            pins.gpio8,
            pins.gpio9,
            &I2cConfig::new().baudrate(100_u32.kHz().into()),
        )
        .map_err(|err| format!("failed to configure I2C bus for SSD1306 display: {err}"))?;

        probe_oled_display(&mut i2c)
            .map_err(|err| format!("OLED hardware validation failed: {err}"))?;

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
        log::info!(
            "Mode button state machine: {} -> {} -> {}",
            DeviceMode::Status.as_str(),
            DeviceMode::Live.as_str(),
            DeviceMode::Sd.as_str()
        );
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

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}
