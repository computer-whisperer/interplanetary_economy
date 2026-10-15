//! The Sun's surface (M1c.md, step 6, part 11 plan): its light across the
//! disk, and on it granulation, spots and faculae from a model of the
//! solar cycle at the date. The constants are `tools/sun/photosphere.py`'s,
//! which names their sources.

use orbit::time::Epoch;
use render::photosphere::{Feature, Granulation, Photosphere};
use std::f64::consts::TAU;

/// Each channel's light at μ over its disk's mean, a polynomial in μ
/// from μ⁰: Neckel and Labs 1994's limb darkening through the channels,
/// weighed by the disk center's spectrum. The center 1.21, 1.25 and 1.32
/// times the mean in red, green and blue; at μ 0.1, 0.54, 0.47 and 0.36.
pub const LIMB: [[f32; 6]; 3] = [
    [0.39638, 1.65543, -2.39083, 3.21023, -2.37271, 0.70779],
    [0.32346, 1.63904, -1.76888, 2.00215, -1.29298, 0.34448],
    [0.20208, 1.73488, -1.61068, 2.03198, -1.46990, 0.43345],
];

/// The quiet surface's temperature, K: the disk center's brightness
/// temperature in green, where [`TEMPERATURES`] is 1.
pub const QUIET: f32 = 6247.0;

/// Each channel's light from a blackbody at a temperature over the quiet
/// surface's, from 3,000 K every 100 K.
pub const TEMPERATURES: [[f32; 3]; 46] = [
    [0.01915, 0.01004, 0.00317],
    [0.02449, 0.01336, 0.00457],
    [0.03084, 0.01746, 0.00644],
    [0.03829, 0.02244, 0.00886],
    [0.04693, 0.02843, 0.01196],
    [0.05685, 0.03552, 0.01586],
    [0.06813, 0.04385, 0.02067],
    [0.08085, 0.05351, 0.02655],
    [0.09507, 0.06462, 0.03363],
    [0.11087, 0.07728, 0.04207],
    [0.12830, 0.09160, 0.05202],
    [0.14741, 0.10768, 0.06364],
    [0.16824, 0.12561, 0.07710],
    [0.19084, 0.14549, 0.09256],
    [0.21525, 0.16739, 0.11017],
    [0.24147, 0.19139, 0.13010],
    [0.26955, 0.21757, 0.15252],
    [0.29950, 0.24598, 0.17757],
    [0.33132, 0.27670, 0.20541],
    [0.36503, 0.30977, 0.23619],
    [0.40063, 0.34524, 0.27005],
    [0.43812, 0.38316, 0.30714],
    [0.47750, 0.42355, 0.34758],
    [0.51877, 0.46645, 0.39151],
    [0.56190, 0.51189, 0.43903],
    [0.60689, 0.55988, 0.49028],
    [0.65372, 0.61044, 0.54534],
    [0.70239, 0.66358, 0.60432],
    [0.75286, 0.71931, 0.66732],
    [0.80512, 0.77763, 0.73441],
    [0.85915, 0.83854, 0.80568],
    [0.91493, 0.90203, 0.88120],
    [0.97243, 0.96810, 0.96103],
    [1.03162, 1.03674, 1.04523],
    [1.09248, 1.10793, 1.13385],
    [1.15500, 1.18166, 1.22694],
    [1.21913, 1.25791, 1.32453],
    [1.28485, 1.33666, 1.42666],
    [1.35214, 1.41789, 1.53337],
    [1.42096, 1.50158, 1.64466],
    [1.49130, 1.58769, 1.76055],
    [1.56312, 1.67621, 1.88107],
    [1.63639, 1.76711, 2.00622],
    [1.71109, 1.86035, 2.13599],
    [1.78720, 1.95591, 2.27040],
    [1.86467, 2.05376, 2.40942],
];

/// The faculae's temperature over the quiet surface's, K, at μ 0.1, 0.2,
/// … 1.0: Yeo et al. 2013's contrast at 617 nm at a plage's 300 G (6–8%
/// at its peak, near μ 0.3–0.4).
pub const FACULAR_EXCESS: [f32; 10] =
    [24.0, 87.0, 119.0, 128.0, 118.0, 96.0, 68.0, 40.0, 18.0, 9.0];

