//! Shaped bodies (M1c.md, step 6, part 9 plan, item 1), offscreen, against
//! exact solids traced on the CPU: a triaxial ellipsoid, and two
//! overlapping spheres that shadow each other. Lambertian, white, 1 au
//! from the Sun: a pixel's radiance is the cosine of the Sun's incidence.

use glam::{DMat3, DVec3};
use render::camera::Camera;
use render::offscreen::Offscreen;
use render::scene::AU_KM;
use render::shape::Shape;
use render::{Image, Look, Maps, Marker, Renderer, Scene, Shading, Sphere, Surface, Target};
use std::sync::Arc;

const CENTER: DVec3 = DVec3::new(AU_KM, 0.0, 0.0);

/// The ellipsoid's semi-axes, km, and its center off the body's origin
/// (so that no plane through the origin mirrors it: a face's mapping
/// mirrored shows).
const AXES: [f64; 3] = [3.0, 2.0, 1.0];
const OFF: DVec3 = DVec3::new(0.4, -0.3, 0.2);

/// A solid traced exactly: where a ray (from `o`, along unit `d`, the
/// body's frame) first meets it, and its outward normal there.
trait Solid {
    fn hit(&self, o: DVec3, d: DVec3) -> Option<(f64, DVec3)>;
    fn radius(&self, d: DVec3) -> f64;
}

/// An ellipsoid of semi-axes `axes` centered at `center` (the origin
/// within it).
struct Ellipsoid {
    axes: [f64; 3],
    center: DVec3,
}

const ELLIPSOID: Ellipsoid = Ellipsoid {
    axes: AXES,
    center: OFF,
};

impl Solid for Ellipsoid {
    fn hit(&self, o: DVec3, d: DVec3) -> Option<(f64, DVec3)> {
        let k = DVec3::from_array(self.axes);
        let (o, d) = ((o - self.center) / k, d / k);
        let (a, b, c) = (d.dot(d), o.dot(d), o.dot(o) - 1.0);
        let disc = b * b - a * c;
        if disc < 0.0 {
            return None;
        }
        let t = (-b - disc.sqrt()) / a;
        (t > 0.0).then(|| (t, ((o + d * t) / k).normalize()))
    }

    fn radius(&self, d: DVec3) -> f64 {
        // From the origin, inside: the far root.
        let k = DVec3::from_array(self.axes);
        let (o, d) = (-self.center / k, d / k);
        let (a, b, c) = (d.dot(d), o.dot(d), o.dot(o) - 1.0);
        (-b + (b * b - a * c).sqrt()) / a
    }
}

/// Two unit spheres centered ±0.95 km along x: a neck 0.31 km across
/// between them.
struct Lobes;

const LOBES: [DVec3; 2] = [DVec3::new(0.95, 0.0, 0.0), DVec3::new(-0.95, 0.0, 0.0)];

fn sphere_hit(c: DVec3, o: DVec3, d: DVec3) -> Option<f64> {
    let oc = o - c;
    let b = oc.dot(d);
    let disc = b * b - (oc.dot(oc) - 1.0);
    if disc < 0.0 {
        return None;
    }
    let t = -b - disc.sqrt();
    (t > 1e-9).then_some(t)
}

impl Solid for Lobes {
    fn hit(&self, o: DVec3, d: DVec3) -> Option<(f64, DVec3)> {
        LOBES
            .iter()
            .filter_map(|&c| sphere_hit(c, o, d).map(|t| (t, (o + d * t - c).normalize())))
            .min_by(|a, b| a.0.total_cmp(&b.0))
    }

    fn radius(&self, d: DVec3) -> f64 {
        LOBES
            .iter()
            .map(|&c| d.dot(c) + (d.dot(c).powi(2) - c.dot(c) + 1.0).sqrt())
            .fold(0.0, f64::max)
    }
}

impl Lobes {
    /// Whether the Sun (toward `l`) is hidden from the surface point `p`.
    fn shadowed(&self, p: DVec3, l: DVec3) -> bool {
        LOBES
            .iter()
            .any(|&c| sphere_hit(c, p + l * 1e-7, l).is_some())
    }
}

fn shaped(solid: &dyn Solid, size: u32) -> Arc<Maps> {
    Arc::new(Maps {
        color: Image::srgba(1, 1, vec![255; 4]),
        mean: [1.0; 3],
        cover: 0.0,
        relief: None,
        clouds: None,
        cloud_albedo: 0.0,
        night: None,
        night_radiance: [0.0; 3],
        water: None,
        shading: Shading::Lambert,
        air: None,
        rings: None,
        shape: Some(Shape::from_fn(size, |d| solid.radius(d))),
    })
}

