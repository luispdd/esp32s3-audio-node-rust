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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SdButtonAction {
    None,
    NextFile,
    PrevFile,
    TogglePlayback,
    StartRecording,
    StopRecording,
    CancelRecording,
    DeleteSelectedFile,
}

#[derive(Debug)]
pub struct SdButtonController {
    long_press_duration: Duration,
    chord_duration: Duration,
    b2_pressed_at: Option<Instant>,
    b3_pressed_at: Option<Instant>,
    chord_pressed_at: Option<Instant>,
    chord_triggered: bool,
    b2_long_press_triggered: bool,
    b2_was_down: bool,
    b3_was_down: bool,
    b2_suppress_release: bool,
    b3_suppress_release: bool,
}

impl SdButtonController {
    pub const DEFAULT_LONG_PRESS_DURATION: Duration = Duration::from_millis(800);
    pub const DEFAULT_CHORD_DURATION: Duration = Duration::from_millis(1000);

    pub fn new(long_press_duration: Duration, chord_duration: Duration) -> Self {
        Self {
            long_press_duration,
            chord_duration,
            b2_pressed_at: None,
            b3_pressed_at: None,
            chord_pressed_at: None,
            chord_triggered: false,
            b2_long_press_triggered: false,
            b2_was_down: false,
            b3_was_down: false,
            b2_suppress_release: false,
            b3_suppress_release: false,
        }
    }

    pub fn update(
        &mut self,
        b2_down: bool,
        b3_down: bool,
        is_recording: bool,
        is_playing: bool,
    ) -> SdButtonAction {
        self.update_with_time(b2_down, b3_down, is_recording, is_playing, Instant::now())
    }

    pub fn update_with_time(
        &mut self,
        b2_down: bool,
        b3_down: bool,
        is_recording: bool,
        is_playing: bool,
        now: Instant,
    ) -> SdButtonAction {
        // 1. If currently recording: direct edge detection on press
        if is_recording {
            self.chord_pressed_at = None;
            self.chord_triggered = false;
            self.b2_pressed_at = None;
            self.b3_pressed_at = None;
            self.b2_long_press_triggered = false;

            let b2_edge = b2_down && !self.b2_was_down;
            let b3_edge = b3_down && !self.b3_was_down;
            self.b2_was_down = b2_down;
            self.b3_was_down = b3_down;

            if b2_edge {
                self.b2_suppress_release = true;
                return SdButtonAction::StopRecording;
            }
            if b3_edge {
                self.b3_suppress_release = true;
                return SdButtonAction::CancelRecording;
            }
            return SdButtonAction::None;
        }

        // 2. If currently playing: direct edge detection on press
        if is_playing {
            self.chord_pressed_at = None;
            self.chord_triggered = false;
            self.b2_pressed_at = None;
            self.b3_pressed_at = None;
            self.b2_long_press_triggered = false;

            let b2_edge = b2_down && !self.b2_was_down;
            let b3_edge = b3_down && !self.b3_was_down;
            self.b2_was_down = b2_down;
            self.b3_was_down = b3_down;

            if b2_edge {
                self.b2_suppress_release = true;
                return SdButtonAction::TogglePlayback;
            }
            if b3_edge {
                self.b3_suppress_release = true;
                return SdButtonAction::NextFile;
            }
            return SdButtonAction::None;
        }

        self.b2_was_down = b2_down;
        self.b3_was_down = b3_down;

        // Handle release suppression after recording or playback transitions
        if self.b2_suppress_release {
            if !b2_down {
                self.b2_suppress_release = false;
                self.b2_pressed_at = None;
                self.b2_long_press_triggered = false;
            } else {
                self.b2_pressed_at = None;
            }
        }

        if self.b3_suppress_release {
            if !b3_down {
                self.b3_suppress_release = false;
                self.b3_pressed_at = None;
            } else {
                self.b3_pressed_at = None;
            }
        }

        // 3. Idle: Simultaneous chord detection (B2 + B3 held for >= chord_duration)
        if b2_down && b3_down {
            if self.chord_pressed_at.is_none() {
                self.chord_pressed_at = Some(now);
            }
            if !self.chord_triggered {
                if let Some(start) = self.chord_pressed_at {
                    if now.saturating_duration_since(start) >= self.chord_duration {
                        self.chord_triggered = true;
                        return SdButtonAction::DeleteSelectedFile;
                    }
                }
            }
            return SdButtonAction::None;
        } else {
            self.chord_pressed_at = None;
        }

        // If chord was triggered, wait for full release of both buttons before allowing single actions
        if self.chord_triggered {
            if !b2_down && !b3_down {
                self.chord_triggered = false;
                self.b2_pressed_at = None;
                self.b3_pressed_at = None;
                self.b2_long_press_triggered = false;
            }
            return SdButtonAction::None;
        }

        // 4. Button 2 handling: short press = TogglePlayback, long press (>= 800ms) = StartRecording
        let mut action = SdButtonAction::None;
        if b2_down && !b3_down && !self.b2_suppress_release {
            if self.b2_pressed_at.is_none() {
                self.b2_pressed_at = Some(now);
                self.b2_long_press_triggered = false;
            }
            if !self.b2_long_press_triggered {
                if let Some(start) = self.b2_pressed_at {
                    if now.saturating_duration_since(start) >= self.long_press_duration {
                        self.b2_long_press_triggered = true;
                        action = SdButtonAction::StartRecording;
                    }
                }
            }
        } else if !b2_down {
            if let Some(start) = self.b2_pressed_at.take() {
                if !self.b2_long_press_triggered && !self.b2_suppress_release {
                    if now.saturating_duration_since(start) < self.long_press_duration {
                        action = SdButtonAction::TogglePlayback;
                    }
                }
                self.b2_long_press_triggered = false;
            }
        }

        if action != SdButtonAction::None {
            return action;
        }

        // 5. Button 3 handling: short press on release = NextFile
        if b3_down && !b2_down && !self.b3_suppress_release {
            if self.b3_pressed_at.is_none() {
                self.b3_pressed_at = Some(now);
            }
        } else if !b3_down {
            if let Some(_start) = self.b3_pressed_at.take() {
                if !self.b3_suppress_release {
                    action = SdButtonAction::NextFile;
                }
            }
        }

        action
    }
}

