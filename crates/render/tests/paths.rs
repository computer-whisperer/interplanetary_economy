//! Paths' lines (M1c.md, step 4 plan, item 7), offscreen: where they are
//! drawn against where 64-bit arithmetic puts them.

use glam::{DMat3, DVec2, DVec3};
use render::camera::Camera;
use render::offscreen::Offscreen;
use render::scene::AU_KM;
use render::{Path, Paths, Pose, Renderer, Sample, Scene, Target};
use std::sync::Arc;

const MOON: DVec3 = DVec3::new(-1.384_1e8, 4.612_3e7, 2.071e4);

fn draw(
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
    gpu.read_rgba16f(&target.color)
}

/// The cubic Hermite curve between two samples, in f64.
fn hermite(a: &Sample, b: &Sample, u: f64) -> DVec3 {
    let h = b.t - a.t;
    let (u2, u3) = (u * u, u * u * u);
    (2.0 * u3 - 3.0 * u2 + 1.0) * a.position
        + (u3 - 2.0 * u2 + u) * h * a.velocity
        + (-2.0 * u3 + 3.0 * u2) * b.position
        + (u3 - u2) * h * b.velocity
}

/// Samples of a circle of `radius` about `center` in the plane spanned by
/// `e1` and `e2`, `every` radians apart, at angular rate `w`, mapped into
/// `pose`'s coordinates.
fn circle(
    pose: &Pose,
    center: DVec3,
    radius: f64,
    e1: DVec3,
    e2: DVec3,
    every: f64,
    w: f64,
) -> Vec<Sample> {
    let n = (std::f64::consts::TAU / every).round() as usize;
    (0..=n)
        .map(|k| {
            let a = k as f64 * every;
            let world = center + radius * (a.cos() * e1 + a.sin() * e2);
            let v = radius * w * (-a.sin() * e1 + a.cos() * e2);
            Sample {
                t: a / w,
                position: pose.from_world(world),
                velocity: pose.axes.transpose() * v / pose.scale,
            }
        })
        .collect()
}

/// The f64 curve through `samples` in the world, projected: `per` points
/// per segment, those in front of the eye.
fn projected(
    cam: &Camera,
    pose: &Pose,
    samples: &[Sample],
    per: usize,
    size: (u32, u32),
) -> Vec<DVec2> {
    samples
        .windows(2)
        .flat_map(|w| {
            (0..=per).map(move |k| pose.to_world(hermite(&w[0], &w[1], k as f64 / per as f64)))
        })
        .filter_map(|p| cam.project(p, size))
        .collect()
}

/// The distance from `p` to the polyline through `points`.
fn distance_to(points: &[DVec2], p: DVec2) -> f64 {
    points
        .windows(2)
        .map(|w| {
            let (a, b) = (w[0], w[1]);
            let ab = b - a;
            let s = if ab.length_squared() > 0.0 {
                ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0)
            } else {
                0.0
            };
            (p - (a + s * ab)).length()
        })
        .fold(f64::INFINITY, f64::min)
}

/// Pixel centers whose green (in the past and future colors both) is over
/// `level`.
fn lit(pixels: &[[f32; 4]], size: (u32, u32), level: f32) -> Vec<DVec2> {
    pixels
        .iter()
        .enumerate()
        .filter(|(_, p)| p[1] > level)
        .map(|(i, _)| {
            let (x, y) = (i as u32 % size.0, i as u32 / size.0);
            DVec2::new(f64::from(x) + 0.5, f64::from(y) + 0.5)
        })
        .collect()
}

/// The worst distance from a lit pixel to the curve, and from a point on
/// the curve (on the screen) to the nearest lit pixel.
fn agreement(pixels: &[[f32; 4]], size: (u32, u32), curve: &[DVec2]) -> (f64, f64) {
    let on = lit(pixels, size, 0.5);
    assert!(!on.is_empty(), "the line is drawn");
    let stray = on
        .iter()
        .map(|&p| distance_to(curve, p))
        .fold(0.0, f64::max);
    let (w, h) = (f64::from(size.0), f64::from(size.1));
    let gap = curve
        .iter()
        .filter(|p| p.x > 2.0 && p.y > 2.0 && p.x < w - 2.0 && p.y < h - 2.0)
        .map(|&c| {
            on.iter()
                .map(|&p| (p - c).length())
                .fold(f64::INFINITY, f64::min)
        })
        .fold(0.0, f64::max);
    (stray, gap)
}

