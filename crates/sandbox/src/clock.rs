//! Playback (M1c.md, step 4 plan, item 6): the simulation's clock, its
//! time warp in named steps, seeking, and jumps between events.

use orbit::time::{Epoch, SECONDS_PER_DAY};

/// Time warp steps: simulated seconds per second of wall time, and their
/// names.
pub const RATES: [(f64, &str); 10] = [
    (1.0, "1 s"),
    (10.0, "10 s"),
    (60.0, "1 min"),
    (600.0, "10 min"),
    (3_600.0, "1 h"),
    (6.0 * 3_600.0, "6 h"),
    (SECONDS_PER_DAY, "1 day"),
    (7.0 * SECONDS_PER_DAY, "1 week"),
    (30.0 * SECONDS_PER_DAY, "1 month"),
    (365.25 * SECONDS_PER_DAY, "1 year"),
];

/// A jump to an event lands past events closer than this, s: the one it
/// is at.
const SAME_EVENT_S: f64 = 1.0;

#[derive(Clone, Copy, Debug)]
pub struct Clock {
    pub t: Epoch,
    /// Into [`RATES`].
    pub rate: usize,
    pub paused: bool,
    /// The span the ephemeris covers.
    pub span: (Epoch, Epoch),
    /// Simulated seconds the last tick advanced (0 paused, or held at the
    /// span's end): what a frame spans.
    pub stepped_s: f64,
}

impl Clock {
    pub fn new(t: Epoch, span: (Epoch, Epoch)) -> Clock {
        Clock {
            t,
            rate: 0,
            paused: false,
            span,
            stepped_s: 0.0,
        }
    }

    /// Advances by `dt` seconds of wall time, within the span.
    pub fn tick(&mut self, dt: f64) {
        let before = self.t;
        if !self.paused {
            self.seek(Epoch(self.t.0 + dt * RATES[self.rate].0 / SECONDS_PER_DAY));
        }
        self.stepped_s = (self.t.0 - before.0) * SECONDS_PER_DAY;
    }

    /// Moves to `t`, within the span.
    pub fn seek(&mut self, t: Epoch) {
        self.t = Epoch(t.0.clamp(self.span.0.0, self.span.1.0));
    }

    pub fn faster(&mut self) {
        self.rate = (self.rate + 1).min(RATES.len() - 1);
    }

    pub fn slower(&mut self) {
        self.rate = self.rate.saturating_sub(1);
    }

    /// Moves to the first of `events` (sorted) after now; `false` if
    /// there is none.
    pub fn next(&mut self, events: &[Epoch]) -> bool {
        let after = self.t.0 + SAME_EVENT_S / SECONDS_PER_DAY;
        match events.iter().find(|e| e.0 > after) {
            Some(&e) => {
                self.seek(e);
                true
            }
            None => false,
        }
    }

    /// Moves to the last of `events` (sorted) before now; `false` if there
    /// is none.
    pub fn previous(&mut self, events: &[Epoch]) -> bool {
        let before = self.t.0 - SAME_EVENT_S / SECONDS_PER_DAY;
        match events.iter().rev().find(|e| e.0 < before) {
            Some(&e) => {
                self.seek(e);
                true
            }
            None => false,
        }
    }

    /// The rate in words: "1 day/s", with ", paused".
    pub fn rate_text(&self) -> String {
        format!(
            "{}/s{}",
            RATES[self.rate].1,
            if self.paused { ", paused" } else { "" }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rates_and_span() {
        let span = (Epoch::from_ymd(2025, 1, 1), Epoch::from_ymd(2080, 1, 1));
        let mut c = Clock::new(Epoch::from_ymd(2079, 12, 31), span);
        assert_eq!(c.rate_text(), "1 s/s");
        for _ in 0..8 {
            c.faster();
        }
        assert_eq!(c.rate_text(), "1 month/s");
        c.tick(1.0);
        assert_eq!(c.t.0, span.1.0, "held at the span's end");
        for _ in 0..20 {
            c.faster();
        }
        assert_eq!(c.rate_text(), "1 year/s");
        c.paused = true;
        let t = c.t;
        c.tick(1.0);
        assert_eq!(c.t, t);
        assert_eq!(c.rate_text(), "1 year/s, paused");
        for _ in 0..20 {
            c.slower();
        }
        assert_eq!(c.rate, 0);
    }

    /// Jumps go to the next and previous events, past the one the clock is
    /// at; with none left, the clock stays.
    #[test]
    fn jumps_between_events() {
        let span = (Epoch::from_ymd(2025, 1, 1), Epoch::from_ymd(2080, 1, 1));
        let d = |day: f64| Epoch(Epoch::from_ymd(2045, 3, 1).0 + day);
        let events = [d(0.0), d(2.5), d(2.5), d(7.0)];
        let mut c = Clock::new(d(1.0), span);
        assert!(c.next(&events));
        assert_eq!(c.t, d(2.5));
        assert!(c.next(&events));
        assert_eq!(c.t, d(7.0));
        assert!(!c.next(&events));
        assert_eq!(c.t, d(7.0));
        assert!(c.previous(&events));
        assert_eq!(c.t, d(2.5));
        assert!(c.previous(&events));
        assert!(!c.previous(&events));
        assert_eq!(c.t, d(0.0));
    }
}
