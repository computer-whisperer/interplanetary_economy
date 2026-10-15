//! How the scene's radiance is shown (M1c.md, step 6 plan, items 1 and
//! 7, and part 10), offscreen: the tonemap, the eye's glare, the dither,
//! and the overlay drawn
//! after them.

use glam::DVec3;
use render::camera::Camera;
use render::glare::Eye;
use render::offscreen::Offscreen;
use render::scene::AU_KM;
use render::{Look, Marker, Renderer, Scene, Shape, Sphere, Surface, Target};

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

fn srgb_encode(c: f64) -> f64 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// The tonemap is the identity up to half the headroom, rises with the
/// radiance after it, and never passes the headroom.
#[test]
fn tonemap_is_linear_to_its_knee_and_under_the_headroom() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue);
    for headroom in [1.97_f32, 4.0] {
        let exposure = 3.0;
        let mut last = -1.0;
        for x in [
            0.0, 0.01, 0.1, 0.2, 0.3, 0.32, 0.5, 0.66, 1.0, 2.0, 5.0, 20.0, 1e3, 4e4,
        ] {
            let scene = Scene {
                background: [x; 3],
                look: Look {
                    exposure,
                    headroom,
                    ..Default::default()
                },
                ..Default::default()
            };
            let (_, shown) = draw(&gpu, &mut renderer, &scene, (16, 16));
            let got = shown[8 * 16 + 8][0];
            let linear = x * exposure;
            println!("headroom {headroom}: {x} × {exposure} → {got}");
            if linear <= 0.5 * headroom {
                assert!(
                    (got - linear).abs() <= 1e-3 * linear.max(1e-3),
                    "{got} vs {linear}"
                );
            } else {
                assert!(got < linear, "{got} vs {linear}");
            }
            assert!(got >= last, "falls: {got} after {last}");
            assert!(got <= headroom, "{got} over {headroom}");
            if x < 1.0 {
                assert!(got > last, "flat: {got} after {last}");
            }
            last = got;
        }
    }
}

/// Past the knee a color keeps its channels' ratios (a bright orange
/// stays orange, not white), its largest channel shown as a gray of that
/// level would be.
#[test]
fn tonemap_keeps_hue() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue);
    let shown = |renderer: &mut Renderer, background: [f32; 3]| {
        let scene = Scene {
            background,
            look: Look {
                exposure: 1.0,
                headroom: 4.0,
                ..Default::default()
            },
            ..Default::default()
        };
        draw(&gpu, renderer, &scene, (16, 16)).1[8 * 16 + 8]
    };
    for level in [1.0f32, 3.0, 30.0, 3e4] {
        let orange = [level, 0.67 * level, 0.34 * level];
        let got = shown(&mut renderer, orange);
        let gray = shown(&mut renderer, [level; 3]);
        println!("{orange:?} → {got:?}; gray → {gray:?}");
        assert!(
            (got[0] - gray[0]).abs() <= 1e-3 * gray[0],
            "{got:?} vs {gray:?}"
        );
        for c in 1..3 {
            let (want, ratio) = (orange[c] / orange[0], got[c] / got[0]);
            assert!(
                (ratio - want).abs() < 2e-3,
                "channel {c}: {ratio} vs {want}"
            );
        }
    }
}

/// A colored sphere exposed past f16's range, as a point and as a disk,
/// keeps its color in the scene's radiance: its spot and its pixels
/// scaled down whole (a focused planet's point at the body lift once
/// showed white).
#[test]
fn colors_keep_past_f16() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue);
    let albedo = [0.3f32, 0.2, 0.1];
    let center = DVec3::new(AU_KM, 0.0, 0.0);
    for (name, distance, exposure) in [("point", 1e7, 1e12f32), ("disk", 2e4, 1e7)] {
        let eye = center + DVec3::new(-distance, 0.0, 0.0);
        let scene = Scene {
            camera: Some(Camera::looking_at(
                eye,
                center,
                DVec3::Z,
                50f64.to_radians(),
            )),
            sun: DVec3::ZERO,
            spheres: vec![Sphere::plain(center, 3_000.0, Surface::Lit(albedo))],
            look: Look {
                exposure,
                ..Default::default()
            },
            ..Default::default()
        };
        let (hdr, _) = draw(&gpu, &mut renderer, &scene, (64, 64));
        let brightest = hdr.iter().map(|p| p[0]).fold(0f32, f32::max);
        let mut sum = [0f64; 3];
        for p in &hdr {
            for c in 0..3 {
                sum[c] += f64::from(p[c]);
            }
        }
        println!("{name}: brightest {brightest}, sum {sum:?}");
        // At the limit: the spot's red sums to it; the disk's pixels reach it.
        let red = if name == "point" {
            sum[0] as f32
        } else {
            brightest
        };
        assert!(red > 59_000.0, "{name} is not at the limit: {red}");
        for c in 1..3 {
            let (got, want) = (sum[c] / sum[0], f64::from(albedo[c] / albedo[0]));
            assert!(
                (got - want).abs() < 0.01 * want,
                "{name}, channel {c}: {got} vs {want}"
            );
        }
    }
}

