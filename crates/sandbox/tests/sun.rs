//! The Sun's surface as drawn (M1c.md, step 6, part 11 plan): its limb's
//! colors, its granulation's contrast and fades, and its spots' and
//! faculae's temperatures through the table.

use glam::DVec3;
use orbit::time::Epoch;
use render::camera::Camera;
use render::offscreen::Offscreen;
use render::photosphere::{EVEN, Feature, Photosphere};
use render::{Look, Renderer, Scene, Sphere, Surface, Target};
use sandbox::sun;
use std::sync::Arc;

const R: f64 = 695_700.0;
const SIZE: u32 = 1000;

/// The Sun's surface with nothing on it but its granulation.
fn quiet() -> Photosphere {
    let mut p = sun::photosphere(R, Epoch::from_ymd(2045, 3, 1));
    p.features.clear();
    p
}

/// The scene's radiance, the Sun of radiance 1 seen from `altitude` km
/// over its disk's center with a field of `fov`°, its limb `limb`, at
/// `now` s with frames spanning `frame_s` s.
fn draw(
    gpu: &Offscreen,
    renderer: &mut Renderer,
    photosphere: Photosphere,
    limb: [[f32; 6]; 3],
    (altitude, fov): (f64, f64),
    frame_s: f64,
) -> Vec<[f32; 4]> {
    let camera = Camera::looking_at(
        DVec3::new(0.0, 0.0, R + altitude),
        DVec3::ZERO,
        DVec3::Y,
        fov.to_radians(),
    );
    let scene = Scene {
        camera: Some(camera),
        spheres: vec![Sphere::plain(
            DVec3::ZERO,
            R,
            Surface::Emissive {
                radiance: [1.0; 3],
                limb,
                photosphere: Some(Arc::new(photosphere)),
            },
        )],
        look: Look {
            headroom: 1e5,
            ..Default::default()
        },
        now: 1.0e9,
        frame_s,
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

/// Each channel's mean and rms contrast over the central 400×400 pixels.
fn stats(image: &[[f32; 4]]) -> [(f64, f64); 3] {
    std::array::from_fn(|c| {
        let v: Vec<f64> = (300..700)
            .flat_map(|y| (300..700).map(move |x| (y, x)))
            .map(|(y, x)| f64::from(image[y * SIZE as usize + x][c]))
            .collect();
        let mean = v.iter().sum::<f64>() / v.len() as f64;
        let sd = (v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / v.len() as f64).sqrt();
        (mean, sd / mean)
    })
}

/// Seen at 141 km a pixel at disk center, granulation's rms contrast is
/// the simulations' in green (18%), more in blue and less in red, and it
/// brightens the disk by little (Planck's curvature: 1.2%).
#[test]
fn granulation_has_the_measured_contrast() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue);
    let image = draw(&gpu, &mut renderer, quiet(), EVEN, (100_000.0, 70.0), 0.0);
    let [r, g, b] = stats(&image);
    println!("contrast: red {r:?}, green {g:?}, blue {b:?}");
    assert!((g.1 - 0.18).abs() < 0.01, "{g:?}");
    assert!(b.1 > g.1 && g.1 > r.1);
    assert!(g.0 > 1.0 && g.0 < 1.03, "{g:?}");
}

/// Granulation fades away where it would alias: where a pixel spans its
/// cells (the disk filling the view, ~1,400 km a pixel), and where a
/// frame spans a sixth of their lives (time warp).
#[test]
fn granulation_fades_when_unresolved_or_blurred() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue);
    let far = draw(&gpu, &mut renderer, quiet(), EVEN, (2.0 * R, 38.0), 0.0);
    let warped = draw(&gpu, &mut renderer, quiet(), EVEN, (100_000.0, 70.0), 60.0);
    for (what, image) in [("far", far), ("warped", warped)] {
        let [_, g, _] = stats(&image);
        println!("{what}: {g:?}");
        assert!(g.1 < 1e-3 && (g.0 - 1.0).abs() < 2e-3, "{what}: {g:?}");
    }
}

