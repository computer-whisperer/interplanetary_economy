//! Simulation time.
//!
//! Time is TDB throughout. The game ignores the ~69 s TDB–UTC offset, so a
//! calendar date is read as TDB midnight.

use std::fmt;
use std::str::FromStr;

/// Julian date of the J2000.0 epoch (2000-01-01 12:00 TDB).
pub const J2000_JD: f64 = 2_451_545.0;
pub const SECONDS_PER_DAY: f64 = 86_400.0;
pub const DAYS_PER_CENTURY: f64 = 36_525.0;

/// An instant, as TDB days since J2000.0.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Epoch(pub f64);

impl Epoch {
    pub fn from_jd(jd: f64) -> Self {
        Epoch(jd - J2000_JD)
    }

    pub fn jd(self) -> f64 {
        self.0 + J2000_JD
    }

    /// Proleptic Gregorian calendar date at 00:00 TDB (Meeus, ch. 7).
    pub fn from_ymd(year: i32, month: u32, day: u32) -> Self {
        let (mut y, mut m) = (year as f64, month as f64);
        if m <= 2.0 {
            y -= 1.0;
            m += 12.0;
        }
        let a = (y / 100.0).floor();
        let b = 2.0 - a + (a / 4.0).floor();
        let jd = (365.25 * (y + 4716.0)).floor() + (30.6001 * (m + 1.0)).floor() + day as f64 + b
            - 1524.5;
        Epoch::from_jd(jd)
    }

    /// Calendar date (year, month, day-with-fraction), Gregorian (Meeus, ch. 7).
    pub fn ymd(self) -> (i32, u32, f64) {
        let jd = self.jd() + 0.5;
        let z = jd.floor();
        let f = jd - z;
        let alpha = ((z - 1_867_216.25) / 36_524.25).floor();
        let a = z + 1.0 + alpha - (alpha / 4.0).floor();
        let b = a + 1524.0;
        let c = ((b - 122.1) / 365.25).floor();
        let d = (365.25 * c).floor();
        let e = ((b - d) / 30.6001).floor();
        let day = b - d - (30.6001 * e).floor() + f;
        let month = if e < 14.0 { e - 1.0 } else { e - 13.0 };
        let year = if month > 2.0 { c - 4716.0 } else { c - 4715.0 };
        (year as i32, month as u32, day)
    }

    /// Julian centuries since J2000.0.
    pub fn centuries(self) -> f64 {
        self.0 / DAYS_PER_CENTURY
    }

    pub fn seconds(self) -> f64 {
        self.0 * SECONDS_PER_DAY
    }

    pub fn plus_days(self, days: f64) -> Self {
        Epoch(self.0 + days)
    }
}

impl fmt::Display for Epoch {
    /// `YYYY-MM-DD`, truncated to the day.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (y, m, d) = self.ymd();
        write!(f, "{y:04}-{m:02}-{:02}", d.floor() as u32)
    }
}

impl FromStr for Epoch {
    type Err = String;

    /// Parses `YYYY-MM-DD`.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bad = || format!("expected YYYY-MM-DD, got {s:?}");
        let mut parts = s.split('-');
        let y = parts.next().and_then(|p| p.parse().ok()).ok_or_else(bad)?;
        let m: u32 = parts.next().and_then(|p| p.parse().ok()).ok_or_else(bad)?;
        let d: u32 = parts.next().and_then(|p| p.parse().ok()).ok_or_else(bad)?;
        if parts.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
            return Err(bad());
        }
        Ok(Epoch::from_ymd(y, m, d))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn j2000_and_known_dates() {
        // 2000-01-01 00:00 is half a day before J2000.0.
        assert_eq!(Epoch::from_ymd(2000, 1, 1).jd(), 2_451_544.5);
        // Horizons: 2040-01-01 00:00 TDB is JD 2466154.5.
        assert_eq!(Epoch::from_ymd(2040, 1, 1).jd(), 2_466_154.5);
        assert_eq!(Epoch::from_ymd(2074, 7, 1).jd(), 2_478_754.5);
    }

    #[test]
    fn calendar_round_trip() {
        for jd in (2_440_000..2_490_000).step_by(37) {
            let e = Epoch::from_jd(jd as f64 + 0.5);
            let (y, m, d) = e.ymd();
            assert_eq!(Epoch::from_ymd(y, m, d as u32).jd(), e.jd(), "{y}-{m}-{d}");
        }
    }

    #[test]
    fn parse_and_display() {
        let e: Epoch = "2033-04-28".parse().unwrap();
        assert_eq!(e.to_string(), "2033-04-28");
        assert!("2033-13-01".parse::<Epoch>().is_err());
        assert!("2033-04".parse::<Epoch>().is_err());
    }
}
