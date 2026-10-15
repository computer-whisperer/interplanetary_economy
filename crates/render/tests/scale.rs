//! Scale (M1c.md, step 3 plan, item 8), offscreen: positions survive
//! from 100 km to 5 au, spheres' silhouettes are exact up close, and
//! radiance and flux have the values the units say.

use glam::{DVec2, DVec3};
use render::camera::Camera;
use render::offscreen::Offscreen;
use render::scene::{AU_KM, lambert_phase};
use render::{Marker, Renderer, Scene, Shape, Sphere, Surface, Target};

/// A barycentric position of the Moon's size of numbers, km.
const MOON: DVec3 = DVec3::new(-1.384_1e8, 4.612_3e7, 2.071e4);
const MOON_RADIUS: f64 = 1_737.4;

/// The scene's radiance, and the shown image.
fn draw(
    gpu: &Offscreen,
    renderer: &mut Renderer,
    scene: &Scene,
    size: (u32, u32),
) -> (Vec<[f32; 4]>, Vec<[f32; 4]>) {
    let target = Target::new(&gpu.device, size.0, size.1);
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    renderer.render(&gpu.device, &gpu.queue, &mut encoder, &target, scene, None);
    gpu.queue.submit(Some(encoder.finish()));
    (
        gpu.read_rgba16f(&target.hdr),
        gpu.read_rgba16f(&target.color),
    )
}

/// The centroid of pixels with green over 0.5.
fn green_centroid(pixels: &[[f32; 4]], size: (u32, u32)) -> Option<DVec2> {
    let (mut sum, mut n) = (DVec2::ZERO, 0.0);
    for (i, p) in pixels.iter().enumerate() {
        if p[1] > 0.5 {
            let (x, y) = (i as u32 % size.0, i as u32 / size.0);
            sum += DVec2::new(f64::from(x) + 0.5, f64::from(y) + 0.5);
            n += 1.0;
        }
    }
    (n > 0.0).then(|| sum / n)
}

/// Gate item 2's check: a ship in low lunar orbit, seen from 100 km to
/// 5 au, lands where 64-bit arithmetic projects it, within a pixel.
#[test]
fn marker_in_low_lunar_orbit_lands_within_a_pixel() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue);
    let size = (800, 600);
    let ship = MOON + DVec3::new(0.31, -0.72, 0.62).normalize() * (MOON_RADIUS + 100.0);
    let fov = 50f64.to_radians();
    let mut worst: f64 = 0.0;
    for distance in [100.0, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 5.0 * AU_KM] {
        for dir in [DVec3::new(1.0, 0.2, 0.1), DVec3::new(-0.3, 0.8, -0.5)] {
            let eye = ship + dir.normalize() * distance;
            // Aimed off the ship, so that it lands off the screen's center.
            let side = dir.cross(DVec3::Z).normalize();
            let target = ship + side * 0.2 * distance + DVec3::Z * 0.1 * distance;
            let camera = Camera::looking_at(eye, target, DVec3::Z, fov);
            let scene = Scene {
                camera: Some(camera),
                markers: vec![Marker {
                    position: ship,
                    color: [0.0, 1.0, 0.0],
                    size_px: 5.0,
                    shape: Shape::Disc,
                }],
                ..Default::default()
            };
            let expected = camera.project(ship, size).unwrap();
            let (_, pixels) = draw(&gpu, &mut renderer, &scene, size);
            let got = green_centroid(&pixels, size).expect("the marker is drawn");
            let error = (got - expected).length();
            println!("{distance:>12.0} km: off by {error:.4} px at {expected:.1}");
            worst = worst.max(error);
        }
    }
    assert!(worst < 0.25, "{worst} px");
}

/// From 100 and 10 km above the Moon, looking at the horizon, the limb
/// sits where 64-bit arithmetic puts the tangent point.
#[test]
fn moon_limb_from_low_orbit() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue);
    let size = (400, 600);
    let up = DVec3::new(0.2, -0.4, 0.9).normalize();
    let ahead = up.cross(DVec3::X).normalize();
    for altitude in [100.0, 10.0] {
        let eye = MOON + up * (MOON_RADIUS + altitude);
        let camera = Camera::looking_at(eye, eye + ahead, up, 60f64.to_radians());
        let scene = Scene {
            camera: Some(camera),
            spheres: vec![Sphere::plain(
                MOON,
                MOON_RADIUS,
                Surface::Emissive {
                    radiance: [1.0; 3],
                    darkening: [0.0; 3],
                },
            )],
            ..Default::default()
        };
        let cos = MOON_RADIUS / (MOON_RADIUS + altitude);
        let tangent = MOON + (up * cos + ahead * (1.0 - cos * cos).sqrt()) * MOON_RADIUS;
        let expected = camera.project(tangent, size).unwrap().y;
        let (pixels, _) = draw(&gpu, &mut renderer, &scene, size);
        let column = size.0 / 2;
        let first = (0..size.1)
            .find(|&y| pixels[(y * size.0 + column) as usize][0] > 0.5)
            .expect("the Moon is in view");
        // The first row whose center is below the limb.
        let error = f64::from(first) + 0.5 - expected;
        println!("{altitude} km up: limb at row {first}, expected {expected:.2}");
        assert!((0.0..1.0).contains(&error), "{error} px");
    }
}