/// A turn that leaves no axis on the scene's.
fn turned() -> DMat3 {
    DMat3::from_rotation_z(0.7) * DMat3::from_rotation_x(0.4) * DMat3::from_rotation_y(-0.3)
}

fn body(maps: &Arc<Maps>, axes: DMat3) -> Sphere {
    let shape = maps.shape.as_ref().unwrap();
    Sphere {
        center: CENTER,
        radius: shape.equivalent_radius(),
        surface: Surface::Lit([1.0; 3]),
        axes,
        maps: Some(maps.clone()),
        gain: [1.0; 3],
        point_scale: [1.0; 3],
    }
}

/// The scene's radiance and the shown image.
fn draw(
    gpu: &Offscreen,
    renderer: &mut Renderer,
    scene: &Scene,
    size: (u32, u32),
) -> (Vec<[f32; 4]>, Vec<[f32; 4]>) {
    let target = Target::new(&gpu.device, size.0, size.1);
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    renderer.render(&gpu.device, &gpu.queue, &mut encoder, &target, scene, None);
    gpu.queue.submit(Some(encoder.finish()));
    (
        gpu.read_rgba16f(&target.hdr),
        gpu.read_rgba16f(&target.color),
    )
}

/// The ray (unit, the scene's frame) through `(x, y)` pixels of a `size`
/// image through `cam`.
fn ray(cam: &Camera, size: (u32, u32), x: f64, y: f64) -> DVec3 {
    let tan_half = (cam.fov_y / 2.0).tan();
    let aspect = f64::from(size.0) / f64::from(size.1);
    let ndc_x = x / f64::from(size.0) * 2.0 - 1.0;
    let ndc_y = 1.0 - y / f64::from(size.1) * 2.0;
    (cam.forward + cam.right * ndc_x * tan_half * aspect + cam.up * ndc_y * tan_half).normalize()
}

/// What a ray shows of `solid` turned by `axes` at `CENTER`, the Sun
/// toward `l`: the cosine of incidence where it hits and is lit (none in
/// `shade`'s shadow), 0 where it misses; and the hit's distance.
fn traced(
    solid: &dyn Solid,
    axes: DMat3,
    eye: DVec3,
    d: DVec3,
    l: DVec3,
    shade: &dyn Fn(DVec3, DVec3) -> bool,
) -> Option<(f64, f64)> {
    let to_body = axes.transpose();
    let (t, n) = solid.hit(to_body * (eye - CENTER), to_body * d)?;
    let (n, l_body) = (axes * n, to_body * l);
    let p = to_body * (eye - CENTER) + to_body * d * t;
    let lit = if shade(p, l_body) {
        0.0
    } else {
        n.dot(l).max(0.0)
    };
    Some((lit, t))
}

/// What a pixel shows by `reference`'s rays.
struct Pixel {
    /// What its rays show on average.
    mean: f64,
    /// Whether all of them hit.
    all: bool,
    /// What its center's ray shows, and how far it goes, where it hits.
    center: Option<(f64, f64)>,
}

/// Per pixel, what `k`×`k` rays across it and its center's show.
fn reference(
    solid: &dyn Solid,
    axes: DMat3,
    cam: &Camera,
    size: (u32, u32),
    l: DVec3,
    k: u32,
    shade: &dyn Fn(DVec3, DVec3) -> bool,
) -> Vec<Pixel> {
    (0..size.0 * size.1)
        .map(|i| {
            let (x, y) = (f64::from(i % size.0), f64::from(i / size.0));
            let (mut sum, mut all) = (0.0, true);
            for a in 0..k {
                for b in 0..k {
                    let (sx, sy) = (
                        x + (f64::from(a) + 0.5) / f64::from(k),
                        y + (f64::from(b) + 0.5) / f64::from(k),
                    );
                    match traced(solid, axes, cam.eye, ray(cam, size, sx, sy), l, shade) {
                        Some((lit, _)) => sum += lit,
                        None => all = false,
                    }
                }
            }
            let center = traced(
                solid,
                axes,
                cam.eye,
                ray(cam, size, x + 0.5, y + 0.5),
                l,
                shade,
            );
            Pixel {
                mean: sum / f64::from(k * k),
                all,
                center,
            }
        })
        .collect()
}

/// Whether pixel `i` and its eight neighbors are all wholly covered (by
/// `reference`'s rays): its shading is checked alone, its coverage being
/// the edges' business (the image's sum).
fn interior(want: &[Pixel], size: (u32, u32), i: usize) -> bool {
    let (x, y) = (i as u32 % size.0, i as u32 / size.0);
    (x > 0 && y > 0 && x + 1 < size.0 && y + 1 < size.1)
        && (0..9).all(|k| want[((y + k / 3 - 1) * size.0 + x + k % 3 - 1) as usize].all)
}

