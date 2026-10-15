//! The bodies' brightness against JPL Horizons' apparent magnitudes
//! (M1c.md, Realism; fixtures from tools/photometry/fixtures.sh): the
//! phase curves as `photometry` computes them, and the flux the renderer
//! shows; and the colors it shows them in.

use glam::DVec3;
use orbit::ephem::{BodyId, Ephemeris};
use orbit::time::Epoch;
use render::offscreen::Offscreen;
use render::{Camera, Look, Renderer, Scene, Target};
use sandbox::bodies;
use sandbox::photometry::{self, Curve, Seen, V_SUN, saturn_with_rings};

/// A fixture's row: when, APmag, the target's distances from the Sun and
/// the observer (au), and its phase angle (S-T-O, degrees).
struct Row {
    t: Epoch,
    magnitude: f64,
    r: f64,
    delta: f64,
    alpha: f64,
}

/// `target`'s rows seen from the body `center` (its NAIF id), but for
/// those Horizons leaves out (past its curves' phase angles).
fn rows(target: &str, center: u32) -> Vec<Row> {
    let path = format!(
        "{}/tests/fixtures/horizons_mag_{target}_from_{center}.txt",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(&path).expect("the fixture reads");
    let body = text
        .split("$$SOE")
        .nth(1)
        .and_then(|s| s.split("$$EOE").next())
        .expect("the fixture's table");
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    body.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split(',').map(str::trim).collect();
            if f.len() < 10 || f[3] == "n.a." {
                return None;
            }
            // "2045-Mar-01 00:00:00.000": all at midnight.
            let date: Vec<&str> = f[0].split([' ', '-']).collect();
            let month = MONTHS.iter().position(|m| *m == date[1])? as u32 + 1;
            Some(Row {
                t: Epoch::from_ymd(date[0].parse().ok()?, month, date[2].parse().ok()?),
                magnitude: f[3].parse().ok()?,
                r: f[5].parse().ok()?,
                delta: f[7].parse().ok()?,
                alpha: f[9].parse().ok()?,
            })
        })
        .collect()
}

/// `target` seen from `observer` at `t`, Horizons' phase angle `alpha`
/// (the latitudes from this game's ephemeris and rotation models).
fn seen(eph: &Ephemeris, target: BodyId, observer: Option<BodyId>, t: Epoch, alpha: f64) -> Seen {
    let s = bodies::sphere(eph, target, t).expect("the target is drawn");
    let sun = eph.barycentric_position(eph.sun(), t);
    let eye = observer.map_or(sun, |o| eph.barycentric_position(o, t));
    Seen {
        alpha,
        ..Seen::from(&s, eye, sun)
    }
}

/// Each curve against Horizons' APmag − 5 log10(r Δ), at the phase angle
/// it gives, every 30 days over 2025–2080, from Earth and, for the outer
/// planets, from a body farther out (to 176° of phase): within
/// 0.002 mag (Uranus's pole's latitudes and Saturn's rings' opening this
/// game's; Saturn with its rings where Horizons counts them, under 6.5°
/// of phase, opened under 27°); Mars within 0.13, the terms for the
/// longitude it shows and its season left out.
#[test]
fn curves_match_horizons() {
    let eph = Ephemeris::standard();
    let id = |s: &str| eph.lookup(s).unwrap();
    for (target, center, observer, tolerance) in [
        ("mercury", 399, Some("earth"), 0.002),
        ("venus", 399, Some("earth"), 0.002),
        ("moon", 399, Some("earth"), 0.002),
        ("mars", 399, Some("earth"), 0.13),
        ("jupiter", 399, Some("earth"), 0.002),
        ("saturn", 399, Some("earth"), 0.002),
        ("uranus", 399, Some("earth"), 0.002),
        ("neptune", 399, Some("earth"), 0.002),
        ("jupiter", 699, Some("saturn"), 0.002),
        ("saturn", 799, Some("uranus"), 0.002),
        ("uranus", 899, Some("neptune"), 0.002),
        // From Pluto, not in the ephemeris: Neptune's curve takes no
        // latitudes.
        ("neptune", 999, None, 0.002),
    ] {
        let b = id(target);
        let curve = Curve::of(eph.body(b)).expect("a curve");
        let rows = rows(target, center);
        assert!(rows.len() > 500, "{target}: {} rows", rows.len());
        let (mut worst, mut rings, mut range) = (0f64, 0, (f64::MAX, 0f64));
        for row in &rows {
            let seen = seen(eph, b, observer.map(id), row.t, row.alpha);
            let want = row.magnitude - 5.0 * (row.r * row.delta).log10();
            let got = match (target, saturn_with_rings(&seen)) {
                ("saturn", Some(v)) => {
                    rings += 1;
                    v
                }
                _ => curve.magnitude(&seen),
            };
            worst = worst.max((got - want).abs());
            range = (range.0.min(row.alpha), range.1.max(row.alpha));
        }
        println!(
            "{target} from {center}: {} rows, {:.1}° to {:.1}° of phase, {rings} with rings; worst {worst:.4} mag",
            rows.len(),
            range.0,
            range.1
        );
        assert!(worst < tolerance, "{target} from {center}: {worst} mag");
    }
}