fn scene(camera: Camera, pose: Pose, samples: Vec<Sample>, now: f64) -> Scene {
    Scene {
        camera: Some(camera),
        paths: vec![Arc::new(Paths {
            paths: vec![Path {
                pose: 0,
                samples,
                future: [1.0, 1.0, 0.0],
                past: [0.0, 1.0, 1.0],
                gap: 0.0,
            }],
        })],
        poses: vec![pose],
        now,
        line_px: 1.5,
        ..Default::default()
    }
}

/// Gate item 2's check, for lines: a low lunar orbit in a frame a few au
/// across, centered far from it (as the Sun–Earth frame is), seen from 100
/// km to 5 au, lies where 64-bit arithmetic projects it. In single
/// precision the frame's coordinates alone would put it ~9 km off: 90
/// pixels from 100 km away.
#[test]
fn line_about_the_moon_from_100_km_to_5_au() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue);
    let size = (800, 600);
    let pose = Pose {
        origin: DVec3::new(2.1e5, -4.4e5, 3.0e3),
        axes: DMat3::from_rotation_z(2.1) * DMat3::from_rotation_x(0.4),
        scale: AU_KM,
    };
    let (e1, e2) = (
        DVec3::new(0.31, -0.72, 0.62).normalize(),
        DVec3::new(0.31, -0.72, 0.62)
            .normalize()
            .any_orthonormal_vector(),
    );
    let radius = 1_737.4 + 100.0;
    let samples = circle(
        &pose,
        MOON,
        radius,
        e1,
        e2,
        2f64.to_radians(),
        2.0 * std::f64::consts::PI / 7_000.0,
    );
    let fov = 50f64.to_radians();
    let mut worst: (f64, f64) = (0.0, 0.0);
    for distance in [100.0, 1e3, 1e4, 1e6, 1e8, 5.0 * AU_KM] {
        // Over the circle, looking at a point on it.
        let on = MOON + radius * e1;
        let eye = on + (e1 * 0.3 + e1.cross(e2)).normalize() * distance;
        let camera = Camera::looking_at(eye, on, e2, fov);
        let pixels = draw(
            &gpu,
            &mut renderer,
            &scene(camera, pose, samples.clone(), -1.0),
            size,
        );
        let curve = projected(&camera, &pose, &samples, 16, size);
        let (stray, gap) = agreement(&pixels, size, &curve);
        println!(
            "{distance:>14.0} km: lit pixels within {stray:.3} px of the curve, curve within {gap:.3} px of a lit pixel"
        );
        worst = (worst.0.max(stray), worst.1.max(gap));
    }
    // Half the 1.5 px width and half a pixel's diagonal.
    assert!(worst.0 < 1.5 && worst.1 < 0.75, "{worst:?}");
}

/// From 30 km over a low Earth orbit sampled every 2°, looking down at
/// the middle of a segment, the line follows the curve: the straight
/// chord there bows ~1 km from it, ~20 pixels.
#[test]
fn line_follows_the_curve_up_close() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue);
    let size = (800, 600);
    let pose = Pose {
        origin: DVec3::new(-2.6e7, 1.45e8, 1.0e4),
        ..Default::default()
    };
    let r = 6_778.0;
    let samples = circle(
        &pose,
        pose.origin,
        r,
        DVec3::X,
        DVec3::Y,
        2f64.to_radians(),
        2.0 * std::f64::consts::PI / 5_550.0,
    );
    let a = 1f64.to_radians();
    let under = pose.origin + r * DVec3::new(a.cos(), a.sin(), 0.0);
    let ahead = DVec3::new(-a.sin(), a.cos(), 0.0);
    let camera = Camera::looking_at(under + DVec3::Z * 30.0, under, ahead, 50f64.to_radians());
    let pixels = draw(
        &gpu,
        &mut renderer,
        &scene(camera, pose, samples.clone(), -1.0),
        size,
    );
    let curve = projected(&camera, &pose, &samples, 256, size);
    let (stray, gap) = agreement(&pixels, size, &curve);
    // How far the segment under the eye bows from its chord, on screen.
    let on_screen = |p: &DVec2| p.x >= 0.0 && p.y >= 0.0 && p.x < 800.0 && p.y < 600.0;
    let chord = projected(&camera, &pose, &samples[..2], 1, size);
    let bowed = projected(&camera, &pose, &samples[..2], 256, size)
        .iter()
        .filter(|p| on_screen(p))
        .map(|&p| distance_to(&chord, p))
        .fold(0.0, f64::max);
    println!(
        "lit within {stray:.3} px of the curve, the curve within {gap:.3} px of lit; the chord bows {bowed:.1} px"
    );
    assert!(bowed > 10.0, "the test sees the bow: {bowed} px");
    assert!(stray < 1.5 && gap < 0.75, "{stray} {gap}");
}

