//! Chess clock: two countdown timers with an optional per-move increment.
//! Time is supplied by the caller (seconds from any monotonic source), so
//! the clock itself is deterministic and easy to test.

use shakmaty::Color;

/// Base time plus increment per move, e.g. 5+3.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimeControl {
    pub base_secs: u32,
    pub increment_secs: u32,
}

impl TimeControl {
    const fn new(base_minutes: u32, increment_secs: u32) -> Self {
        Self {
            base_secs: base_minutes * 60,
            increment_secs,
        }
    }
}

/// Label and time control; `None` plays without a clock.
pub const PRESETS: &[(&str, Option<TimeControl>)] = &[
    ("Unlimited", None),
    ("1+0", Some(TimeControl::new(1, 0))),
    ("3+2", Some(TimeControl::new(3, 2))),
    ("5+0", Some(TimeControl::new(5, 0))),
    ("10+0", Some(TimeControl::new(10, 0))),
    ("15+10", Some(TimeControl::new(15, 10))),
    ("30+0", Some(TimeControl::new(30, 0))),
];

/// Look a preset up by label. Unknown labels play without a clock.
pub fn preset(label: &str) -> Option<TimeControl> {
    PRESETS
        .iter()
        .find(|(name, _)| *name == label)
        .and_then(|(_, tc)| *tc)
}

#[derive(Clone, Debug)]
pub struct Clock {
    /// Milliseconds left for White and Black.
    remaining_ms: [i64; 2],
    increment_ms: i64,
    /// Whose clock is counting down; `None` while stopped.
    running: Option<Color>,
    /// Time of the last update, in seconds.
    last_tick: f64,
    flagged: Option<Color>,
}

fn idx(color: Color) -> usize {
    match color {
        Color::White => 0,
        Color::Black => 1,
    }
}

impl Clock {
    pub fn new(tc: TimeControl) -> Self {
        let base = i64::from(tc.base_secs) * 1000;
        Self {
            remaining_ms: [base, base],
            increment_ms: i64::from(tc.increment_secs) * 1000,
            running: None,
            last_tick: 0.0,
            flagged: None,
        }
    }

    pub fn remaining_ms(&self, color: Color) -> i64 {
        self.remaining_ms[idx(color)]
    }

    pub fn increment_ms(&self) -> i64 {
        self.increment_ms
    }

    pub fn running(&self) -> Option<Color> {
        self.running
    }

    /// The side that ran out of time, if any.
    pub fn flagged(&self) -> Option<Color> {
        self.flagged
    }

    /// Charge the elapsed time to the running side.
    pub fn tick(&mut self, now: f64) {
        if let Some(color) = self.running {
            let elapsed = ((now - self.last_tick) * 1000.0).round() as i64;
            let left = &mut self.remaining_ms[idx(color)];
            *left = (*left - elapsed.max(0)).max(0);
            if *left == 0 {
                self.flagged = Some(color);
                self.running = None;
            }
        }
        self.last_tick = now;
    }

    /// `mover` has just made a move. Their clock stops and gains the
    /// increment, and the opponent's clock starts. The very first move of a
    /// game starts the clocks without charging or rewarding anyone.
    pub fn on_move(&mut self, mover: Color, now: f64) {
        if self.flagged.is_some() {
            return;
        }
        if self.running == Some(mover) {
            self.tick(now);
            if self.flagged.is_some() {
                return;
            }
            self.remaining_ms[idx(mover)] += self.increment_ms;
        }
        self.running = Some(mover.other());
        self.last_tick = now;
    }

    /// Stop both clocks, charging the time used so far.
    pub fn pause(&mut self, now: f64) {
        self.tick(now);
        self.running = None;
    }
}

