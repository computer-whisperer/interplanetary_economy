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
                // The share of its tolerance used.
                let e = (f64::from(p[0]) - w).abs() / (0.006 + 0.01 * w);
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
            "view {k} (phase {phase:.0}°): a pixel within the edges uses {worst:.2} of its tolerance; sum {:.5} of exact ({got_sum:.3} vs {want_sum:.3})",
            got_sum / want_sum
        );
        let area = want.iter().filter(|w| w.center.is_some()).count() as f64;
        assert!(worst < 1.0, "view {k}: {worst}");
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
                worst_lit = worst_lit.max((got - w).abs() / (0.002 + 0.02 * w));
            }
            _ => {}
        }
    }
    println!(
        "{size:?}: {cast} pixels in cast shadow, brightest {worst_cast:.4}; {lit} lit, using {worst_lit:.2} of their tolerance"
    );
    assert!(cast > 100 && lit > 2000, "{cast} {lit}");
    assert!(
        worst_cast < 0.01 && worst_lit < 1.0,
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
            worst = worst.max((f64::from(p[0]) - w).abs() / (0.002 + 0.01 * w));
        }
    }
    println!("{covered} pixels within the edges, using {worst:.2} of their tolerance");
    assert!(covered > 10_000 && worst < 1.0, "{covered} {worst}");
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

/// Of the pixels whose 7×7 neighbors' center rays all hit (3 pixels or
/// more from the image's edge), how many, and how many the GPU shows off
/// the exact cosine at their center by more than 0.05 + 10%: holes, or
/// worse.
fn wrong_interior(
    gpu: &Offscreen,
    renderer: &mut Renderer,
    sc: &Scene,
    solid: &dyn Solid,
    axes: DMat3,
    l: DVec3,
    size: (u32, u32),
) -> (usize, usize) {
    let cam = sc.camera.unwrap();
    let (image, _) = draw(gpu, renderer, sc, size);
    let center: Vec<Option<f64>> = (0..size.0 * size.1)
        .map(|i| {
            let (x, y) = (f64::from(i % size.0) + 0.5, f64::from(i / size.0) + 0.5);
            traced(solid, axes, cam.eye, ray(&cam, size, x, y), l, &|_, _| {
                false
            })
            .map(|c| c.0)
        })
        .collect();
    let (mut hits, mut wrong) = (0, 0);
    for y in 3..size.1 - 3 {
        for x in 3..size.0 - 3 {
            let all = (0..49)
                .all(|k| center[((y + k / 7 - 3) * size.0 + x + k % 7 - 3) as usize].is_some());
            let i = (y * size.0 + x) as usize;
            if let (true, Some(w)) = (all, center[i]) {
                hits += 1;
                if (f64::from(image[i][0]) - w).abs() > 0.05 + 0.1 * w {
                    wrong += 1;
                }
            }
        }
    }
    (hits, wrong)
}

/// No holes: the ellipsoid 300 to 3,000 pixels across (at a 1080-row
/// screen's pixel, 50° high) from three directions, and the eye 10 m to
/// 1 km over it looking down and along it, every pixel within its edges
/// as traced. (Rays spending their steps short of the surface were
/// dropped: at 1,500 px, 8,617 of 89,745 wrong; 6,447 of 62,229 from 10 m
/// looking along it; the GPU timing's own view had 13,233 holes. Found in
/// review.)
#[test]
fn no_holes_large_or_near() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue);
    let maps = shaped(&ELLIPSOID, 128);
    let axes = turned();
    let size = (480u32, 270u32);
    let pixel = 2.0 * 25f64.to_radians().tan() / 1080.0;
    let fov = 2.0 * (pixel * f64::from(size.1) / 2.0).atan();
    let (mut worst, mut checked) = (0, 0);
    for from in [
        DVec3::new(-1.0, 0.2, 0.1),
        DVec3::new(1.0, 1.0, 1.0),
        DVec3::new(-0.2, -0.5, 1.0),
    ] {
        let from = from.normalize();
        let side = from.cross(DVec3::Z).normalize();
        let l = (from * 0.5 + side * 0.866).normalize();
        for across in [300.0f64, 1000.0, 3000.0] {
            let eye = CENTER + from * (6.0 / (across * pixel));
            // Toward the limb, a quarter of the way out.
            let cam = Camera::looking_at(eye, CENTER + side * 1.2, DVec3::Z, fov);
            let sc = Scene {
                sun: CENTER + l * AU_KM,
                ..scene(cam, vec![body(&maps, axes)])
            };
            let (hits, wrong) = wrong_interior(&gpu, &mut renderer, &sc, &ELLIPSOID, axes, l, size);
            println!("from {from:.2}, {across} px across: {wrong} of {hits} wrong");
            (worst, checked) = (worst.max(wrong), checked + hits);
        }
    }
    for over in [DVec3::new(0.3, 0.0, 1.0), DVec3::new(1.0, 0.2, 0.1)] {
        let over = over.normalize();
        let r = ELLIPSOID.radius(over);
        let l = (axes * (over + DVec3::new(0.3, -0.2, 0.1))).normalize();
        let along = over.cross(DVec3::Z).normalize();
        for altitude in [1.0f64, 0.1, 0.01] {
            let eye = CENTER + axes * (over * (r + altitude));
            for target in [CENTER, CENTER + axes * (over * r + along * 1.5)] {
                let up = axes * along.cross(over);
                let cam = Camera::looking_at(eye, target, up, 60f64.to_radians());
                let sc = Scene {
                    sun: CENTER + l * AU_KM,
                    ..scene(cam, vec![body(&maps, axes)])
                };
                let (hits, wrong) =
                    wrong_interior(&gpu, &mut renderer, &sc, &ELLIPSOID, axes, l, size);
                println!("{altitude} km over {over:.2}: {wrong} of {hits} wrong");
                (worst, checked) = (worst.max(wrong), checked + hits);
            }
        }
    }
    assert!(worst == 0 && checked > 500_000, "{worst} of {checked}");
}