/// Earth's curve is Robinson 2025's fit (not Horizons', Mallama's): its
/// full-phase magnitude from its V-band albedo, 0.215, and its phase
/// function, a Henyey-Greenstein's of g = −0.33 at 180° − α, at 90° and
/// 144° (computed apart, in Python).
#[test]
fn earths_curve_is_robinsons() {
    let eph = Ephemeris::standard();
    let curve = Curve::of(eph.body(eph.lookup("earth").unwrap())).unwrap();
    let at = |alpha| {
        curve.magnitude(&Seen {
            alpha,
            observer_lat: 0.0,
            sun_lat: 0.0,
        })
    };
    let full = at(0.0);
    let phase = |alpha| 10f64.powf(-0.4 * (at(alpha) - full));
    println!(
        "Earth: V(1,0) {full:.4}, phase function at 90° {:.4}, 144° {:.4}",
        phase(90.0),
        phase(144.0)
    );
    assert!((full - -3.216).abs() < 0.001, "{full}");
    assert!((phase(90.0) - 0.258).abs() < 0.001);
    assert!((phase(144.0) - 0.143).abs() < 0.001);
    assert_eq!(curve.range(), (0.0, 144.0));
}

/// The flux the renderer shows of each planet and the Moon, from Earth's
/// center, on dates across their phases (each pixel × the sky it
/// subtends, as a camera records it): as a V magnitude (the stars' units)
/// against Horizons' APmag. The planets are points in the 50° view
/// (Saturn's carrying its rings' light), to 177° of phase: within 0.01
/// mag, Mars 0.15. The Moon is a disk ~390 px across in a 0.8° one:
/// within 0.35 mag to 150° of phase, as its map lights it (waxing and
/// waning differ by up to 0.33 mag near 100°, where Allen's curve is one
/// for both); a thinner crescent shows limb highlands brighter than its
/// near side's mean, which the curve counts already (at 166°, 0.5 mag
/// over).
#[test]
fn bodies_show_their_brightness() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue);
    sandbox::maps::wait().expect("data/bodies reads");
    let eph = Ephemeris::standard();
    let earth = eph.lookup("earth").unwrap();
    let size = 600u32;
    for (target, tolerance, phases) in [
        ("mercury", 0.01, 180.0),
        ("venus", 0.01, 180.0),
        ("moon", 0.35, 150.0),
        ("mars", 0.15, 180.0),
        ("jupiter", 0.01, 180.0),
        ("saturn", 0.01, 180.0),
        ("uranus", 0.01, 180.0),
        ("neptune", 0.01, 180.0),
    ] {
        let b = eph.lookup(target).unwrap();
        let rows = rows(target, 399);
        let fov = if target == "moon" { 0.8f64 } else { 50.0 }.to_radians();
        let tan_half = (fov / 2.0).tan();
        let side = 2.0 * tan_half / f64::from(size);
        let mut report = Vec::new();
        // Every 24th row, and every one past 165° of phase (Mercury's and
        // Venus's inferior conjunctions, where a point follows its curve
        // past where a disk's gain is held).
        let picked = rows
            .iter()
            .enumerate()
            .filter(|(i, r)| i % (rows.len() / 24) == 0 || (target != "moon" && r.alpha > 165.0))
            .map(|(_, r)| r);
        for row in picked {
            let eye = eph.barycentric_position(earth, row.t);
            let at = eph.barycentric_position(b, row.t);
            let up = if (at - eye).normalize().z.abs() > 0.9 {
                DVec3::X
            } else {
                DVec3::Z
            };
            let camera = Camera::looking_at(eye, at, up, fov);
            let mut spheres = bodies::spheres(eph, &[b], row.t, eye);
            let exposure = 1e4f32;
            let scene = Scene {
                camera: Some(camera),
                sun: eph.barycentric_position(eph.sun(), row.t),
                spheres: std::mem::take(&mut spheres),
                look: Look {
                    exposure,
                    ..Default::default()
                },
                ..Default::default()
            };
            let target_tex = Target::new(&gpu.device, size, size);
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            renderer.render(
                &gpu.device,
                &gpu.queue,
                &mut encoder,
                &target_tex,
                &scene,
                None,
            );
            gpu.queue.submit(Some(encoder.finish()));
            let image = gpu.read_rgba16f(&target_tex.hdr);
            let mut flux = 0.0;
            for (i, p) in image.iter().enumerate() {
                let (x, y) = (
                    (i as u32 % size) as f64 + 0.5,
                    (i as u32 / size) as f64 + 0.5,
                );
                let r = DVec3::new(
                    (x / f64::from(size) * 2.0 - 1.0) * tan_half,
                    (1.0 - y / f64::from(size) * 2.0) * tan_half,
                    -1.0,
                );
                let lum =
                    0.2126 * f64::from(p[0]) + 0.7152 * f64::from(p[1]) + 0.0722 * f64::from(p[2]);
                flux += lum * side * side / r.length().powi(3);
            }
            let v = V_SUN - 2.5 * (flux / f64::from(exposure) / std::f64::consts::PI).log10();
            report.push((row.alpha, v - row.magnitude));
        }
        let worst = report
            .iter()
            .filter(|(a, _)| *a <= phases)
            .map(|(_, d)| d.abs())
            .fold(0f64, f64::max);
        let shown: Vec<String> = report
            .iter()
            .map(|(a, d)| format!("{a:.0}° {d:+.3}"))
            .collect();
        println!("{target}: {}", shown.join(", "));
        assert!(worst < tolerance, "{target}: {worst} mag");
    }
}

