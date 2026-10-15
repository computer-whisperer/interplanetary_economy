//! Bodies' maps (M1c.md, step 6 plan, items 4 and 7), offscreen, with
//! synthetic maps: a place on the map lands where the body's frame puts
//! it, and relief tilts the surface the way the map says.

use glam::{DMat3, DVec2, DVec3};
use render::camera::Camera;
use render::maps::{Image, Maps, Shading, f32_to_f16};
use render::offscreen::Offscreen;
use render::scene::AU_KM;
use render::{Renderer, Scene, Sphere, Surface, Target};
use std::sync::Arc;

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

fn maps(color: Image, relief: Option<Image>) -> Arc<Maps> {
    Arc::new(Maps {
        mean: [0.5; 3],
        color,
        relief,
        clouds: None,
        cloud_albedo: 0.0,
        night: None,
        night_radiance: [0.0; 3],
        water: None,
        shading: Shading::Lambert,
    })
}

/// The unit vector at east longitude `lon` and latitude `lat` (degrees),
/// in a body's frame.
fn place(lon: f64, lat: f64) -> DVec3 {
    let (lon, lat) = (lon.to_radians(), lat.to_radians());
    DVec3::new(lat.cos() * lon.cos(), lat.cos() * lon.sin(), lat.sin())
}

/// A white mark at 37° E, 21° N on a black map, on a sphere turned
/// arbitrarily, lit from straight above it, lands within half a pixel of
/// where the body's frame puts that place, projected in f64. A map read
/// mirrored, shifted or with its axes swapped lands it hundreds of pixels
/// off.
#[test]
fn a_place_lands_where_the_frame_puts_it() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue);
    let (w, h) = (1024u32, 512u32);
    let (lon, lat) = (37.0, 21.0);
    let mut data = vec![0u8; (w * h * 4) as usize];
    let (cx, cy) = (
        ((lon + 180.0) / 360.0 * f64::from(w)) as i64,
        ((90.0 - lat) / 180.0 * f64::from(h)) as i64,
    );
    for y in cy - 1..=cy + 1 {
        for x in cx - 1..=cx + 1 {
            let k = (y as u32 * w + x as u32) as usize * 4;
            data[k..k + 4].copy_from_slice(&[255; 4]);
        }
    }
    // The mark's center: the middle of its 3 x 3 texels.
    let (lon, lat) = (
        (cx as f64 + 0.5) / f64::from(w) * 360.0 - 180.0,
        90.0 - (cy as f64 + 0.5) / f64::from(h) * 180.0,
    );
    let axes = DMat3::from_axis_angle(DVec3::new(0.3, -0.5, 0.8).normalize(), 2.1);
    let center = DVec3::new(AU_KM, 0.0, 0.0);
    let radius = 1_000.0;
    let normal = axes * place(lon, lat);
    let spot = center + normal * radius;
    let size = (400, 400);
    for off in [DVec3::new(0.2, 0.1, 0.0), DVec3::new(-0.15, 0.25, 0.1)] {
        let eye = spot + (normal + off).normalize() * 3_000.0;
        let camera = Camera::looking_at(eye, spot + off * 300.0, axes.z_axis, 20f64.to_radians());
        let scene = Scene {
            camera: Some(camera),
            sun: spot + normal * AU_KM,
            spheres: vec![Sphere {
                axes,
                maps: Some(maps(Image::srgba(w, h, data.clone()), None)),
                ..Sphere::plain(center, radius, Surface::Lit([1.0; 3]))
            }],
            ..Default::default()
        };
        let image = radiance(&gpu, &mut renderer, &scene, size);
        let (mut sum, mut weight) = (DVec2::ZERO, 0.0);
        for (i, p) in image.iter().enumerate() {
            if p[0] > 0.05 {
                let (x, y) = (i as u32 % size.0, i as u32 / size.0);
                sum += DVec2::new(f64::from(x) + 0.5, f64::from(y) + 0.5) * f64::from(p[0]);
                weight += f64::from(p[0]);
            }
        }
        assert!(weight > 0.0, "the mark is not drawn");
        let got = sum / weight;
        let want = camera.project(spot, size).unwrap();
        let error = (got - want).length();
        println!("mark at {got:.2}, its place at {want:.2}: {error:.3} px");
        assert!(error < 0.5, "{error} px");
    }
}

/// Relief whose normals lean 30° east (or north) everywhere: under a Sun
/// 30° east of overhead the surface faces it squarely (1 / cos 30° the
/// flat surface's light), under one 30° west it is lit at 60° (cos 60° /
/// cos 30°).
#[test]
fn relief_leans_the_way_the_map_says() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue);
    let lean = 30f64.to_radians();
    let center = DVec3::new(AU_KM, 0.0, 0.0);
    let radius = 1_000.0;
    // The sub-camera point: 0° E, 0° N, in an unturned frame: up is x,
    // east y, north z.
    let up = DVec3::X;
    let size = (64, 64);
    let eye = center + up * 5_000.0;
    let camera = Camera::looking_at(eye, center, DVec3::Z, 5f64.to_radians());
    let white = Image::srgba(4, 2, vec![255; 32]);
    for (name, toward, components) in [
        ("east", DVec3::Y, [lean.sin(), 0.0]),
        ("north", DVec3::Z, [0.0, lean.sin()]),
    ] {
        let texel: Vec<u8> = components
            .iter()
            .flat_map(|&c| f32_to_f16(c as f32).to_le_bytes())
            .collect();
        let relief = Image {
            width: 4,
            height: 2,
            format: wgpu::TextureFormat::Rg16Float,
            data: texel.repeat(8),
        };
        let mut light = |sun: DVec3, relief: Option<Image>| {
            let scene = Scene {
                camera: Some(camera),
                sun: center + sun * AU_KM,
                spheres: vec![Sphere {
                    maps: Some(maps(white.clone(), relief)),
                    ..Sphere::plain(center, radius, Surface::Lit([1.0; 3]))
                }],
                ..Default::default()
            };
            radiance(&gpu, &mut renderer, &scene, size)[32 * 64 + 32][0]
        };
        for (side, sign, want) in [
            ("the same side", 1.0, 1.0 / lean.cos()),
            ("the other side", -1.0, (2.0 * lean).cos() / lean.cos()),
        ] {
            let sun = (up * lean.cos() + toward * sign * lean.sin()).normalize();
            let ratio = f64::from(light(sun, Some(relief.clone()))) / f64::from(light(sun, None));
            println!("leaning {name}, the Sun on {side}: {ratio:.4} (want {want:.4})");
            assert!(
                (ratio / want - 1.0).abs() < 0.01,
                "{name}, {side}: {ratio} vs {want}"
            );
        }
    }
}