/// One lobe's edge over the other, lit from the eye: of the pixels both
/// lobes show in (8×8 exact rays, all hitting), fewer than one in ten
/// under 70% of their exact mean (the farther lobe shaded for its share).
/// Shaded as the nearer lobe's limb alone, 28 of 127 were; without the
/// look past the edge at all, 35.
#[test]
fn a_near_edge_over_a_far_part() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue);
    let maps = shaped(&Lobes, 256);
    let size = (320u32, 240u32);
    let off = DVec3::new(-5.0, -0.7, 1.0);
    let cam = Camera::looking_at(CENTER + off, CENTER, DVec3::Z, 30f64.to_radians());
    let l = off.normalize();
    let sc = Scene {
        sun: CENTER + l * AU_KM,
        ..scene(cam, vec![body(&maps, DMat3::IDENTITY)])
    };
    let (image, _) = draw(&gpu, &mut renderer, &sc, size);
    let (mut seam, mut dark) = (0, 0);
    for (i, p) in image.iter().enumerate() {
        let (x, y) = (f64::from(i as u32 % size.0), f64::from(i as u32 / size.0));
        let (mut sum, mut all, mut lobes) = (0.0, true, [0, 0]);
        for k in 0..64 {
            let d = ray(
                &cam,
                size,
                x + (f64::from(k % 8) + 0.5) / 8.0,
                y + (f64::from(k / 8) + 0.5) / 8.0,
            );
            match Lobes.hit(cam.eye - CENTER, d) {
                Some((t, n)) => {
                    sum += n.dot(l).max(0.0);
                    lobes[usize::from((cam.eye - CENTER + d * t).x > 0.0)] += 1;
                }
                None => all = false,
            }
        }
        if all && lobes[0] > 0 && lobes[1] > 0 {
            seam += 1;
            if f64::from(p[0]) < 0.7 * sum / 64.0 {
                dark += 1;
            }
        }
    }
    println!("{seam} pixels both lobes show in, {dark} under 70% of exact");
    assert!(seam > 100 && dark * 10 < seam, "{dark} of {seam}");
}

