//! The optimizer's clock: equal periods from a start date.

use orbit::time::Epoch;

pub use sim::params::DAYS_PER_YEAR;

#[derive(Clone, Copy, Debug)]
pub struct Calendar {
    pub start: Epoch,
    pub period_days: f64,
    pub periods: usize,
}

impl Calendar {
    /// Periods of one twelfth of a year covering `[start, end)`.
    pub fn monthly(start: Epoch, end: Epoch) -> Self {
        Self::with_period(start, end, DAYS_PER_YEAR / 12.0)
    }

    pub fn with_period(start: Epoch, end: Epoch, period_days: f64) -> Self {
        Calendar {
            start,
            period_days,
            periods: ((end.0 - start.0) / period_days).round() as usize,
        }
    }

    pub fn end(&self) -> Epoch {
        self.start_of(self.periods)
    }

    /// The period containing `t`, if any.
    pub fn period_of(&self, t: Epoch) -> Option<usize> {
        let p = ((t.0 - self.start.0) / self.period_days).floor();
        (p >= 0.0 && p < self.periods as f64).then_some(p as usize)
    }

    pub fn start_of(&self, p: usize) -> Epoch {
        self.start.plus_days(p as f64 * self.period_days)
    }

    /// Length of a period in years.
    pub fn period_years(&self) -> f64 {
        self.period_days / DAYS_PER_YEAR
    }

    /// Years from the start to the middle of period `p`.
    pub fn years_at(&self, p: usize) -> f64 {
        (p as f64 + 0.5) * self.period_years()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monthly_covers_the_scenario() {
        let cal = Calendar::monthly(Epoch::from_ymd(2040, 1, 1), Epoch::from_ymd(2076, 1, 1));
        assert_eq!(cal.periods, 432);
        assert_eq!(cal.period_of(cal.start), Some(0));
        assert_eq!(cal.period_of(cal.start.plus_days(-0.1)), None);
        assert_eq!(cal.period_of(cal.end().plus_days(-0.1)), Some(431));
        assert_eq!(cal.period_of(cal.end()), None);
        assert!((cal.end().0 - Epoch::from_ymd(2076, 1, 1).0).abs() < 1.0);
    }
}