/// Spots' umbrae's temperature, K: 12% of the quiet surface's light at
/// 600 nm (Maltby et al. 1986's darkest large cores 3,900 K; Borrero and
/// Ichimoto 2011: 5–25% in the visible).
pub const UMBRA: f32 = 4000.0;

/// Their penumbrae's: 75% at 600 nm (Borrero and Ichimoto: 65–85%).
pub const PENUMBRA: f32 = 5765.0;

/// An umbra's share of its spot's radius: the penumbra 5–6 times the
/// umbra's area (Hathaway 2013).
const UMBRA_SHARE: f64 = 0.4;

/// The granulation: a jittered grid's cells, 1.46 a squared spacing on
/// the surface, so that 1,409 km gives 1.36 Mm² (Sánchez Cuberes et al.
/// 2000); each living 6 min (Hirzberger et al. 1999's mean); 255 K rms:
/// at disk center 18% rms contrast in green, 15.5% in red and 22.6% in
/// blue (Wedemeyer-Böhm and Rouppe van der Voort 2009's simulations: 18%,
/// 14% and 25% at 555, 668 and 450 nm), the disk 1.2% brighter for it
/// (Planck's curvature, and the pattern's skew toward bright granules).
const GRANULE_SPACING_KM: f64 = 1409.0;
const GRANULE_LIFE_S: f64 = 360.0;
const GRANULE_RMS_K: f32 = 255.0;

/// The Sun's surface at `t`, on a sphere of `radius_km`.
pub fn photosphere(radius_km: f64, t: Epoch) -> Photosphere {
    let mut features = Vec::new();
    for g in groups(t) {
        g.features(t.0, &mut features);
    }
    Photosphere {
        quiet: QUIET,
        temperatures: TEMPERATURES.to_vec(),
        first: 3000.0,
        step: 100.0,
        granulation: Some(Granulation {
            spacing_km: GRANULE_SPACING_KM,
            radius_km,
            lifetime_s: GRANULE_LIFE_S,
            rms_k: GRANULE_RMS_K,
        }),
        umbra: UMBRA,
        penumbra: PENUMBRA,
        facular_excess: FACULAR_EXCESS,
        features,
    }
}

/// `t` as a decimal year.
fn year(t: Epoch) -> f64 {
    2000.0 + (t.0 + 0.5) / 365.25
}

/// A solar cycle in Hathaway 2011's shape (his Eqs. 2–3): from `start`
/// (a decimal year), its smoothed maximum `max` (version 2 sunspot
/// numbers).
#[derive(Clone, Copy, Debug)]
struct Cycle {
    start: f64,
    max: f64,
}

/// Version 1 sunspot numbers over version 2's, for the shape's width.
const V1_PER_V2: f64 = 0.66;

impl Cycle {
    /// The `k`th from cycle 25: 25 from 2020.1 at 160.9 (SILSO: its
    /// minimum December 2019, its maximum October 2024 at 160.9); 26 from
    /// 2030.7 at 140 (the middle of published predictions, 121–158, with
    /// Gerçeker et al. 2025's maximum in June 2035); later ones every
    /// 11.04 years at 179 (cycles 1–24's mean length and maximum).
    fn nth(k: usize) -> Cycle {
        match k {
            0 => Cycle {
                start: 2020.1,
                max: 160.9,
            },
            _ => Cycle {
                start: 2030.7 + 11.04 * (k - 1) as f64,
                max: if k == 1 { 140.0 } else { 179.0 },
            },
        }
    }

    /// Its sunspot number in `year`.
    fn number(&self, year: f64) -> f64 {
        let b = 27.12 + 25.15 * (100.0 / (V1_PER_V2 * self.max)).powf(0.25);
        let x = (year - self.start) * 12.0 / b;
        if x <= 0.0 {
            return 0.0;
        }
        self.max * 2.0 * x.powi(3) / ((x * x).exp() - 0.71)
    }

