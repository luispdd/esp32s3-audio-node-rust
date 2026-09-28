use crate::sd::check_sd_card_is_writable;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SystemStatus {
    pub wifi: bool,
    pub mic: bool,
    pub pir: bool,
    pub sd: bool,
}

impl SystemStatus {
    pub fn from_runtime(wifi_connected: bool) -> Self {
        Self {
            wifi: wifi_connected,
            mic: true,
            pir: false,
            sd: check_sd_card_is_writable(),
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
mod tests {

    #[test]
    fn status_health_summary_reports_each_device_state() {
        let status = SystemStatus {
            wifi: true,
            mic: true,
            pir: false,
            sd: true,
        };

        let summary = status.summary_lines();

        assert!(summary[0].contains("wifi OK"));
        assert!(summary[0].contains("mic OK"));
        assert!(summary[1].contains("pir KO"));
        assert!(summary[1].contains("sd OK"));
    }
}
