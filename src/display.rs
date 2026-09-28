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
        DeviceMode::Live => render_live_screen(display, connection, text_style),
        DeviceMode::Sd => render_sd_screen(display, status, text_style),
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
        Point::new(MARGIN_LEFT, MARGIN_TOP + 12),
        text_style,
        Baseline::Top,
    )
    .draw(display)
    .map_err(|_| "failed to draw live IP line".to_string())?;

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
    let (header, line1, line2) = match &status.sd_card {
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
            format!("{} (0)", DeviceMode::Sd.as_str()),
            "/audio empty".to_string(),
            "No files found".to_string(),
        ),
        SdCardStatus::Files(files) => {
            let header = format!("{} ({})", DeviceMode::Sd.as_str(), files.len());
            let line1 = files
                .first()
                .map(|f| truncate_display_line(f, 20))
                .unwrap_or_default();
            let line2 = if files.len() > 1 {
                truncate_display_line(&files[1], 20)
            } else {
                "(end of list)".to_string()
            };
            (header, line1, line2)
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
    fn renders_live_mode_screen() {
        let mut display: MockDisplay<BinaryColor> = MockDisplay::new();
        display.set_allow_overdraw(true);
        display.set_allow_out_of_bounds_drawing(true);
        let wifi = mock_wifi();
        let status = SystemStatus::from_runtime(true);
        let style = default_text_style();

        assert!(render_live_screen(&mut display, &wifi, style).is_ok());
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
            SdCardStatus::Files(vec!["rec001.opus".to_string()]),
        );
        assert!(render_sd_screen(&mut display, &status_single, style).is_ok());

        // 5. Multiple files with truncation
        let status_multi = SystemStatus::from_runtime_with_sd(
            true,
            SdCardStatus::Files(vec![
                "rec_very_long_file_name_2026.opus".to_string(),
                "rec002.opus".to_string(),
                "rec003.opus".to_string(),
            ]),
        );
        assert!(render_sd_screen(&mut display, &status_multi, style).is_ok());
    }

    #[test]
    fn truncates_long_display_lines() {
        assert_eq!(truncate_display_line("short.opus", 20), "short.opus");
        assert_eq!(
            truncate_display_line("this_is_a_very_long_file_name.opus", 20),
            "this_is_a_very_lo..."
        );
    }
}
