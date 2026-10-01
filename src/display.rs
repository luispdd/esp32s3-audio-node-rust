use embedded_graphics::{
    draw_target::DrawTarget,
    mono_font::{ascii::FONT_6X10, MonoTextStyle, MonoTextStyleBuilder},
    pixelcolor::BinaryColor,
    prelude::Point,
    text::{Baseline, Text},
    Drawable,
};
use esp_idf_svc::hal::delay::BLOCK;
use esp_idf_svc::hal::i2c::I2cDriver;
use ssd1306::{
    mode::BufferedGraphicsMode, prelude::*, I2CDisplayInterface, Ssd1306,
};

use crate::modes::DeviceMode;
use crate::network::WifiConnection;
use crate::status::SystemStatus;

pub const MARGIN_LEFT: i32 = 2;
pub const MARGIN_TOP: i32 = 1;

pub type Ssd1306Driver<'a> = Ssd1306<
    I2CInterface<I2cDriver<'a>>,
    DisplaySize128x32,
    BufferedGraphicsMode<DisplaySize128x32>,
>;

/// Extensible OLED screen manager encapsulating SSD1306 hardware lifecycle,
/// power management, and mode-specific layout rendering.
pub struct OledDisplay<'a> {
    display: Ssd1306Driver<'a>,
    text_style: MonoTextStyle<'static, BinaryColor>,
    is_on: bool,
}

impl<'a> OledDisplay<'a> {
    /// Probes and initializes the SSD1306 OLED display over I2C.
    pub fn init(mut i2c: I2cDriver<'a>) -> Result<Self, String> {
        probe_oled_display(&mut i2c)
            .map_err(|err| format!("OLED hardware validation failed: {err}"))?;

        let interface = I2CDisplayInterface::new(i2c);
        let mut display = Ssd1306::new(interface, DisplaySize128x32, DisplayRotation::Rotate0)
            .into_buffered_graphics_mode();

        display
            .init()
            .map_err(|err| format!("failed to initialize SSD1306 OLED display: {:?}", err))?;

        display
            .set_display_on(true)
            .map_err(|err| format!("failed to turn on OLED display: {:?}", err))?;

        let text_style = default_text_style();

        Ok(Self {
            display,
            text_style,
            is_on: true,
        })
    }

    /// Returns whether the display is currently powered on.
    pub fn is_on(&self) -> bool {
        self.is_on
    }

    /// Toggles or sets display power state (sleep / wake).
    pub fn set_power(&mut self, on: bool) -> Result<(), String> {
        self.display
            .set_display_on(on)
            .map_err(|err| format!("failed to set OLED display power state to {on}: {:?}", err))?;
        self.is_on = on;
        Ok(())
    }

    /// Renders the current mode screen and flushes buffer to the physical panel.
    /// If the display is powered off, rendering is skipped to save bus traffic.
    pub fn render(
        &mut self,
        mode: DeviceMode,
        connection: &WifiConnection,
        status: &SystemStatus,
    ) -> Result<(), String> {
        if !self.is_on {
            return Ok(());
        }

        render_mode_screen(
            &mut self.display,
            mode,
            connection,
            status,
            self.text_style,
        )?;

        self.display
            .flush()
            .map_err(|err| format!("failed to flush OLED screen: {:?}", err))?;

        Ok(())
    }
}

pub fn default_text_style() -> MonoTextStyle<'static, BinaryColor> {
    MonoTextStyleBuilder::new()
        .font(&FONT_6X10)
        .text_color(BinaryColor::On)
        .build()
}

pub fn set_display_power<DI, SIZE, MODE>(
    display: &mut Ssd1306<DI, SIZE, MODE>,
    on: bool,
) -> Result<(), String>
where
    DI: WriteOnlyDataCommand,
    SIZE: DisplaySize,
{
    display
        .set_display_on(on)
        .map_err(|err| format!("failed to set OLED display power state to {on}: {:?}", err))
}