/// On SDR the shown image is dithered by an 8-bit step where it will be
/// encoded: across a slow gradient (the Moon from low orbit, the Sun
/// low) the 8-bit image's error is noise, unbiased, rather than the
/// sawtooth of flat steps, which shows as bands. Measured by the error's
/// correlation between neighboring pixels; without the dither (headroom
/// just over 1: the same curve) it is near 1.
#[test]
fn sdr_dither_follows_a_slow_gradient() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue);
    let size = (512, 64);
    let moon = DVec3::new(AU_KM, 0.0, 0.0);
    let radius = 1_737.4;
    let eye = moon + DVec3::Z * (radius + 100.0);
    let camera = Camera::looking_at(eye, moon, DVec3::Y, 5f64.to_radians());
    // The Sun 88° from the zenith below the eye, off to the right.
    let sun_dir = DVec3::new(88f64.to_radians().sin(), 0.0, 88f64.to_radians().cos());
    // The error's mean and its correlation between neighbors, codes.
    let mut error = |headroom: f32| {
        let scene = Scene {
            camera: Some(camera),
            sun: moon + sun_dir * AU_KM,
            spheres: vec![Sphere::plain(moon, radius, Surface::Lit([0.5; 3]))],
            look: Look {
                exposure: 1.0,
                headroom,
                ..Default::default()
            },
            ..Default::default()
        };
        let (radiance, shown) = draw(&gpu, &mut renderer, &scene, size);
        let row = 32 * size.0 as usize;
        let code = |v: f32| (srgb_encode(f64::from(v)) * 255.0).round();
        let codes: Vec<f64> = shown[row..row + size.0 as usize]
            .iter()
            .map(|p| code(p[0]))
            .collect();
        let wanted: Vec<f64> = radiance[row..row + size.0 as usize]
            .iter()
            .map(|p| srgb_encode(f64::from(p[0])) * 255.0)
            .collect();
        let span = wanted[wanted.len() - 1] - wanted[0];
        let e: Vec<f64> = codes.iter().zip(&wanted).map(|(c, w)| c - w).collect();
        let mean = e.iter().sum::<f64>() / e.len() as f64;
        let variance: f64 = e.iter().map(|x| (x - mean).powi(2)).sum();
        let lagged: f64 = e.windows(2).map(|w| (w[0] - mean) * (w[1] - mean)).sum();
        let correlation = lagged / variance;
        println!(
            "headroom {headroom}: the gradient spans {span:.1} codes over {} px; error mean {mean:.3} codes, neighbors' correlation {correlation:.3}",
            size.0
        );
        assert!(span.abs() > 4.0 && span.abs() < 60.0, "{span} codes");
        (mean, correlation)
    };
    // All within the knee on SDR: radiance at most 0.5 × cos 88°.
    let (mean, correlation) = error(1.0);
    assert!(
        mean.abs() < 0.1 && correlation < 0.3,
        "{mean}, {correlation}"
    );
    let (_, correlation) = error(1.0001);
    assert!(correlation > 0.5, "{correlation}");
}

/// The tonemap's curve (post.wgsl), on one channel.
fn tonemap(x: f64, headroom: f64) -> f64 {
    let knee = 0.5 * headroom;
    if x <= knee {
        return x;
    }
    let room = headroom - knee;
    knee + room * (1.0 - (-(x - knee) / room).exp())
}

/// A disk `across` px wide of `radiance` at the center of a 1920×1080
/// view, seen with the eye's glare at the sandbox's pixel (`pixel`
/// radians) or without: the scene's radiance and the shown image.
fn disk(
    gpu: &Offscreen,
    renderer: &mut Renderer,
    radiance: f32,
    across: f64,
    headroom: f32,
    pixel: f32,
    eye: Option<Eye>,
) -> (Vec<[f32; 4]>, Vec<[f32; 4]>) {
    let size = (1920, 1080);
    let center = DVec3::new(AU_KM, 0.0, 0.0);
    let fov = 30f64.to_radians();
    let camera = Camera::looking_at(DVec3::ZERO, center, DVec3::Z, fov);
    let px = 2.0 * (fov / 2.0).tan() / 1080.0;
    let scene = Scene {
        camera: Some(camera),
        spheres: vec![Sphere::plain(
            center,
            AU_KM * px * across / 2.0,
            Surface::Emissive {
                radiance: [radiance; 3],
                limb: render::photosphere::EVEN,
                photosphere: None,
            },
        )],
        look: Look {
            exposure: 1.0,
            headroom,
            pixel,
            eye,
            ..Default::default()
        },
        ..Default::default()
    };
    draw(gpu, renderer, &scene, size)
}

