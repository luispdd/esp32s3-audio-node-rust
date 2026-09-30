use crate::audio::{ActivePlaybackInfo, ActiveRecordingInfo};
use crate::sd::{check_sd_card_is_writable, SdCardStatus};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemStatus {
    pub wifi: bool,
    pub mic: bool,
    pub pir: bool,
    pub sd: bool,
    pub sd_card: SdCardStatus,
    pub gain_percent: u8,
    pub recording: Option<ActiveRecordingInfo>,
    pub playback: Option<ActivePlaybackInfo>,
    pub selected_file_index: usize,
}

impl SystemStatus {
    pub fn new(wifi: bool, mic: bool, pir: bool, sd_card: SdCardStatus, gain_percent: u8) -> Self {
        Self {
            wifi,
            mic,
            pir,
            sd: sd_card.is_mounted(),
            sd_card,
            gain_percent,
            recording: None,
            playback: None,
            selected_file_index: 0,
        }
    }

    pub fn with_recording(mut self, recording: Option<ActiveRecordingInfo>) -> Self {
        self.recording = recording;
        self
    }

    pub fn with_playback(mut self, playback: Option<ActivePlaybackInfo>) -> Self {
        self.playback = playback;
        self
    }

    pub fn with_selected_file_index(mut self, index: usize) -> Self {
        self.selected_file_index = index;
        self
    }

    pub fn from_runtime(wifi_connected: bool) -> Self {
        let is_writable = check_sd_card_is_writable();
        let sd_card = if is_writable {
            SdCardStatus::Empty
        } else {
            SdCardStatus::Unavailable("Card not mounted".to_string())
        };

        Self {
            wifi: wifi_connected,
            mic: true,
            pir: false,
            sd: is_writable,
            sd_card,
            gain_percent: 100,
            recording: None,
            playback: None,
            selected_file_index: 0,
        }
    }

    pub fn from_runtime_with_sensors(
        wifi_connected: bool,
        mic_detected: bool,
        pir_detected: bool,
        sd_card: SdCardStatus,
        gain_percent: u8,
    ) -> Self {
        Self {
            wifi: wifi_connected,
            mic: mic_detected,
            pir: pir_detected,
            sd: sd_card.is_mounted(),
            sd_card,
            gain_percent,
            recording: None,
            playback: None,
            selected_file_index: 0,
        }
    }

    pub fn from_runtime_with_sd(wifi_connected: bool, sd_card: SdCardStatus) -> Self {
        Self::from_runtime_with_sensors(wifi_connected, true, false, sd_card, 100)
    }

    pub fn summary_lines(&self) -> Vec<String> {
        vec![
            format!("wifi {} mic {}", status_marker(self.wifi), status_marker(self.mic)),
            format!(
                "pir {} sd {} g:{}%",
                status_marker(self.pir),
                status_marker(self.sd),
                self.gain_percent
            ),
        ]
    }
}

pub fn status_marker(value: bool) -> &'static str {
    if value {
        "OK"
    } else {
        "KO"
    }
}

#[cfg(test)]
#[allow(dead_code, unused_imports)]
mod tests {
    use super::*;

    #[test]
    fn status_health_summary_reports_each_device_state() {
        let status = SystemStatus {
            wifi: true,
            mic: true,
            pir: false,
            sd: true,
            sd_card: SdCardStatus::Empty,
            gain_percent: 75,
            recording: None,
            playback: None,
            selected_file_index: 0,
        };

        let summary = status.summary_lines();

        assert!(summary[0].contains("wifi OK"));
        assert!(summary[0].contains("mic OK"));
        assert!(summary[1].contains("pir KO"));
        assert!(summary[1].contains("sd OK"));
        assert!(summary[1].contains("g:75%"));
    }

    #[test]
    fn from_runtime_with_sensors_sets_live_sensor_flags() {
        let status = SystemStatus::from_runtime_with_sensors(
            true,
            false,
            true,
            SdCardStatus::Empty,
            50,
        );
        assert!(status.wifi);
        assert!(!status.mic);
        assert!(status.pir);
        assert!(status.sd);
        assert_eq!(status.gain_percent, 50);

        let summary = status.summary_lines();
        assert!(summary[0].contains("wifi OK"));
        assert!(summary[0].contains("mic KO"));
        assert!(summary[1].contains("pir OK"));
        assert!(summary[1].contains("sd OK"));
        assert!(summary[1].contains("g:50%"));
    }

    #[test]
    fn from_runtime_with_sd_sets_sd_flag_according_to_status() {
        let mounted = SystemStatus::from_runtime_with_sd(true, SdCardStatus::Empty);
        assert!(mounted.sd);

        let unmounted = SystemStatus::from_runtime_with_sd(true, SdCardStatus::Unavailable("err".into()));
        assert!(!unmounted.sd);
    }
}
