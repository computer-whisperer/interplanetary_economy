//! Atmospheres (M1c.md, step 6 plan, items 5 and 7), offscreen: Earth's
//! air (Hillaire's values) over a black ground.

use glam::DVec3;
use render::atmosphere::{Air, Atmosphere};
use render::camera::Camera;
use render::maps::{Image, Maps, Shading};
use render::offscreen::Offscreen;
use render::scene::AU_KM;
use render::{Renderer, Scene, Sphere, Surface, Target};
use std::sync::Arc;

const RADIUS: f64 = 6_371.0;

fn earth_air() -> Arc<Maps> {
    let air = Air::new(Atmosphere {
        bottom: RADIUS,
        top: RADIUS + 100.0,
        rayleigh: [5.802e-3, 13.558e-3, 33.1e-3],
        rayleigh_height: 8.0,
        mie_scattering: [3.996e-3; 3],
        mie_absorption: [4.40e-3; 3],
        mie_height: 1.2,
        mie_g: 0.8,
        absorption: [0.650e-3, 1.881e-3, 0.085e-3],
        absorption_center: 25.0,
        absorption_width: 15.0,
        ground_albedo: 0.3,
    });
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
        air: Some(air),
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

/// A black ground under Earth's air, the Sun behind the eye: the light is
/// all the air's, blue, and brightest toward the limb, where the eye sees
/// through the most of it.
#[test]
fn air_brightens_toward_the_limb() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue);
    let size = (400, 400);
    let center = DVec3::new(AU_KM, 0.0, 0.0);
    let eye = center - DVec3::X * 30_000.0;
    let camera = Camera::looking_at(eye, center, DVec3::Z, 30f64.to_radians());
    let scene = Scene {
        camera: Some(camera),
        sun: eye - DVec3::X * AU_KM,
        spheres: vec![Sphere {
            maps: Some(earth_air()),
            ..Sphere::plain(center, RADIUS, Surface::Lit([0.0; 3]))
        }],
        ..Default::default()
    };
    let image = radiance(&gpu, &mut renderer, &scene, size);
    let limb = camera
        .project(center + DVec3::Z * (RADIUS - 30.0), size)
        .unwrap();
    let mid = image[200 * 400 + 200];
    let edge = image[limb.y as usize * 400 + 200];
    println!("disk center {mid:?}, near the limb {edge:?}");
    assert!(
        mid[2] > mid[1] && mid[1] > mid[0] && mid[0] > 0.0,
        "blue: {mid:?}"
    );
    assert!(edge[2] > 2.0 * mid[2], "{edge:?} vs {mid:?}");
}

/// From the night side, the Sun just behind the limb: the air on the
/// Sun's side of the limb glows, the ground in the disk's middle is dark.
#[test]
fn night_side_shows_a_lit_limb() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue);
    let size = (400, 400);
    let center = DVec3::new(AU_KM, 0.0, 0.0);
    let distance = 30_000.0;
    let eye = center - DVec3::X * distance;
    let camera = Camera::looking_at(eye, center, DVec3::Z, 60f64.to_radians());
    // The Earth's angular radius from the eye, and the Sun 0.6° inside
    // its limb, above it.
    let radius_angle = (RADIUS / distance).asin();
    let behind = radius_angle - 0.6f64.to_radians();
    let sun = DVec3::new(behind.cos(), 0.0, behind.sin());
    let scene = Scene {
        camera: Some(camera),
        sun: eye + sun * AU_KM,
        spheres: vec![Sphere {
            maps: Some(earth_air()),
            ..Sphere::plain(center, RADIUS, Surface::Lit([0.3; 3]))
        }],
        ..Default::default()
    };
    let image = radiance(&gpu, &mut renderer, &scene, size);
    // The rays tangent 20 km over the limb, above and below.
    let tangent = ((RADIUS + 20.0) / distance).asin();
    let ray = |up: f64| eye + DVec3::new(tangent.cos(), 0.0, up * tangent.sin()) * distance;
    let top = camera.project(ray(1.0), size).unwrap();
    let bottom = camera.project(ray(-1.0), size).unwrap();
    let at = |p: glam::DVec2| image[p.y as usize * 400 + p.x as usize];
    let (lit, far, mid) = (at(top), at(bottom), image[200 * 400 + 200]);
    println!("limb toward the Sun {lit:?}, away from it {far:?}, disk center {mid:?}");
    assert!(lit[0] > 0.05, "{lit:?}");
    assert!(lit[0] > 100.0 * far[0].max(1e-6), "{lit:?} vs {far:?}");
    assert!(mid[0] < 1e-4, "{mid:?}");
}
