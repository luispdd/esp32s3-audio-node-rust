use std::time::{Duration, Instant};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeButtonAction {
    None,
    CycleMode,
    TurnScreenOff,
    TurnScreenOn,
}

#[derive(Debug)]
pub struct ModeButtonController {
    long_press_duration: Duration,
    pressed_at: Option<Instant>,
    long_press_triggered: bool,
    wake_triggered: bool,
}

impl ModeButtonController {
    pub const DEFAULT_LONG_PRESS_DURATION: Duration = Duration::from_millis(800);

    pub fn new(long_press_duration: Duration) -> Self {
        Self {
            long_press_duration,
            pressed_at: None,
            long_press_triggered: false,
            wake_triggered: false,
        }
    }

    pub fn update(&mut self, is_pressed: bool, screen_is_on: bool) -> ModeButtonAction {
        self.update_with_time(is_pressed, screen_is_on, Instant::now())
    }

    pub fn update_with_time(
        &mut self,
        is_pressed: bool,
        screen_is_on: bool,
        now: Instant,
    ) -> ModeButtonAction {
        if is_pressed {
            match self.pressed_at {
                None => {
                    self.pressed_at = Some(now);
                    self.long_press_triggered = false;

                    if !screen_is_on {
                        self.wake_triggered = true;
                        ModeButtonAction::TurnScreenOn
                    } else {
                        self.wake_triggered = false;
                        ModeButtonAction::None
                    }
                }
                Some(start) => {
                    if screen_is_on && !self.long_press_triggered && !self.wake_triggered {
                        if now.saturating_duration_since(start) >= self.long_press_duration {
                            self.long_press_triggered = true;
                            ModeButtonAction::TurnScreenOff
                        } else {
                            ModeButtonAction::None
                        }
                    } else {
                        ModeButtonAction::None
                    }
                }
            }
        } else {
            let was_pressed = self.pressed_at.is_some();
            let action = if was_pressed && !self.long_press_triggered && !self.wake_triggered {
                ModeButtonAction::CycleMode
            } else {
                ModeButtonAction::None
            };

            self.pressed_at = None;
            self.long_press_triggered = false;
            self.wake_triggered = false;

            action
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

    #[test]
    fn short_press_cycles_mode_when_screen_is_on() {
        let mut controller = ModeButtonController::new(Duration::from_millis(800));
        let start = Instant::now();

        // Button pressed down
        assert_eq!(
            controller.update_with_time(true, true, start),
            ModeButtonAction::None
        );

        // Released after 200ms
        assert_eq!(
            controller.update_with_time(false, true, start + Duration::from_millis(200)),
            ModeButtonAction::CycleMode
        );
    }

    #[test]
    fn long_press_turns_screen_off_without_cycling_mode() {
        let mut controller = ModeButtonController::new(Duration::from_millis(800));
        let start = Instant::now();

        // Button pressed down
        assert_eq!(
            controller.update_with_time(true, true, start),
            ModeButtonAction::None
        );

        // Held for 500ms -> None
        assert_eq!(
            controller.update_with_time(true, true, start + Duration::from_millis(500)),
            ModeButtonAction::None
        );

        // Held for 800ms -> TurnScreenOff
        assert_eq!(
            controller.update_with_time(true, true, start + Duration::from_millis(800)),
            ModeButtonAction::TurnScreenOff
        );

        // Still held at 1000ms -> None
        assert_eq!(
            controller.update_with_time(true, false, start + Duration::from_millis(1000)),
            ModeButtonAction::None
        );

        // Released at 1200ms -> None (does not cycle mode)
        assert_eq!(
            controller.update_with_time(false, false, start + Duration::from_millis(1200)),
            ModeButtonAction::None
        );
    }

    #[test]
    fn press_when_screen_is_off_turns_screen_on_without_cycling_mode() {
        let mut controller = ModeButtonController::new(Duration::from_millis(800));
        let start = Instant::now();

        // Button pressed down while screen is off
        assert_eq!(
            controller.update_with_time(true, false, start),
            ModeButtonAction::TurnScreenOn
        );

        // Released after 200ms -> None (does not cycle mode)
        assert_eq!(
            controller.update_with_time(false, true, start + Duration::from_millis(200)),
            ModeButtonAction::None
        );
    }

    #[test]
    fn long_press_when_waking_screen_does_not_turn_off_again_in_same_gesture() {
        let mut controller = ModeButtonController::new(Duration::from_millis(800));
        let start = Instant::now();

        // Pressed while screen off -> wake
        assert_eq!(
            controller.update_with_time(true, false, start),
            ModeButtonAction::TurnScreenOn
        );

        // Kept held for 1000ms -> None (doesn't immediately turn off again)
        assert_eq!(
            controller.update_with_time(true, true, start + Duration::from_millis(1000)),
            ModeButtonAction::None
        );

        // Released -> None
        assert_eq!(
            controller.update_with_time(false, true, start + Duration::from_millis(1100)),
            ModeButtonAction::None
        );
    }
}
