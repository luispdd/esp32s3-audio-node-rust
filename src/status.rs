use crate::sd::{check_sd_card_is_writable, SdCardStatus};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemStatus {
    pub wifi: bool,
    pub mic: bool,
    pub pir: bool,
    pub sd: bool,
    pub sd_card: SdCardStatus,
}

impl SystemStatus {
    pub fn new(wifi: bool, mic: bool, pir: bool, sd_card: SdCardStatus) -> Self {
        Self {
            wifi,
            mic,
            pir,
            sd: sd_card.is_mounted(),
            sd_card,
        }
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
        }
    }

    pub fn from_runtime_with_sd(wifi_connected: bool, sd_card: SdCardStatus) -> Self {
        Self {
            wifi: wifi_connected,
            mic: true,
            pir: false,
            sd: sd_card.is_mounted(),
            sd_card,
        }
    }

    pub fn summary_lines(&self) -> Vec<String> {
        vec![
            format!("wifi {} mic {}", status_marker(self.wifi), status_marker(self.mic)),
            format!("pir {} sd {}", status_marker(self.pir), status_marker(self.sd)),
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
        };

        let summary = status.summary_lines();

        assert!(summary[0].contains("wifi OK"));
        assert!(summary[0].contains("mic OK"));
        assert!(summary[1].contains("pir KO"));
        assert!(summary[1].contains("sd OK"));
    }

    #[test]
    fn from_runtime_with_sd_sets_sd_flag_according_to_status() {
        let mounted = SystemStatus::from_runtime_with_sd(true, SdCardStatus::Empty);
        assert!(mounted.sd);

        let unmounted = SystemStatus::from_runtime_with_sd(true, SdCardStatus::Unavailable("err".into()));
        assert!(!unmounted.sd);
    }
}
