//! Mars's air (M1c.md, step 6, part 8 plan) against a Monte Carlo of the
//! same medium (`tools/atmosphere/mc.py`): a uniform Mars (ground albedo
//! 0.25) 1.52 au from the Sun, seen from 13,558 km up at phase angles of
//! 40°, 90° and 130°, at pixels from high Sun to near the terminator.
//!
//! To regenerate the fixture (after changing Mars's air):
//!   GEOMETRY=/tmp/geometry.csv cargo test --release -p sandbox --test \
//!     atmospheres -- --ignored write_geometry
//!   python3 -I tools/atmosphere/mc.py /tmp/geometry.csv \
//!     > crates/sandbox/tests/fixtures/mars_air.csv

use glam::DVec3;
use render::atmosphere::{Air, Atmosphere};
use render::camera::Camera;
use render::offscreen::Offscreen;
use render::scene::AU_KM;
use render::{Image, Maps, Renderer, Scene, Shading, Sphere, Surface, Target};
use std::collections::HashMap;
use std::sync::Arc;

const SIZE: u32 = 400;
const FOV_DEGREES: f64 = 30.0;
const ALBEDO: f64 = 0.25;
const PHASES: [f64; 3] = [40.0, 90.0, 130.0];
const DISTANCE_AU: f64 = 1.52;
const ALTITUDE: f64 = 13_558.0;
/// The Sun's cosines the pixels are picked at.
const MU0: [f64; 7] = [0.9, 0.7, 0.5, 0.35, 0.25, 0.18, 0.12];

fn radius() -> f64 {
    let eph = orbit::ephem::Ephemeris::standard();
    eph.body(eph.lookup("mars").unwrap()).radius
}

fn atmosphere() -> Atmosphere {
    sandbox::atmospheres::of("mars", radius()).unwrap()
}

fn center() -> DVec3 {
    DVec3::new(DISTANCE_AU * AU_KM, 0.0, 0.0)
}

/// The Sun at the origin; the eye `phase` degrees round from it.
fn camera(phase: f64) -> Camera {
    let to_sun = -DVec3::X;
    let side = to_sun.cross(DVec3::Z).normalize();
    let a = phase.to_radians();
    let eye = center() + (to_sun * a.cos() + side * a.sin()) * (radius() + ALTITUDE);
    Camera::looking_at(eye, center(), DVec3::Z, FOV_DEGREES.to_radians())
}

/// The ground under pixel `(x, y)`'s center: the Sun's cosine there, the
/// eye's, and the phase angle's.
fn geometry(camera: &Camera, x: u32, y: u32) -> Option<(f64, f64, f64)> {
    let tan_half = (FOV_DEGREES.to_radians() / 2.0).tan();
    let n = f64::from(SIZE);
    let ndc = (
        (f64::from(x) + 0.5) / n * 2.0 - 1.0,
        1.0 - (f64::from(y) + 0.5) / n * 2.0,
    );
    let d = (camera.forward + camera.right * ndc.0 * tan_half + camera.up * ndc.1 * tan_half)
        .normalize();
    let oc = center() - camera.eye;
    let b = d.dot(oc);
    let disc = b * b - (oc.length_squared() - radius() * radius());
    if disc <= 0.0 {
        return None;
    }
    let up = (camera.eye + d * (b - disc.sqrt()) - center()) / radius();
    let to_sun = -DVec3::X;
    Some((up.dot(to_sun), up.dot(-d), to_sun.dot(-d)))
}

/// Radiance over a uniform ground of `albedo`.
fn render(gpu: &Offscreen, renderer: &mut Renderer, camera: Camera, albedo: f64) -> Vec<[f32; 4]> {
    let maps = Arc::new(Maps {
        color: Image::srgba(1, 1, vec![255; 4]),
        mean: [1.0; 3],
        cover: 0.0,
        relief: None,
        clouds: None,
        cloud_albedo: 0.0,
        night: None,
        night_radiance: [0.0; 3],
        water: None,
        shading: Shading::Lambert,
        air: Some(Air::new(atmosphere())),
        rings: None,
        shape: None,
    });
    let scene = Scene {
        camera: Some(camera),
        sun: DVec3::ZERO,
        spheres: vec![Sphere {
            maps: Some(maps),
            ..Sphere::plain(center(), radius(), Surface::Lit([albedo as f32; 3]))
        }],
        ..Default::default()
    };
    let target = Target::new(&gpu.device, SIZE, SIZE);
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    renderer.render(&gpu.device, &gpu.queue, &mut encoder, &target, &scene, None);
    gpu.queue.submit(Some(encoder.finish()));
    gpu.read_rgba16f(&target.hdr)
}

/// The medium and setup, as `mc.py` reads them.
fn medium() -> Vec<(&'static str, Vec<f64>)> {
    let a = atmosphere();
    vec![
        ("rayleigh", a.rayleigh.to_vec()),
        ("rayleigh_height", vec![a.rayleigh_height]),
        ("mie_scattering", a.mie_scattering.to_vec()),
        ("mie_absorption", a.mie_absorption.to_vec()),
        ("mie_height", vec![a.mie_height]),
        ("mie_g", vec![a.mie_g]),
        ("absorption", a.absorption.to_vec()),
        ("absorption_center", vec![a.absorption_center]),
        ("absorption_width", vec![a.absorption_width]),
        ("top", vec![a.top - a.bottom]),
        ("albedo", vec![ALBEDO]),
        ("irradiance", vec![1.0 / (DISTANCE_AU * DISTANCE_AU)]),
    ]
}