/// The sandbox's pixel, radians (`sandbox::look::PIXEL`).
const PIXEL: f32 = 1.0 / (96.0 * 28.0);

/// The eye's glare is of the light past what the output shows: a disk
/// under the tonemap's knee shows none, and as much without its eye as
/// with; one past the headroom glares, in proportion to its excess, at
/// the Sun's radiance (46,000) as at 100 (at a headroom of 10, the glare
/// linear 40 px off). (Found in review of the bloom
/// before it: the levels' sum, stored in f16, saturated near the Sun.)
#[test]
fn glare_is_of_what_the_output_cannot_show() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue);
    let (_, with) = disk(
        &gpu,
        &mut renderer,
        0.45,
        40.0,
        1.0,
        PIXEL,
        Some(Eye::STANDARD),
    );
    let (_, without) = disk(&gpu, &mut renderer, 0.45, 40.0, 1.0, PIXEL, None);
    assert_eq!(with, without, "under the knee");
    // 40 px off a 6 px disk, against the light past the shown.
    let off = 1080 / 2 * 1920 + 1920 / 2 + 43;
    let mut glare = Vec::new();
    for radiance in [100.0f32, 46_000.0] {
        let (hdr, shown) = disk(
            &gpu,
            &mut renderer,
            radiance,
            6.0,
            10.0,
            PIXEL,
            Some(Eye::STANDARD),
        );
        let excess: f64 = hdr
            .iter()
            .map(|p| f64::from(p[0]) - tonemap(f64::from(p[0]), 10.0))
            .sum();
        let far = f64::from(shown[off][0]);
        println!("{radiance}: excess {excess:.4e}, 40 px off {far:.4e}");
        assert!(far > 1e-4, "{far}");
        glare.push(far / excess);
    }
    assert!((glare[1] / glare[0] - 1.0).abs() < 0.01, "{glare:?}");
}

/// The glare lands as the eye's does: of a small disk's light past the
/// headroom, the share landing in each octave of distance from it, 4 px
/// to 1,024 (where the image's edges cut it off), within 15% of the CIE's
/// glare spread function's (glare.rs); 1–4 px, where the first level's
/// one shape holds it, within 45%. At the sandbox's pixel and at half
/// its angle (a display scale of 2).
#[test]
fn glare_lands_as_the_eyes() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue);
    // A headroom of 1,000 keeps the glare linear a few pixels out; the
    // tonemap is undone where it is not.
    let headroom = 1000.0f32;
    let undo = |y: f32| {
        let (y, knee) = (f64::from(y), 0.5 * f64::from(headroom));
        if y <= knee {
            y
        } else {
            knee - knee * (1.0 - (y - knee) / knee).ln()
        }
    };
    for pixel in [PIXEL, PIXEL / 2.0] {
        let (hdr, without) = disk(&gpu, &mut renderer, 2000.0, 2.0, headroom, pixel, None);
        let (_, with) = disk(
            &gpu,
            &mut renderer,
            2000.0,
            2.0,
            headroom,
            pixel,
            Some(Eye::STANDARD),
        );
        let excess: f64 = hdr
            .iter()
            .map(|p| f64::from(p[0]) - tonemap(f64::from(p[0]), f64::from(headroom)))
            .sum();
        // Octave b ≥ 1: 2^(b-1)–2^b px; 0: under 1.
        let mut octaves = [0.0f64; 14];
        for (i, (w, wo)) in with.iter().zip(&without).enumerate() {
            let (x, y) = (
                (i % 1920) as f64 + 0.5 - 960.0,
                (i / 1920) as f64 + 0.5 - 540.0,
            );
            let r = x.hypot(y);
            let b = if r < 1.0 {
                0
            } else {
                (r.log2().floor() as usize + 1).min(13)
            };
            octaves[b] += undo(w[0]) - undo(wo[0]);
        }
        let degrees = f64::from(pixel).to_degrees();
        for (b, light) in octaves.iter().enumerate().take(11).skip(1) {
            let from = 2f64.powi(b as i32 - 1);
            let want = Eye::STANDARD.share(from * degrees, 2.0 * from * degrees);
            let got = light / excess;
            println!("pixel {pixel:.3e}, {from} px out: {got:.5}, the eye's {want:.5}");
            let tolerance = if b < 3 { 0.45 } else { 0.15 };
            assert!(
                (got / want - 1.0).abs() < tolerance,
                "{from} px: {got} vs {want}"
            );
        }
    }
}

