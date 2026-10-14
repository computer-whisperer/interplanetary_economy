//! A minimal simulation clock (M1c.md, step 3 plan, item 7), so that
//! following a moving body can be seen; step 4 builds playback.

use orbit::time::{Epoch, SECONDS_PER_DAY};

/// Rates run in steps of tenfold, from real time to this, simulated
/// seconds per second (about three years).
const MAX_RATE: f64 = 1e8;

#[derive(Clone, Copy, Debug)]
pub struct Clock {
    pub t: Epoch,
    /// Simulated seconds per second of wall time.
    pub rate: f64,
    pub paused: bool,
    /// The span the ephemeris covers.
    pub span: (Epoch, Epoch),
}

impl Clock {
    pub fn new(t: Epoch, span: (Epoch, Epoch)) -> Clock {
        Clock {
            t,
            rate: 1.0,
            paused: false,
            span,
        }
    }

    /// Advances by `dt` seconds of wall time, within the span.
    pub fn tick(&mut self, dt: f64) {
        if !self.paused {
            let t = self.t.0 + dt * self.rate / SECONDS_PER_DAY;
            self.t = Epoch(t.clamp(self.span.0.0, self.span.1.0));
        }
    }

    pub fn faster(&mut self) {
        self.rate = (self.rate * 10.0).min(MAX_RATE);
    }

    pub fn slower(&mut self) {
        self.rate = (self.rate / 10.0).max(1.0);
    }

    /// The rate in words: "1 s/s", "1 d/s".
    pub fn rate_text(&self) -> String {
        let r = self.rate;
        let (value, unit) = if r < 60.0 {
            (r, "s")
        } else if r < 3_600.0 {
            (r / 60.0, "min")
        } else if r < SECONDS_PER_DAY {
            (r / 3_600.0, "h")
        } else if r < 365.25 * SECONDS_PER_DAY {
            (r / SECONDS_PER_DAY, "d")
        } else {
            (r / 365.25 / SECONDS_PER_DAY, "yr")
        };
        let value = if value < 10.0 {
            format!("{value:.1}")
        } else {
            format!("{value:.0}")
        };
        format!(
            "{value} {unit}/s{}",
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
        assert_eq!(c.rate_text(), "1.0 s/s");
        for _ in 0..5 {
            c.faster();
        }
        assert_eq!(c.rate_text(), "1.2 d/s");
        c.tick(10.0);
        assert_eq!(c.t.0, span.1.0, "held at the span's end");
        for _ in 0..20 {
            c.faster();
        }
        assert_eq!(c.rate, MAX_RATE);
        c.paused = true;
        let t = c.t;
        c.tick(1.0);
        assert_eq!(c.t, t);
    }
}