fn scene(cam: Camera, spheres: Vec<Sphere>) -> Scene {
    Scene {
        camera: Some(cam),
        sun: DVec3::ZERO,
        spheres,
        look: Look::default(),
        ..Default::default()
    }
}

/// The ellipsoid, turned, from five directions under three Suns (phase
/// angles 13° to 157°), 150–300 px across: each pixel within its edges
/// radiance within 1% + 0.006 of the exact cosine at its center's ray
/// (0.006: a normal 0.35° off, as the radius map's own are at 128² a face
/// where the ellipsoid curves most, 0.12–0.39° there on the CPU), and the
/// image's sum (coverage at the edges included) within 0.5% of 8×8 exact
/// rays a pixel, and a hundred-thousandth of the pixels it covers at full
/// light: a crescent thinner than a pixel, on the tip at 157°, has 0.06 of
/// light in all, which a pixel's center over it samples high (it showed
/// 0.95 more, shaded at the limb itself: found here). Compared with the
/// pixel's mean instead, a limb's pixel reads bright: the cosine rises as
/// the root of the distance in.
#[test]
fn an_ellipsoid_shows_as_traced() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue);
    let solid = ELLIPSOID;
    let maps = shaped(&solid, 128);
    let axes = turned();
    let size = (240, 200);
    let no_shade = |_: DVec3, _: DVec3| false;
    let views = [
        DVec3::new(-1.0, 0.2, 0.1),
        DVec3::new(0.3, 1.0, -0.4),
        DVec3::new(-0.2, -0.5, 1.0),
        DVec3::new(1.0, 1.0, 1.0),
        DVec3::new(-0.6, -1.0, -0.3),
    ];
    let suns = [
        -DVec3::X,
        DVec3::new(-0.5, 0.8, 0.2),
        DVec3::new(0.4, 0.6, -0.6),
    ];
    for (k, view) in views.iter().enumerate() {
        let from = view.normalize();
        let eye = CENTER + from * 9.0;
        let cam = Camera::looking_at(eye, CENTER, DVec3::Z, 45f64.to_radians());
        // The Sun from where the body's center sees it.
        let l = suns[k % suns.len()].normalize();
        let mut s = body(&maps, axes);
        // At 1 au along -l from the body: irradiance 1.
        let sun = CENTER + l * AU_KM;
        s.center = CENTER;
        let sc = Scene {
            sun,
            ..scene(cam, vec![s])
        };
        let (image, _) = draw(&gpu, &mut renderer, &sc, size);
        let want = reference(&solid, axes, &cam, size, l, 8, &no_shade);
        let (mut worst, mut got_sum, mut want_sum) = (0.0f64, 0.0, 0.0);
        let mut where_worst = (0, 0.0, 0.0);
        for (i, (p, px)) in image.iter().zip(&want).enumerate() {
            got_sum += f64::from(p[0]);
            want_sum += px.mean;
            if let (true, Some((w, _))) = (interior(&want, size, i), px.center) {
                let e = ((f64::from(p[0]) - w).abs() - 0.006) / w.max(1e-3);
                if e > worst {
                    worst = e;
                    where_worst = (i, f64::from(p[0]), w);
                }
            }
        }
        let (i, g, w) = where_worst;
        let (x, y) = (i as u32 % size.0, i as u32 / size.0);
        println!("  worst at ({x}, {y}): {g:.4} vs {w:.4}");
        let phase = from.angle_between(l).to_degrees();
        println!(
            "view {k} (phase {phase:.0}°): worst covered pixel {worst:.4}, sum {:.5} of exact ({got_sum:.3} vs {want_sum:.3})",
            got_sum / want_sum
        );
        let area = want.iter().filter(|w| w.center.is_some()).count() as f64;
        assert!(worst < 0.01, "view {k}: {worst}");
        assert!(
            (got_sum - want_sum).abs() < 0.005 * want_sum + 1e-5 * area,
            "view {k}: {got_sum} vs {want_sum}"
        );
    }
}

/// Two lobes lit from low over one (10°), its crown's shadow across the
/// other's side: pixels facing the Sun in the near lobe's exact shadow
/// dark, lit ones within 2% (+0.002) of the cosine, two pixels or more
/// from the shadow's edge and the body's, away from the neck; ~120 and
/// ~500 pixels across, the shadow cast from as far. (Where the
/// neck's crease, which the radius map rounds over a texel, casts the
/// edge, a low Sun stretched that to 1–2 pixels; and with the night side
/// counted as shadowed, this had passed without any self-shadowing.)
#[test]
fn lobes_shadow_each_other() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue);
    let maps = shaped(&Lobes, 256);
    for size in [(240, 160), (960, 640)] {
        lobes_at(&gpu, &mut renderer, &maps, size);
    }
}