/// A round shape and the exact sphere, 3 to 20 pixels in radius, at phase
/// angles 0° to 140°: the flux drawn within 5% of 8×8 exact rays a pixel.
/// (One sample a pixel showed a crescent at 140° 37% short at 3 pixels,
/// 4% at 20: found in review.)
#[test]
fn small_bodies_carry_their_light() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue);
    let radius = 1.5;
    let round = Ellipsoid {
        axes: [radius; 3],
        center: DVec3::ZERO,
    };
    let maps = shaped(&round, 128);
    let size = (64u32, 64u32);
    let fov = 20f64.to_radians();
    let pixel = 2.0 * (fov / 2.0).tan() / f64::from(size.1);
    let from = DVec3::new(-1.0, 0.2, 0.1).normalize();
    let side = from.cross(DVec3::Z).normalize();
    let mut worst = 0.0f64;
    for phase in [0.0f64, 60.0, 100.0, 140.0] {
        let l = (from * phase.to_radians().cos() + side * phase.to_radians().sin()).normalize();
        for radius_px in [3.0f64, 4.0, 6.0, 10.0, 20.0] {
            let eye = CENTER + from * (radius / (radius_px * pixel).sin());
            let cam = Camera::looking_at(eye, CENTER + side * 0.3, DVec3::Z, fov);
            let mut s = body(&maps, DMat3::IDENTITY);
            s.radius = radius;
            let sc = Scene {
                sun: CENTER + l * AU_KM,
                ..scene(cam, vec![s])
            };
            let (image, _) = draw(&gpu, &mut renderer, &sc, size);
            let got: f64 = image.iter().map(|p| f64::from(p[0])).sum();
            let want: f64 = reference(&round, DMat3::IDENTITY, &cam, size, l, 8, &|_, _| false)
                .iter()
                .map(|p| p.mean)
                .sum();
            let off = got / want - 1.0;
            println!("{phase}°, {radius_px} px: {:+.2}%", off * 100.0);
            worst = worst.max(off.abs());
        }
    }
    assert!(worst < 0.05, "{worst}");
}

/// The first point of `shape` (its frame the scene's, at `CENTER`) along
/// o + d t, by a fine march and bisection on the CPU.
fn map_hit(shape: &Shape, o: DVec3, d: DVec3, step: f64) -> Option<DVec3> {
    let b = o.dot(d);
    let disc = b * b - (o.dot(o) - shape.greatest * shape.greatest);
    if disc < 0.0 {
        return None;
    }
    let above = |t: f64| {
        let p = o + d * t;
        p.length() - shape.radius(p)
    };
    let (mut t, end) = ((-b - disc.sqrt()).max(0.0), -b + disc.sqrt());
    let mut last = t;
    while t < end {
        if above(t) < 0.0 {
            let (mut lo, mut hi) = (last, t);
            for _ in 0..40 {
                let mid = 0.5 * (lo + hi);
                if above(mid) > 0.0 {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            return Some(o + d * hi);
        }
        last = t;
        t += step.max(above(t) * 0.5);
    }
    None
}

/// Whether `shape`'s surface point `p` is in its own shadow, the Sun
/// toward `l`: a fine march on the CPU.
fn map_shadowed(shape: &Shape, p: DVec3, l: DVec3, step: f64) -> bool {
    let o = p + p.normalize() * 1e-5;
    let b = o.dot(l);
    let end = -b
        + (b * b - (o.dot(o) - shape.greatest * shape.greatest))
            .max(0.0)
            .sqrt();
    let mut t = 1e-4;
    while t < end {
        let q = o + l * t;
        if q.length() < shape.radius(q) {
            return true;
        }
        t += step;
    }
    false
}

/// A ridge round the equator, 8% of the radius high and 0.03 rad wide
/// (five texels at 256² a face), under a point Sun 30° up over it: of the
/// pixels two or more from any edge in its exact shadow (a fine march on
/// the same map), fewer than one in ten shown lit (over 0.1), at ~150
/// and ~500 pixels' radius; none lit ones under half their cosine. (Stepping by a 24th of the
/// radius, 26% and 22% were lit; at half a texel, 4% and 2% are: rays
/// that clip the ridge's crest over less than a step. Found in review.)
#[test]
fn ridges_cast_their_shadows() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue);
    let shape = Shape::from_fn(256, |d| 1.0 + 0.08 * (-(d.z / 0.03).powi(2)).exp());
    let maps = Arc::new(Maps {
        shape: Some(shape.clone()),
        ..(*shaped(&ELLIPSOID, 16)).clone()
    });
    let l = DMat3::from_rotation_z(0.2)
        * DVec3::new(60f64.to_radians().cos(), 0.0, -60f64.to_radians().sin());
    let size = (480u32, 320u32);
    for fov in [40.0f64, 12.0] {
        let eye =
            CENTER + DVec3::new(25f64.to_radians().cos(), 0.0, 25f64.to_radians().sin()) * 4.0;
        let look = CENTER + DVec3::new(12f64.to_radians().cos(), 0.0, 12f64.to_radians().sin());
        let cam = Camera::looking_at(eye, look, DVec3::Z, fov.to_radians());
        let mut s = body(&maps, DMat3::IDENTITY);
        s.radius = shape.equivalent_radius();
        let sc = Scene {
            sun: CENTER + l * AU_KM,
            ..scene(cam, vec![s])
        };
        let (image, _) = draw(&gpu, &mut renderer, &sc, size);
        let state: Vec<(char, f64)> = (0..size.0 * size.1)
            .map(|i| {
                let (x, y) = (f64::from(i % size.0) + 0.5, f64::from(i / size.0) + 0.5);
                match map_hit(&shape, cam.eye - CENTER, ray(&cam, size, x, y), 2e-4) {
                    None => ('n', 0.0),
                    Some(p) => {
                        let up = p.normalize();
                        let (e1, h) = (up.any_orthonormal_vector(), 1e-4);
                        let e2 = up.cross(e1);
                        let at = |v: DVec3| v.normalize() * shape.radius(v);
                        let n = (at(up + e1 * h) - at(up - e1 * h))
                            .cross(at(up + e2 * h) - at(up - e2 * h));
                        let c = n.normalize().dot(l) * n.dot(up).signum();
                        if c <= 0.0 {
                            ('n', 0.0)
                        } else if map_shadowed(&shape, p, l, 2e-4) {
                            ('c', c)
                        } else {
                            ('l', c)
                        }
                    }
                }
            })
            .collect();
        let deep = |i: usize| {
            let (x, y) = ((i as u32 % size.0) as i64, (i as u32 / size.0) as i64);
            (-2..=2).all(|dy| {
                (-2..=2).all(|dx| {
                    let (xx, yy) = (x + dx, y + dy);
                    (0..i64::from(size.0)).contains(&xx)
                        && (0..i64::from(size.1)).contains(&yy)
                        && state[(yy * i64::from(size.0) + xx) as usize].0 == state[i].0
                })
            })
        };
        let (mut cast, mut cast_lit, mut lit, mut lit_dark) = (0, 0, 0, 0);
        for (i, p) in image.iter().enumerate() {
            if !deep(i) {
                continue;
            }
            match state[i] {
                ('c', _) => {
                    cast += 1;
                    cast_lit += usize::from(p[0] > 0.1);
                }
                ('l', c) => {
                    lit += 1;
                    lit_dark += usize::from(f64::from(p[0]) < 0.5 * c);
                }
                _ => {}
            }
        }
        println!(
            "{fov}°: {cast_lit} of {cast} in shadow shown lit; {lit_dark} of {lit} lit shown dark"
        );
        assert!(
            cast > 2000 && cast_lit * 10 < cast && lit_dark == 0,
            "{cast_lit} {cast} {lit_dark}"
        );
    }
}