/// A segment from in front of the eye to behind it, passing within a
/// meter of it, draws its front part only, down to the near plane (1 m
/// ahead, on the screen here). Unclipped, its end behind projects through
/// the eye to the other side.
#[test]
fn segment_behind_the_eye_is_clipped() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue);
    let size = (400, 300);
    let camera = Camera::looking_at(DVec3::ZERO, DVec3::X, DVec3::Z, 60f64.to_radians());
    // Through (0, 0.4 m, 0.2 m), in the eye's plane.
    let (p0, p1) = (DVec3::new(50.0, 6e-4, 3e-4), DVec3::new(-50.0, 2e-4, 1e-4));
    let h = 100.0;
    let v = (p1 - p0) / h;
    let samples = vec![
        Sample {
            t: 0.0,
            position: p0,
            velocity: v,
        },
        Sample {
            t: h,
            position: p1,
            velocity: v,
        },
    ];
    let pixels = draw(
        &gpu,
        &mut renderer,
        &scene(camera, Pose::default(), samples, -1.0),
        size,
    );
    // The front part, at distances ahead from 50 km down to the near
    // plane, 1 m.
    let front: Vec<DVec2> = (0..=4_000)
        .map(|k| {
            let ahead = 50.0 * (1e-3f64 / 50.0).powf(k as f64 / 4_000.0);
            p0 + (p1 - p0) * ((50.0 - ahead) / 100.0)
        })
        .filter_map(|p| camera.project(p, size))
        .collect();
    let (stray, gap) = agreement(&pixels, size, &front);
    let (start, end) = (front[0], *front.last().unwrap());
    // Lit pixels beyond the start, away from the end.
    let wrong = lit(&pixels, size, 0.5)
        .iter()
        .filter(|&&p| (p - start).dot(end - start) < -2.0 * (end - start).length())
        .count();
    println!(
        "front part from {start:.1} to {end:.1}: lit within {stray:.3} px of it, it within {gap:.3} px of lit; {wrong} pixels on the wrong side"
    );
    assert!(
        (end - start).length() > 50.0,
        "the near-plane end is on the screen, apart"
    );
    assert!(
        stray < 1.5 && gap < 0.75 && wrong == 0,
        "{stray} {gap} {wrong}"
    );
}

/// Where the past color ends and the future one starts, in pixels along
/// the line from where the f64 curve is at `now` (within 6 px of it). Past
/// pixels are cyan (no red), future ones yellow (no blue).
fn boundary(
    pixels: &[[f32; 4]],
    size: (u32, u32),
    camera: &Camera,
    pose: &Pose,
    samples: &[Sample],
    now: f64,
) -> (f64, f64) {
    let k = samples.partition_point(|s| s.t <= now) - 1;
    let u = (now - samples[k].t) / (samples[k + 1].t - samples[k].t);
    let at = |u: f64| {
        camera
            .project(
                pose.to_world(hermite(&samples[k], &samples[k + 1], u)),
                size,
            )
            .unwrap()
    };
    let expected = at(u);
    let along = (at(u + 1e-4) - expected).normalize();
    let mut past_end = f64::NEG_INFINITY;
    let mut future_start = f64::INFINITY;
    for (i, p) in pixels.iter().enumerate() {
        let c = DVec2::new(
            f64::from(i as u32 % size.0) + 0.5,
            f64::from(i as u32 / size.0) + 0.5,
        );
        if (c - expected).length() > 6.0 {
            continue;
        }
        let s = (c - expected).dot(along);
        if p[1] > 0.5 && p[0] < 0.05 {
            past_end = past_end.max(s);
        } else if p[1] > 0.5 && p[2] < 0.05 {
            future_start = future_start.min(s);
        }
    }
    (past_end, future_start)
}

