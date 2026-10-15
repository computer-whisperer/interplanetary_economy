//! The bodies' brightness against JPL Horizons' apparent magnitudes
//! (M1c.md, Realism; fixtures from tools/photometry/fixtures.sh): the
//! phase curves as `photometry` computes them, and the flux the renderer
//! shows.

use glam::DVec3;
use orbit::ephem::{BodyId, Ephemeris};
use orbit::time::Epoch;
use render::offscreen::Offscreen;
use render::{Camera, Look, Renderer, Scene, Target};
use sandbox::bodies;
use sandbox::photometry::{Curve, Seen, V_SUN, saturn_with_rings};

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
/// planets and Earth, from a body farther out (to 176° of phase): within
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
        ("earth", 499, Some("mars"), 0.002),
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
            let up = if (at - eye).normalize().z.abs() > 0.9 { DVec3::X } else { DVec3::Z };
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
            renderer.render(&gpu.device, &gpu.queue, &mut encoder, &target_tex, &scene, None);
            gpu.queue.submit(Some(encoder.finish()));
            let image = gpu.read_rgba16f(&target_tex.hdr);
            let mut flux = 0.0;
            for (i, p) in image.iter().enumerate() {
                let (x, y) = ((i as u32 % size) as f64 + 0.5, (i as u32 / size) as f64 + 0.5);
                let r = DVec3::new(
                    (x / f64::from(size) * 2.0 - 1.0) * tan_half,
                    (1.0 - y / f64::from(size) * 2.0) * tan_half,
                    -1.0,
                );
                let lum = 0.2126 * f64::from(p[0]) + 0.7152 * f64::from(p[1]) + 0.0722 * f64::from(p[2]);
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
        let shown: Vec<String> = report.iter().map(|(a, d)| format!("{a:.0}° {d:+.3}")).collect();
        println!("{target}: {}", shown.join(", "));
        assert!(worst < tolerance, "{target}: {worst} mag");
    }
}

/// Disks without air or maps (Uranus, Neptune, Phobos, Bennu) and
/// Jupiter's, seen from 100 radii and resolved (~100 px across), from 0°
/// to 120° of phase: the flux rendered against their curves' at that
/// geometry, within 0.03 mag. (Nearer, a disk shows less than a
/// hemisphere: from 12 radii, 0.2 mag under its curve at 120°.)
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
            let want = curve.magnitude(&seen) + 5.0 * ((sun - c).length() / au * d / au).log10();
            let scene = Scene {
                camera: Some(Camera::looking_at(eye, c, DVec3::Z, fov)),
                sun,
                spheres,
                ..Default::default()
            };
            let target = Target::new(&gpu.device, size, size);
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            renderer.render(&gpu.device, &gpu.queue, &mut encoder, &target, &scene, None);
            gpu.queue.submit(Some(encoder.finish()));
            let tan_half = (fov / 2.0).tan();
            let side_px = 2.0 * tan_half / f64::from(size);
            let flux: f64 = gpu
                .read_rgba16f(&target.hdr)
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let (x, y) = ((i as u32 % size) as f64 + 0.5, (i as u32 / size) as f64 + 0.5);
                    let r = DVec3::new(
                        (x / f64::from(size) * 2.0 - 1.0) * tan_half,
                        (1.0 - y / f64::from(size) * 2.0) * tan_half,
                        -1.0,
                    );
                    let lum = 0.2126 * f64::from(p[0]) + 0.7152 * f64::from(p[1]) + 0.0722 * f64::from(p[2]);
                    lum * side_px * side_px / r.length().powi(3)
                })
                .sum();
            let v = V_SUN - 2.5 * (flux / std::f64::consts::PI).log10();
            report.push((alpha, v - want));
        }
        let shown: Vec<String> = report.iter().map(|(a, d)| format!("{a:.0}° {d:+.3}")).collect();
        println!("{id}: {}", shown.join(", "));
        let worst = report.iter().map(|(_, d)| d.abs()).fold(0f64, f64::max);
        assert!(worst < 0.03, "{id}: {worst} mag");
    }
}