/// The Sun its true size (eclipses on): the two lobes' cast shadow as
/// dark and their lit pixels as lit as with a point Sun (its penumbra,
/// 0.27° wide at 1 au, is under a pixel here); and a sphere between them
/// and the Sun, its umbra over them, leaves them dark.
#[test]
fn the_suns_size_and_an_eclipse() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue);
    let maps = shaped(&Lobes, 256);
    let size = (240, 160);
    let l = DVec3::new(1.0, 0.15, 0.18).normalize();
    let cam = Camera::looking_at(
        CENTER + DVec3::new(-1.0, -2.0, 6.0),
        CENTER,
        DVec3::Y,
        40f64.to_radians(),
    );
    let sun = CENTER + l * AU_KM;
    let point = Scene {
        sun,
        ..scene(cam, vec![body(&maps, DMat3::IDENTITY)])
    };
    let sized = Scene {
        sun_radius: 696_000.0,
        ..point.clone()
    };
    let (a, _) = draw(&gpu, &mut renderer, &point, size);
    let (b, _) = draw(&gpu, &mut renderer, &sized, size);
    let differ = a
        .iter()
        .zip(&b)
        .filter(|(p, q)| (p[0] - q[0]).abs() > 0.01 + 0.02 * p[0])
        .count();
    let lit = a.iter().filter(|p| p[0] > 0.05).count();
    println!("{differ} of {lit} lit pixels differ with the Sun's size");
    assert!(lit > 2000 && differ * 100 < lit, "{differ} of {lit}");
    // A body of 50 km, 1,000 km sunward: its umbra 41 km across here.
    let shade = Sphere::plain(CENTER + l * 1000.0, 50.0, Surface::Lit([1.0; 3]));
    let eclipsed = Scene {
        spheres: vec![body(&maps, DMat3::IDENTITY), shade],
        ..sized
    };
    let (c, _) = draw(&gpu, &mut renderer, &eclipsed, size);
    let brightest = c.iter().map(|p| p[0]).fold(0.0f32, f32::max);
    println!("eclipsed: brightest {brightest}");
    assert!(brightest < 1e-3, "{brightest}");
}