fn assert_meets((past_end, future_start): (f64, f64)) {
    assert!(past_end > -1.5 && past_end < 1.0, "{past_end}");
    assert!(future_start < 1.5 && future_start > -1.0, "{future_start}");
}

/// The past and future colors meet where the curve is at the scene's
/// moment, within a pixel.
#[test]
fn past_meets_future_at_the_moment() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue);
    let size = (400, 300);
    let camera = Camera::looking_at(
        DVec3::new(0.0, -300.0, 0.0),
        DVec3::ZERO,
        DVec3::Z,
        60f64.to_radians(),
    );
    // Across the screen, curving, at uneven speed, over 4 segments; the
    // times far from zero, as a plan's are.
    let t0 = 1.4e9;
    let samples: Vec<Sample> = (0..=4)
        .map(|k| {
            let u = f64::from(k) / 4.0;
            let x = -150.0 + 300.0 * u * u;
            Sample {
                t: t0 + 100.0 * u,
                position: DVec3::new(x, 0.0, 40.0 * (u * 3.0).sin()),
                velocity: DVec3::new(600.0 * u / 100.0, 0.0, 120.0 * (u * 3.0).cos() / 100.0),
            }
        })
        .collect();
    for at in [37.0, 61.5] {
        let now = t0 + at;
        let pose = Pose::default();
        let pixels = draw(
            &gpu,
            &mut renderer,
            &scene(camera, pose, samples.clone(), now),
            size,
        );
        let b = boundary(&pixels, size, &camera, &pose, &samples, now);
        println!(
            "at {at} s: past ends {:.2} px, future starts {:.2} px from the f64 point",
            b.0, b.1
        );
        assert_meets(b);
    }
}

/// On a segment receding from 100 to 10,000 km, the colors meet where the
/// curve is at the moment, not halfway across the screen (found in review:
/// interpolated across the screen, the time was 100+ px off).
#[test]
fn past_meets_future_on_a_receding_segment() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue);
    let size = (400, 300);
    let camera = Camera::looking_at(DVec3::ZERO, DVec3::X, DVec3::Z, 60f64.to_radians());
    let (p0, p1) = (
        DVec3::new(100.0, 40.0, -20.0),
        DVec3::new(10_000.0, -2_000.0, 600.0),
    );
    let h = 1_000.0;
    let v = (p1 - p0) / h;
    let t0 = 1.4e9;
    let samples = vec![
        Sample {
            t: t0,
            position: p0,
            velocity: v,
        },
        Sample {
            t: t0 + h,
            position: p1,
            velocity: v,
        },
    ];
    for at in [100.0, 500.0, 800.0] {
        let now = t0 + at;
        let pose = Pose::default();
        let pixels = draw(
            &gpu,
            &mut renderer,
            &scene(camera, pose, samples.clone(), now),
            size,
        );
        let b = boundary(&pixels, size, &camera, &pose, &samples, now);
        println!(
            "at {at} s: past ends {:.2} px, future starts {:.2} px from the f64 point",
            b.0, b.1
        );
        assert_meets(b);
    }
}

/// 100 days into a low orbit, seen from 30 km, the colors meet where the
/// ship is (found in review: times as f32 seconds since the path's start
/// put them 200 px apart).
#[test]
fn past_meets_future_100_days_in() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue);
    let size = (400, 300);
    let pose = Pose {
        origin: DVec3::new(-2.6e7, 1.45e8, 1.0e4),
        ..Default::default()
    };
    // A spiral widening 2 km a turn, so that its 1,560 turns (100 days)
    // lie apart, from t = 1.4e9 s.
    let w = 2.0 * std::f64::consts::PI / 5_550.0;
    let widening = 2.0 / 5_550.0;
    let samples: Vec<Sample> = (0..=1_560 * 180)
        .map(|k| {
            let a = f64::from(k) * 2f64.to_radians();
            let t = a / w;
            let r = 6_778.0 + widening * t;
            let (s, c) = a.sin_cos();
            Sample {
                t: 1.4e9 + t,
                position: DVec3::new(r * c, r * s, 0.0),
                velocity: r * w * DVec3::new(-s, c, 0.0) + widening * DVec3::new(c, s, 0.0),
            }
        })
        .collect();
    let now = samples[samples.len() - 50].t + 13.0;
    let at = pose.to_world(
        Path {
            pose: 0,
            samples: samples.clone(),
            future: [0.0; 3],
            past: [0.0; 3],
            gap: 0.0,
        }
        .at(now)
        .unwrap(),
    );
    let ahead = DVec3::new(pose.origin.y - at.y, at.x - pose.origin.x, 0.0).normalize();
    let camera = Camera::looking_at(
        at + DVec3::Z * 30.0 - ahead * 10.0,
        at,
        ahead,
        60f64.to_radians(),
    );
    let pixels = draw(
        &gpu,
        &mut renderer,
        &scene(camera, pose, samples.clone(), now),
        size,
    );
    let b = boundary(&pixels, size, &camera, &pose, &samples, now);
    println!(
        "past ends {:.2} px, future starts {:.2} px from the f64 point",
        b.0, b.1
    );
    assert_meets(b);
}