/// Disks without air or maps (Uranus, Neptune; Phobos, Bennu, shaped)
/// and Jupiter's, seen from 100 radii and resolved (~100 px across), from
/// 0° to 120° of phase: the flux rendered against their curves' at that
/// geometry, within 0.03 mag; a shaped body's with its lightcurve there
/// (`lightcurve`), within 0.1 (its self-shadowing there against its mean
/// at that phase, and 0.03–0.06 under at 120°:
/// `shaped_disks_average_to_their_curves`). (Nearer, a
/// disk shows less than a hemisphere: from 12 radii, 0.2 mag under its
/// curve at 120°.)
#[test]
fn disks_show_their_curves() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue);
    sandbox::maps::wait().expect("data/bodies reads");
    let eph = Ephemeris::standard();
    let t = Epoch::from_ymd(2045, 3, 1);
    let sun = eph.barycentric_position(eph.sun(), t);
    let size = 600u32;
    for id in ["uranus", "neptune", "phobos", "bennu", "jupiter"] {
        let b = eph.lookup(id).unwrap();
        let curve = Curve::of(eph.body(b)).expect("a curve");
        let (c, radius) = (eph.barycentric_position(b, t), eph.body(b).radius);
        let to_sun = (sun - c).normalize();
        let side = to_sun.cross(DVec3::Z).normalize();
        let mut report = Vec::new();
        for alpha in [0.5f64, 30.0, 60.0, 90.0, 120.0] {
            let a = alpha.to_radians();
            let d = radius * 100.0;
            let eye = c + (to_sun * a.cos() + side * a.sin()) * d;
            let fov = (radius / d).asin() * 6.0;
            let spheres = bodies::spheres(eph, &[b], t, eye);
            let seen = Seen::from(&spheres[0], eye, sun);
            let au = render::scene::AU_KM;
            let want = curve.magnitude(&seen)
                + 5.0 * ((sun - c).length() / au * d / au).log10()
                + lightcurve(&spheres[0], to_sun, (eye - c).normalize(), seen.alpha);
            let scene = Scene {
                camera: Some(Camera::looking_at(eye, c, DVec3::Z, fov)),
                sun,
                spheres,
                ..Default::default()
            };
            let v = shown_magnitude(&gpu, &mut renderer, &scene, fov, size);
            report.push((alpha, v - want));
        }
        let shown: Vec<String> = report
            .iter()
            .map(|(a, d)| format!("{a:.0}° {d:+.3}"))
            .collect();
        println!("{id}: {}", shown.join(", "));
        let worst = report.iter().map(|(_, d)| d.abs()).fold(0f64, f64::max);
        let tolerance = if bodies::shape(eph, b).is_some() {
            0.1
        } else {
            0.03
        };
        assert!(worst < tolerance, "{id}: {worst} mag");
    }
}

