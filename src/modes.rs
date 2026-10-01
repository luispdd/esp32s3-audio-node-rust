use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceMode {
    Status,
    Live,
    Sd,
    Pir,
}

impl DeviceMode {
    pub fn next(self) -> Self {
        match self {
            Self::Status => Self::Live,
            Self::Live => Self::Sd,
            Self::Sd => Self::Pir,
            Self::Pir => Self::Status,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::Status => Self::Pir,
            Self::Live => Self::Status,
            Self::Sd => Self::Live,
            Self::Pir => Self::Sd,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Status => "STATUS_MODE",
            Self::Live => "LIVE_MODE",
            Self::Sd => "SD_MODE",
            Self::Pir => "PIR_MODE",
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PirButtonAction {
    None,
    StartArming,
    Disarm,
    StopRecording,
    CancelRecording,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PirMotionAction {
    None,
    StartRecording,
    StopRecordingTimeout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PirOperationalState {
    Idle,
    Arming(Instant),
    Armed,
}

#[derive(Debug)]
pub struct PirButtonController {
    arming_duration: Duration,
    recording_duration: Duration,
    state: PirOperationalState,
    last_motion_at: Option<Instant>,
    b2_was_down: bool,
    b3_was_down: bool,
}

impl PirButtonController {
    pub const DEFAULT_ARMING_DURATION: Duration = Duration::from_secs(10);
    pub const DEFAULT_RECORDING_DURATION: Duration = Duration::from_secs(20);

    pub fn new(arming_duration: Duration) -> Self {
        Self::with_durations(arming_duration, Self::DEFAULT_RECORDING_DURATION)
    }

    pub fn with_durations(arming_duration: Duration, recording_duration: Duration) -> Self {
        Self {
            arming_duration,
            recording_duration,
            state: PirOperationalState::Idle,
            last_motion_at: None,
            b2_was_down: false,
            b3_was_down: false,
        }
    }

    pub fn state(&self) -> PirOperationalState {
        self.state
    }

    pub fn is_idle(&self) -> bool {
        matches!(self.state, PirOperationalState::Idle)
    }

    pub fn is_arming(&self) -> bool {
        matches!(self.state, PirOperationalState::Arming(_))
    }

    pub fn is_armed(&self) -> bool {
        matches!(self.state, PirOperationalState::Armed)
    }

    pub fn arming_countdown_secs(&self, now: Instant) -> Option<u8> {
        if let PirOperationalState::Arming(started_at) = self.state {
            let elapsed = now.saturating_duration_since(started_at);
            if elapsed < self.arming_duration {
                let rem = (self.arming_duration - elapsed).as_secs() + 1;
                Some(rem as u8)
            } else {
                None
            }
        } else {
            None
        }
    }

    pub fn remaining_recording_secs(&self, now: Instant) -> Option<u8> {
        if let Some(last_motion) = self.last_motion_at {
            let elapsed = now.saturating_duration_since(last_motion);
            if elapsed < self.recording_duration {
                let rem = (self.recording_duration - elapsed).as_secs() + 1;
                Some((rem as u8).min(self.recording_duration.as_secs() as u8))
            } else {
                Some(0)
            }
        } else {
            None
        }
    }

    pub fn disarm(&mut self) {
        self.state = PirOperationalState::Idle;
        self.last_motion_at = None;
    }

    /// Immediately begins the 10-second arming delay (acting like Button 2 press in IDLE).
    pub fn start_arming(&mut self, now: Instant) {
        self.state = PirOperationalState::Arming(now);
        self.last_motion_at = None;
    }

    /// Evaluates motion and acoustic sound activity, triggering recording or extending active recording.
    /// Task 6.5: sound activity exceeding baseline threshold extends/resets the 20-second recording timer.
    pub fn handle_activity(
        &mut self,
        motion_detected: bool,
        sound_detected: bool,
        is_recording: bool,
        now: Instant,
    ) -> PirMotionAction {
        if !self.is_armed() {
            self.last_motion_at = None;
            return PirMotionAction::None;
        }

        if !is_recording {
            if motion_detected {
                self.last_motion_at = Some(now);
                PirMotionAction::StartRecording
            } else {
                PirMotionAction::None
            }
        } else {
            if motion_detected || sound_detected {
                self.last_motion_at = Some(now);
                PirMotionAction::None
            } else if let Some(last_motion) = self.last_motion_at {
                if now.saturating_duration_since(last_motion) >= self.recording_duration {
                    self.last_motion_at = None;
                    PirMotionAction::StopRecordingTimeout
                } else {
                    PirMotionAction::None
                }
            } else {
                self.last_motion_at = Some(now);
                PirMotionAction::None
            }
        }
    }

    pub fn handle_motion(
        &mut self,
        motion_detected: bool,
        is_recording: bool,
        now: Instant,
    ) -> PirMotionAction {
        self.handle_activity(motion_detected, false, is_recording, now)
    }

    pub fn update(&mut self, b2_down: bool, b3_down: bool, is_recording: bool) -> PirButtonAction {
        self.update_with_time(b2_down, b3_down, is_recording, Instant::now())
    }

    pub fn update_with_time(
        &mut self,
        b2_down: bool,
        b3_down: bool,
        is_recording: bool,
        now: Instant,
    ) -> PirButtonAction {
        // Advance arming state if arming duration has elapsed
        if let PirOperationalState::Arming(started_at) = self.state {
            if now.saturating_duration_since(started_at) >= self.arming_duration {
                self.state = PirOperationalState::Armed;
            }
        }

        let b2_edge = b2_down && !self.b2_was_down;
        let b3_edge = b3_down && !self.b3_was_down;
        self.b2_was_down = b2_down;
        self.b3_was_down = b3_down;

        if is_recording {
            if b2_edge {
                self.state = PirOperationalState::Idle;
                self.last_motion_at = None;
                return PirButtonAction::StopRecording;
            }
            if b3_edge {
                self.last_motion_at = None;
                return PirButtonAction::CancelRecording;
            }
            return PirButtonAction::None;
        }

        if b2_edge {
            match self.state {
                PirOperationalState::Idle => {
                    self.state = PirOperationalState::Arming(now);
                    self.last_motion_at = None;
                    PirButtonAction::StartArming
                }
                PirOperationalState::Arming(_) | PirOperationalState::Armed => {
                    self.state = PirOperationalState::Idle;
                    self.last_motion_at = None;
                    PirButtonAction::Disarm
                }
            }
        } else {
            PirButtonAction::None
        }
    }
}

#[cfg(test)]
#[allow(unused_imports)]
mod tests {
    use super::*;

    #[test]
    fn mode_button_cycles_through_all_modes() {
        let mut mode = DeviceMode::Status;

        mode = mode.next();
        assert_eq!(mode, DeviceMode::Live);

        mode = mode.next();
        assert_eq!(mode, DeviceMode::Sd);

        mode = mode.next();
        assert_eq!(mode, DeviceMode::Pir);

        mode = mode.next();
        assert_eq!(mode, DeviceMode::Status);

        mode = mode.prev();
        assert_eq!(mode, DeviceMode::Pir);

        mode = mode.prev();
        assert_eq!(mode, DeviceMode::Sd);

        mode = mode.prev();
        assert_eq!(mode, DeviceMode::Live);

        mode = mode.prev();
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

    #[test]
    fn pir_button_arming_flow_and_completion() {
        let mut ctrl = PirButtonController::new(Duration::from_secs(10));
        let t0 = Instant::now();

        assert!(ctrl.is_idle());
        assert_eq!(ctrl.arming_countdown_secs(t0), None);

        // Tap B2 -> starts arming
        assert_eq!(ctrl.update_with_time(true, false, false, t0), PirButtonAction::StartArming);
        assert!(ctrl.is_arming());
        assert_eq!(ctrl.arming_countdown_secs(t0), Some(10));

        // Release B2
        assert_eq!(ctrl.update_with_time(false, false, false, t0 + Duration::from_millis(150)), PirButtonAction::None);
        assert!(ctrl.is_arming());

        // Check countdown at 4 seconds elapsed -> 7s remaining
        let t4 = t0 + Duration::from_secs(4);
        assert_eq!(ctrl.update_with_time(false, false, false, t4), PirButtonAction::None);
        assert!(ctrl.is_arming());
        assert_eq!(ctrl.arming_countdown_secs(t4), Some(7));

        // Check countdown at 9.5s -> 1s remaining
        let t9_5 = t0 + Duration::from_millis(9500);
        assert_eq!(ctrl.arming_countdown_secs(t9_5), Some(1));

        // At 10 seconds -> becomes Armed!
        let t10 = t0 + Duration::from_secs(10);
        assert_eq!(ctrl.update_with_time(false, false, false, t10), PirButtonAction::None);
        assert!(ctrl.is_armed());
        assert_eq!(ctrl.arming_countdown_secs(t10), None);
    }

    #[test]
    fn pir_button_disarms_during_arming_and_when_armed() {
        let mut ctrl = PirButtonController::new(Duration::from_secs(10));
        let t0 = Instant::now();

        // 1. Arm and cancel during arming
        assert_eq!(ctrl.update_with_time(true, false, false, t0), PirButtonAction::StartArming);
        assert!(ctrl.is_arming());
        assert_eq!(ctrl.update_with_time(false, false, false, t0 + Duration::from_millis(100)), PirButtonAction::None);

        // Press B2 again at 3s -> Disarms
        assert_eq!(ctrl.update_with_time(true, false, false, t0 + Duration::from_secs(3)), PirButtonAction::Disarm);
        assert!(ctrl.is_idle());

        // 2. Arm to completion, then disarm
        let t1 = t0 + Duration::from_secs(5);
        assert_eq!(ctrl.update_with_time(true, false, false, t1), PirButtonAction::StartArming);
        assert_eq!(ctrl.update_with_time(false, false, false, t1 + Duration::from_secs(10)), PirButtonAction::None);
        assert!(ctrl.is_armed());

        // Press B2 while armed -> Disarms
        assert_eq!(ctrl.update_with_time(true, false, false, t1 + Duration::from_secs(12)), PirButtonAction::Disarm);
        assert!(ctrl.is_idle());
    }

    #[test]
    fn pir_button_recording_stop_and_cancel() {
        let mut ctrl = PirButtonController::new(Duration::from_secs(10));
        let t0 = Instant::now();

        // Start arming and advance to armed
        ctrl.update_with_time(true, false, false, t0);
        ctrl.update_with_time(false, false, false, t0 + Duration::from_secs(10));
        assert!(ctrl.is_armed());

        // While recording, B2 stops recording and disarms
        let t1 = t0 + Duration::from_secs(12);
        assert_eq!(ctrl.update_with_time(true, false, true, t1), PirButtonAction::StopRecording);
        assert!(ctrl.is_idle());

        // Manual re-arm and test B3 cancel
        ctrl.update_with_time(true, false, false, t1 + Duration::from_secs(1));
        ctrl.update_with_time(false, false, false, t1 + Duration::from_secs(11));
        assert!(ctrl.is_armed());

        // While recording, B3 cancels recording
        let t2 = t1 + Duration::from_secs(13);
        assert_eq!(ctrl.update_with_time(false, true, true, t2), PirButtonAction::CancelRecording);
    }

    #[test]
    fn pir_motion_trigger_and_duration_extension() {
        let mut ctrl = PirButtonController::with_durations(Duration::from_secs(10), Duration::from_secs(20));
        let t0 = Instant::now();

        // 1. Motion ignored when idle
        assert_eq!(ctrl.handle_motion(true, false, t0), PirMotionAction::None);

        // 2. Motion ignored when arming
        ctrl.update_with_time(true, false, false, t0);
        assert!(ctrl.is_arming());
        assert_eq!(ctrl.handle_motion(true, false, t0 + Duration::from_secs(3)), PirMotionAction::None);

        // 3. Advance to armed
        ctrl.update_with_time(false, false, false, t0 + Duration::from_secs(10));
        assert!(ctrl.is_armed());

        // 4. Motion detected while armed -> StartRecording!
        let t_motion = t0 + Duration::from_secs(11);
        assert_eq!(ctrl.handle_motion(true, false, t_motion), PirMotionAction::StartRecording);
        assert_eq!(ctrl.remaining_recording_secs(t_motion), Some(20));

        // 5. While recording, new motion at +15s resets the 20s counter
        let t_motion2 = t_motion + Duration::from_secs(15);
        assert_eq!(ctrl.handle_motion(true, true, t_motion2), PirMotionAction::None);
        assert_eq!(ctrl.remaining_recording_secs(t_motion2), Some(20));

        // 6. At +10s after second motion -> 10s remaining
        let t_check = t_motion2 + Duration::from_secs(10);
        assert_eq!(ctrl.handle_motion(false, true, t_check), PirMotionAction::None);
        assert_eq!(ctrl.remaining_recording_secs(t_check), Some(10));

        // 7. At +20s after second motion -> StopRecordingTimeout!
        let t_timeout = t_motion2 + Duration::from_secs(20);
        assert_eq!(ctrl.handle_motion(false, true, t_timeout), PirMotionAction::StopRecordingTimeout);
        assert!(ctrl.is_armed()); // stays armed and ready for next motion

        // 8. Subsequent motion after finalization cleanly starts a new recording
        let t_motion3 = t_timeout + Duration::from_secs(5);
        assert_eq!(ctrl.handle_motion(true, false, t_motion3), PirMotionAction::StartRecording);
        assert_eq!(ctrl.remaining_recording_secs(t_motion3), Some(20));
    }

    #[test]
    fn pir_continuous_motion_repeatedly_resets_20s_timeout() {
        let mut ctrl = PirButtonController::with_durations(Duration::from_secs(10), Duration::from_secs(20));
        let t0 = Instant::now();

        // Arm and enter armed state
        ctrl.update_with_time(true, false, false, t0);
        ctrl.update_with_time(false, false, false, t0 + Duration::from_secs(10));
        assert!(ctrl.is_armed());

        // First motion starts recording
        let t1 = t0 + Duration::from_secs(11);
        assert_eq!(ctrl.handle_motion(true, false, t1), PirMotionAction::StartRecording);

        // Repeated motion events at +5s intervals extend the deadline each time
        for step in 1..=10 {
            let t_step = t1 + Duration::from_secs(step * 5);
            assert_eq!(ctrl.handle_motion(true, true, t_step), PirMotionAction::None);
            assert_eq!(ctrl.remaining_recording_secs(t_step), Some(20));
        }

        // 50 seconds since t1, but last motion was at +50s. At +65s (15s after last motion):
        let t_check = t1 + Duration::from_secs(65);
        assert_eq!(ctrl.handle_motion(false, true, t_check), PirMotionAction::None);
        assert_eq!(ctrl.remaining_recording_secs(t_check), Some(5));

        // At +70s (20s after last motion) -> times out and cleanly stops
        let t_timeout = t1 + Duration::from_secs(70);
        assert_eq!(ctrl.handle_motion(false, true, t_timeout), PirMotionAction::StopRecordingTimeout);
        assert!(ctrl.is_armed());
        assert_eq!(ctrl.remaining_recording_secs(t_timeout), None);
    }

    #[test]
    fn pir_sound_detection_resets_20s_timeout() {
        let mut ctrl = PirButtonController::with_durations(Duration::from_secs(10), Duration::from_secs(20));
        let t0 = Instant::now();

        // Arm and enter armed state
        ctrl.update_with_time(true, false, false, t0);
        ctrl.update_with_time(false, false, false, t0 + Duration::from_secs(10));
        assert!(ctrl.is_armed());

        // Initial motion triggers recording
        let t_motion = t0 + Duration::from_secs(11);
        assert_eq!(ctrl.handle_activity(true, false, false, t_motion), PirMotionAction::StartRecording);

        // At +15s, no motion but sound detected -> resets timer!
        let t_sound = t_motion + Duration::from_secs(15);
        assert_eq!(ctrl.handle_activity(false, true, true, t_sound), PirMotionAction::None);
        assert_eq!(ctrl.remaining_recording_secs(t_sound), Some(20));

        // At +10s after sound (25s after initial motion), 10s remain
        let t_check = t_sound + Duration::from_secs(10);
        assert_eq!(ctrl.handle_activity(false, false, true, t_check), PirMotionAction::None);
        assert_eq!(ctrl.remaining_recording_secs(t_check), Some(10));

        // At +20s after sound -> StopRecordingTimeout
        let t_timeout = t_sound + Duration::from_secs(20);
        assert_eq!(ctrl.handle_activity(false, false, true, t_timeout), PirMotionAction::StopRecordingTimeout);
        assert!(ctrl.is_armed());
    }

    #[test]
    fn pir_button_start_arming_begins_10s_delay() {
        let mut ctrl = PirButtonController::new(Duration::from_secs(10));
        let t0 = Instant::now();
        assert!(ctrl.is_idle());

        ctrl.start_arming(t0);
        assert!(ctrl.is_arming());
        assert_eq!(ctrl.arming_countdown_secs(t0), Some(10));
        assert_eq!(ctrl.arming_countdown_secs(t0 + Duration::from_secs(4)), Some(6));

        // After 10s, update advances state to armed
        ctrl.update_with_time(false, false, false, t0 + Duration::from_secs(10));
        assert!(ctrl.is_armed());
    }
}
