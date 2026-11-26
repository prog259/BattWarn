// The warning decision logic.
//
// Deliberately free of any Windows imports so it can be exercised on any
// platform (see `tools/engine_check.rs`).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Low,
    Critical,
    Final,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Warn {
    pub percent: u8,
    pub level: Level,
}

/// Decides when to nag, and how loudly.
///
/// Rules:
///  * On AC or charging -> fully re-armed, never warn.
///  * Above the threshold -> re-armed, never warn.
///  * At/below the threshold -> warn once, then only again every `step`
///    percent (20 -> 15 -> 10 -> 5), so dismissing the popup does not make it
///    disappear forever.
pub struct Engine {
    threshold: u8,
    step: u8,
    next_at: u8,
    armed: bool,
}

impl Engine {
    pub fn new(threshold: u8, step: u8) -> Self {
        Engine {
            threshold,
            step: step.max(1),
            next_at: threshold,
            armed: true,
        }
    }

    pub fn on_sample(&mut self, percent: u8, ac_online: bool, charging: bool) -> Option<Warn> {
        if ac_online || charging {
            self.next_at = self.threshold;
            self.armed = true;
            return None;
        }

        if percent > self.threshold {
            self.next_at = self.threshold;
            self.armed = true;
            return None;
        }

        if !self.armed || percent > self.next_at {
            return None;
        }

        let level = if percent <= 5 {
            Level::Final
        } else if percent <= 10 {
            Level::Critical
        } else {
            Level::Low
        };

        self.next_at = percent.saturating_sub(self.step);
        if self.next_at == 0 {
            self.armed = false;
        }

        Some(Warn { percent, level })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fires_at_threshold_once() {
        let mut e = Engine::new(20, 5);
        assert_eq!(e.on_sample(21, false, false), None);
        assert_eq!(
            e.on_sample(20, false, false),
            Some(Warn { percent: 20, level: Level::Low })
        );
        assert_eq!(e.on_sample(19, false, false), None);
        assert_eq!(e.on_sample(16, false, false), None);
        assert_eq!(
            e.on_sample(15, false, false),
            Some(Warn { percent: 15, level: Level::Low })
        );
    }

    #[test]
    fn charging_silences_and_rearms() {
        let mut e = Engine::new(20, 5);
        assert!(e.on_sample(12, true, false).is_none());
        assert!(e.on_sample(12, false, true).is_none());
        assert!(e.on_sample(12, false, false).is_some());
        // back on power, then off again at the same level: warns again
        assert!(e.on_sample(50, true, false).is_none());
        assert!(e.on_sample(12, false, false).is_some());
    }

    #[test]
    fn jumps_past_several_levels_warn_once() {
        let mut e = Engine::new(20, 5);
        assert_eq!(
            e.on_sample(9, false, false),
            Some(Warn { percent: 9, level: Level::Critical })
        );
        assert_eq!(e.on_sample(8, false, false), None);
        assert_eq!(
            e.on_sample(4, false, false),
            Some(Warn { percent: 4, level: Level::Final })
        );
        assert_eq!(e.on_sample(3, false, false), None);
    }

    #[test]
    fn stops_at_rock_bottom() {
        let mut e = Engine::new(20, 5);
        assert!(e.on_sample(1, false, false).is_some());
        assert!(e.on_sample(1, false, false).is_none());
        assert!(e.on_sample(0, false, false).is_none());
    }
}