/// `m:ss` normally, `h:mm:ss` from an hour up, and tenths of a second in
/// the last ten seconds (`0:07.4`).
pub fn format_clock(ms: i64) -> String {
    let ms = ms.max(0);
    if ms < 10_000 {
        return format!("0:{:02}.{}", ms / 1000, (ms % 1000) / 100);
    }
    // Round up so a clock showing 0:10 still has at least ten seconds.
    let total = (ms + 999) / 1000;
    let (h, m, s) = (total / 3600, (total % 3600) / 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Color::{Black, White};

    fn clock(base_minutes: u32, inc: u32) -> Clock {
        Clock::new(TimeControl::new(base_minutes, inc))
    }

    #[test]
    fn starts_stopped_with_full_time() {
        let c = clock(5, 0);
        assert_eq!(c.remaining_ms(White), 300_000);
        assert_eq!(c.remaining_ms(Black), 300_000);
        assert_eq!(c.running(), None);
        assert_eq!(c.flagged(), None);
    }

    #[test]
    fn nothing_is_charged_before_the_first_move() {
        let mut c = clock(5, 0);
        c.tick(42.0);
        assert_eq!(c.remaining_ms(White), 300_000);
        assert_eq!(c.remaining_ms(Black), 300_000);
    }

    #[test]
    fn first_move_starts_the_opponents_clock_free_of_charge() {
        let mut c = clock(5, 3);
        c.on_move(White, 10.0);
        assert_eq!(c.running(), Some(Black));
        // No deduction and no increment for the opening move.
        assert_eq!(c.remaining_ms(White), 300_000);

        c.tick(14.0);
        assert_eq!(c.remaining_ms(Black), 296_000);
        assert_eq!(c.remaining_ms(White), 300_000);
    }

    #[test]
    fn moving_charges_time_and_adds_increment() {
        let mut c = clock(3, 2);
        c.on_move(White, 0.0);
        c.on_move(Black, 5.0); // Black used 5 s, gains 2 s
        assert_eq!(c.remaining_ms(Black), 177_000);
        assert_eq!(c.running(), Some(White));

        c.on_move(White, 6.5); // White used 1.5 s, gains 2 s
        assert_eq!(c.remaining_ms(White), 180_500);
        assert_eq!(c.running(), Some(Black));
    }

    #[test]
    fn running_out_of_time_flags_and_stops() {
        let mut c = clock(1, 0);
        c.on_move(White, 0.0);
        c.tick(59.0);
        assert_eq!(c.flagged(), None);
        c.tick(61.0);
        assert_eq!(c.remaining_ms(Black), 0);
        assert_eq!(c.flagged(), Some(Black));
        assert_eq!(c.running(), None);

        // A move arriving after the flag changes nothing.
        c.on_move(Black, 62.0);
        assert_eq!(c.remaining_ms(Black), 0);
        assert_eq!(c.running(), None);
    }

    #[test]
    fn move_made_after_time_expired_is_a_flag_not_an_increment() {
        let mut c = clock(1, 10);
        c.on_move(White, 0.0);
        c.on_move(Black, 75.0);
        assert_eq!(c.flagged(), Some(Black));
        assert_eq!(c.remaining_ms(Black), 0);
    }

    #[test]
    fn pause_stops_the_countdown() {
        let mut c = clock(5, 0);
        c.on_move(White, 0.0);
        c.pause(10.0);
        assert_eq!(c.remaining_ms(Black), 290_000);
        c.tick(500.0);
        assert_eq!(c.remaining_ms(Black), 290_000);

        // Resumes with the next move, again without charging the pause.
        c.on_move(Black, 600.0);
        assert_eq!(c.remaining_ms(Black), 290_000);
        assert_eq!(c.running(), Some(White));
    }

    #[test]
    fn time_going_backwards_is_ignored() {
        let mut c = clock(5, 0);
        c.on_move(White, 100.0);
        c.tick(90.0);
        assert_eq!(c.remaining_ms(Black), 300_000);
    }

    #[test]
    fn formats_minutes_hours_and_tenths() {
        assert_eq!(format_clock(300_000), "5:00");
        assert_eq!(format_clock(299_001), "5:00");
        assert_eq!(format_clock(65_000), "1:05");
        assert_eq!(format_clock(10_000), "0:10");
        assert_eq!(format_clock(9_999), "0:09.9");
        assert_eq!(format_clock(7_450), "0:07.4");
        assert_eq!(format_clock(0), "0:00.0");
        assert_eq!(format_clock(-5), "0:00.0");
        assert_eq!(format_clock(3_600_000), "1:00:00");
        assert_eq!(format_clock(5_400_000), "1:30:00");
    }

    #[test]
    fn presets_resolve_by_label() {
        assert_eq!(preset("Unlimited"), None);
        assert_eq!(preset("no such preset"), None);
        assert_eq!(
            preset("3+2"),
            Some(TimeControl {
                base_secs: 180,
                increment_secs: 2
            })
        );
        assert_eq!(
            preset("15+10"),
            Some(TimeControl {
                base_secs: 900,
                increment_secs: 10
            })
        );
    }

    #[test]
    fn preset_labels_are_unique_and_match_their_values() {
        let mut seen = std::collections::HashSet::new();
        for (label, tc) in PRESETS {
            assert!(seen.insert(*label), "duplicate preset {label}");
            if let Some(tc) = tc {
                let expected = format!("{}+{}", tc.base_secs / 60, tc.increment_secs);
                assert_eq!(*label, expected);
            }
        }
    }
}
