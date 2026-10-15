//! The sky's data (M1c.md, step 6 plan, item 2): the Hipparcos stars and
//! the Milky Way, from `data/sky/` (built by `tools/sky/`), read once per
//! process on a thread of its own. The view shows them once they arrive.

use crate::data_dir;
use orbit::ephem::OBLIQUITY_J2000;
use orbit::time::Epoch;
use render::{Image, Star};
use serde::Deserialize;
use std::path::Path;
use std::sync::{Arc, Once, OnceLock};

/// What `data/sky/` holds.
#[derive(Clone, Debug)]
pub struct SkyData {
    pub stars: Arc<Vec<Star>>,
    pub milky_way: Arc<Image>,
    /// Radiance of one unit of `milky_way`, linear.
    pub milky_way_radiance: f32,
}

#[derive(Deserialize)]
struct SkyFile {
    milky_way_radiance: f32,
}

/// `stars.bin`'s first bytes, then a count and a zero (u32s).
const MAGIC: &[u8; 8] = b"STARS001";

/// Reads `dir`'s stars, Milky Way and calibration.
pub fn read(dir: &Path) -> Result<SkyData, String> {
    let file = |name: &str| {
        let path = dir.join(name);
        std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))
    };
    let bytes = file("stars.bin")?;
    let header = MAGIC.len() + 8;
    if bytes.len() < header || &bytes[..MAGIC.len()] != MAGIC {
        return Err("stars.bin: not a star list".into());
    }
    let count = u32::from_le_bytes(bytes[8..12].try_into().expect("four bytes")) as usize;
    let body = &bytes[header..];
    if body.len() != count * std::mem::size_of::<Star>() {
        return Err(format!("stars.bin: {} bytes for {count} stars", body.len()));
    }
    let stars: Vec<Star> = body
        .chunks_exact(std::mem::size_of::<Star>())
        .map(bytemuck::pod_read_unaligned)
        .collect();
    let calibration: SkyFile = toml::from_str(
        &String::from_utf8(file("sky.toml")?).map_err(|e| format!("sky.toml: {e}"))?,
    )
    .map_err(|e| format!("sky.toml: {e}"))?;
    let decoded =
        image::load_from_memory_with_format(&file("milkyway.jpg")?, image::ImageFormat::Jpeg)
            .map_err(|e| format!("milkyway.jpg: {e}"))?
            .to_rgba8();
    Ok(SkyData {
        stars: Arc::new(stars),
        milky_way: Arc::new(Image::srgba(
            decoded.width(),
            decoded.height(),
            decoded.into_raw(),
        )),
        milky_way_radiance: calibration.milky_way_radiance,
    })
}

static SKY: OnceLock<Result<SkyData, String>> = OnceLock::new();
static STARTED: Once = Once::new();

/// Starts reading the sky, once per process.
pub fn start() {
    STARTED.call_once(|| {
        std::thread::spawn(|| {
            let read = read(&data_dir().join("sky"));
            if let Err(why) = &read {
                eprintln!("sandbox: no sky: {why}");
            }
            let _ = SKY.set(read);
        });
    });
}

/// The sky, once read.
pub fn loaded() -> Option<&'static SkyData> {
    start();
    SKY.get().and_then(|r| r.as_ref().ok())
}

/// The sky, waiting for it to be read.
pub fn wait() -> Result<&'static SkyData, String> {
    start();
    SKY.wait().as_ref().map_err(Clone::clone)
}

/// From the sky's coordinates (equatorial, ICRS) to the ephemeris's
/// (ecliptic J2000): a turn of the obliquity about their shared x axis.
pub fn orientation() -> glam::DMat3 {
    glam::DMat3::from_rotation_x(-OBLIQUITY_J2000)
}

/// The sky as the renderer takes it, at `t`.
pub fn sky(data: &SkyData, t: Epoch) -> render::Sky {
    render::Sky {
        stars: data.stars.clone(),
        milky_way: Some(data.milky_way.clone()),
        milky_way_radiance: data.milky_way_radiance,
        orientation: orientation(),
        // Epoch counts days from J2000.
        years: t.0 / 365.25,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::DVec3;

    /// The ICRS z axis lands on Earth's pole as `ephem` has it: the
    /// obliquity from the ecliptic's pole, toward +y.
    #[test]
    fn the_celestial_pole_is_earths() {
        let pole = orientation() * DVec3::Z;
        let earth = DVec3::new(0.0, OBLIQUITY_J2000.sin(), OBLIQUITY_J2000.cos());
        assert!((pole - earth).length() < 1e-12, "{pole}");
        assert!((orientation() * DVec3::X - DVec3::X).length() < 1e-12);
    }

    /// The data reads, and the brightest star is Sirius: at its ICRS place
    /// (RA 6h 45m 8.9s, Dec −16° 42′ 58″), V −1.44, bluish white.
    #[test]
    fn sirius_leads_the_catalog() {
        let data = wait().expect("data/sky reads");
        assert!(data.stars.len() > 100_000);
        assert!(data.milky_way.width == 8192 && data.milky_way.height == 4096);
        let sirius = data.stars[0];
        let (ra, dec) = (
            (6.0 + 45.0 / 60.0 + 8.917 / 3600.0) * 15f64.to_radians(),
            -(16.0 + 42.0 / 60.0 + 58.02 / 3600.0f64).to_radians(),
        );
        let want = DVec3::new(dec.cos() * ra.cos(), dec.cos() * ra.sin(), dec.sin());
        let got = DVec3::from(sirius.direction.map(f64::from));
        let error = want.angle_between(got).to_degrees() * 3600.0;
        assert!(error < 1.0, "{error} arcsec");
        let [r, g, b] = [0, 8, 16].map(|s| (sirius.color >> s) & 255);
        assert!(b == 255 && r < b && g < b, "{r} {g} {b}");
        // Flux: π 10^(−0.4 (V − V☉)), the largest channel's.
        let flux = std::f64::consts::PI * 10f64.powf(-0.4 * (-1.44 + 26.74));
        let luminance =
            (0.2126 * f64::from(r) + 0.7152 * f64::from(g) + 0.0722 * f64::from(b)) / 255.0;
        let got = f64::from(sirius.flux) * luminance;
        // Within the color's rounding to 8 bits.
        assert!((got / flux - 1.0).abs() < 5e-3, "{got} vs {flux}");
    }
}
