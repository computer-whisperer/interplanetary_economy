//! Eclipses (M1c.md, step 6 plan, items 6 and 7), offscreen: a sphere
//! the size of the Moon (a little larger, for an umbra) shading one the
//! size of Earth, the Sun 1 au away, against f64 geometry.

use glam::{DVec2, DVec3};
use render::camera::Camera;
use render::offscreen::Offscreen;
use render::scene::AU_KM;
use render::{Renderer, Scene, Sphere, Surface, Target};

const SUN_RADIUS: f64 = 695_700.0;
const EARTH: f64 = 6_371.0;
const MOON: f64 = 1_900.0;

fn radiance(
    gpu: &Offscreen,
    renderer: &mut Renderer,
    scene: &Scene,
    size: (u32, u32),
) -> Vec<[f32; 4]> {
    let target = Target::new(&gpu.device, size.0, size.1);
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    renderer.render(&gpu.device, &gpu.queue, &mut encoder, &target, scene, None);
    gpu.queue.submit(Some(encoder.finish()));
    gpu.read_rgba16f(&target.hdr)
}

/// The share of a disk of angular radius `a` covered by one of radius `b`,
/// centers `t` apart (flat; f64).
fn covered(a: f64, b: f64, t: f64) -> f64 {
    if t >= a + b {
        return 0.0;
    }
    if t <= (a - b).abs() {
        return (b * b).min(a * a) / (a * a);
    }
    let x = ((t * t + a * a - b * b) / (2.0 * t * a))
        .clamp(-1.0, 1.0)
        .acos();
    let y = ((t * t + b * b - a * a) / (2.0 * t * b))
        .clamp(-1.0, 1.0)
        .acos();
    let k = ((-t + a + b) * (t + a - b) * (t - a + b) * (t + a + b))
        .max(0.0)
        .sqrt();
    (a * a * x + b * b * y - 0.5 * k) / (std::f64::consts::PI * a * a)
}

/// The shadow lands where the line from the Sun's center through the
/// Moon's meets the ground (the umbra's centroid within half a pixel; the
/// penumbra is wider than the view, so its weight is cut off unevenly),
/// and across the penumbra the ground is dimmed by the share of the
/// Sun's disk the Moon covers there, computed in f64 (within 0.5%).
#[test]
fn the_moons_shadow_falls_where_geometry_puts_it() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue);
    let sun = DVec3::ZERO;
    let earth = DVec3::new(AU_KM, 0.0, 0.0);
    let moon = earth + DVec3::new(-384_400.0, 2_000.0, 1_500.0);
    // Where the axis from the Sun's center through the Moon's meets the
    // ground (the near root).
    let axis = (moon - sun).normalize();
    let oc = sun - earth;
    let b = oc.dot(axis);
    let hit = sun + axis * (-b - (b * b - (oc.length_squared() - EARTH * EARTH)).sqrt());
    let normal = (hit - earth).normalize();
    let eye = hit + (normal * 3.0 + DVec3::Y + DVec3::Z).normalize() * 15_000.0;
    let camera = Camera::looking_at(eye, hit, DVec3::Z, 12f64.to_radians());
    let size = (400, 400);
    let mut scene = Scene {
        camera: Some(camera),
        sun,
        sun_radius: SUN_RADIUS,
        spheres: vec![
            Sphere::plain(earth, EARTH, Surface::Lit([1.0; 3])),
            Sphere::plain(moon, MOON, Surface::Lit([0.1; 3])),
        ],
        ..Default::default()
    };
    let shaded = radiance(&gpu, &mut renderer, &scene, size);
    scene.sun_radius = 0.0;
    let bright = radiance(&gpu, &mut renderer, &scene, size);
    let (mut sum, mut umbra) = (DVec2::ZERO, 0);
    for (i, (s, l)) in shaded.iter().zip(&bright).enumerate() {
        if l[0] > 0.0 && s[0] == 0.0 {
            sum += DVec2::new(
                f64::from(i as u32 % size.0) + 0.5,
                f64::from(i as u32 / size.0) + 0.5,
            );
            umbra += 1;
        }
    }
    let got = sum / f64::from(umbra.max(1));
    let want = camera.project(hit, size).unwrap();
    let error = (got - want).length();
    println!("shadow's centroid {got:.2}, the axis {want:.2}: {error:.3} px; {umbra} px in umbra");
    assert!(umbra > 0, "no umbra");
    assert!(error < 0.5, "{error} px");
    // Across the penumbra: each pixel's ray to the ground in f64, and the
    // share of the Sun the Moon covers from there.
    let tan_half = (6f64.to_radians()).tan();
    let mut worst: f64 = 0.0;
    let mut checked = 0;
    for x in (0..400).step_by(7) {
        let y = want.y as u32;
        let i = (y * 400 + x) as usize;
        if bright[i][0] <= 0.0 {
            continue;
        }
        let ndc = DVec2::new(
            (f64::from(x) + 0.5) / 200.0 - 1.0,
            1.0 - (f64::from(y) + 0.5) / 200.0,
        ) * tan_half;
        let d = (camera.forward + camera.right * ndc.x + camera.up * ndc.y).normalize();
        let oc = eye - earth;
        let b = oc.dot(d);
        let disc = b * b - (oc.length_squared() - EARTH * EARTH);
        if disc < 0.0 {
            continue;
        }
        let p = eye + d * (-b - disc.sqrt());
        let (to_sun, to_moon) = (sun - p, moon - p);
        let share = covered(
            (SUN_RADIUS / to_sun.length()).asin(),
            (MOON / to_moon.length()).asin(),
            to_moon.angle_between(to_sun),
        );
        let seen = f64::from(shaded[i][0] / bright[i][0]);
        if share > 0.02 && share < 0.98 {
            worst = worst.max((seen - (1.0 - share)).abs());
            checked += 1;
        }
    }
    println!("{checked} penumbral pixels: worst {worst:.4} off the covered share");
    assert!(checked >= 5 && worst < 0.005, "{checked}, {worst}");
}