/// Long segments seen from close by, between their samples, where the
/// GPU's f32 would not do (found in review): a 650,000 km cruise segment
/// at 1 au from 10 km (f32 put it ~20 px off), and a 13,000 km segment
/// bowing ~80 km (an NRHO's, Earth-relative) from 18 km (32 pieces left
/// 3.5 px).
#[test]
fn long_segments_up_close() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue);
    let size = (800, 600);
    let pose = Pose::default();
    for (name, radius, chord, eye_km) in [
        ("cruise", AU_KM, 650_000.0, 10.0),
        ("NRHO stay", 274_000.0, 13_000.0, 18.0),
    ] {
        let every = chord / radius;
        let w = 2e-7;
        let samples: Vec<Sample> = (0..=3)
            .map(|k| {
                let a = 1.0 + every * f64::from(k);
                let (s, c) = a.sin_cos();
                Sample {
                    t: 1.4e9 + a / w,
                    position: radius * DVec3::new(c, s, 0.1),
                    velocity: radius * w * DVec3::new(-s, c, 0.0),
                }
            })
            .collect();
        let path = Path {
            pose: 0,
            samples: samples.clone(),
            future: [1.0, 1.0, 0.0],
            past: [0.0, 1.0, 1.0],
            gap: 0.0,
        };
        // 30% into the middle segment, looking along the curve from
        // beside it.
        let t = samples[1].t + 0.3 * (samples[2].t - samples[1].t);
        let on = path.at(t).unwrap();
        let ahead = (path.at(t + 1.0).unwrap() - on).normalize();
        let eye = on + DVec3::Z * eye_km;
        let camera =
            Camera::looking_at(eye, on + ahead * 4.0 * eye_km, DVec3::Z, 50f64.to_radians());
        let pixels = draw(
            &gpu,
            &mut renderer,
            &scene(camera, pose, samples.clone(), 0.0),
            size,
        );
        let curve = projected(&camera, &pose, &samples, 20_000, size);
        let (stray, gap) = agreement(&pixels, size, &curve);
        println!(
            "{name}: lit within {stray:.3} px of the curve, the curve within {gap:.3} px of lit"
        );
        assert!(stray < 1.5 && gap < 0.75, "{name}: {stray} {gap}");
    }
}