    /// Its spots' mean latitude in `year`, degrees (Hathaway 2011, Eq. 4).
    fn latitude(&self, year: f64) -> f64 {
        28.0 * (-(year - self.start) * 12.0 / 90.0).exp()
    }
}

/// The cycles under way in `year`, with their sunspot numbers.
fn cycles(year: f64) -> impl Iterator<Item = (Cycle, f64)> {
    (0..)
        .map(Cycle::nth)
        .take_while(move |c| c.start < year)
        .map(move |c| (c, c.number(year)))
        .filter(|(_, n)| *n > 0.0)
}

/// The sunspot number (version 2) at `t`.
pub fn sunspot_number(t: Epoch) -> f64 {
    cycles(year(t)).map(|(_, n)| n).sum()
}

/// The visible hemisphere's spot area at sunspot number `n`, millionths
/// of a hemisphere (MSH): Nagovitsyn and Osipova 2021's fit to version 2
/// numbers.
pub fn spot_area(n: f64) -> f64 {
    9.08 * n + 0.0114 * n * n
}

/// Groups' largest areas (MSH): Baumann and Solanki 2005's fit, dN/dA ∝
/// exp(−(ln A − ln 62.2)² / (2 ln 2.45)), a Gaussian in ln A centered at
/// ln 62.2 + ln 2.45 (a median of 152 MSH); taken between these.
const LEAST_AREA: f64 = 10.0;
const MOST_AREA: f64 = 5000.0;
const AREA_LN: (f64, f64) = (4.130_355, 0.896_088);

/// A group grows over this many days, then decays at this rate
/// (Gnevyshev–Waldmeier); its plage, 10× its area, at half it.
const GROWTH_DAYS: f64 = 2.0;
const DECAY_MSH_PER_DAY: f64 = 10.0;
const PLAGE_PER_SPOTS: f64 = 10.0;

/// The days a group's plage may outlive its birth by.
const LONGEST_DAYS: f64 = GROWTH_DAYS + 2.0 * MOST_AREA / DECAY_MSH_PER_DAY;

/// A group's spots' area over its life, MSH·days, on average over the
/// distribution of largest areas: growth's A × GROWTH_DAYS / 2, decay's
/// A² / (2 × DECAY_MSH_PER_DAY).
fn mean_life_area() -> f64 {
    static MEAN: std::sync::OnceLock<f64> = std::sync::OnceLock::new();
    *MEAN.get_or_init(|| {
        let (lo, hi) = (LEAST_AREA.ln(), MOST_AREA.ln());
        let n = 2000;
        let (mut w, mut sum) = (0.0, 0.0);
        for i in 0..n {
            let u = lo + (hi - lo) * (i as f64 + 0.5) / n as f64;
            let p = (-(u - AREA_LN.0 - AREA_LN.1).powi(2) / (2.0 * AREA_LN.1)).exp();
            let a = u.exp();
            w += p;
            sum += p * (a * GROWTH_DAYS / 2.0 + a * a / (2.0 * DECAY_MSH_PER_DAY));
        }
        sum / w
    })
}

/// Groups born a day at `t`, over the whole Sun: so that the visible
/// hemisphere's spot area, half the Sun's on average, holds
/// [`spot_area`].
fn births_per_day(t: Epoch) -> f64 {
    2.0 * spot_area(sunspot_number(t)) / mean_life_area()
}

/// The Sun's rotation the body frame turns at, °/day (IAU: the
/// Carrington rate), and spots' at latitude φ (Balthasar et al. 1986),
/// sidereal.
const FRAME_RATE: f64 = 14.1844;
fn spot_rate(latitude: f64) -> f64 {
    14.551 - 2.87 * latitude.sin().powi(2)
}

/// An active region: its spots and their plage.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Group {
    /// Days since J2000.
    born: f64,
    /// Its spots' largest total area, MSH.
    area: f64,
    /// Radians, in the body frame: its latitude, its longitude at birth,
    /// and its axis's tilt toward the equator (Joy's law).
    latitude: f64,
    longitude: f64,
    tilt: f64,
    /// Its spots behind the leader.
    followers: u32,
    seed: u64,
}