fn lobes_at(gpu: &Offscreen, renderer: &mut Renderer, maps: &Arc<Maps>, size: (u32, u32)) {
    let solid = Lobes;
    let axes = DMat3::IDENTITY;
    // Seen from above, off to the side; the Sun low over +x, 10° up: the
    // near lobe's crown shadows the far one's side toward it.
    let l = DVec3::new(1.0, 0.15, 0.18).normalize();
    let cam = Camera::looking_at(
        CENTER + DVec3::new(-1.0, -2.0, 6.0),
        CENTER,
        DVec3::Y,
        40f64.to_radians(),
    );
    let sc = Scene {
        sun: CENTER + l * AU_KM,
        ..scene(cam, vec![body(maps, axes)])
    };
    let (image, _) = draw(gpu, renderer, &sc, size);
    let shade = |p: DVec3, l: DVec3| Lobes.shadowed(p, l);
    let want = reference(&solid, axes, &cam, size, l, 3, &shade);
    let at = |i: usize| {
        let (x, y) = (
            (i as u32 % size.0) as f64 + 0.5,
            (i as u32 / size.0) as f64 + 0.5,
        );
        traced(&solid, axes, cam.eye, ray(&cam, size, x, y), l, &|_, _| {
            false
        })
        .map(|(_, t)| {
            let d = ray(&cam, size, x, y);
            cam.eye - CENTER + d * t
        })
    };
    // Each pixel's center ray: facing the Sun and in the other lobe's
    // shadow (cast), facing it and lit, or facing away or missing.
    let state: Vec<char> = (0..size.0 * size.1)
        .map(|i| {
            let (x, y) = (f64::from(i % size.0) + 0.5, f64::from(i / size.0) + 0.5);
            let d = ray(&cam, size, x, y);
            let facing = traced(&solid, axes, cam.eye, d, l, &|_, _| false).map(|(v, _)| v > 0.0);
            let lit = traced(&solid, axes, cam.eye, d, l, &shade).map(|(v, _)| v > 0.0);
            match (facing, lit) {
                (Some(true), Some(false)) => 'c',
                (Some(true), Some(true)) => 'l',
                _ => 'n',
            }
        })
        .collect();
    // Two pixels or more from any other state.
    let deep = |i: usize| {
        let (x, y) = ((i as u32 % size.0) as i64, (i as u32 / size.0) as i64);
        (-2..=2).all(|dy| {
            (-2..=2).all(|dx| {
                let (xx, yy) = (x + dx, y + dy);
                (0..i64::from(size.0)).contains(&xx)
                    && (0..i64::from(size.1)).contains(&yy)
                    && state[(yy * i64::from(size.0) + xx) as usize] == state[i]
            })
        })
    };
    let (mut cast, mut lit, mut worst_cast, mut worst_lit) = (0, 0, 0.0f64, 0.0f64);
    for (i, (p, px)) in image.iter().zip(&want).enumerate() {
        let (Some(point), Some((w, _))) = (at(i), px.center) else {
            continue;
        };
        if !interior(&want, size, i) || point.x.abs() < 0.3 || !deep(i) {
            continue;
        }
        let got = f64::from(p[0]);
        match state[i] {
            'c' => {
                cast += 1;
                worst_cast = worst_cast.max(got);
            }
            'l' => {
                lit += 1;
                worst_lit = worst_lit.max(((got - w).abs() - 0.002) / w.max(1e-3));
            }
            _ => {}
        }
    }
    println!(
        "{size:?}: {cast} pixels in cast shadow, brightest {worst_cast:.4}; {lit} lit, worst {worst_lit:.4}"
    );
    assert!(cast > 100 && lit > 2000, "{cast} {lit}");
    assert!(
        worst_cast < 0.01 && worst_lit < 0.02,
        "{worst_cast} {worst_lit}"
    );
}

