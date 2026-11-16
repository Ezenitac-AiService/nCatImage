//! Toolkit-independent animation controls.
pub const SPEEDS: [f64; 9] = [0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 2.0, 3.0, 4.0];
#[derive(Clone, Copy, Debug)]
pub struct Playback {
    pub active: bool,
    pub paused: bool,
    pub ended: bool,
    pub busy: bool,
    pub index: u32,
    pub cycle: u32,
    pub total: Option<u32>,
    pub delay_ms: u32,
    pub speed: f64,
}

impl Default for Playback {
    fn default() -> Self {
        Self {
            active: false,
            paused: false,
            ended: false,
            busy: false,
            index: 0,
            cycle: 0,
            total: None,
            delay_ms: 100,
            speed: 1.0,
        }
    }
}

impl Playback {
    /// Controls stay visually stable during every automatic decode.
    pub fn frame_controls_enabled(&self) -> bool {
        self.active
    }

    pub fn can_jump(&self) -> bool {
        self.active && self.paused
    }

    pub fn delay(&self) -> u64 {
        (f64::from(self.delay_ms) / self.speed.clamp(0.25, 4.0))
            .ceil()
            .clamp(1.0, 240_000.0) as u64
    }
    pub fn set_speed(&mut self, speed: f64) {
        if speed.is_finite() {
            self.speed = speed.clamp(0.25, 4.0);
        }
    }
    pub fn displayed(&mut self, index: u32, cycle: u32, delay_ms: u32) {
        if self.active && cycle > self.cycle {
            self.total = Some(self.index + 1);
        }
        self.active = true;
        self.busy = false;
        self.ended = false;
        self.index = index;
        self.cycle = cycle;
        self.delay_ms = delay_ms;
    }
    pub fn finish(&mut self) {
        self.total = Some(self.index + 1);
        self.busy = false;
        self.ended = true;
        self.paused = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decode_and_frame_boundaries_do_not_flash_controls() {
        let mut state = Playback::default();
        assert!(!state.frame_controls_enabled());
        state.displayed(0, 0, 10);
        assert!(state.frame_controls_enabled());
        state.busy = true;
        assert!(state.frame_controls_enabled());
        assert!(!state.can_jump());
        state.paused = true;
        assert!(state.can_jump());
        state.finish();
        assert!(state.frame_controls_enabled() && state.can_jump());
    }
    #[test]
    fn speed_changes_delay_without_zero_or_unbounded_timers() {
        let mut state = Playback::default();
        state.set_speed(4.0);
        assert_eq!(state.delay(), 25);
        state.set_speed(0.25);
        assert_eq!(state.delay(), 400);
        state.set_speed(f64::NAN);
        assert_eq!(state.speed, 0.25);
    }
    #[test]
    fn total_is_known_after_wrap_or_end_not_guessed_from_first_frame() {
        let mut state = Playback::default();
        state.displayed(0, 0, 10);
        assert_eq!(state.total, None);
        state.displayed(2, 0, 10);
        state.displayed(0, 1, 10);
        assert_eq!(state.total, Some(3));
        state.displayed(2, 1, 10);
        state.finish();
        assert!(state.paused && state.ended);
    }
}
