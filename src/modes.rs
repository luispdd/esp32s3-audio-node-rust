#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceMode {
    Status,
    Live,
    Sd,
}

impl DeviceMode {
    pub fn next(self) -> Self {
        match self {
            Self::Status => Self::Live,
            Self::Live => Self::Sd,
            Self::Sd => Self::Status,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::Status => Self::Sd,
            Self::Live => Self::Status,
            Self::Sd => Self::Live,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Status => "STATUS_MODE",
            Self::Live => "LIVE_MODE",
            Self::Sd => "SD_MODE",
        }
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn mode_button_cycles_through_status_live_and_sd() {
        let mut mode = DeviceMode::Status;

        mode = mode.next();
        assert_eq!(mode, DeviceMode::Live);

        mode = mode.next();
        assert_eq!(mode, DeviceMode::Sd);

        mode = mode.next();
        assert_eq!(mode, DeviceMode::Status);
    }
}
