//! Bodies' maps (M1c.md, step 6 plan, items 4 and 5): `data/bodies/`
//! (built by `tools/textures/`) and the atmospheres (`atmospheres`), read
//! and tabulated once per process on a thread of its own, the bodies in
//! parallel. Spheres show them once they arrive. A body with an
//! atmosphere and no maps (Venus) gets a plain one.

use crate::data_dir;
use orbit::ephem::Ephemeris;
use rayon::prelude::*;
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

/// `entry`'s maps, from `dir`, on a body of `radius` km.
fn body(dir: &Path, entry: &Entry, radius: f64) -> Result<BodyMaps, String> {
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
    Ok(BodyMaps {
        maps: Arc::new(Maps {
            mean: render::maps::mean_color(&color),
            color,
            relief,
            clouds: optional(&entry.clouds)?,
            cloud_albedo: entry.cloud_albedo.unwrap_or(0.0),
            night: optional(&entry.night)?,
            night_radiance: entry.night_radiance.unwrap_or([0.0; 3]),
            water: optional(&entry.water)?,
            shading,
            air: None,
        }),
        albedo: [entry.albedo; 3],
    })
}

/// A plain white map, for a body with an atmosphere and no maps; its
/// albedo carries its color.
fn plain(id: &str) -> BodyMaps {
    BodyMaps {
        maps: Arc::new(Maps {
            color: Image::srgba(1, 1, vec![255; 4]),
            mean: [1.0; 3],
            relief: None,
            clouds: None,
            cloud_albedo: 0.0,
            night: None,
            night_radiance: [0.0; 3],
            water: None,
            shading: Shading::Lambert,
            air: None,
        }),
        albedo: crate::bodies::albedo(id),
    }
}

/// Reads `dir`'s maps (`bodies.toml` and each body's directory), for the
/// bodies `eph` has.
pub fn read(dir: &Path, eph: &Ephemeris) -> Result<HashMap<String, BodyMaps>, String> {
    let path = dir.join("bodies.toml");
    let src = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let file: File = toml::from_str(&src).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut ids: Vec<&str> = file.body.keys().map(String::as_str).collect();
    for b in eph.ids() {
        let id = eph.body(b).id.as_str();
        if !ids.contains(&id) && crate::atmospheres::of(id, 1.0).is_some() {
            ids.push(id);
        }
    }
    ids.par_iter()
        .map(|&id| {
            let b = eph
                .lookup(id)
                .ok_or_else(|| format!("bodies.toml: no body {id:?}"))?;
            let radius = eph.body(b).radius;
            let mut maps = match file.body.get(id) {
                Some(entry) => {
                    body(&dir.join(id), entry, radius).map_err(|e| format!("{id}: {e}"))?
                }
                None => plain(id),
            };
            if let Some(a) = crate::atmospheres::of(id, radius) {
                Arc::get_mut(&mut maps.maps).expect("just made").air =
                    Some(render::atmosphere::Air::new(a));
            }
            Ok((id.to_string(), maps))
        })
        .collect()
}

static MAPS: OnceLock<Result<HashMap<String, BodyMaps>, String>> = OnceLock::new();
static STARTED: Once = Once::new();

/// Starts reading the maps, once per process.
pub fn start() {
    STARTED.call_once(|| {
        std::thread::spawn(|| {
            let read = read(&data_dir().join("bodies"), Ephemeris::standard());
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