/// The eye inside the ellipsoid's bounding sphere but outside it, 0.6 km
/// over its smallest axis: drawn (the whole screen its quad), as traced;
/// inside the body itself, nothing.
#[test]
fn an_eye_within_the_bounds_sees_the_body() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue);
    let solid = ELLIPSOID;
    let maps = shaped(&solid, 128);
    let axes = turned();
    let size = (160, 120);
    let l = (axes * DVec3::new(0.3, 0.2, 1.0)).normalize();
    let eye = CENTER + axes * DVec3::new(0.2, 0.1, 1.6);
    let cam = Camera::looking_at(eye, CENTER, axes * DVec3::Y, 70f64.to_radians());
    let sc = Scene {
        sun: CENTER + l * AU_KM,
        ..scene(cam, vec![body(&maps, axes)])
    };
    let (image, _) = draw(&gpu, &mut renderer, &sc, size);
    let want = reference(&solid, axes, &cam, size, l, 4, &|_, _| false);
    let (mut covered, mut worst) = (0, 0.0f64);
    for (i, (p, px)) in image.iter().zip(&want).enumerate() {
        if let (true, Some((w, _))) = (interior(&want, size, i), px.center) {
            covered += 1;
            worst = worst.max(((f64::from(p[0]) - w).abs() - 0.002) / w.max(1e-3));
        }
    }
    println!("{covered} covered pixels, worst {worst:.4}");
    assert!(covered > 10_000 && worst < 0.01, "{covered} {worst}");
    let inside = Scene {
        camera: Some(Camera::looking_at(
            CENTER + axes * DVec3::new(0.0, 0.0, 0.5),
            CENTER,
            axes * DVec3::Y,
            70f64.to_radians(),
        )),
        ..sc
    };
    let (image, _) = draw(&gpu, &mut renderer, &inside, size);
    assert!(image.iter().all(|p| p[0] == 0.0), "drawn from inside");
}

/// The surface's depth: a marker a hair (0.2% of the distance) in front
/// of the traced surface shows over it, one a hair behind is hidden, at
/// a pixel over the body's middle and at its edge.
#[test]
fn markers_meet_the_surface() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue);
    let solid = ELLIPSOID;
    let maps = shaped(&solid, 128);
    let axes = turned();
    let size = (200, 200);
    let cam = Camera::looking_at(
        CENTER + DVec3::new(-7.0, 2.0, 3.0),
        CENTER,
        DVec3::Z,
        50f64.to_radians(),
    );
    let l = DVec3::new(-1.0, 0.3, 0.4).normalize();
    let want = reference(&solid, axes, &cam, size, l, 1, &|_, _| false);
    // Over the body's middle, and one and three pixels in from its left
    // edge on two rows.
    let mut pixels = vec![(100, 100)];
    for y in [80, 120] {
        let first = (0..size.0)
            .find(|&x| want[(y * size.0 + x) as usize].center.is_some())
            .expect("the body on the row");
        pixels.extend([(first, y), (first + 2, y)]);
    }
    let mut checked = 0;
    for (x, y) in pixels {
        let i = (y * size.0 + x) as usize;
        let Some((_, t)) = want[i].center else {
            continue;
        };
        let d = ray(&cam, size, f64::from(x) + 0.5, f64::from(y) + 0.5);
        let mut shown = |k: f64| {
            let sc = Scene {
                sun: CENTER + l * AU_KM,
                markers: vec![Marker {
                    position: cam.eye + d * t * k,
                    color: [0.0, 1.0, 0.0],
                    size_px: 5.0,
                    shape: render::Shape::Block,
                }],
                ..scene(cam, vec![body(&maps, axes)])
            };
            draw(&gpu, &mut renderer, &sc, size).1[i]
        };
        let (front, behind) = (shown(0.998), shown(1.002));
        assert!(
            front[1] > 0.5 && front[0] < 0.05,
            "({x}, {y}) in front: {front:?}"
        );
        assert!(behind[0] > 0.05, "({x}, {y}) behind: {behind:?}");
        checked += 1;
    }
    assert_eq!(checked, 5);
}

/// GPU time of the ellipsoid filling 1920×1080, lit at 60° of phase.
#[test]
#[ignore = "a measurement: run with --ignored --nocapture"]
fn shape_gpu_time() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue);
    let maps = shaped(&ELLIPSOID, 128);
    let cam = Camera::looking_at(
        CENTER + DVec3::new(-4.0, 1.0, 0.5),
        CENTER,
        DVec3::Z,
        50f64.to_radians(),
    );
    let sc = Scene {
        sun: CENTER + DVec3::new(-1.0, 1.7, 0.0).normalize() * AU_KM,
        ..scene(cam, vec![body(&maps, turned())])
    };
    for _ in 0..3 {
        match gpu.time_scene(&mut renderer, &sc, (1920, 1080)) {
            Some(ms) => println!(
                "ellipsoid filling 1080p: {ms:.3} ms on {}",
                gpu.adapter.get_info().name
            ),
            None => println!("no timestamps"),
        }
    }
}
