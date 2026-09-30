use std::thread;
use std::time::Duration;

use esp_idf_svc::hal::gpio::{PinDriver, Pull};
use esp_idf_svc::hal::i2c::{I2cConfig, I2cDriver};
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::hal::units::*;

use crate::config::Config;
use crate::display::OledDisplay;
use crate::modes::{DeviceMode, ModeButtonAction, ModeButtonController};
use crate::network::{wifi_credentials, WifiConnection};
use crate::sd::{SdCard, SdCardStatus};
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
            i2s0,
            spi3,
            adc1,
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

        let i2c = I2cDriver::new(
            i2c0,
            pins.gpio8,
            pins.gpio9,
            &I2cConfig::new().baudrate(100_u32.kHz().into()),
        )
        .map_err(|err| format!("failed to configure I2C bus for SSD1306 display: {err}"))?;

        let mut display = OledDisplay::init(i2c)?;

        #[cfg(target_arch = "xtensa")]
        let sd_card = SdCard::mount(
            spi3,
            pins.gpio12,
            pins.gpio11,
            pins.gpio13,
            pins.gpio10,
        );

        #[cfg(not(target_arch = "xtensa"))]
        let sd_card: Result<SdCard, String> = Ok(SdCard);

        let mut sd_status = match &sd_card {
            Ok(card) => {
                log::info!("MicroSD card mounted on /sdcard");
                card.inspect()
            }
            Err(err) => {
                log::warn!("MicroSD card mount skipped or failed: {err}");
                SdCardStatus::Unavailable(err.clone())
            }
        };

        let audio_buffer = crate::audio::SharedAudioBuffer::new(50);

        #[cfg(target_arch = "xtensa")]
        let mut potentiometer = crate::potentiometer::Potentiometer::new(adc1, pins.gpio4);

        #[cfg(not(target_arch = "xtensa"))]
        let mut potentiometer = crate::potentiometer::Potentiometer::new();

        let initial_gain = match &mut potentiometer {
            Ok(p) => p.last_gain(),
            Err(err) => {
                log::warn!("Potentiometer initialization failed: {err}");
                crate::potentiometer::DEFAULT_MAX_GAIN
            }
        };
        let initial_gain_percent = match &mut potentiometer {
            Ok(p) => p.last_gain_percent(),
            Err(_) => 100,
        };
        audio_buffer.set_gain(initial_gain, initial_gain_percent);

        #[cfg(target_arch = "xtensa")]
        let mut mic = crate::audio::Microphone::new(
            i2s0,
            pins.gpio14,
            pins.gpio15,
            pins.gpio16,
        );

        #[cfg(not(target_arch = "xtensa"))]
        let mut mic = crate::audio::Microphone::new();

        let initial_mic_status = match &mut mic {
            Ok(m) => m.probe_signal(),
            Err(err) => {
                log::warn!("Microphone initialization skipped or failed: {err}");
                false
            }
        };

        // Start real-time audio capture worker on Core 1 if microphone is initialized
        let capture_buffer = audio_buffer.clone();
        let _capture_thread = if let Ok(mut active_mic) = mic {
            #[cfg(target_arch = "xtensa")]
            {
                use esp_idf_svc::hal::cpu::Core;
                use esp_idf_svc::hal::task::thread::ThreadSpawnConfiguration;
                let thread_config = ThreadSpawnConfiguration {
                    name: Some(c"audio-capture"),
                    stack_size: 8192,
                    priority: 15,
                    pin_to_core: Some(Core::Core1),
                    ..Default::default()
                };
                let _ = thread_config.set();
            }

            Some(
                std::thread::Builder::new()
                    .name("audio-capture".into())
                    .stack_size(8192)
                    .spawn(move || {
                        log::info!("Audio capture worker thread started on Core 1");
                        loop {
                            // Poll the physical potentiometer every frame so its value is always
                            // fresh and ready to be applied the moment the web override is lifted.
                            if let Ok(ref mut pot) = potentiometer {
                                match pot.read_gain() {
                                    Ok(g) => {
                                        let pct = pot.last_gain_percent();
                                        capture_buffer.set_gain(g, pct);
                                    }
                                    Err(err) => {
                                        log::warn!("Potentiometer read error: {err}");
                                    }
                                }
                            }

                            // Active gain respects the web override if enabled, else uses potentiometer
                            let current_gain = capture_buffer.current_gain();

                            match active_mic.read_frame_with_gain(320, 50, current_gain) {
                                Ok(frame) => {
                                    capture_buffer.push_frame(frame);
                                }
                                Err(err) => {
                                    log::warn!("Microphone read error: {err}");
                                    thread::sleep(Duration::from_millis(20));
                                }
                            }
                        }
                    })
                    .map_err(|err| format!("Failed to spawn audio capture thread: {err}")),
            )
        } else {
            None
        };

        // Start live audio HTTP server on Core 0
        let _http_server = crate::audio::LiveAudioStream::start_server(audio_buffer.clone(), 80);
        match &_http_server {
            Ok(_) => {
                log::info!(
                    "Live audio streaming server running at http://{}/ (and /stream.wav)",
                    connection.ip
                );
            }
            Err(err) => {
                log::warn!("Live audio streaming server failed to start: {err}");
            }
        }

        let pir_sensor = PinDriver::input(pins.gpio3, Pull::Down)
            .map_err(|err| format!("failed to configure PIR sensor on GPIO 3: {err}"))?;
        let initial_pir_status = pir_sensor.is_high();

        let mode_button = PinDriver::input(pins.gpio5, Pull::Up)
            .map_err(|err| format!("failed to configure mode button on GPIO 5: {err}"))?;
        let record_button = PinDriver::input(pins.gpio6, Pull::Up)
            .map_err(|err| format!("failed to configure record button on GPIO 6: {err}"))?;
        let other_button = PinDriver::input(pins.gpio7, Pull::Up)
            .map_err(|err| format!("failed to configure other button on GPIO 7: {err}"))?;

        let mut current_mode = DeviceMode::Status;
        let mut button_controller =
            ModeButtonController::new(ModeButtonController::DEFAULT_LONG_PRESS_DURATION);

        let mut button2_was_pressed = false;
        let mut button3_was_pressed = false;

        let mut system_status = SystemStatus::from_runtime_with_sensors(
            connection.connected,
            initial_mic_status,
            initial_pir_status,
            sd_status.clone(),
            initial_gain_percent,
        );
        display.render(current_mode, &connection, &system_status)?;

        log::info!("WiFi credentials loaded from the embedded credential module.");
        log::info!("SSID configured: {}", config.wifi.ssid);
        log::info!("WiFi connection established: {}", connection.status_line);
        log::info!("DHCP lease assigned to this node: {}", connection.ip);
        log::info!("Initial device mode: {}", current_mode.as_str());
        log::info!("Display status: {}", connection.screen_status());
        log::info!(
            "Mode button state machine: {} -> {} -> {} (short press cycles mode, long press toggles display power)",
            DeviceMode::Status.as_str(),
            DeviceMode::Live.as_str(),
            DeviceMode::Sd.as_str()
        );
        log::info!("Mode button is active-low and configured on GPIO 5 with internal pull-up enabled.");
        log::info!("Record button (Button 2) configured on GPIO 6 with internal pull-up enabled.");
        log::info!("Other button (Button 3) configured on GPIO 7 with internal pull-up enabled.");
        log::info!("PIR sensor input configured on GPIO 3 with pull-down enabled.");
        log::info!("Potentiometer configured on GPIO 4 (ADC1_CH3) controlling microphone gain in real time.");

        loop {
            let pressed = mode_button.is_low();
            match button_controller.update(pressed, display.is_on()) {
                ModeButtonAction::CycleMode => {
                    let previous_mode = current_mode;
                    current_mode = current_mode.next();
                    log::info!(
                        "Mode switch on GPIO 5: {} -> {}",
                        previous_mode.as_str(),
                        current_mode.as_str()
                    );
                }
                ModeButtonAction::TurnScreenOff => {
                    log::info!("Button 1 long press: turning OLED display off");
                    display.set_power(false)?;
                }
                ModeButtonAction::TurnScreenOn => {
                    log::info!("Button 1 press: turning OLED display on");
                    display.set_power(true)?;
                }
                ModeButtonAction::None => {}
            }

            let b2_pressed = record_button.is_low();
            if b2_pressed && !button2_was_pressed {
                log::info!("Button 2 (GPIO 6) pressed: Record button triggered");
            }
            button2_was_pressed = b2_pressed;

            let b3_pressed = other_button.is_low();
            if b3_pressed && !button3_was_pressed {
                log::info!("Button 3 (GPIO 7) pressed: Other button triggered");
            }
            button3_was_pressed = b3_pressed;

            let pir_detected = pir_sensor.is_high();
            let mic_detected = audio_buffer.is_signal_present();
            let gain_percent = audio_buffer.current_gain_percent();

            if display.is_on() {
                if current_mode == DeviceMode::Sd {
                    if let Ok(card) = &sd_card {
                        sd_status = card.inspect();
                    }
                }
                system_status = SystemStatus::from_runtime_with_sensors(
                    connection.connected,
                    mic_detected,
                    pir_detected,
                    sd_status.clone(),
                    gain_percent,
                );
                display.render(current_mode, &connection, &system_status)?;
            }

            if !pressed {
                log::info!(
                    "Mode {} status (display {}): WiFi: {}, Mic: {}, PIR: {}, SD: {}, Gain: {}%",
                    current_mode.as_str(),
                    if display.is_on() { "ON" } else { "OFF" },
                    if connection.connected { "OK" } else { "KO" },
                    if mic_detected { "OK" } else { "KO" },
                    if pir_detected { "ACTIVE" } else { "IDLE" },
                    if sd_status.is_mounted() { "OK" } else { "KO" },
                    gain_percent,
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