impl Group {
    /// Its spots' total area at `day`, MSH.
    fn spots(&self, day: f64) -> f64 {
        let age = day - self.born;
        if age < 0.0 {
            0.0
        } else if age < GROWTH_DAYS {
            self.area * age / GROWTH_DAYS
        } else {
            (self.area - DECAY_MSH_PER_DAY * (age - GROWTH_DAYS)).max(0.0)
        }
    }

    /// Its plage's area at `day`, MSH.
    fn plage(&self, day: f64) -> f64 {
        let age = day - self.born;
        let area = if age < 0.0 {
            0.0
        } else if age < GROWTH_DAYS {
            self.area * age / GROWTH_DAYS
        } else {
            (self.area - DECAY_MSH_PER_DAY / 2.0 * (age - GROWTH_DAYS)).max(0.0)
        };
        PLAGE_PER_SPOTS * area
    }

    /// Its center at `day`, carried by differential rotation against the
    /// frame.
    fn center(&self, day: f64) -> (f64, f64) {
        let drift = (spot_rate(self.latitude) - FRAME_RATE).to_radians() * (day - self.born);
        (self.latitude, self.longitude + drift)
    }

    /// Its plage and spots at `day`, pushed onto `out`. The leader takes
    /// 60% of the area, ahead in rotation and toward the equator, its
    /// followers the rest; 3° + 0.008° per MSH of its largest area apart
    /// (made up: a 1,000 MSH group's 11°).
    fn features(&self, day: f64, out: &mut Vec<Feature>) {
        let (spots, plage) = (self.spots(day), self.plage(day));
        if spots <= 0.0 && plage <= 0.0 {
            return;
        }
        let (lat, lon) = self.center(day);
        let center = glam::DVec3::new(lat.cos() * lon.cos(), lat.cos() * lon.sin(), lat.sin());
        if plage > 0.0 {
            out.push(Feature::Facula {
                center,
                radius: cap_radius(plage),
                strength: 1.0,
            });
        }
        if spots <= 0.0 {
            return;
        }
        let east = glam::DVec3::new(-lon.sin(), lon.cos(), 0.0);
        let north = glam::DVec3::new(-lat.sin() * lon.cos(), -lat.sin() * lon.sin(), lat.cos());
        let axis = east * self.tilt.cos() - north * lat.signum() * self.tilt.sin();
        let half = (3.0 + 0.008 * self.area).to_radians() / 2.0;
        out.push(Feature::Spot {
            center: (center + axis * half).normalize(),
            radius: cap_radius(0.6 * spots),
            umbra: UMBRA_SHARE,
        });
        let mut rng = Rng::new(self.seed);
        let each = 0.4 * spots / f64::from(self.followers);
        for _ in 0..self.followers {
            let (r, a) = (half / 2.0 * rng.unit(), TAU * rng.unit());
            let off = east * (r * a.cos()) + north * (r * a.sin());
            out.push(Feature::Spot {
                center: (center - axis * half + off).normalize(),
                radius: cap_radius(each),
                umbra: UMBRA_SHARE,
            });
        }
    }
}

/// The radius of a cap of `msh` on the unit sphere, radians: its area,
/// msh × 10⁻⁶ × 2π, is π θ².
fn cap_radius(msh: f64) -> f64 {
    (2e-6 * msh).sqrt()
}

/// The groups whose spots or plage show at `t`.
fn groups(t: Epoch) -> Vec<Group> {
    let day = t.0;
    born_by(day.floor() as i64)
        .iter()
        .filter(|g| g.born <= day && (g.spots(day) > 0.0 || g.plage(day) > 0.0))
        .copied()
        .collect()
}

/// The groups born over the days whose groups may show on day `last`,
/// each day's births drawn from that day's seed, so that a date always
/// shows the same; kept for the last day asked.
fn born_by(last: i64) -> std::sync::Arc<Vec<Group>> {
    static KEPT: std::sync::Mutex<Option<(i64, std::sync::Arc<Vec<Group>>)>> =
        std::sync::Mutex::new(None);
    let mut kept = KEPT.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((day, groups)) = kept.as_ref()
        && *day == last
    {
        return groups.clone();
    }
    let groups = std::sync::Arc::new(births((last as f64 - LONGEST_DAYS).floor() as i64, last));
    *kept = Some((last, groups.clone()));
    groups
}