/// A lit sphere's center, facing the Sun at 1 au, has radiance albedo ×
/// cos 0; the Sun's disk its own.
#[test]
fn radiance_has_its_units() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue);
    let size = (300, 300);
    let center = DVec3::new(AU_KM, 0.0, 0.0);
    let eye = center + DVec3::new(0.0, -10_000.0, 0.0);
    let camera = Camera::looking_at(eye, center, DVec3::Z, 40f64.to_radians());
    let scene = Scene {
        camera: Some(camera),
        // Behind the eye, 1 au from the sphere.
        sun: center + DVec3::new(0.0, -AU_KM, 0.0),
        spheres: vec![Sphere::plain(
            center,
            1_000.0,
            Surface::Lit([0.5, 0.25, 0.125]),
        )],
        ..Default::default()
    };
    let (pixels, _) = draw(&gpu, &mut renderer, &scene, size);
    let mid = pixels[150 * 300 + 150];
    println!("lit center {mid:?}");
    for (got, want) in mid.iter().zip([0.5, 0.25, 0.125]) {
        assert!((got - want).abs() < 1e-3 * want, "{mid:?}");
    }
    let sun_radiance = (AU_KM / 695_700.0).powi(2) as f32;
    let sun_scene = Scene {
        camera: Some(Camera::looking_at(
            DVec3::ZERO,
            DVec3::new(AU_KM, 0.0, 0.0),
            DVec3::Z,
            1f64.to_radians(),
        )),
        spheres: vec![Sphere::plain(
            DVec3::new(AU_KM, 0.0, 0.0),
            695_700.0,
            Surface::Emissive {
                radiance: [sun_radiance; 3],
                darkening: [0.0; 3],
            },
        )],
        ..Default::default()
    };
    let (pixels, _) = draw(&gpu, &mut renderer, &sun_scene, size);
    let mid = pixels[150 * 300 + 150];
    println!("the Sun {mid:?}, wanted {sun_radiance}");
    assert!((mid[0] / sun_radiance - 1.0).abs() < 1e-3);
}