#[cfg(test)]
#[allow(unused_imports)]
mod tests {
    use super::*;

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

    #[test]
    fn sd_button_navigation_short_presses() {
        let mut ctrl = SdButtonController::new(Duration::from_millis(800), Duration::from_millis(1000));
        let t0 = Instant::now();

        // Button 3 tapped (Next)
        assert_eq!(ctrl.update_with_time(false, true, false, false, t0), SdButtonAction::None);
        assert_eq!(ctrl.update_with_time(false, false, false, false, t0 + Duration::from_millis(150)), SdButtonAction::NextFile);

        // Button 2 tapped (TogglePlayback)
        let t1 = t0 + Duration::from_millis(300);
        assert_eq!(ctrl.update_with_time(true, false, false, false, t1), SdButtonAction::None);
        assert_eq!(ctrl.update_with_time(false, false, false, false, t1 + Duration::from_millis(150)), SdButtonAction::TogglePlayback);
    }

    #[test]
    fn sd_button_start_recording_on_long_press_b2() {
        let mut ctrl = SdButtonController::new(Duration::from_millis(800), Duration::from_millis(1000));
        let t0 = Instant::now();

        // Hold B2 for 800ms
        assert_eq!(ctrl.update_with_time(true, false, false, false, t0), SdButtonAction::None);
        assert_eq!(ctrl.update_with_time(true, false, false, false, t0 + Duration::from_millis(500)), SdButtonAction::None);
        assert_eq!(ctrl.update_with_time(true, false, false, false, t0 + Duration::from_millis(800)), SdButtonAction::StartRecording);

        // Release B2 -> should NOT trigger TogglePlayback
        assert_eq!(ctrl.update_with_time(false, false, false, false, t0 + Duration::from_millis(900)), SdButtonAction::None);
    }