/// The groups born from day `first` to day `last`.
fn births(first: i64, last: i64) -> Vec<Group> {
    let mut out = Vec::new();
    for k in first..=last {
        let mut rng = Rng::new(k as u64 ^ 0x5eed_5a17_c0de_d00d);
        let midday = Epoch(k as f64 + 0.5);
        let y = year(midday);
        let n = sunspot_number(midday);
        for _ in 0..rng.poisson(births_per_day(midday)) {
            let born = k as f64 + rng.unit();
            let area = loop {
                let a = (AREA_LN.0 + AREA_LN.1 + AREA_LN.1.sqrt() * rng.normal()).exp();
                if (LEAST_AREA..=MOST_AREA).contains(&a) {
                    break a;
                }
            };
            // Of a cycle in proportion to its sunspot number; spread
            // 1.5° + 3.8° (1 − e^(−A/400)), A the hemisphere's area
            // (Hathaway 2015, Eq. 9).
            let mut pick = rng.unit() * n;
            let cycle = cycles(y)
                .find(|(_, c)| {
                    pick -= c;
                    pick <= 0.0
                })
                .or_else(|| cycles(y).last())
                .map(|(c, _)| c)
                .expect("a cycle under way");
            let spread = 1.5 + 3.8 * (1.0 - (-spot_area(n) / 2.0 / 400.0).exp());
            let latitude = (cycle.latitude(y) + spread * rng.normal()).abs().min(60.0);
            let hemisphere = if rng.unit() < 0.5 { 1.0 } else { -1.0 };
            let group = Group {
                born,
                area,
                latitude: (hemisphere * latitude).to_radians(),
                longitude: TAU * rng.unit(),
                // Joy's law: 0.27 φ, scattered by as much (Dasi-Espuig et
                // al. 2010).
                tilt: (0.27 * latitude + 5.0 * rng.normal()).to_radians(),
                followers: 1 + (3.0 * rng.unit()) as u32,
                seed: rng.next(),
            };
            out.push(group);
        }
    }
    out
}

