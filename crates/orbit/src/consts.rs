//! Physical constants. Units: km, s, km/s.

/// Astronomical unit, km (IAU 2012).
pub const AU_KM: f64 = 149_597_870.7;

/// Gravitational parameters, km³/s² (JPL DE440).
pub const MU_SUN: f64 = 132_712_440_041.279;
pub const MU_EARTH: f64 = 398_600.435;
pub const MU_MOON: f64 = 4_902.800;
pub const MU_MARS: f64 = 42_828.375;

/// Mean radii, km.
pub const R_EARTH: f64 = 6_378.137;
pub const R_MOON: f64 = 1_737.4;
pub const R_MARS: f64 = 3_389.5;