/// The Sun's dimming (`Look::sun_lift`) takes its light, once, at every
/// size: as a point, fading into its disk, and as a disk; and the body
/// lift leaves it (it took the Sun's point to 2¹⁷ its flux: found in
/// review).
#[test]
fn the_suns_lift_dims_it_once_at_every_size() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue);
    let size = (400, 400);
    let center = DVec3::new(AU_KM, 0.0, 0.0);
    let fov = 30f64.to_radians();
    let camera = Camera::looking_at(DVec3::ZERO, center, DVec3::Z, fov);
    let px = 2.0 * (fov / 2.0).tan() / 400.0;
    for radius_px in [0.5, 2.0, 10.0] {
        let mut flux = |sun_lift: f32, body_lift: f32| {
            let scene = Scene {
                camera: Some(camera),
                spheres: vec![Sphere::plain(
                    center,
                    AU_KM * px * radius_px,
                    Surface::Emissive {
                        radiance: [100.0; 3],
                        limb: render::photosphere::EVEN,
                        photosphere: None,
                    },
                )],
                look: Look {
                    headroom: 1e5,
                    sun_lift,
                    body_lift,
                    ..Default::default()
                },
                ..Default::default()
            };
            let (radiance, _) = draw(&gpu, &mut renderer, &scene, size);
            radiance.iter().map(|p| f64::from(p[0])).sum::<f64>()
        };
        let (shown, dimmed, lifted) = (
            flux(1.0, 1.0),
            flux(1.0 / 64.0, 1.0),
            flux(1.0, 2f32.powi(15)),
        );
        println!("{radius_px} px: {shown:.4e}, dimmed {dimmed:.4e}, lifted {lifted:.4e}");
        assert!((dimmed * 64.0 / shown - 1.0).abs() < 2e-3, "{radius_px} px");
        assert!((lifted / shown - 1.0).abs() < 2e-3, "{radius_px} px");
    }
}

/// Lines and markers are drawn after the tonemap: exposure does not
/// touch them, while it does the body behind.
#[test]
fn exposure_leaves_the_overlay_alone() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue);
    let size = (200, 200);
    let center = DVec3::new(AU_KM, 0.0, 0.0);
    let eye = center - DVec3::X * 20_000.0;
    let camera = Camera::looking_at(eye, center, DVec3::Z, 40f64.to_radians());
    let color = [0.2, 0.6, 0.9];
    let mut seen = Vec::new();
    for exposure in [1.0, 40.0] {
        let scene = Scene {
            camera: Some(camera),
            sun: eye - DVec3::X * AU_KM,
            spheres: vec![Sphere::plain(center, 3_000.0, Surface::Lit([0.01; 3]))],
            markers: vec![Marker {
                position: center - DVec3::X * 5_000.0,
                color,
                size_px: 21.0,
                shape: Shape::Block,
            }],
            look: Look {
                exposure,
                headroom: 2.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let (_, shown) = draw(&gpu, &mut renderer, &scene, size);
        let marker = shown[100 * 200 + 100];
        let body = shown[100 * 200 + 140];
        println!("exposure {exposure}: marker {marker:?}, body {body:?}");
        seen.push((marker, body));
    }
    assert_eq!(seen[0].0, seen[1].0);
    for (got, want) in seen[0].0.iter().zip(color) {
        assert!((got - want).abs() < 1e-3, "{:?}", seen[0].0);
    }
    assert!(seen[1].1[0] > 10.0 * seen[0].1[0], "{seen:?}");
}

/// GPU time of the chain from scene to shown image at 1920×1080, with
/// nothing in the scene: the glare's levels and the tonemap.
#[test]
#[ignore = "a measurement: run with --ignored --nocapture"]
fn post_chain_gpu_time() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue);
    let scene = Scene {
        camera: Some(Camera::looking_at(
            DVec3::ZERO,
            DVec3::X,
            DVec3::Z,
            50f64.to_radians(),
        )),
        look: Look {
            exposure: 1.0,
            headroom: 1.0,
            pixel: PIXEL,
            eye: Some(Eye::STANDARD),
            ..Default::default()
        },
        ..Default::default()
    };
    for _ in 0..3 {
        match gpu.time_scene(&mut renderer, &scene, (1920, 1080)) {
            Some(ms) => println!(
                "empty scene with post: {ms:.3} ms on {}",
                gpu.adapter.get_info().name
            ),
            None => println!("no timestamps"),
        }
    }
}