/// The light across the disk is the limb polynomial's: at its center
/// and halfway out (μ 0.5), each channel within 0.5%.
#[test]
fn the_limb_has_its_colors() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue);
    let mut p = quiet();
    p.granulation = None;
    // The whole disk in view, from 4 radii out.
    let (altitude, fov) = (3.0 * R, 30.0);
    let image = draw(&gpu, &mut renderer, p, sun::LIMB, (altitude, fov), 0.0);
    let d = R + altitude;
    let tan_half = (fov.to_radians() / 2.0).tan();
    // The pixel whose ray meets the surface at μ, along +x.
    let at_mu = |mu: f64| {
        // The ray's angle off the axis to the point at angle θ from the
        // sub-eye point, where μ = cos of the angle between normal and ray.
        let theta = (0..=100_000)
            .map(|i| f64::from(i) * std::f64::consts::FRAC_PI_2 / 100_000.0)
            .find(|&th| {
                let p = DVec3::new(R * th.sin(), 0.0, R * th.cos());
                let to_eye = (DVec3::new(0.0, 0.0, d) - p).normalize();
                p.normalize().dot(to_eye) <= mu
            })
            .unwrap();
        let p = DVec3::new(R * theta.sin(), 0.0, R * theta.cos());
        let x = p.x / (d - p.z) / tan_half;
        let px = ((x + 1.0) / 2.0 * f64::from(SIZE)) as usize;
        image[(SIZE as usize / 2) * SIZE as usize + px]
    };
    for mu in [1.0, 0.5] {
        let seen = at_mu(mu);
        for (c, (seen, limb)) in seen.iter().zip(sun::LIMB).enumerate() {
            let want: f64 = (0..6).map(|k| f64::from(limb[k]) * mu.powi(k as i32)).sum();
            println!("μ {mu}, channel {c}: {seen} against {want:.4}");
            assert!((f64::from(*seen) / want - 1.0).abs() < 5e-3, "μ {mu}, {c}");
        }
    }
}

/// A spot at disk center shows its umbra's and penumbra's temperatures
/// through the table, and a facula near the limb its excess there.
#[test]
fn spots_and_faculae_take_their_temperatures() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue);
    let table = |t: f32| -> [f32; 3] {
        let x = (t - 3000.0) / 100.0;
        let k = x.floor() as usize;
        std::array::from_fn(|c| {
            sun::TEMPERATURES[k][c]
                + (x - k as f32) * (sun::TEMPERATURES[k + 1][c] - sun::TEMPERATURES[k][c])
        })
    };
    let mut p = quiet();
    p.granulation = None;
    // A spot 20,000 km in radius at the center; the view 100,000 km wide.
    let radius = 20_000.0 / R;
    p.features.push(Feature::Spot {
        center: DVec3::Z,
        radius,
        umbra: 0.4,
    });
    let (altitude, fov) = (100_000.0, 2.0 * (50_000f64 / 100_000.0).atan().to_degrees());
    let image = draw(&gpu, &mut renderer, p, EVEN, (altitude, fov), 0.0);
    let km_per_px = 100_000.0 / f64::from(SIZE);
    let at =
        |km: f64| image[(SIZE as usize / 2) * SIZE as usize + (500.0 + km / km_per_px) as usize];
    for (km, t, what) in [
        (0.0, sun::UMBRA, "umbra"),
        (14_000.0, sun::PENUMBRA, "penumbra"),
        (30_000.0, sun::QUIET, "quiet"),
    ] {
        let (seen, want) = (at(km), table(t));
        println!("{what}: {seen:?} against {want:?}");
        for c in 0..3 {
            assert!((seen[c] / want[c] - 1.0).abs() < 1e-2, "{what} {c}");
        }
    }

    // A plage at μ 0.45: at its center the faculae's full excess there,
    // halfway between the table's entries at 0.4 and 0.5.
    let mut p = quiet();
    p.granulation = None;
    let theta = 0.45f64.acos();
    let center = DVec3::new(theta.sin(), 0.0, theta.cos());
    p.features.push(Feature::Facula {
        center,
        radius: 0.05,
        strength: 1.0,
    });
    // Seen from far along +z, where it is seen at μ 0.45.
    let image = draw(&gpu, &mut renderer, p, EVEN, (50.0 * R, 2.5), 0.0);
    let d = 51.0 * R;
    let tan_half = (2.5f64.to_radians() / 2.0).tan();
    let x = R * theta.sin() / (d - R * theta.cos()) / tan_half;
    let px = ((x + 1.0) / 2.0 * f64::from(SIZE)) as usize;
    let seen = image[(SIZE as usize / 2) * SIZE as usize + px];
    let excess = (sun::FACULAR_EXCESS[3] + sun::FACULAR_EXCESS[4]) / 2.0;
    let want = table(sun::QUIET + excess);
    println!("facula at μ 0.45: {seen:?} against {want:?}");
    for c in 0..3 {
        assert!((seen[c] / want[c] - 1.0).abs() < 3e-3, "facula {c}");
    }
}

/// The Sun's light over a 1000-pixel view with a 30° field, each pixel
/// weighed by the sky it subtends, of a surface `p` with limb `limb` seen
/// from `d` radii.
fn flux(
    gpu: &Offscreen,
    renderer: &mut Renderer,
    p: Photosphere,
    limb: [[f32; 6]; 3],
    d: f64,
) -> f64 {
    let image = draw(gpu, renderer, p, limb, ((d - 1.0) * R, 30.0), 0.0);
    let tan_half = 15f64.to_radians().tan();
    image
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let at = |k: usize| ((k as f64 + 0.5) / 500.0 - 1.0) * tan_half;
            let (x, y) = (at(i % SIZE as usize), at(i / SIZE as usize));
            f64::from(p[1]) / (x * x + y * y + 1.0).powf(1.5)
        })
        .sum()
}

