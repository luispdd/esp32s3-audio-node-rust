use embedded_graphics::{
    draw_target::DrawTarget,
    mono_font::MonoTextStyle,
    pixelcolor::BinaryColor,
    prelude::Point,
    text::{Baseline, Text},
    Drawable,
};
use esp_idf_svc::hal::delay::BLOCK;
use esp_idf_svc::hal::i2c::I2cDriver;

use crate::modes::DeviceMode;
use crate::network::WifiConnection;
use crate::status::SystemStatus;

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

    const MARGIN_LEFT: i32 = 2;
    const MARGIN_TOP: i32 = 1;

    match mode {
        DeviceMode::Status => {
            Text::with_baseline(
                mode.as_str(),
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
        }
        DeviceMode::Live => {
            Text::with_baseline(
                mode.as_str(),
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
        }
        DeviceMode::Sd => {
            Text::with_baseline(
                mode.as_str(),
                Point::new(MARGIN_LEFT, MARGIN_TOP),
                text_style,
                Baseline::Top,
            )
            .draw(display)
            .map_err(|_| "failed to draw SD_MODE label".to_string())?;

            let sd_line = if status.sd {
                "SD writable"
            } else {
                "SD unavailable"
            };
            Text::with_baseline(
                sd_line,
                Point::new(MARGIN_LEFT, MARGIN_TOP + 12),
                text_style,
                Baseline::Top,
            )
            .draw(display)
            .map_err(|_| "failed to draw SD_MODE detail".to_string())?;
        }
    }

    Ok(())
}