/// Writes the pixels `mc.py` takes: per phase, for each of `MU0`, the
/// pixel nearest it (within 0.01) where the eye looks most steeply down.
#[test]
#[ignore = "writes the Monte Carlo's input: see the module's doc"]
fn write_geometry() {
    let path = std::env::var("GEOMETRY").expect("GEOMETRY=path");
    let mut out = String::new();
    for (key, v) in medium() {
        let v: Vec<String> = v.iter().map(|x| format!("{x:?}")).collect();
        out += &format!("# {key} = {}\n", v.join(", "));
    }
    out += "phase,x,y,mu0,mu,cos_alpha\n";
    for phase in PHASES {
        let camera = camera(phase);
        for target in MU0 {
            let mut best: Option<(u32, u32, (f64, f64, f64))> = None;
            for y in 0..SIZE {
                for x in 0..SIZE {
                    if let Some(g) = geometry(&camera, x, y)
                        && (g.0 - target).abs() < 0.01
                        && g.1 > 0.25
                        && best.is_none_or(|b| g.1 > b.2.1)
                    {
                        best = Some((x, y, g));
                    }
                }
            }
            if let Some((x, y, (mu0, mu, ca))) = best {
                out += &format!("{phase},{x},{y},{mu0:.9},{mu:.9},{ca:.9}\n");
            }
        }
    }
    std::fs::write(&path, out).unwrap();
}

/// The air over black ground (all haze) and over the ground, at each
/// pixel of the fixture, against the Monte Carlo's. Where the Sun is over
/// 0.2 in cosine: the ground's part within 10%, the haze within 25% (the
/// multiple scattering's angular model: its most is −22%, at 130° phase);
/// everywhere, the whole within 22%. Nearer the terminator, the Monte
/// Carlo's plane-parallel air dims the Sun more than the shell does (its
/// beam through the shell 1.14× the slab's at a cosine of 0.19, 1.7× at
/// 0.11): the render's ground runs to 1.2× there, its whole to 1.19×.
/// Found by it: the multiple scattering was isotropic (2.4× too bright at
/// 40°, 0.1× at 130°), the ground had no skylight, and the light the air
/// scatters from the ground toward the eye was lost; the ground's part
/// was 0.05–0.6 of the Monte Carlo's, Mars washed out near the terminator.
#[test]
fn mars_air_against_the_monte_carlo() {
    let path = format!("{}/tests/fixtures/mars_air.csv", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).expect("the fixture");
    // The fixture's medium is today's.
    let header: HashMap<&str, Vec<f64>> = text
        .lines()
        .filter_map(|l| l.strip_prefix("# ")?.split_once(" = "))
        .map(|(k, v)| {
            let v = v.trim_matches(|c| c == '[' || c == ']');
            (k, v.split(',').map(|x| x.trim().parse().unwrap()).collect())
        })
        .collect();
    for (key, want) in medium() {
        let got = &header[key];
        assert!(
            got.iter()
                .zip(&want)
                .all(|(g, w)| (g - w).abs() <= 1e-9 * w.abs()),
            "{key}: the fixture's {got:?}, today's {want:?}: regenerate it"
        );
    }
    let rows: Vec<Vec<f64>> = text
        .lines()
        .filter(|l| l.starts_with(|c: char| c.is_ascii_digit()))
        .map(|l| l.split(',').map(|x| x.parse().unwrap()).collect())
        .collect();
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue);
    let (mut worst_haze, mut worst_ground, mut worst_total) = (0.0f64, 0.0f64, 0.0f64);
    for phase in PHASES {
        let camera = camera(phase);
        let full = render(&gpu, &mut renderer, camera, ALBEDO);
        let black = render(&gpu, &mut renderer, camera, 0.0);
        println!("phase {phase}°: per channel, render / Monte Carlo: haze, ground, total");
        for row in rows.iter().filter(|r| r[0] == phase) {
            let (x, y) = (row[1] as u32, row[2] as u32);
            let g = geometry(&camera, x, y).unwrap();
            assert!(
                (g.0 - row[3]).abs() < 1e-6 && (g.1 - row[4]).abs() < 1e-6,
                "pixel {x},{y}: {g:?} vs the fixture's: regenerate it"
            );
            let k = (y * SIZE + x) as usize;
            let mut line = format!("  mu0 {:.2} mu {:.2}:", row[3], row[4]);
            for c in 0..3 {
                let parts = &row[6 + 4 * c..10 + 4 * c];
                let (haze, total) = (parts[0], parts.iter().sum::<f64>());
                let (h, t) = (f64::from(black[k][c]) / haze, f64::from(full[k][c]) / total);
                let ground = f64::from(full[k][c] - black[k][c]) / (total - haze);
                line += &format!(" {h:.2} {ground:.2} {t:.2} |");
                if row[3] >= 0.2 {
                    worst_haze = worst_haze.max((h - 1.0).abs());
                    worst_ground = worst_ground.max((ground - 1.0).abs());
                }
                worst_total = worst_total.max((t - 1.0).abs());
            }
            println!("{line}");
        }
    }
    println!("worst: haze {worst_haze:.3}, ground {worst_ground:.3}, total {worst_total:.3}");
    assert!(worst_haze < 0.25, "haze {worst_haze}");
    assert!(worst_ground < 0.1, "ground {worst_ground}");
    assert!(worst_total < 0.22, "total {worst_total}");
}