    #[test]
    fn sd_button_dual_chord_delete() {
        let mut ctrl = SdButtonController::new(Duration::from_millis(800), Duration::from_millis(1000));
        let t0 = Instant::now();

        // Press both B2 and B3
        assert_eq!(ctrl.update_with_time(true, true, false, false, t0), SdButtonAction::None);
        assert_eq!(ctrl.update_with_time(true, true, false, false, t0 + Duration::from_millis(500)), SdButtonAction::None);
        // Held for 1000ms -> DeleteSelectedFile
        assert_eq!(ctrl.update_with_time(true, true, false, false, t0 + Duration::from_millis(1000)), SdButtonAction::DeleteSelectedFile);

        // Release buttons -> should NOT trigger navigation or recording
        assert_eq!(ctrl.update_with_time(true, false, false, false, t0 + Duration::from_millis(1100)), SdButtonAction::None);
        assert_eq!(ctrl.update_with_time(false, false, false, false, t0 + Duration::from_millis(1200)), SdButtonAction::None);
    }

    #[test]
    fn sd_button_actions_while_recording() {
        let mut ctrl = SdButtonController::new(Duration::from_millis(800), Duration::from_millis(1000));
        let t0 = Instant::now();

        // While recording, pressing B2 stops recording
        assert_eq!(ctrl.update_with_time(true, false, true, false, t0), SdButtonAction::StopRecording);
        assert_eq!(ctrl.update_with_time(false, false, true, false, t0 + Duration::from_millis(50)), SdButtonAction::None);

        // While recording, pressing B3 cancels recording
        assert_eq!(ctrl.update_with_time(false, true, true, false, t0 + Duration::from_millis(100)), SdButtonAction::CancelRecording);
    }

    #[test]
    fn sd_button_actions_while_playing() {
        let mut ctrl = SdButtonController::new(Duration::from_millis(800), Duration::from_millis(1000));
        let t0 = Instant::now();

        // While playing, pressing B2 toggles/stops playback
        assert_eq!(ctrl.update_with_time(true, false, false, true, t0), SdButtonAction::TogglePlayback);
        assert_eq!(ctrl.update_with_time(false, false, false, true, t0 + Duration::from_millis(50)), SdButtonAction::None);

        // While playing, pressing B3 advances to next file
        assert_eq!(ctrl.update_with_time(false, true, false, true, t0 + Duration::from_millis(100)), SdButtonAction::NextFile);
    }

    #[test]
    fn sd_button_stop_recording_does_not_trigger_toggle_playback_on_release() {
        let mut ctrl = SdButtonController::new(Duration::from_millis(800), Duration::from_millis(1000));
        let t0 = Instant::now();

        // While recording, press B2 to stop
        assert_eq!(ctrl.update_with_time(true, false, true, false, t0), SdButtonAction::StopRecording);

        // Recording finishes in worker, so is_recording becomes false, but B2 is still held down by user
        let t1 = t0 + Duration::from_millis(50);
        assert_eq!(ctrl.update_with_time(true, false, false, false, t1), SdButtonAction::None);

        // B2 is released 150ms later -> MUST NOT trigger TogglePlayback!
        let t2 = t0 + Duration::from_millis(200);
        assert_eq!(ctrl.update_with_time(false, false, false, false, t2), SdButtonAction::None);

        // Subsequent fresh tap on B2 triggers TogglePlayback normally
        let t3 = t0 + Duration::from_millis(400);
        assert_eq!(ctrl.update_with_time(true, false, false, false, t3), SdButtonAction::None);
        assert_eq!(ctrl.update_with_time(false, false, false, false, t3 + Duration::from_millis(100)), SdButtonAction::TogglePlayback);
    }

    #[test]
    fn sd_button_cancel_recording_does_not_trigger_next_file_on_release() {
        let mut ctrl = SdButtonController::new(Duration::from_millis(800), Duration::from_millis(1000));
        let t0 = Instant::now();

        // While recording, press B3 to cancel
        assert_eq!(ctrl.update_with_time(false, true, true, false, t0), SdButtonAction::CancelRecording);

        // Recording cancelled, is_recording becomes false, B3 is still held down
        let t1 = t0 + Duration::from_millis(50);
        assert_eq!(ctrl.update_with_time(false, true, false, false, t1), SdButtonAction::None);

        // B3 is released -> MUST NOT trigger NextFile!
        let t2 = t0 + Duration::from_millis(200);
        assert_eq!(ctrl.update_with_time(false, false, false, false, t2), SdButtonAction::None);
    }
}