/// Spots take their area's light however small the disk: the date's
/// spots and faculae move the disk's light alike (within 0.15%) whether
/// it is 450 pixels in radius or 4. Found in review: spread over a
/// pixel's width, a spot under a pixel took half of it, and spots moved
/// a 4-pixel disk's light by −2.4% against +0.1% resolved.
#[test]
fn spots_take_their_area_at_every_size() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue);
    let mut plain = quiet();
    plain.granulation = None;
    let mut spotted = sun::photosphere(R, Epoch::from_ymd(2045, 3, 1));
    spotted.granulation = None;
    // Radii in pixels: R / d over tan 15°, times 500.
    let tan_half = 15f64.to_radians().tan();
    let mut changes = Vec::new();
    for px in [450.0, 40.0, 8.0, 4.0] {
        let d = 500.0 / (px * tan_half);
        let (a, b) = (
            flux(&gpu, &mut renderer, spotted.clone(), sun::LIMB, d),
            flux(&gpu, &mut renderer, plain.clone(), sun::LIMB, d),
        );
        println!(
            "{px} px: the surface moves the light by {:+.3}%",
            (a / b - 1.0) * 100.0
        );
        changes.push(a / b - 1.0);
    }
    for c in &changes[1..] {
        assert!((c - changes[0]).abs() < 1.5e-3, "{changes:?}");
    }
}

/// The limb keeps the disk's light: drawn with the measured limb, the
/// disk's light is the even disk's within 0.2%.
#[test]
fn the_limb_keeps_the_disks_light() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue);
    let mut p = quiet();
    p.granulation = None;
    let d = 500.0 / (300.0 * 15f64.to_radians().tan());
    let (limbed, even) = (
        flux(&gpu, &mut renderer, p.clone(), sun::LIMB, d),
        flux(&gpu, &mut renderer, p, EVEN, d),
    );
    println!("limbed {limbed:.1}, even {even:.1}");
    assert!((limbed / even - 1.0).abs() < 2e-3);
}

/// Granulation falls toward the limb as the simulations': seen at μ 0.5,
/// its rms contrast in green ~0.84 of disk center's.
#[test]
fn granulation_falls_toward_the_limb() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue);
    // From 50,000 km, along the normal and at 60° from it.
    let mut contrast = |angle: f64| {
        let n = DVec3::Z;
        let side = DVec3::new(angle.sin(), 0.0, angle.cos());
        let camera =
            Camera::looking_at(n * R + side * 50_000.0, n * R, DVec3::Y, 30f64.to_radians());
        let scene = Scene {
            camera: Some(camera),
            spheres: vec![Sphere::plain(
                DVec3::ZERO,
                R,
                Surface::Emissive {
                    radiance: [1.0; 3],
                    limb: EVEN,
                    photosphere: Some(Arc::new(quiet())),
                },
            )],
            look: Look {
                headroom: 1e5,
                ..Default::default()
            },
            now: 1.0e9,
            ..Default::default()
        };
        let target = Target::new(&gpu.device, SIZE, SIZE);
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        renderer.render(&gpu.device, &gpu.queue, &mut encoder, &target, &scene, None);
        gpu.queue.submit(Some(encoder.finish()));
        stats(&gpu.read_rgba16f(&target.hdr))[1].1
    };
    let (center, half) = (contrast(0.0), contrast(60f64.to_radians()));
    println!(
        "rms at μ 1 {center:.4}, at μ 0.5 {half:.4}: {:.3}",
        half / center
    );
    assert!((half / center - 0.84).abs() < 0.06);
}

/// GPU time of the Sun filling a 1920×1080 view up close, its granulation
/// resolved and March 2045's spots and faculae on it, against it plain.
#[test]
#[ignore = "a measurement: run with --ignored --nocapture"]
fn photosphere_gpu_time() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue);
    let t = Epoch::from_ymd(2045, 3, 1);
    let camera = Camera::looking_at(
        DVec3::new(0.0, 0.0, R + 100_000.0),
        DVec3::ZERO,
        DVec3::Y,
        70f64.to_radians(),
    );
    for (what, photosphere) in [
        ("plain", None),
        ("with its surface", Some(Arc::new(sun::photosphere(R, t)))),
    ] {
        let scene = Scene {
            camera: Some(camera),
            spheres: vec![Sphere::plain(
                DVec3::ZERO,
                R,
                Surface::Emissive {
                    radiance: [1.0; 3],
                    limb: sun::LIMB,
                    photosphere,
                },
            )],
            now: t.seconds(),
            ..Default::default()
        };
        for _ in 0..3 {
            if let Some(ms) = gpu.time_scene(&mut renderer, &scene, (1920, 1080)) {
                println!("{what}: {ms:.3} ms on {}", gpu.adapter.get_info().name);
            }
        }
    }
}