/// From 300 km over a low Earth orbit, the GPU cuts the segments near the
/// eye into a few pieces each (their 1 km of bow is several pixels), and
/// the line follows the curve (found in review: the up-close test only
/// reached the cut for an eye inside a chunk's bounds).
#[test]
fn line_cut_on_the_gpu_from_300_km() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue);
    let size = (800, 600);
    let pose = Pose {
        origin: DVec3::new(-2.6e7, 1.45e8, 1.0e4),
        ..Default::default()
    };
    let r = 6_778.0;
    let samples = circle(
        &pose,
        pose.origin,
        r,
        DVec3::X,
        DVec3::Y,
        2f64.to_radians(),
        2.0 * std::f64::consts::PI / 5_550.0,
    );
    let under = pose.origin + DVec3::new(r, 0.0, 0.0);
    let camera = Camera::looking_at(
        under + DVec3::Z * 300.0,
        under,
        DVec3::Y,
        50f64.to_radians(),
    );
    let scene = scene(camera, pose, samples.clone(), -1.0);
    let paths = scene.paths[0].clone();
    let visible = render::paths::Prepared::new(&paths).visible(&paths, &camera, &[pose], size, 0.0);
    let cuts: Vec<u32> = visible.draws.iter().map(|d| d.1).collect();
    println!(
        "pieces per segment in the draws: {cuts:?}; {} placed by the CPU",
        visible.pieces.len()
    );
    assert!(cuts.iter().any(|&n| n > 1 && n <= 32) && visible.pieces.is_empty());
    let pixels = draw(&gpu, &mut renderer, &scene, size);
    let curve = projected(&camera, &pose, &samples, 256, size);
    let (stray, gap) = agreement(&pixels, size, &curve);
    let chords = projected(&camera, &pose, &samples, 1, size);
    let bowed = curve
        .iter()
        .filter(|p| p.x >= 0.0 && p.y >= 0.0 && p.x < 800.0 && p.y < 600.0)
        .map(|&p| distance_to(&chords, p))
        .fold(0.0, f64::max);
    println!(
        "lit within {stray:.3} px of the curve, the curve within {gap:.3} px of lit; chords bow {bowed:.1} px"
    );
    assert!(bowed > 2.0, "the test sees the bow: {bowed} px");
    assert!(stray < 1.5 && gap < 0.75, "{stray} {gap}");
}

/// Gate item 3's headroom, measured (run with `--release --ignored`): two
/// million segments, 20 low orbits of 100,000 samples each turning slowly,
/// seen whole and from 10 km over one of them.
#[test]
#[ignore]
fn two_million_segments_gpu_time() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue);
    let center = DVec3::new(-2.6e7, 1.45e8, 1.0e4);
    let (r, w) = (6_778.0, 2.0 * std::f64::consts::PI / 5_550.0);
    let paths: Vec<Path> = (0..20)
        .map(|p| {
            let tilt = 0.3 + 0.02 * f64::from(p);
            let samples = (0..100_000)
                .map(|k| {
                    let t = f64::from(k) * 31.0;
                    let (sn, cn) = (1e-6 * t + f64::from(p)).sin_cos();
                    let e1 = DVec3::new(cn, sn, 0.0);
                    let e2 = DVec3::new(-sn * tilt.cos(), cn * tilt.cos(), tilt.sin());
                    let (s, c) = (w * t).sin_cos();
                    Sample {
                        t,
                        position: center + r * (c * e1 + s * e2),
                        velocity: r * w * (-s * e1 + c * e2),
                    }
                })
                .collect();
            Path {
                pose: 0,
                samples,
                future: [0.3, 0.6, 1.0],
                past: [0.1, 0.2, 0.35],
                gap: 0.0,
            }
        })
        .collect();
    let paths = Arc::new(Paths { paths });
    let size = (1280, 800);
    let views = [
        (
            "whole, from 60,000 km",
            center + DVec3::new(0.0, -50_000.0, 33_000.0),
        ),
        (
            "10 km over one",
            center + DVec3::new(0.3f64.cos(), 0.0, 0.3f64.sin()) * (r + 10.0),
        ),
    ];
    for (name, eye) in views {
        let camera = Camera::looking_at(eye, center, DVec3::Z, 50f64.to_radians());
        let scene = Scene {
            camera: Some(camera),
            paths: vec![paths.clone()],
            poses: vec![Pose::default()],
            now: 1.5e6,
            line_px: 1.5,
            ..Default::default()
        };
        let cpu = std::time::Instant::now();
        let prepared = render::paths::Prepared::new(&paths);
        let prepare_ms = cpu.elapsed().as_secs_f64() * 1e3;
        let cpu = std::time::Instant::now();
        let visible = prepared.visible(&paths, &camera, &scene.poses, size, scene.now);
        let visible_ms = cpu.elapsed().as_secs_f64() * 1e3;
        match gpu.time_scene(&mut renderer, &scene, size) {
            Some(ms) => println!(
                "{name}: GPU {ms:.2} ms; CPU {visible_ms:.2} ms a frame to cull and cut ({} runs, {} pieces placed), {prepare_ms:.0} ms to prepare",
                visible.instances.len(),
                visible.pieces.len()
            ),
            None => println!("no timestamps on {}", gpu.adapter.get_info().name),
        }
    }
}
