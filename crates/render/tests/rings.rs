//! Rings (M1c.md, step 6 plan, items 6 and 7), offscreen: uniform
//! synthetic rings (optical depth 0.5, albedo 0.5) against the slab's
//! formulas in f64, and in their planet's shadow.

use glam::{DMat3, DVec2, DVec3};
use render::camera::Camera;
use render::maps::{Image, Maps, Rings, Shading};
use render::offscreen::Offscreen;
use render::scene::AU_KM;
use render::{Renderer, Scene, Sphere, Surface, Target};
use std::sync::Arc;

const PLANET: f64 = 60_000.0;
const INNER: f64 = 80_000.0;
const OUTER: f64 = 130_000.0;
const TAU: f64 = 0.5;
const ALBEDO: f64 = 0.5;

fn ringed() -> Arc<Maps> {
    let n = 500;
    Arc::new(Maps {
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
        rings: Some(Rings::new(
            INNER,
            (OUTER - INNER) / f64::from(n),
            &vec![TAU as f32; n as usize],
            &vec![[ALBEDO as f32; 3]; n as usize],
        )),
    })
}

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

/// The slab's radiance (this crate's units, irradiance 1), seen at cosine
/// `mu` from its plane's normal, lit at `mu0`, on the same side or not,
/// at phase angle `alpha`: Lambert-sphere particles scattering once.
fn slab(mu: f64, mu0: f64, same_side: bool, alpha: f64) -> f64 {
    let phase = 8.0 / (3.0 * std::f64::consts::PI)
        * (alpha.sin() + (std::f64::consts::PI - alpha) * alpha.cos());
    let share = if same_side {
        mu0 / (mu0 + mu) * (1.0 - (-TAU * (1.0 / mu + 1.0 / mu0)).exp())
    } else {
        mu0 / (mu - mu0) * ((-TAU / mu).exp() - (-TAU / mu0).exp())
    };
    ALBEDO * phase / 4.0 * share
}

/// From 30° above the rings' plane, the Sun 50° above it on the same side
/// and then below it: a ring pixel's light is the slab's (within 1%); and
/// a point of the rings in the planet's shadow is dark.
#[test]
fn rings_light_as_a_slab_and_take_the_planets_shadow() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue);
    let center = DVec3::new(AU_KM, 0.0, 0.0);
    // The rings' plane: the ecliptic's (axes the identity), its normal z.
    let spot = center + DVec3::new(0.0, -105_000.0, 0.0);
    let view = DVec3::new(0.3, -0.5, 30f64.to_radians().tan() * 0.583).normalize();
    let eye = spot + view * 400_000.0;
    let camera = Camera::looking_at(eye, spot, DVec3::Z, 10f64.to_radians());
    let size = (200, 200);
    let pixel = camera.project(spot, size).unwrap();
    let at = |image: &[[f32; 4]], p: DVec2| image[p.y as usize * 200 + p.x as usize][0];
    for (side, sun_dir) in [
        (
            "lit face",
            DVec3::new(-0.6, -0.3, 50f64.to_radians().tan() * 0.67).normalize(),
        ),
        (
            "unlit face",
            DVec3::new(-0.6, -0.3, -50f64.to_radians().tan() * 0.67).normalize(),
        ),
    ] {
        let scene = Scene {
            camera: Some(camera),
            sun: spot + sun_dir * AU_KM,
            spheres: vec![Sphere {
                axes: DMat3::IDENTITY,
                maps: Some(ringed()),
                ..Sphere::plain(center, PLANET, Surface::Lit([0.5; 3]))
            }],
            ..Default::default()
        };
        let image = radiance(&gpu, &mut renderer, &scene, size);
        // The ray through that pixel's center, in f64.
        let tan_half = 5f64.to_radians().tan();
        let ndc = DVec2::new(
            (pixel.x.floor() + 0.5) / 100.0 - 1.0,
            1.0 - (pixel.y.floor() + 0.5) / 100.0,
        ) * tan_half;
        let d = (camera.forward + camera.right * ndc.x + camera.up * ndc.y).normalize();
        let t = (center.z - eye.z) / d.z;
        let p = eye + d * t;
        let light = (center + (sun_dir * AU_KM + spot - center) - p).normalize();
        let (mu, mu0) = (d.z.abs(), light.z.abs());
        let alpha = light.angle_between(-d);
        let want = slab(mu, mu0, light.z * -d.z > 0.0, alpha);
        let got = f64::from(at(&image, pixel));
        println!("{side}: {got:.5} (want {want:.5})");
        assert!((got / want - 1.0).abs() < 0.01, "{side}: {got} vs {want}");
    }
    // The Sun low beyond the planet: the rings straight behind it, away
    // from the Sun, are in its shadow; those to the side are not.
    let sun_dir = DVec3::new(0.0, 1.0, 0.2).normalize();
    // Looking down on the whole ring system.
    let camera = Camera::looking_at(
        center + DVec3::Z * 600_000.0,
        center,
        DVec3::Y,
        40f64.to_radians(),
    );
    let scene = Scene {
        camera: Some(camera),
        sun: center + sun_dir * AU_KM,
        spheres: vec![Sphere {
            maps: Some(ringed()),
            ..Sphere::plain(center, PLANET, Surface::Lit([0.5; 3]))
        }],
        ..Default::default()
    };
    let image = radiance(&gpu, &mut renderer, &scene, size);
    let place = |p: DVec3| at(&image, camera.project(center + p, size).unwrap());
    let shaded = place(DVec3::new(0.0, -105_000.0, 0.0));
    let aside = place(DVec3::new(105_000.0, 0.0, 0.0));
    println!("in the planet's shadow {shaded}, aside {aside}");
    assert!(shaded == 0.0 && aside > 1e-3, "{shaded}, {aside}");
}
