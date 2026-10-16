//! Bodies' maps (M1c.md, step 6 plan, items 4 and 5): `data/bodies/`
//! (built by `tools/textures/`) and the atmospheres (`atmospheres`), read
//! and tabulated once per process on a thread of its own, the bodies in
//! parallel, with rings where a body has a `rings.toml` (Saturn, built by
//! `tools/rings/`) and shapes where `shapes.toml` lists one (built by
//! `tools/shapes/`; step 6, part 9 plan). Spheres show them once they
//! arrive. A body with an atmosphere, rings or a shape and no maps (Venus,
//! Saturn, the small bodies), or whose maps do not read, gets a plain one.

use crate::data_dir;
use orbit::ephem::{Body, Ephemeris};
use rayon::prelude::*;
use render::maps::Weight;
use render::{Image, Maps, Shading};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Once, OnceLock};

#[derive(Deserialize)]
struct File {
    body: HashMap<String, Entry>,
}

/// One body's maps in `bodies.toml`: see `tools/textures/build.py`.
#[derive(Deserialize)]
struct Entry {
    color: String,
    albedo: f32,
    shading: String,
    clouds: Option<String>,
    cloud_albedo: Option<f32>,
    water: Option<String>,
    night: Option<String>,
    night_radiance: Option<[f32; 3]>,
    elevation: Option<Elevation>,
}

/// `shapes.toml`: see `tools/shapes/build.py`.
#[derive(Deserialize)]
struct ShapeFile {
    body: HashMap<String, ShapeEntry>,
}

#[derive(Deserialize)]
struct ShapeEntry {
    file: String,
    size: u32,
    shadowed: Option<Vec<f64>>,
}

/// The shape in `path`, `size` texels a face within its edges.
fn shape(path: &Path, size: u32) -> Result<render::shape::Shape, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let radii: Vec<f32> = bytes
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect();
    let side = (size + 2 * size / render::shape::BORDER_SHARE) as usize;
    if radii.len() != 6 * side * side || !size.is_power_of_two() || size < 16 {
        return Err(format!("{}: not {size} texels a face", path.display()));
    }
    Ok(render::shape::Shape::new(size, radii))
}

/// `rings.toml`: see `tools/rings/build.py`.
#[derive(Deserialize)]
struct RingFile {
    inner_km: f64,
    step_km: f64,
    tau: Vec<f32>,
    region: Vec<Region>,
}

#[derive(Deserialize)]
struct Region {
    outer_km: f64,
    albedo: [f32; 3],
}