pub fn probe_oled_display(i2c: &mut I2cDriver<'_>) -> Result<(), String> {
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

pub fn render_mode_screen<D>(
    display: &mut D,
    mode: DeviceMode,
    connection: &WifiConnection,
    status: &SystemStatus,
    text_style: MonoTextStyle<'_, BinaryColor>,
) -> Result<(), String>
where
    D: DrawTarget<Color = BinaryColor>,
    D::Error: core::fmt::Debug,
{
    display
        .clear(BinaryColor::Off)
        .map_err(|_| "failed to clear OLED display".to_string())?;

    match mode {
        DeviceMode::Status => render_status_screen(display, status, text_style),
        DeviceMode::Live => render_live_screen(display, connection, status, text_style),
        DeviceMode::Sd => render_sd_screen(display, status, text_style),
        DeviceMode::Pir => render_pir_screen(display, status, text_style),
    }
}

pub fn render_status_screen<D>(
    display: &mut D,
    status: &SystemStatus,
    text_style: MonoTextStyle<'_, BinaryColor>,
) -> Result<(), String>
where
    D: DrawTarget<Color = BinaryColor>,
    D::Error: core::fmt::Debug,
{
    Text::with_baseline(
        DeviceMode::Status.as_str(),
        Point::new(MARGIN_LEFT, MARGIN_TOP),
        text_style,
        Baseline::Top,
    )
    .draw(display)
    .map_err(|_| "failed to draw STATUS_MODE label".to_string())?;

    let lines = status.summary_lines();
    Text::with_baseline(
        lines[0].as_str(),
        Point::new(MARGIN_LEFT, MARGIN_TOP + 10),
        text_style,
        Baseline::Top,
    )
    .draw(display)
    .map_err(|_| "failed to draw first status row".to_string())?;

    Text::with_baseline(
        lines[1].as_str(),
        Point::new(MARGIN_LEFT, MARGIN_TOP + 20),
        text_style,
        Baseline::Top,
    )
    .draw(display)
    .map_err(|_| "failed to draw second status row".to_string())?;

    Ok(())
}

pub fn render_live_screen<D>(
    display: &mut D,
    connection: &WifiConnection,
    status: &SystemStatus,
    text_style: MonoTextStyle<'_, BinaryColor>,
) -> Result<(), String>
where
    D: DrawTarget<Color = BinaryColor>,
    D::Error: core::fmt::Debug,
{
    Text::with_baseline(
        DeviceMode::Live.as_str(),
        Point::new(MARGIN_LEFT, MARGIN_TOP),
        text_style,
        Baseline::Top,
    )
    .draw(display)
    .map_err(|_| "failed to draw LIVE_MODE label".to_string())?;

    let ip_line = format!("IP {}", connection.ip);
    Text::with_baseline(
        ip_line.as_str(),
        Point::new(MARGIN_LEFT, MARGIN_TOP + 10),
        text_style,
        Baseline::Top,
    )
    .draw(display)
    .map_err(|_| "failed to draw live IP line".to_string())?;

    let noise_line = format!("Noise: {}%  Gain: {}%", status.noise_level, status.gain_percent);
    Text::with_baseline(
        noise_line.as_str(),
        Point::new(MARGIN_LEFT, MARGIN_TOP + 20),
        text_style,
        Baseline::Top,
    )
    .draw(display)
    .map_err(|_| "failed to draw live noise line".to_string())?;

    Ok(())
}

use crate::sd::SdCardStatus;

pub fn truncate_display_line(text: &str, max_len: usize) -> String {
    if text.chars().count() <= max_len {
        text.to_string()
    } else if max_len > 3 {
        let truncated: String = text.chars().take(max_len - 3).collect();
        format!("{truncated}...")
    } else {
        text.chars().take(max_len).collect()
    }
}

pub fn render_sd_screen<D>(
    display: &mut D,
    status: &SystemStatus,
    text_style: MonoTextStyle<'_, BinaryColor>,
) -> Result<(), String>
where
    D: DrawTarget<Color = BinaryColor>,
    D::Error: core::fmt::Debug,
{
    let (header, line1, line2) = if let Some(ref rec) = status.recording {
        let mins = rec.duration_secs / 60;
        let secs = rec.duration_secs % 60;
        (
            format!("{} [REC]", DeviceMode::Sd.as_str()),
            format!("{:02}:{:02} ({}f)", mins, secs, rec.frames_recorded),
            truncate_display_line(&rec.filename, 20),
        )
    } else if let Some(ref pb) = status.playback {
        let cur_m = pb.duration_secs / 60;
        let cur_s = pb.duration_secs % 60;
        let tot_m = pb.total_secs / 60;
        let tot_s = pb.total_secs % 60;
        (
            format!("{} [PLAY]", DeviceMode::Sd.as_str()),
            format!("{:02}:{:02} / {:02}:{:02}", cur_m, cur_s, tot_m, tot_s),
            truncate_display_line(&format!("*{}", pb.filename), 20),
        )
    } else {
        match &status.sd_card {
            SdCardStatus::Unavailable(_) => (
                DeviceMode::Sd.as_str().to_string(),
                "SD unavailable".to_string(),
                "Card not mounted".to_string(),
            ),
        SdCardStatus::FolderCreateFailed(err) => (
            DeviceMode::Sd.as_str().to_string(),
            "/audio create fail".to_string(),
            truncate_display_line(err, 20),
        ),
        SdCardStatus::FolderMissing => (
            DeviceMode::Sd.as_str().to_string(),
            "/audio missing".to_string(),
            "Folder not found".to_string(),
        ),
        SdCardStatus::Empty => (
            format!("{} (1/1)", DeviceMode::Sd.as_str()),
            "* [Record new]".to_string(),
            " (No files)".to_string(),
        ),
        SdCardStatus::Files(files) => {
            if files.is_empty() {
                (
                    format!("{} (1/1)", DeviceMode::Sd.as_str()),
                    "* [Record new]".to_string(),
                    " (No files)".to_string(),
                )
            } else {
                let total_items = files.len() + 1;
                let selected = status.selected_file_index.min(total_items.saturating_sub(1));
                let header = format!("{} ({}/{})", DeviceMode::Sd.as_str(), selected + 1, total_items);
                let (line1, line2) = if selected == 0 {
                    (
                        "* [Record new]".to_string(),
                        format!(" {}", files[0]),
                    )
                } else {
                    let file_idx = selected - 1;
                    let l1 = format!("*{}", files[file_idx]);
                    let l2 = if file_idx + 1 < files.len() {
                        format!(" {}", files[file_idx + 1])
                    } else {
                        " [Record new]".to_string()
                    };
                    (l1, l2)
                };
                (
                    header,
                    truncate_display_line(&line1, 21),
                    truncate_display_line(&line2, 21),
                )
            }
        }
    }
    };

    Text::with_baseline(
        header.as_str(),
        Point::new(MARGIN_LEFT, MARGIN_TOP),
        text_style,
        Baseline::Top,
    )
    .draw(display)
    .map_err(|_| "failed to draw SD_MODE label".to_string())?;

    Text::with_baseline(
        line1.as_str(),
        Point::new(MARGIN_LEFT, MARGIN_TOP + 10),
        text_style,
        Baseline::Top,
    )
    .draw(display)
    .map_err(|_| "failed to draw SD_MODE detail line 1".to_string())?;

    Text::with_baseline(
        line2.as_str(),
        Point::new(MARGIN_LEFT, MARGIN_TOP + 20),
        text_style,
        Baseline::Top,
    )
    .draw(display)
    .map_err(|_| "failed to draw SD_MODE detail line 2".to_string())?;

    Ok(())
}

pub fn render_pir_screen<D>(
    display: &mut D,
    status: &SystemStatus,
    text_style: MonoTextStyle<'_, BinaryColor>,
) -> Result<(), String>
where
    D: DrawTarget<Color = BinaryColor>,
    D::Error: core::fmt::Debug,
{
    let noise = status.noise_level;
    let (header, line1, line2) = if let Some(ref rec) = status.recording {
        let mins = rec.duration_secs / 60;
        let secs = rec.duration_secs % 60;
        let rem_str = if let Some(rem) = status.pir_mode.recording_remaining_secs {
            format!("{rem}s rem")
        } else {
            format!("{:02}:{:02}", mins, secs)
        };
        let act_tag = match (status.pir, status.pir_mode.sound_detected) {
            (true, true) => "M+S",
            (true, false) => "MOT",
            (false, true) => "SND",
            (false, false) => "---",
        };
        (
            format!("{} [REC]", DeviceMode::Pir.as_str()),
            format!("{rem_str} [{act_tag}] N:{noise}%"),
            truncate_display_line(&rec.filename, 20),
        )
    } else if let Some(countdown) = status.pir_mode.arming_countdown {
        (
            format!("{} [ARMING]", DeviceMode::Pir.as_str()),
            format!("Arm in: {countdown}s"),
            format!("Noise: {noise}% (ign)"),
        )
    } else if status.pir_mode.armed {
        let motion_line = if status.pir {
            "Motion: DETECTED"
        } else {
            "Motion: CLEAR"
        };
        let active_line = if status.pir_mode.sound_detected {
            format!("Act [SND] Noise:{noise}%")
        } else {
            format!("Active  Noise:{noise}%")
        };
        (
            format!("{} [ARMED]", DeviceMode::Pir.as_str()),
            motion_line.to_string(),
            active_line,
        )
    } else {
        let pir_state = if status.pir {
            "PIR: DETECTED"
        } else {
            "PIR: CLEAR"
        };
        (
            format!("{} [IDLE]", DeviceMode::Pir.as_str()),
            format!("{pir_state}  N:{noise}%"),
            "Btn2: Arm (10s delay)".to_string(),
        )
    };

    Text::with_baseline(
        header.as_str(),
        Point::new(MARGIN_LEFT, MARGIN_TOP),
        text_style,
        Baseline::Top,
    )
    .draw(display)
    .map_err(|_| "failed to draw PIR_MODE label".to_string())?;

    Text::with_baseline(
        line1.as_str(),
        Point::new(MARGIN_LEFT, MARGIN_TOP + 10),
        text_style,
        Baseline::Top,
    )
    .draw(display)
    .map_err(|_| "failed to draw PIR_MODE detail line 1".to_string())?;

    Text::with_baseline(
        line2.as_str(),
        Point::new(MARGIN_LEFT, MARGIN_TOP + 20),
        text_style,
        Baseline::Top,
    )
    .draw(display)
    .map_err(|_| "failed to draw PIR_MODE detail line 2".to_string())?;

    Ok(())
}

#[cfg(test)]
#[allow(dead_code, unused_imports)]
mod tests {
    use super::*;
    use embedded_graphics::mock_display::MockDisplay;

    fn mock_wifi() -> WifiConnection {
        WifiConnection {
            ip: "192.168.1.100".to_string(),
            ssid: "TestSSID".to_string(),
            connected: true,
            status_line: "WiFi connected".to_string(),
        }
    }

    #[test]
    fn renders_status_mode_screen() {
        let mut display: MockDisplay<BinaryColor> = MockDisplay::new();
        display.set_allow_overdraw(true);
        display.set_allow_out_of_bounds_drawing(true);
        let wifi = mock_wifi();
        let status = SystemStatus::from_runtime(true);
        let style = default_text_style();

        assert!(render_status_screen(&mut display, &status, style).is_ok());
        assert!(render_mode_screen(&mut display, DeviceMode::Status, &wifi, &status, style).is_ok());
    }

    #[test]
    fn renders_status_mode_screen_with_potentiometer_gain() {
        let mut display: MockDisplay<BinaryColor> = MockDisplay::new();
        display.set_allow_overdraw(true);
        display.set_allow_out_of_bounds_drawing(true);
        let wifi = mock_wifi();
        let status = SystemStatus::from_runtime_with_sensors(true, true, true, SdCardStatus::Empty, 42);
        let style = default_text_style();

        assert!(render_status_screen(&mut display, &status, style).is_ok());
        assert!(render_mode_screen(&mut display, DeviceMode::Status, &wifi, &status, style).is_ok());
    }

    #[test]
    fn renders_live_mode_screen() {
        let mut display: MockDisplay<BinaryColor> = MockDisplay::new();
        display.set_allow_overdraw(true);
        display.set_allow_out_of_bounds_drawing(true);
        let wifi = mock_wifi();
        let status = SystemStatus::from_runtime(true).with_noise_level(30, 25);
        let style = default_text_style();

        assert!(render_live_screen(&mut display, &wifi, &status, style).is_ok());
        assert!(render_mode_screen(&mut display, DeviceMode::Live, &wifi, &status, style).is_ok());
    }

    #[test]
    fn renders_sd_mode_screen_all_states() {
        let mut display: MockDisplay<BinaryColor> = MockDisplay::new();
        display.set_allow_overdraw(true);
        display.set_allow_out_of_bounds_drawing(true);
        let wifi = mock_wifi();
        let style = default_text_style();

        // 1. Unavailable
        let status_unavail =
            SystemStatus::from_runtime_with_sd(true, SdCardStatus::Unavailable("missing".into()));
        assert!(render_sd_screen(&mut display, &status_unavail, style).is_ok());
        assert!(render_mode_screen(
            &mut display,
            DeviceMode::Sd,
            &wifi,
            &status_unavail,
            style
        )
        .is_ok());

        // 2. Folder missing
        let status_folder_missing =
            SystemStatus::from_runtime_with_sd(true, SdCardStatus::FolderMissing);
        assert!(render_sd_screen(&mut display, &status_folder_missing, style).is_ok());

        // 3. Folder creation failed
        let status_folder_create_failed = SystemStatus::from_runtime_with_sd(
            true,
            SdCardStatus::FolderCreateFailed("mkdir failed: disk full".to_string()),
        );
        assert!(render_sd_screen(&mut display, &status_folder_create_failed, style).is_ok());

        // 3. Empty
        let status_empty = SystemStatus::from_runtime_with_sd(true, SdCardStatus::Empty);
        assert!(render_sd_screen(&mut display, &status_empty, style).is_ok());

        // 4. Single file
        let status_single = SystemStatus::from_runtime_with_sd(
            true,
            SdCardStatus::Files(vec!["rec001.wav".to_string()]),
        );
        assert!(render_sd_screen(&mut display, &status_single, style).is_ok());

        // 5. Multiple files with truncation
        let status_multi = SystemStatus::from_runtime_with_sd(
            true,
            SdCardStatus::Files(vec![
                "rec_very_long_file_name_2026.wav".to_string(),
                "rec002.wav".to_string(),
                "rec003.wav".to_string(),
            ]),
        );
        // Default index 0 = [Record new]
        assert!(render_sd_screen(&mut display, &status_multi, style).is_ok());

        // Index 1 = first file
        let status_multi_f1 = status_multi.clone().with_selected_file_index(1);
        assert!(render_sd_screen(&mut display, &status_multi_f1, style).is_ok());

        // Index 3 = last file
        let status_multi_last = status_multi.with_selected_file_index(3);
        assert!(render_sd_screen(&mut display, &status_multi_last, style).is_ok());
    }

    #[test]
    fn truncates_long_display_lines() {
        assert_eq!(truncate_display_line("short.wav", 20), "short.wav");
        assert_eq!(
            truncate_display_line("this_is_a_very_long_file_name.wav", 20),
            "this_is_a_very_lo..."
        );
    }

    #[test]
    fn renders_pir_mode_screen_all_states() {
        let mut display: MockDisplay<BinaryColor> = MockDisplay::new();
        display.set_allow_overdraw(true);
        display.set_allow_out_of_bounds_drawing(true);
        let wifi = mock_wifi();
        let style = default_text_style();

        // 1. Idle (motion clear)
        let status_idle = SystemStatus::from_runtime(true);
        assert!(render_pir_screen(&mut display, &status_idle, style).is_ok());
        assert!(render_mode_screen(&mut display, DeviceMode::Pir, &wifi, &status_idle, style).is_ok());

        // 2. Idle (motion detected)
        let status_idle_motion = SystemStatus::from_runtime_with_sensors(true, true, true, SdCardStatus::Empty, 100);
        assert!(render_pir_screen(&mut display, &status_idle_motion, style).is_ok());

        // 3. Arming countdown
        let mut status_arming = SystemStatus::from_runtime(true);
        status_arming.pir_mode.arming_countdown = Some(7);
        assert!(render_pir_screen(&mut display, &status_arming, style).is_ok());

        // 4. Armed monitoring (no motion)
        let mut status_armed_clear = SystemStatus::from_runtime(true);
        status_armed_clear.pir_mode.armed = true;
        assert!(render_pir_screen(&mut display, &status_armed_clear, style).is_ok());

        // 5. Armed monitoring (motion detected)
        let mut status_armed_motion = SystemStatus::from_runtime_with_sensors(true, true, true, SdCardStatus::Empty, 100);
        status_armed_motion.pir_mode.armed = true;
        assert!(render_pir_screen(&mut display, &status_armed_motion, style).is_ok());

        // 6. Recording in progress with remaining seconds
        let mut status_rec = SystemStatus::from_runtime(true).with_noise_level(45, 25);
        status_rec.pir_mode.armed = true;
        status_rec.pir_mode.recording_remaining_secs = Some(18);
        status_rec.recording = Some(crate::audio::ActiveRecordingInfo {
            filename: "20261001_011243.wav".to_string(),
            duration_secs: 2,
            frames_recorded: 100,
        });
        assert!(render_pir_screen(&mut display, &status_rec, style).is_ok());

        // 7. Recording in progress with sound detected (M+S tag)
        status_rec.pir = true;
        status_rec.pir_mode.sound_detected = true;
        assert!(render_pir_screen(&mut display, &status_rec, style).is_ok());

        // 8. Armed monitoring with acoustic activity detected
        let mut status_armed_sound = SystemStatus::from_runtime(true).with_noise_level(60, 25);
        status_armed_sound.pir_mode.armed = true;
        status_armed_sound.pir_mode.sound_detected = true;
        assert!(render_pir_screen(&mut display, &status_armed_sound, style).is_ok());
    }
}