/// The V magnitude the renderer shows for `scene`, `fov` high on `size`²
/// pixels: its pixels' luminance, each times the sky it subtends.
fn shown_magnitude(
    gpu: &Offscreen,
    renderer: &mut Renderer,
    scene: &Scene,
    fov: f64,
    size: u32,
) -> f64 {
    let target = Target::new(&gpu.device, size, size);
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    renderer.render(&gpu.device, &gpu.queue, &mut encoder, &target, scene, None);
    gpu.queue.submit(Some(encoder.finish()));
    let tan_half = (fov / 2.0).tan();
    let side = 2.0 * tan_half / f64::from(size);
    let flux: f64 = gpu
        .read_rgba16f(&target.hdr)
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let (x, y) = (
                (i as u32 % size) as f64 + 0.5,
                (i as u32 / size) as f64 + 0.5,
            );
            let r = DVec3::new(
                (x / f64::from(size) * 2.0 - 1.0) * tan_half,
                (1.0 - y / f64::from(size) * 2.0) * tan_half,
                -1.0,
            );
            let lum =
                0.2126 * f64::from(p[0]) + 0.7152 * f64::from(p[1]) + 0.0722 * f64::from(p[2]);
            lum * side * side / r.length().powi(3)
        })
        .sum();
    V_SUN - 2.5 * (flux / std::f64::consts::PI).log10()
}

/// A shaped sphere's lightcurve, mag, the Sun toward `to_sun` and the eye
/// toward `to_eye` (unit), `alpha` degrees apart: its facets' light there
/// over their mean over orientations (a quarter of its area times its
/// disk's mean). 0 for a sphere.
fn lightcurve(sphere: &render::Sphere, to_sun: DVec3, to_eye: DVec3, alpha: f64) -> f64 {
    let Some(shape) = sphere.maps.as_ref().and_then(|m| m.shape.as_ref()) else {
        return 0.0;
    };
    let (_, shading) = sphere.reflectance().expect("lit");
    let to_body = sphere.axes.transpose();
    let a = alpha.to_radians();
    let lit = shape.lit_area(to_body * to_sun, to_body * to_eye, shading, a);
    -2.5 * (4.0 * lit / (shape.area * render::scene::disk_mean(shading, a))).log10()
}

/// Shaped bodies' disks over 32 random orientations each, from 100 radii
/// at 30°, 90° and 120° of phase, against their curves with their
/// lightcurves there (`lightcurve`: their facets' light, less their mean
/// self-shadowing at that phase): on the mean within 0.08 mag, each within
/// 0.3 (its self-shadowing then against its mean). At 120°, 0.03–0.06 mag
/// under on the mean, the same at 400 and 1,600 px across: shadows finer
/// than the facets the shadow tables are taken over (1.5% of Bennu's),
/// and pixels at a part's edge over another. (Against the curve alone, the
/// lightcurves, up to ±0.9 mag at 120°, leave the mean of 32 good to only
/// ~0.1: printed.) Itokawa's showed 0.8–1.5 mag over: one slope bound for
/// the whole body, set by the curtain its overhang makes, left its
/// marches spending their steps short of the surface (found here; the
/// bounds are now local).
#[test]
fn shaped_disks_average_to_their_curves() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue);
    sandbox::maps::wait().expect("data/bodies reads");
    let eph = Ephemeris::standard();
    let t = Epoch::from_ymd(2045, 3, 1);
    let sun = eph.barycentric_position(eph.sun(), t);
    let mut state = 11u64;
    let mut f = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
    };
    for id in ["phobos", "bennu", "itokawa", "ryugu"] {
        let b = eph.lookup(id).unwrap();
        let curve = Curve::of(eph.body(b)).unwrap();
        let c = eph.barycentric_position(b, t);
        let radius = bodies::radius(eph, b);
        let to_sun = (sun - c).normalize();
        let side = to_sun.cross(DVec3::Z).normalize();
        let mut shown = Vec::new();
        for alpha in [30.0f64, 90.0, 120.0] {
            let a = alpha.to_radians();
            let d = radius * 100.0;
            let eye = c + (to_sun * a.cos() + side * a.sin()) * d;
            let fov = (radius / d).asin() * 8.0;
            let (mut mean, mut worst, mut bias) = (0.0, 0.0f64, 0.0);
            for _ in 0..32 {
                let axis = DVec3::new(f(), f(), f()).normalize();
                let turn = glam::DMat3::from_axis_angle(axis, f() * std::f64::consts::PI);
                let mut spheres = bodies::spheres(eph, &[b], t, eye);
                spheres[0].axes = turn * spheres[0].axes;
                photometry::calibrate(eph.body(b), &mut spheres[0], eye, sun);
                let seen = Seen::from(&spheres[0], eye, sun);
                let au = render::scene::AU_KM;
                let want =
                    curve.magnitude(&seen) + 5.0 * ((sun - c).length() / au * d / au).log10();
                let shape = lightcurve(&spheres[0], to_sun, (eye - c).normalize(), seen.alpha);
                let scene = Scene {
                    camera: Some(Camera::looking_at(eye, c, DVec3::Z, fov)),
                    sun,
                    spheres,
                    ..Default::default()
                };
                let v = shown_magnitude(&gpu, &mut renderer, &scene, fov, 400);
                mean += 10f64.powf(-0.4 * (v - want)) / 32.0;
                worst = worst.max((v - want - shape).abs());
                bias += (v - want - shape) / 32.0;
            }
            let mean = -2.5 * mean.log10();
            shown.push(format!(
                "{alpha}° {bias:+.3} (each within {worst:.3}; against the curve alone, {mean:+.3})"
            ));
            assert!(
                bias.abs() < 0.08 && worst < 0.3,
                "{id} at {alpha}°: {bias} {worst}"
            );
        }
        println!("{id}: {}", shown.join(", "));
    }
}