/// The rings in `path`: each step's albedo its region's.
fn rings(path: &Path) -> Result<render::maps::Rings, String> {
    let src = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let file: RingFile = toml::from_str(&src).map_err(|e| format!("{}: {e}", path.display()))?;
    let albedo = (0..file.tau.len())
        .map(|k| {
            let r = file.inner_km + (k as f64 + 0.5) * file.step_km;
            file.region
                .iter()
                .find(|g| r < g.outer_km)
                .map(|g| g.albedo)
                .ok_or_else(|| format!("{}: no region at {r} km", path.display()))
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(render::maps::Rings::new(
        file.inner_km,
        file.step_km,
        &file.tau,
        &albedo,
    ))
}

#[derive(Deserialize)]
struct Elevation {
    file: String,
    metres_per_unit: f64,
    offset_m: f64,
}

/// A body's maps, and the albedo its color map is multiplied by.
#[derive(Clone, Debug)]
pub struct BodyMaps {
    pub maps: Arc<Maps>,
    pub albedo: [f32; 3],
}

fn decode(path: &Path) -> Result<image::DynamicImage, String> {
    image::open(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn color(path: &Path) -> Result<Image, String> {
    let rgba = decode(path)?.to_rgba8();
    Ok(Image::srgba(rgba.width(), rgba.height(), rgba.into_raw()))
}

fn gray(path: &Path) -> Result<Image, String> {
    let luma = decode(path)?.to_luma8();
    Ok(Image::gray(luma.width(), luma.height(), luma.into_raw()))
}

/// `entry`'s maps, from `dir`, on the body `id` of `radius` km.
fn body(dir: &Path, id: &str, entry: &Entry, radius: f64) -> Result<BodyMaps, String> {
    let optional = |name: &Option<String>| name.as_ref().map(|n| gray(&dir.join(n))).transpose();
    let relief = match &entry.elevation {
        Some(e) => {
            let path = dir.join(&e.file);
            let levels = decode(&path)?.into_luma16();
            let (w, h) = levels.dimensions();
            let km: Vec<f32> = levels
                .into_raw()
                .into_iter()
                .map(|v| ((f64::from(v) * e.metres_per_unit + e.offset_m) / 1000.0) as f32)
                .collect();
            Some(render::maps::relief(w, h, &km, radius))
        }
        None => None,
    };
    let shading = match entry.shading.as_str() {
        "lambert" => Shading::Lambert,
        "lunar-lambert" => Shading::LunarLambert,
        other => return Err(format!("no shading {other:?}")),
    };
    let color = color(&dir.join(&entry.color))?;
    let clouds = optional(&entry.clouds)?;
    // Its mean as its disk shows it from its equator: the published
    // brightness it is held to (`photometry`) is that; the Moon's is its
    // near side's.
    let weight = Weight::Disk {
        shading,
        near: id == "moon",
    };
    let (mean, cover) = render::maps::means(&color, clouds.as_ref(), weight);
    Ok(BodyMaps {
        maps: Arc::new(Maps {
            mean,
            cover,
            color,
            relief,
            clouds,
            cloud_albedo: entry.cloud_albedo.unwrap_or(0.0),
            night: optional(&entry.night)?,
            night_radiance: entry.night_radiance.unwrap_or([0.0; 3]),
            water: optional(&entry.water)?,
            shading,
            air: None,
            rings: None,
            shape: None,
        }),
        albedo: [entry.albedo; 3],
    })
}

/// A plain white map, for a body with an atmosphere, rings or a shape
/// and no maps, shaded by `shading`; its albedo carries its color.
fn plain(body: &Body, shading: Shading) -> BodyMaps {
    BodyMaps {
        maps: Arc::new(Maps {
            color: Image::srgba(1, 1, vec![255; 4]),
            mean: [1.0; 3],
            cover: 0.0,
            relief: None,
            clouds: None,
            cloud_albedo: 0.0,
            night: None,
            night_radiance: [0.0; 3],
            water: None,
            shading,
            air: None,
            rings: None,
            shape: None,
        }),
        albedo: crate::bodies::albedo(body),
    }
}

/// Reads `dir`'s maps (`bodies.toml`, `shapes.toml` and each body's
/// directory), for the bodies `eph` has; and what kept any body's maps,
/// rings or shape from reading (a body whose maps do not read is plain,
/// its atmosphere kept). Only `bodies.toml` not reading fails it.
pub fn read(
    dir: &Path,
    eph: &Ephemeris,
) -> Result<(HashMap<String, BodyMaps>, Vec<String>), String> {
    let path = dir.join("bodies.toml");
    let src = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let file: File = toml::from_str(&src).map_err(|e| format!("{}: {e}", path.display()))?;
    let ringed = |id: &str| dir.join(id).join("rings.toml").exists();
    let shapes = match std::fs::read_to_string(dir.join("shapes.toml")) {
        Ok(src) => {
            toml::from_str::<ShapeFile>(&src)
                .map_err(|e| format!("{}: {e}", dir.join("shapes.toml").display()))?
                .body
        }
        Err(_) => HashMap::new(),
    };
    let mut ids: Vec<&str> = file.body.keys().map(String::as_str).collect();
    for b in eph.ids() {
        let id = eph.body(b).id.as_str();
        if !ids.contains(&id)
            && (crate::atmospheres::of(id, 1.0).is_some() || ringed(id) || shapes.contains_key(id))
        {
            ids.push(id);
        }
    }
    let read: Vec<(&str, Option<BodyMaps>, Vec<String>)> = ids
        .par_iter()
        .map(|&id| {
            let Some(b) = eph.lookup(id) else {
                return (id, None, vec![format!("bodies.toml: no body {id:?}")]);
            };
            let radius = eph.body(b).radius;
            let mut problems = Vec::new();
            // Without maps, an airless body is shaded as regolith.
            let bare = match crate::atmospheres::of(id, 1.0) {
                Some(_) => Shading::Lambert,
                None if ringed(id) => Shading::Lambert,
                None => Shading::LunarLambert,
            };
            let mut maps = match file
                .body
                .get(id)
                .map(|e| body(&dir.join(id), id, e, radius))
            {
                Some(Ok(maps)) => maps,
                Some(Err(why)) => {
                    problems.push(format!("{id}: {why}"));
                    plain(eph.body(b), bare)
                }
                None => plain(eph.body(b), bare),
            };
            let made = Arc::get_mut(&mut maps.maps).expect("just made");
            if let Some(a) = crate::atmospheres::of(id, radius) {
                made.air = Some(render::atmosphere::Air::new(a));
            }
            if ringed(id) {
                match rings(&dir.join(id).join("rings.toml")) {
                    Ok(r) => made.rings = Some(r),
                    Err(why) => problems.push(format!("{id}: {why}")),
                }
            }
            if let Some(entry) = shapes.get(id) {
                match shape(&dir.join(id).join(&entry.file), entry.size) {
                    Ok(mut s) => {
                        match &entry.shadowed {
                            Some(t) if t.len() != 19 => problems.push(format!(
                                "{id}: shadowed: {} values, not 19; drawn without",
                                t.len()
                            )),
                            t => s.shadowed = t.clone(),
                        }
                        made.shape = Some(s);
                    }
                    Err(why) => problems.push(format!("{id}: {why}")),
                }
            }
            (id, Some(maps), problems)
        })
        .collect();
    let mut maps = HashMap::new();
    let mut problems = Vec::new();
    for (id, m, p) in read {
        maps.extend(m.map(|m| (id.to_string(), m)));
        problems.extend(p);
    }
    Ok((maps, problems))
}

static MAPS: OnceLock<Result<HashMap<String, BodyMaps>, String>> = OnceLock::new();
static STARTED: Once = Once::new();

/// Starts reading the maps, once per process.
pub fn start() {
    STARTED.call_once(|| {
        std::thread::spawn(|| {
            // Apart from rayon's global pool, whose tasks the view's
            // rebuilds would wait behind.
            let pool = crate::background::pool("maps");
            let read = pool
                .install(|| read(&data_dir().join("bodies"), Ephemeris::standard()))
                .map(|(maps, problems)| {
                    for p in problems {
                        eprintln!("sandbox: {p}; drawn without");
                    }
                    maps
                });
            if let Err(why) = &read {
                eprintln!("sandbox: no maps: {why}");
            }
            let _ = MAPS.set(read);
        });
    });
}

/// The maps of body `id`, once read.
pub fn loaded(id: &str) -> Option<&'static BodyMaps> {
    start();
    MAPS.get()?.as_ref().ok()?.get(id)
}

/// All the maps, waiting for them to be read.
pub fn wait() -> Result<&'static HashMap<String, BodyMaps>, String> {
    start();
    MAPS.wait().as_ref().map_err(Clone::clone)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A body whose maps do not read is plain, and said why; the others,
    /// and the atmospheres, read as ever (found in review: one missing
    /// file left every body plain, and without air or rings).
    #[test]
    fn a_missing_map_spares_the_rest() {
        let dir = std::env::temp_dir().join(format!("sandbox-maps-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("mercury")).unwrap();
        image::RgbaImage::from_pixel(4, 2, image::Rgba([128, 128, 128, 255]))
            .save(dir.join("mercury").join("color.png"))
            .unwrap();
        let toml = "[body.mercury]\ncolor = \"color.png\"\nalbedo = 1.0\nshading = \"lunar-lambert\"\n\
                    [body.jupiter]\ncolor = \"color.jpg\"\nalbedo = 1.0\nshading = \"lambert\"\n";
        std::fs::write(dir.join("bodies.toml"), toml).unwrap();
        let read = read(&dir, Ephemeris::standard());
        std::fs::remove_dir_all(&dir).unwrap();
        let (maps, problems) = read.expect("bodies.toml reads");
        assert_eq!(maps["mercury"].maps.color.width, 4);
        assert_eq!(maps["jupiter"].maps.color.width, 1, "plain");
        assert!(
            problems.len() == 1 && problems[0].starts_with("jupiter"),
            "{problems:?}"
        );
        assert!(maps["earth"].maps.air.is_some());
    }

    /// Saturn's gaps are empty on the mean, its A ring not: the Roche
    /// Division's (137,300-139,500 km) mean optical depth under 0.01, the
    /// A ring's middle (126,000-130,000 km) over 0.2. Found in review:
    /// with each 10 km bin's noise taken as at least 0, every gap read as
    /// a faint ring (the Roche Division's 0.016).
    #[test]
    fn saturns_gaps_are_empty() {
        let path = data_dir().join("bodies/saturn/rings.toml");
        let file: RingFile = toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let mean = |lo: f64, hi: f64| {
            let k = |r: f64| ((r - file.inner_km) / file.step_km) as usize;
            let span = &file.tau[k(lo)..k(hi)];
            span.iter().map(|&t| f64::from(t)).sum::<f64>() / span.len() as f64
        };
        let (roche, a_ring) = (mean(137_300.0, 139_500.0), mean(126_000.0, 130_000.0));
        assert!(roche < 0.01 && a_ring > 0.2, "{roche}, {a_ring}");
    }
}
