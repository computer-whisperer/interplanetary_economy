//! Earth–Moon periodic-orbit families, where stations can sit
//! (`data/orbits.toml`, sampled from JPL's catalog by `tools/orbits/`). Any
//! member between two catalog samples is found by interpolating and
//! correcting, so a station's orbit is a continuous choice within its family.

use crate::cr3bp::{Cr3bp, Fix};
use serde::Deserialize;
use std::sync::OnceLock;

/// A periodic orbit in the CR3BP: its start on the x-z plane and its period,
/// in CR3BP units.
#[derive(Clone, Copy, Debug)]
pub struct PeriodicOrbit {
    pub state: [f64; 6],
    pub period: f64,
}

/// A catalog sample: its orbit and what the catalog and the build tool
/// report about it.
#[derive(Clone, Copy, Debug)]
pub struct Member {
    pub orbit: PeriodicOrbit,
    pub jacobi: f64,
    pub stability: f64,
    /// Closest approaches to Earth's and the Moon's centers, km.
    pub perigee_km: f64,
    pub perilune_km: f64,
    /// The continuous stretch of the family this sample is on: samples
    /// are interpolated only within one.
    pub run: usize,
}

#[derive(Clone, Debug)]
pub struct Family {
    pub id: String,
    pub name: String,
    pub members: Vec<Member>,
}

/// The families, in the Earth–Moon CR3BP with JPL's mass ratio and units.
#[derive(Clone, Debug)]
pub struct Catalog {
    pub system: Cr3bp,
    pub length_km: f64,
    pub time_s: f64,
    pub families: Vec<Family>,
}

#[derive(Deserialize)]
struct File {
    mass_ratio: f64,
    length_unit_km: f64,
    time_unit_s: f64,
    family: Vec<FamilyRow>,
}

#[derive(Deserialize)]
struct FamilyRow {
    id: String,
    name: String,
    /// x, z, vy, period, jacobi, stability index, perigee km, perilune km,
    /// run.
    members: Vec<[f64; 9]>,
}

impl Catalog {
    /// The families in `data/orbits.toml`, loaded once per process.
    pub fn standard() -> &'static Catalog {
        static STANDARD: OnceLock<Catalog> = OnceLock::new();
        STANDARD.get_or_init(|| {
            Self::parse(include_str!("../../../data/orbits.toml"))
                .expect("data/orbits.toml is valid")
        })
    }

    pub fn parse(src: &str) -> Result<Catalog, String> {
        let file: File = toml::from_str(src).map_err(|e| e.to_string())?;
        let families = file
            .family
            .into_iter()
            .map(|f| Family {
                id: f.id,
                name: f.name,
                members: f
                    .members
                    .iter()
                    .map(
                        |&[
                            x,
                            z,
                            vy,
                            period,
                            jacobi,
                            stability,
                            perigee_km,
                            perilune_km,
                            run,
                        ]| Member {
                            orbit: PeriodicOrbit {
                                state: [x, 0.0, z, 0.0, vy, 0.0],
                                period,
                            },
                            jacobi,
                            stability,
                            perigee_km,
                            perilune_km,
                            run: run as usize,
                        },
                    )
                    .collect(),
            })
            .collect();
        Ok(Catalog {
            system: Cr3bp {
                mu: file.mass_ratio,
            },
            length_km: file.length_unit_km,
            time_s: file.time_unit_s,
            families,
        })
    }

    pub fn family(&self, id: &str) -> Option<&Family> {
        self.families.iter().find(|f| f.id == id)
    }

    /// The member at position `u` along a family, from 0 to the number of
    /// samples less one: the catalog sample at whole `u`, and between two
    /// samples their interpolation, corrected to a periodic orbit holding the
    /// starting coordinate (x or z) that changes more between them. Where a
    /// family bends too much for one correction, the member is reached by
    /// continuation from the nearer sample in 4, 16 or 64 corrected steps.
    /// `None` if `u` is out of range, falls between two runs, or every
    /// attempt fails.
    pub fn member(&self, family: &Family, u: f64) -> Option<PeriodicOrbit> {
        let last = family.members.len() - 1;
        if !(0.0..=last as f64).contains(&u) {
            return None;
        }
        let k = (u.floor() as usize).min(last.saturating_sub(1));
        let w = u - k as f64;
        let (a, b) = (
            family.members[k].orbit,
            family.members[(k + 1).min(last)].orbit,
        );
        if w == 0.0 {
            return Some(a);
        }
        if w == 1.0 {
            return Some(b);
        }
        if family.members[k].run != family.members[k + 1].run {
            return None;
        }
        let fix = if (b.state[2] - a.state[2]).abs() > (b.state[0] - a.state[0]).abs() {
            Fix::Z
        } else {
            Fix::X
        };
        // Start from the nearer sample and aim at the interpolated member;
        // `w` becomes the offset from that sample, in samples.
        let (from, w) = if w <= 0.5 { (a, w) } else { (b, w - 1.0) };
        let target = PeriodicOrbit {
            state: std::array::from_fn(|i| from.state[i] + (b.state[i] - a.state[i]) * w),
            period: from.period + (b.period - a.period) * w,
        };
        [1, 4, 16, 64]
            .into_iter()
            .find_map(|steps| self.continue_to(from, target, fix, steps))
    }

    /// Corrected orbits from `from` toward `target` in `steps` equal steps;
    /// each guess is the previous solution plus an equal share of what
    /// remains.
    fn continue_to(
        &self,
        from: PeriodicOrbit,
        target: PeriodicOrbit,
        fix: Fix,
        steps: usize,
    ) -> Option<PeriodicOrbit> {
        let mut at = from;
        for j in 0..steps {
            let left = (steps - j) as f64;
            let guess: [f64; 6] =
                std::array::from_fn(|i| at.state[i] + (target.state[i] - at.state[i]) / left);
            let period = at.period + (target.period - at.period) / left;
            let (state, period) = self.system.correct_symmetric(guess, period, fix)?;
            at = PeriodicOrbit { state, period };
        }
        Some(at)
    }
}