/// SplitMix64.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Rng {
        let mut r = Rng(seed);
        r.next();
        r
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// In [0, 1).
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Standard normal (Box–Muller).
    fn normal(&mut self) -> f64 {
        let u = 1.0 - self.unit();
        (-2.0 * u.ln()).sqrt() * (TAU * self.unit()).cos()
    }

    /// Poisson with mean `mean` (Knuth's; `mean` is small).
    fn poisson(&mut self, mean: f64) -> u32 {
        let limit = (-mean).exp();
        let (mut k, mut p) = (0, self.unit());
        while p > limit {
            k += 1;
            p *= self.unit();
        }
        k
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(y: i32, m: u32) -> Epoch {
        Epoch::from_ymd(y, m, 1)
    }

    /// Cycle 25's maximum where SILSO has it (October 2024, 160.9); 26's
    /// in mid-2035; March 2045 on cycle 27's rise.
    #[test]
    fn the_cycles_peak_where_they_should() {
        let n = |y, m| sunspot_number(at(y, m));
        println!(
            "2019-12 {:.1}, 2024-10 {:.1}, 2030-09 {:.1}, 2035-06 {:.1}, 2045-03 {:.1}",
            n(2019, 12),
            n(2024, 10),
            n(2030, 9),
            n(2035, 6),
            n(2045, 3)
        );
        assert!((n(2024, 10) / 160.9 - 1.0).abs() < 0.03);
        assert!((n(2035, 6) / 140.0 - 1.0).abs() < 0.05);
        assert!(n(2019, 12) < 1.0 && n(2030, 9) < 20.0);
        assert!(n(2045, 3) > 130.0 && n(2045, 3) < n(2046, 6));
    }

    /// A date shows the same spots each time it is drawn.
    #[test]
    fn a_date_shows_the_same_surface() {
        let t = Epoch::from_ymd(2045, 3, 1);
        assert_eq!(groups(t), groups(t));
        assert!(!groups(t).is_empty());
    }

    /// Over a year near cycle 27's maximum, the Sun's spot area averages
    /// twice the visible hemisphere's at the sunspot number (within 25%:
    /// a year's few dozen groups are a small sample), and the groups
    /// showing on either hemisphere number about the version-1 Wolf
    /// number's ~0.6 SN / 15.
    #[test]
    fn the_spots_hold_the_cycles_area() {
        let (mut area, mut want, mut groups_seen) = (0.0, 0.0, 0.0);
        let days = 365;
        for d in 0..days {
            let t = Epoch(Epoch::from_ymd(2046, 1, 1).0 + d as f64);
            let gs = groups(t);
            area += gs.iter().map(|g| g.spots(t.0)).sum::<f64>();
            want += 2.0 * spot_area(sunspot_number(t));
            groups_seen += gs.iter().filter(|g| g.spots(t.0) > 0.0).count() as f64;
        }
        let (area, want, groups_seen) = (
            area / days as f64,
            want / days as f64,
            groups_seen / days as f64,
        );
        println!("the Sun's spot area {area:.0} MSH against {want:.0}; {groups_seen:.1} groups");
        assert!((area / want - 1.0).abs() < 0.25);
        assert!(groups_seen > 5.0 && groups_seen < 30.0);
    }

    /// Groups start high and end low: those born in a cycle's first two
    /// years lie further from the equator than those of its last two.
    #[test]
    fn latitudes_fall_through_the_cycle() {
        let mean = |from: Epoch, to: Epoch| {
            let gs: Vec<Group> = (0..((to.0 - from.0) as i64))
                .step_by(30)
                .flat_map(|d| groups(Epoch(from.0 + d as f64)))
                .collect();
            gs.iter()
                .map(|g| g.latitude.abs().to_degrees())
                .sum::<f64>()
                / gs.len() as f64
        };
        let (early, late) = (
            mean(at(2032, 6), at(2034, 6)),
            mean(at(2039, 1), at(2041, 1)),
        );
        println!("early {early:.1}°, late {late:.1}°");
        assert!(early > late + 8.0);
    }

    /// A group's leader is ahead in rotation (east, the frame turning
    /// about +z) and toward the equator, in either hemisphere.
    #[test]
    fn leaders_lead_toward_the_equator() {
        for lat in [20.0f64, -20.0] {
            let g = Group {
                born: 0.0,
                area: 500.0,
                latitude: lat.to_radians(),
                longitude: 1.0,
                tilt: 8f64.to_radians(),
                followers: 1,
                seed: 1,
            };
            let mut out = Vec::new();
            g.features(10.0, &mut out);
            let (glat, glon) = g.center(10.0);
            let Some(Feature::Spot { center, .. }) = out.get(1) else {
                panic!("a leader");
            };
            let (llat, llon) = (center.z.asin(), center.y.atan2(center.x));
            println!(
                "{lat}: leader at {:.2}°, {:.2}°; group at {:.2}°, {:.2}°",
                llat.to_degrees(),
                llon.to_degrees(),
                glat.to_degrees(),
                glon.to_degrees()
            );
            assert!(llon > glon, "{lat}: ahead");
            assert!(llat.abs() < glat.abs(), "{lat}: toward the equator");
        }
    }

    /// A group drifts against the frame at its latitude's rate: at the
    /// equator 0.37°/day ahead, at 30° 0.35°/day behind.
    #[test]
    fn groups_drift_with_differential_rotation() {
        let g = |latitude: f64| Group {
            born: 0.0,
            area: 100.0,
            latitude: latitude.to_radians(),
            longitude: 0.0,
            tilt: 0.0,
            followers: 1,
            seed: 1,
        };
        for (lat, want) in [(0.0, 0.3666), (30.0, -0.3509)] {
            let drift = g(lat).center(10.0).1.to_degrees() / 10.0;
            assert!((drift - want).abs() < 1e-3, "{lat}: {drift}");
        }
    }
}
