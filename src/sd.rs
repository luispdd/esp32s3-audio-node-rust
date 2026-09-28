use esp_idf_svc::fs::fatfs::Fatfs;
use esp_idf_svc::hal::gpio::AnyIOPin;
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::hal::sd::{spi::SdSpiHostDriver, SdCardConfiguration, SdCardDriver};
use esp_idf_svc::hal::spi::{config::DriverConfig, Dma, SpiDriver};
use esp_idf_svc::io::vfs::MountedFatfs;

pub fn check_sd_card_is_writable() -> bool {
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
