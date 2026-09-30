use esp_idf_svc::sntp::{EspSntp, SntpConf, SyncMode, SyncStatus};

pub struct NtpClient {
    sntp: EspSntp<'static>,
}

impl NtpClient {
    /// Initialize the SNTP client configured for pool.ntp.org.
    /// Operates asynchronously on Core 0 in the background.
    pub fn init() -> Result<Self, String> {
        let conf = SntpConf {
            servers: ["pool.ntp.org"],
            sync_mode: SyncMode::Immediate,
            ..Default::default()
        };
        let sntp = EspSntp::new(&conf)
            .map_err(|err| format!("failed to initialize SNTP client: {err}"))?;
        log::info!("NTP client started; synchronizing system time in background via pool.ntp.org");
        Ok(Self { sntp })
    }

    /// Check if NTP has completed synchronization.
    pub fn is_synchronized(&self) -> bool {
        self.sntp.get_sync_status() == SyncStatus::Completed
    }

    /// Retrieve raw sync status.
    pub fn sync_status(&self) -> SyncStatus {
        self.sntp.get_sync_status()
    }
}

/// Represents a broken-down UTC date and time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UtcDateTime {
    pub year: i32,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

impl UtcDateTime {
    pub const fn new(year: i32, month: u8, day: u8, hour: u8, minute: u8, second: u8) -> Self {
        Self {
            year,
            month,
            day,
            hour,
            minute,
            second,
        }
    }

    /// Query the current system RTC time as UTC.
    pub fn now() -> Self {
        unsafe {
            let mut now: esp_idf_svc::sys::time_t = 0;
            esp_idf_svc::sys::time(&mut now);
            let mut tm: esp_idf_svc::sys::tm = std::mem::zeroed();
            esp_idf_svc::sys::gmtime_r(&now, &mut tm);
            Self {
                year: tm.tm_year + 1900,
                month: (tm.tm_mon + 1) as u8,
                day: tm.tm_mday as u8,
                hour: tm.tm_hour as u8,
                minute: tm.tm_min as u8,
                second: tm.tm_sec as u8,
            }
        }
    }

    /// Check if the timestamp appears synchronized (year >= 2024).
    pub fn is_valid_ntp_year(&self) -> bool {
        self.year >= 2024
    }

    /// Formats the date and time as a recording filename: `YYYYMMDD_HHMMSS.wav`.
    pub fn format_recording_filename(&self) -> String {
        format!(
            "{:04}{:02}{:02}_{:02}{:02}{:02}.wav",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        )
    }

    /// Formats the date and time as `YYYYMMDD_HHMMSS`.
    pub fn format_timestamp(&self) -> String {
        format!(
            "{:04}{:02}{:02}_{:02}{:02}{:02}",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        )
    }

    /// Formats the date and time as `YYYY-MM-DD HH:MM:SS UTC`.
    pub fn format_iso(&self) -> String {
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        )
    }
}

/// Helper to generate the current recording filename based on current UTC time.
pub fn current_recording_filename() -> String {
    UtcDateTime::now().format_recording_filename()
}

#[cfg(test)]
#[allow(unused_imports)]
mod tests {
    use super::*;

    #[test]
    fn format_recording_filename_produces_expected_pattern() {
        let dt = UtcDateTime::new(2026, 9, 30, 17, 45, 12);
        assert_eq!(dt.format_recording_filename(), "20260930_174512.wav");
        assert_eq!(dt.format_timestamp(), "20260930_174512");
        assert_eq!(dt.format_iso(), "2026-09-30 17:45:12 UTC");
        assert!(dt.is_valid_ntp_year());
    }

    #[test]
    fn fallback_timestamp_for_unadjusted_epoch() {
        let dt = UtcDateTime::new(1970, 1, 1, 0, 1, 23);
        assert_eq!(dt.format_recording_filename(), "19700101_000123.wav");
        assert_eq!(dt.format_timestamp(), "19700101_000123");
        assert!(!dt.is_valid_ntp_year());
    }

    #[test]
    fn zero_padding_formatting_is_correct() {
        let dt = UtcDateTime::new(2025, 1, 5, 4, 3, 2);
        assert_eq!(dt.format_recording_filename(), "20250105_040302.wav");
    }

    #[test]
    fn current_recording_filename_has_wav_extension() {
        let name = current_recording_filename();
        assert!(name.ends_with(".wav"));
        assert_eq!(name.len(), 19); // YYYYMMDD_HHMMSS.wav = 8 + 1 + 6 + 4 = 19 chars
    }
}