/// The flux a sphere puts on screen, summed over its pixels, each
/// weighted by the sky it subtends, at the screen's center and near its
/// corner:
/// - resolved (40 and 200 pixels across), the same as the image
///   integrated in f64 on the CPU, 8×8 rays a pixel (the Lambertian
///   formula for a distant sphere runs 2–7% low at these sizes: up close
///   less than a hemisphere is visible); edges are antialiased, so the
///   pixels at the limb carry their share (with one ray a pixel, the
///   reference was itself 0.7% off at 40 px);
/// - as a point (one pixel across), the distant sphere's flux, (2/3)·A·E·
///   Φ(α)·Ω (found in review: off the axis, points were too faint by up
///   to 1/cos³θ).
#[test]
fn points_carry_their_spheres_flux() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue);
    let size = (400, 300);
    let fov = 60f64.to_radians();
    let radius = 1_000.0;
    let center = DVec3::new(AU_KM, 0.0, 0.0);
    // The Sun 60° off the line of sight: phase angle 60°, at 1 au.
    let alpha = 60f64.to_radians();
    let sun = center + DVec3::new(-alpha.cos(), alpha.sin(), 0.0) * AU_KM;
    let light = (sun - center).normalize();
    let tan_half = (fov / 2.0).tan();
    let aspect = f64::from(size.0) / f64::from(size.1);
    // A view direction (unnormalized) through `(x, y)` in pixels, and
    // the sky a pixel there subtends.
    let ray = |cam: &Camera, x: f64, y: f64| {
        let ndc_x = x / f64::from(size.0) * 2.0 - 1.0;
        let ndc_y = 1.0 - y / f64::from(size.1) * 2.0;
        cam.forward + cam.right * ndc_x * tan_half * aspect + cam.up * ndc_y * tan_half
    };
    const RAYS: u32 = 8;
    let side = 2.0 * tan_half / f64::from(size.1);
    let sky = |r: DVec3| side * side / r.length().powi(3);
    for (place, offset) in [("center", (0.0, 0.0)), ("corner", (0.85, 0.8))] {
        for pixels_across in [200.0, 40.0, 1.0] {
            let distance = radius / (pixels_across / 2.0 * side).sin();
            // Aimed off the sphere so that it lands at `offset` in NDC.
            let eye = center - DVec3::X * distance;
            let off = DVec3::new(0.0, -offset.0 * tan_half * aspect, offset.1 * tan_half);
            let camera = Camera::looking_at(eye, eye + (DVec3::X - off) * distance, DVec3::Z, fov);
            let scene = Scene {
                camera: Some(camera),
                sun,
                spheres: vec![Sphere::plain(center, radius, Surface::Lit([1.0; 3]))],
                ..Default::default()
            };
            let (image, _) = draw(&gpu, &mut renderer, &scene, size);
            let (mut flux, mut exact) = (0.0, 0.0);
            let oc = center - eye;
            for (i, p) in image.iter().enumerate() {
                let (x, y) = (f64::from(i as u32 % size.0), f64::from(i as u32 / size.0));
                flux += f64::from(p[0]) * sky(ray(&camera, x + 0.5, y + 0.5));
                // The same pixel ray-cast in f64, a sky's worth per ray.
                for k in 0..RAYS * RAYS {
                    let (u, v) = (
                        (f64::from(k % RAYS) + 0.5) / f64::from(RAYS),
                        (f64::from(k / RAYS) + 0.5) / f64::from(RAYS),
                    );
                    let r = ray(&camera, x + u, y + v);
                    let d = r.normalize();
                    let b = d.dot(oc);
                    let disc = b * b - (oc.length_squared() - radius * radius);
                    if disc >= 0.0 && b > 0.0 {
                        let n = (eye + d * (b - disc.sqrt()) - center) / radius;
                        exact += n.dot(light).max(0.0) * sky(r) / f64::from(RAYS * RAYS);
                    }
                }
            }
            let half = (radius / distance).asin();
            let distant = 2.0 / 3.0 * lambert_phase(alpha) * std::f64::consts::PI * half * half;
            println!(
                "{place}, {pixels_across} px across: flux {flux:.4e}, f64 {exact:.4e}, distant sphere {distant:.4e}"
            );
            if pixels_across > 2.0 {
                assert!(
                    (flux / exact - 1.0).abs() < 1e-3,
                    "{place}: {flux} vs {exact}"
                );
            } else {
                assert!(
                    (flux / distant - 1.0).abs() < 1e-2,
                    "{place}: {flux} vs {distant}"
                );
            }
        }
    }
}

/// A glowing sphere darkened toward its limb keeps its flux: its pixels,
/// each weighted by the sky it subtends, sum to the even disk's within
/// 0.3%, while its center is brighter by 1 / (1 − u/3).
#[test]
fn limb_darkening_keeps_the_flux() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue);
    let size = (300, 300);
    let fov = 4f64.to_radians();
    let center = DVec3::new(AU_KM, 0.0, 0.0);
    let camera = Camera::looking_at(DVec3::ZERO, center, DVec3::Z, fov);
    let u = [0.6f32, 0.67, 0.75];
    let tan_half = (fov / 2.0).tan();
    let side = 2.0 * tan_half / 300.0;
    let flux = |image: &[[f32; 4]], c: usize| -> f64 {
        image
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let (x, y) = ((i % 300) as f64 + 0.5, (i / 300) as f64 + 0.5);
                let r = DVec3::new(
                    (x / 150.0 - 1.0) * tan_half,
                    (1.0 - y / 150.0) * tan_half,
                    -1.0,
                );
                f64::from(p[c]) * side * side / r.length().powi(3)
            })
            .sum()
    };
    let mut seen = Vec::new();
    for darkening in [[0.0; 3], u] {
        let scene = Scene {
            camera: Some(camera),
            spheres: vec![Sphere::plain(
                center,
                695_700.0,
                Surface::Emissive {
                    radiance: [1.0; 3],
                    darkening,
                },
            )],
            ..Default::default()
        };
        let (image, _) = draw(&gpu, &mut renderer, &scene, size);
        seen.push((image[150 * 300 + 150], [0, 1, 2].map(|c| flux(&image, c))));
    }
    for (c, u) in u.into_iter().enumerate() {
        let (center_even, center_dark) = (seen[0].0[c], seen[1].0[c]);
        let want = 1.0 / (1.0 - u / 3.0);
        println!(
            "channel {c}: center {center_dark} (want {want}), flux {:.5} vs {:.5}",
            seen[1].1[c], seen[0].1[c]
        );
        assert!((center_dark / center_even / want - 1.0).abs() < 2e-3);
        assert!((seen[1].1[c] / seen[0].1[c] - 1.0).abs() < 3e-3);
    }
}
