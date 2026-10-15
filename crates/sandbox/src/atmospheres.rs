//! The atmospheres drawn (M1c.md, step 6 plan, item 5), over each body's
//! radius. Earth's are Hillaire's (2020) values; Mars's and Venus's are
//! estimates from the physical quantities given beside them, close enough
//! for their color and their glow at the limb.

use render::atmosphere::Atmosphere;

/// Earth's Rayleigh scattering at sea level, per km (Hillaire 2020).
const EARTH_RAYLEIGH: [f64; 3] = [5.802e-3, 13.558e-3, 33.1e-3];

/// `id`'s atmosphere, over a ground of `radius` km.
pub fn of(id: &str, radius: f64) -> Option<Atmosphere> {
    match id {
        "earth" => Some(Atmosphere {
            bottom: radius,
            top: radius + 100.0,
            rayleigh: EARTH_RAYLEIGH,
            rayleigh_height: 8.0,
            mie_scattering: [3.996e-3; 3],
            mie_absorption: [4.40e-3; 3],
            mie_height: 1.2,
            mie_g: 0.8,
            absorption: [0.650e-3, 1.881e-3, 0.085e-3],
            absorption_center: 25.0,
            absorption_width: 15.0,
            ground_albedo: 0.3,
        }),
        // CO₂ at 6.1 mbar and 210 K, 1/121 of air's number density at sea
        // level, each molecule scattering 2.45 times air's: Earth's
        // Rayleigh × 0.0202, scale height 11.1 km. Dust: visible optical
        // depth 0.35 (the clear season around aphelion; at 0.5 the haze
        // hid the surface toward the terminator) over a scale height of
        // 11 km, single-scattering albedo 0.97 / 0.93 / 0.80 in red /
        // green / blue (it absorbs blue: the butterscotch sky), asymmetry
        // 0.65 (Wolff et al. 2009's range).
        "mars" => {
            let dust = 0.35 / 11.0;
            let albedo = [0.97, 0.93, 0.80];
            Some(Atmosphere {
                bottom: radius,
                top: radius + 80.0,
                rayleigh: EARTH_RAYLEIGH.map(|b| b * 0.0202),
                rayleigh_height: 11.1,
                mie_scattering: albedo.map(|w| dust * w),
                mie_absorption: albedo.map(|w| dust * (1.0 - w)),
                mie_height: 11.0,
                mie_g: 0.65,
                absorption: [0.0; 3],
                absorption_center: 0.0,
                absorption_width: 1.0,
                ground_albedo: 0.25,
            })
        }
        // Over the cloud deck (drawn as the body's surface, 1% inside the
        // real one at ~70 km): CO₂ at ~30 mbar and 230 K, 0.037 of air's
        // number density, Rayleigh × 0.09, scale height 5 km; the upper
        // haze, optical depth 0.1 over 5 km, bright but for some blue
        // absorbed, asymmetry 0.7.
        "venus" => {
            let haze = 0.1 / 5.0;
            let albedo = [0.99, 0.98, 0.92];
            Some(Atmosphere {
                bottom: radius,
                top: radius + 60.0,
                rayleigh: EARTH_RAYLEIGH.map(|b| b * 0.09),
                rayleigh_height: 5.0,
                mie_scattering: albedo.map(|w| haze * w),
                mie_absorption: albedo.map(|w| haze * (1.0 - w)),
                mie_height: 5.0,
                mie_g: 0.7,
                absorption: [0.0; 3],
                absorption_center: 0.0,
                absorption_width: 1.0,
                ground_albedo: 0.75,
            })
        }
        _ => None,
    }
}