/// `image`'s pixels summed, as a color of luminance 1.
fn color_of(image: &[[f32; 4]]) -> [f64; 3] {
    let mut sum = [0f64; 3];
    for p in image {
        for c in 0..3 {
            sum[c] += f64::from(p[c]);
        }
    }
    let l = 0.2126 * sum[0] + 0.7152 * sum[1] + 0.0722 * sum[2];
    sum.map(|x| x / l)
}

/// The colors the renderer shows bodies in, against their own
/// (`photometry::color`), at 10° of phase: as points (from 5,000 radii),
/// the planets, the Moon, Phobos, Deimos and small bodies of five
/// classes, within 0.003 in each channel; as disks (from 30 radii) whose
/// maps' mean sets their color and which have no air (the Moon, Mercury,
/// Jupiter), within 0.02, as the face shown varies from their maps' mean.
/// Disks with air add its color (Mars, Venus), or keep their maps' and
/// take it from their air (Earth): printed.
#[test]
fn bodies_show_their_colors() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue);
    sandbox::maps::wait().expect("data/bodies reads");
    let eph = Ephemeris::standard();
    let t = Epoch::from_ymd(2045, 3, 1);
    let sun = eph.barycentric_position(eph.sun(), t);
    let size = 400u32;
    let points = [
        "mercury", "venus", "earth", "moon", "mars", "jupiter", "saturn", "uranus", "neptune",
        "phobos", "deimos", "bennu", "ryugu", "didymos", "apophis", "nereus",
    ];
    let disks = ["moon", "mercury", "jupiter", "earth", "mars", "venus"];
    let mut shown = |id: &str, radii: f64| {
        let b = eph.lookup(id).unwrap();
        let (c, radius) = (eph.barycentric_position(b, t), eph.body(b).radius);
        let to_sun = (sun - c).normalize();
        let side = to_sun.cross(DVec3::Z).normalize();
        let a = 10f64.to_radians();
        let eye = c + (to_sun * a.cos() + side * a.sin()) * radius * radii;
        // Its brightest pixel at ~100, within f16's precision.
        let mut draw = |exposure: f32| {
            let scene = Scene {
                camera: Some(Camera::looking_at(eye, c, DVec3::Z, 50f64.to_radians())),
                sun,
                spheres: bodies::spheres(eph, &[b], t, eye),
                look: Look {
                    exposure,
                    ..Default::default()
                },
                ..Default::default()
            };
            let target = Target::new(&gpu.device, size, size);
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            renderer.render(&gpu.device, &gpu.queue, &mut encoder, &target, &scene, None);
            gpu.queue.submit(Some(encoder.finish()));
            gpu.read_rgba16f(&target.hdr)
        };
        let peak = draw(1.0).iter().map(|p| p[1]).fold(0f32, f32::max);
        let got = color_of(&draw(100.0 / peak));
        let want = photometry::color(eph.body(b)).expect("a color");
        let off = (0..3)
            .map(|k| (got[k] - want[k]).abs())
            .fold(0f64, f64::max);
        println!("{id} from {radii} radii: {got:.3?}, its own {want:.3?}: off {off:.3}");
        off
    };
    for id in points {
        assert!(shown(id, 5000.0) < 0.003, "{id}'s point");
    }
    for id in disks {
        let off = shown(id, 30.0);
        if !["earth", "mars", "venus"].contains(&id) {
            assert!(off < 0.02, "{id}'s disk");
        }
    }
}
