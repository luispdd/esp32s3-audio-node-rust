use std::path::Path;

#[cfg(target_arch = "xtensa")]
use esp_idf_svc::fs::fatfs::Fatfs;
#[cfg(target_arch = "xtensa")]
use esp_idf_svc::hal::gpio::{AnyIOPin, Gpio10, Gpio11, Gpio12, Gpio13};
#[cfg(target_arch = "xtensa")]
use esp_idf_svc::hal::sd::{spi::SdSpiHostDriver, SdCardConfiguration, SdCardDriver};
#[cfg(target_arch = "xtensa")]
use esp_idf_svc::hal::spi::{config::DriverConfig, Dma, SpiDriver, SPI3};
#[cfg(target_arch = "xtensa")]
use esp_idf_svc::io::vfs::MountedFatfs;

pub const SD_MOUNT_POINT: &str = "/sdcard";
pub const SD_AUDIO_DIR: &str = "/sdcard/audio";

/// Represents the high-level state of the SD card and its `/audio` directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SdCardStatus {
    /// SD card is missing or could not be mounted.
    Unavailable(String),
    /// SD card is mounted, but the `/audio` directory could not be created.
    FolderCreateFailed(String),
    /// SD card is mounted, but the `/audio` directory does not exist.
    FolderMissing,
    /// SD card is mounted and `/audio` exists, but has no recorded files.
    Empty,
    /// SD card is mounted, `/audio` exists, and contains recorded files.
    Files(Vec<String>),
}

impl SdCardStatus {
    pub fn is_mounted(&self) -> bool {
        !matches!(self, Self::Unavailable(_))
    }

    pub fn file_count(&self) -> usize {
        match self {
            Self::Files(files) => files.len(),
            _ => 0,
        }
    }
}

/// Inspects a given path to check if it exists as an audio folder and returns its status.
/// If the folder is missing, attempts to create it; if creation fails, returns an error status.
pub fn inspect_audio_folder(audio_dir_path: &str) -> SdCardStatus {
    let path = Path::new(audio_dir_path);
    let resolved_path = if path.exists() {
        path
    } else if Path::new("/audio").exists() {
        Path::new("/audio")
    } else {
        if let Err(err) = std::fs::create_dir_all(path) {
            log::warn!("Failed to create audio folder {audio_dir_path}: {err}");
            return SdCardStatus::FolderCreateFailed(format!("mkdir error: {err}"));
        }
        log::info!("Created missing audio folder at {audio_dir_path}");
        path
    };

    if !resolved_path.is_dir() {
        return SdCardStatus::FolderCreateFailed("path is not a dir".to_string());
    }

    match std::fs::read_dir(resolved_path) {
        Ok(entries) => {
            let mut files: Vec<String> = entries
                .filter_map(|entry| entry.ok())
                .filter(|entry| entry.file_type().map(|ft| ft.is_file()).unwrap_or(false))
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect();
            files.sort();
            if files.is_empty() {
                SdCardStatus::Empty
            } else {
                SdCardStatus::Files(files)
            }
        }
        Err(err) => {
            log::warn!("Failed to read audio directory {audio_dir_path}: {err}");
            SdCardStatus::Unavailable(format!("read error: {err}"))
        }
    }
}

/// Ensures the `/audio` directory exists on the SD card.
pub fn ensure_audio_folder(audio_dir_path: &str) -> Result<(), String> {
    let path = Path::new(audio_dir_path);
    if !path.exists() {
        std::fs::create_dir_all(path)
            .map_err(|err| format!("failed to create audio folder {audio_dir_path}: {err}"))?;
    }
    Ok(())
}

/// Returns the primary active audio directory path.
pub fn get_audio_dir() -> &'static str {
    if Path::new(SD_MOUNT_POINT).exists() {
        SD_AUDIO_DIR
    } else if Path::new("/audio").exists() {
        "/audio"
    } else {
        SD_AUDIO_DIR
    }
}

/// Returns list of files in the audio directory in reverse chronological order (newest first).
pub fn list_audio_files() -> Vec<String> {
    let dir = get_audio_dir();
    let path = Path::new(dir);
    if let Ok(entries) = std::fs::read_dir(path) {
        let mut files: Vec<String> = entries
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|ft| ft.is_file()).unwrap_or(false))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        files.sort();
        files.reverse();
        files
    } else {
        Vec::new()
    }
}

/// Backward-compatible probe helper checking whether `/sdcard` is mounted and writable.
pub fn check_sd_card_is_writable() -> bool {
    let path = Path::new(SD_MOUNT_POINT);
    if !path.exists() {
        return false;
    }

    let probe_file = format!("{}/.probe", SD_MOUNT_POINT);
    if std::fs::write(&probe_file, b"ok").is_ok() {
        let _ = std::fs::remove_file(&probe_file);
        true
    } else {
        false
    }
}

#[cfg(target_arch = "xtensa")]
pub type SdCardMounted<'a> = MountedFatfs<Fatfs<SdCardDriver<SdSpiHostDriver<'a, SpiDriver<'a>>>>>;

#[cfg(target_arch = "xtensa")]
pub struct SdCard<'a> {
    _mounted: SdCardMounted<'a>,
}

#[cfg(target_arch = "xtensa")]
impl<'a> SdCard<'a> {
    pub fn mount(
        spi: SPI3<'a>,
        sck: Gpio12<'a>,
        mosi: Gpio11<'a>,
        miso: Gpio13<'a>,
        cs: Gpio10<'a>,
    ) -> Result<Self, String> {
        let spi_driver = SpiDriver::new(
            spi,
            sck,
            mosi,
            Some(miso),
            &DriverConfig::default().dma(Dma::Auto(4096)),
        )
        .map_err(|err| format!("SPI bus init failed: {err}"))?;

        let sd_host = SdSpiHostDriver::new(
            spi_driver,
            Some(cs),
            AnyIOPin::none(),
            AnyIOPin::none(),
            AnyIOPin::none(),
            None,
        )
        .map_err(|err| format!("SD host init failed: {err}"))?;

        let sd_card_driver = SdCardDriver::new_spi(sd_host, &SdCardConfiguration::new())
            .map_err(|err| format!("SD card init failed: {err}"))?;

        let fatfs = Fatfs::new_sdcard(0, sd_card_driver)
            .map_err(|err| format!("FATFS driver init failed: {err}"))?;

        let mounted = MountedFatfs::mount(fatfs, SD_MOUNT_POINT, 4)
            .map_err(|err| format!("SD FATFS mount failed: {err}"))?;

        log::info!("MicroSD card mounted at {}", SD_MOUNT_POINT);
        Ok(Self { _mounted: mounted })
    }

    pub fn inspect(&self) -> SdCardStatus {
        inspect_audio_folder(SD_AUDIO_DIR)
    }
}

#[cfg(not(target_arch = "xtensa"))]
pub struct SdCard;

#[cfg(not(target_arch = "xtensa"))]
impl SdCard {
    pub fn inspect(&self) -> SdCardStatus {
        inspect_audio_folder(SD_AUDIO_DIR)
    }
}

#[cfg(test)]
#[allow(dead_code, unused_imports)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn inspect_missing_folder_creates_it_and_reports_empty() {
        let missing = "/tmp/test_missing_dir_sd_audio_12345";
        let _ = fs::remove_dir_all(missing);
        assert!(!Path::new(missing).exists());
        assert_eq!(inspect_audio_folder(missing), SdCardStatus::Empty);
        assert!(Path::new(missing).is_dir());
        let _ = fs::remove_dir_all(missing);
    }

    #[test]
    fn inspect_missing_folder_failure_reports_create_failed() {
        let file_blocker = "/tmp/test_file_blocker_12345";
        let _ = fs::write(file_blocker, b"content");
        let invalid_path = format!("{file_blocker}/audio");
        let status = inspect_audio_folder(&invalid_path);
        match status {
            SdCardStatus::FolderCreateFailed(_) => {}
            other => panic!("expected FolderCreateFailed, got {:?}", other),
        }
        let _ = fs::remove_file(file_blocker);
    }

    #[test]
    fn inspect_empty_folder_reports_empty() {
        let empty_dir = "/tmp/test_empty_sd_audio_12345";
        let _ = fs::create_dir_all(empty_dir);
        assert_eq!(inspect_audio_folder(empty_dir), SdCardStatus::Empty);
        let _ = fs::remove_dir_all(empty_dir);
    }

    #[test]
    fn inspect_folder_with_files_returns_sorted_files() {
        let test_dir = "/tmp/test_sd_audio_files_12345";
        let _ = fs::create_dir_all(test_dir);
        fs::write(format!("{test_dir}/rec002.wav"), b"sample").unwrap();
        fs::write(format!("{test_dir}/rec001.wav"), b"sample").unwrap();
        fs::write(format!("{test_dir}/rec003.wav"), b"sample").unwrap();

        let status = inspect_audio_folder(test_dir);
        match status {
            SdCardStatus::Files(files) => {
                assert_eq!(files, vec!["rec001.wav", "rec002.wav", "rec003.wav"]);
            }
            other => panic!("expected Files status, got {:?}", other),
        }

        let _ = fs::remove_dir_all(test_dir);
    }

    #[test]
    fn ensure_audio_folder_creates_directory() {
        let target = "/tmp/test_ensure_sd_audio_dir";
        let _ = fs::remove_dir_all(target);
        assert!(!Path::new(target).exists());

        assert!(ensure_audio_folder(target).is_ok());
        assert!(Path::new(target).is_dir());
        let _ = fs::remove_dir_all(target);
    }
}
