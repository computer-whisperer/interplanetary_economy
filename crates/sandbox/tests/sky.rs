//! The sky (M1c.md, step 6 plan, items 2 and 7), offscreen: catalog stars
//! drawn where their published places put them, and carrying their flux.

use glam::{DVec2, DVec3};
use orbit::rotation::equatorial_to_ecliptic;
use orbit::time::Epoch;
use render::camera::Camera;
use render::offscreen::Offscreen;
use render::{Look, Renderer, Scene, Target};
use sandbox::sky;

/// A star's place from SIMBAD (ICRS, J2000): right ascension (h, m, s),
/// declination (°, ′, ″), and proper motion (μα cos δ, μδ; mas a year).
struct Place {
    name: &'static str,
    ra: (f64, f64, f64),
    dec: (f64, f64, f64),
    motion: (f64, f64),
}

const PLACES: [Place; 4] = [
    Place {
        name: "Sirius",
        ra: (6.0, 45.0, 8.91728),
        dec: (-16.0, 42.0, 58.0171),
        motion: (-546.01, -1223.07),
    },
    Place {
        name: "Betelgeuse",
        ra: (5.0, 55.0, 10.30536),
        dec: (7.0, 24.0, 25.4304),
        motion: (27.54, 11.30),
    },
    Place {
        name: "Polaris",
        ra: (2.0, 31.0, 49.09456),
        dec: (89.0, 15.0, 50.7923),
        motion: (44.48, -11.85),
    },
    Place {
        name: "Barnard's star",
        ra: (17.0, 57.0, 48.49847),
        dec: (4.0, 41.0, 36.11388),
        motion: (-801.551, 10362.394),
    },
];

impl Place {
    /// The direction in the sky's coordinates, `years` after J2000.
    fn at(&self, years: f64) -> DVec3 {
        let ra = (self.ra.0 + self.ra.1 / 60.0 + self.ra.2 / 3600.0) * 15f64.to_radians();
        let sign = self.dec.0.signum();
        let dec = sign * (self.dec.0.abs() + self.dec.1 / 60.0 + self.dec.2 / 3600.0);
        let dec = dec.to_radians();
        let p = DVec3::new(dec.cos() * ra.cos(), dec.cos() * ra.sin(), dec.sin());
        let east = DVec3::new(-ra.sin(), ra.cos(), 0.0);
        let north = DVec3::new(-dec.sin() * ra.cos(), -dec.sin() * ra.sin(), dec.cos());
        let mas = 1e-3 / 3600.0 * std::f64::consts::PI / 180.0;
        (p + (east * self.motion.0 + north * self.motion.1) * mas * years).normalize()
    }
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

/// Each star, at 2060, lands within half a pixel (10″ pixels) of its
/// published place carried forward by its proper motion and projected in
/// f64: the build, the file, the turn into ecliptic coordinates and the
/// shader together. Barnard's star has moved 62 pixels since J2000. Its
/// spot's pixels sum to its flux.
#[test]
fn stars_land_on_their_places() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue);
    let data = sky::wait().expect("data/sky reads");
    let t = Epoch::from_ymd(2060, 1, 1);
    let years = t.0 / 365.25;
    let size = (1000, 1000);
    // 10″ a pixel.
    let fov = 1000.0 * 10.0 / 3600.0 * std::f64::consts::PI / 180.0;
    let gain = 1e5_f32;
    for place in &PLACES {
        let d = equatorial_to_ecliptic() * place.at(years);
        // Aimed off the star, so that it lands off the screen's center.
        let up = if d.z.abs() > 0.9 { DVec3::X } else { DVec3::Z };
        let side = d.cross(up).normalize();
        let aim = (d + side * 0.003 + d.cross(side) * 0.002).normalize();
        let camera = Camera::looking_at(DVec3::ZERO, aim, up, fov);
        let scene = Scene {
            camera: Some(camera),
            sky: Some(render::Sky {
                milky_way: None,
                ..sky::sky(data, t)
            }),
            look: Look {
                exposure: gain,
                ..Default::default()
            },
            ..Default::default()
        };
        let image = radiance(&gpu, &mut renderer, &scene, size);
        let expected = camera.project(d, size).expect("in front");
        // The spot's centroid, over a window around the expectation.
        let (mut sum, mut weight) = (DVec2::ZERO, 0.0);
        let (cx, cy) = (expected.x as i64, expected.y as i64);
        for y in cy - 6..=cy + 6 {
            for x in cx - 6..=cx + 6 {
                let w = f64::from(image[(y * size.0 as i64 + x) as usize][1]);
                sum += DVec2::new(x as f64 + 0.5, y as f64 + 0.5) * w;
                weight += w;
            }
        }
        assert!(weight > 0.0, "{} is not drawn near {expected}", place.name);
        let got = sum / weight;
        let error = (got - expected).length();
        let at_j2000 = camera
            .project(equatorial_to_ecliptic() * place.at(0.0), size)
            .unwrap();
        println!(
            "{}: off by {error:.3} px; {:.1} px from its J2000 place",
            place.name,
            (got - at_j2000).length()
        );
        assert!(error < 0.5, "{}: {error} px", place.name);
        if place.name == "Barnard's star" {
            assert!((got - at_j2000).length() > 50.0);
        }
        if place.name == "Sirius" {
            // Its flux as shown, as a camera records it and through a
            // viewer's pixels 3× and a third the scene's (at a tenth the
            // exposure, its spot under f16's largest): the green channel's
            // pixels × the sky each subtends.
            let tan_half = (fov / 2.0).tan();
            let pixel = 2.0 * tan_half / f64::from(size.1);
            let viewer = |times: f64| Look {
                pixel: (pixel * times) as f32,
                exposure: gain / 10.0,
                ..scene.look
            };
            for look in [scene.look, viewer(3.0), viewer(1.0 / 3.0)] {
                let image = radiance(
                    &gpu,
                    &mut renderer,
                    &Scene {
                        look,
                        ..scene.clone()
                    },
                    size,
                );
                let mut flux = 0.0;
                for y in cy - 6..=cy + 6 {
                    for x in cx - 6..=cx + 6 {
                        let ndc = DVec2::new(
                            (x as f64 + 0.5) / f64::from(size.0) * 2.0 - 1.0,
                            1.0 - (y as f64 + 0.5) / f64::from(size.1) * 2.0,
                        ) * tan_half;
                        let r = DVec3::new(ndc.x, ndc.y, -1.0);
                        let sky = pixel * pixel / r.length().powi(3);
                        flux += f64::from(image[(y * size.0 as i64 + x) as usize][1]) * sky;
                    }
                }
                // Times the gain the look gives a point where it lands
                // (pixel² cos³θ of sky), and the exposure.
                let star = data.stars[0];
                let cos = -camera.rotate(d).z;
                let gain = look.point_gain(0.0, pixel * cos.powf(1.5));
                let want = f64::from(star.flux) * f64::from((star.color >> 8) & 255) / 255.0
                    * gain
                    * f64::from(look.exposure);
                println!("Sirius, gain {gain:.3}: flux {flux:.4e}, wanted {want:.4e}");
                assert!((flux / want - 1.0).abs() < 1e-2, "{flux} vs {want}");
            }
        }
    }
}

/// The Milky Way shows its map's radiance at the scene's exposure: a 2°
/// view toward the Sagittarius star cloud and the north galactic pole,
/// at exposures 1e5 and 1e7 (the exposure keys reach the sky; at 1e4,
/// f16 holds the pole's 1.6e-6 to 2%), against
/// the map sampled on the CPU along each pixel's ray (its texels decoded
/// from sRGB, bilinear, × its radiance), within 1%.
#[test]
fn the_milky_way_takes_the_exposure() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue);
    let data = sky::wait().expect("data/sky reads");
    let map = &data.milky_way;
    let (w, h) = (map.width as i64, map.height as i64);
    let linear = |c: u8| {
        let c = f64::from(c) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    // Mean of r, g and b at a texel, wrapped in right ascension.
    let texel = |x: i64, y: i64| {
        let i = ((y.clamp(0, h - 1) * w + x.rem_euclid(w)) * 4) as usize;
        (0..3).map(|c| linear(map.data[i + c])).sum::<f64>() / 3.0
    };
    let size = (200, 200);
    let fov = 2f64.to_radians();
    let tan_half = (fov / 2.0).tan();
    let sky = render::Sky {
        stars: Default::default(),
        ..sky::sky(data, Epoch(0.0))
    };
    for (name, ra, dec) in [
        ("Sagittarius star cloud", 270.9f64, -30.0f64),
        ("north galactic pole", 192.859, 27.128),
    ] {
        let (ra, dec) = (ra.to_radians(), dec.to_radians());
        let d = DVec3::new(dec.cos() * ra.cos(), dec.cos() * ra.sin(), dec.sin());
        let camera = Camera::looking_at(DVec3::ZERO, sky.orientation * d, DVec3::Z, fov);
        // The map's mean over the pixels' rays.
        let mut want = 0.0;
        for y in 0..size.1 {
            for x in 0..size.0 {
                let ndc = DVec2::new(
                    (f64::from(x) + 0.5) / f64::from(size.0) * 2.0 - 1.0,
                    1.0 - (f64::from(y) + 0.5) / f64::from(size.1) * 2.0,
                ) * tan_half;
                let ray = camera.right * ndc.x + camera.up * ndc.y + camera.forward;
                let r = sky.orientation.transpose() * ray.normalize();
                let u = 0.5 - r.y.atan2(r.x) / std::f64::consts::TAU;
                let v = 0.5 - r.z.clamp(-1.0, 1.0).asin() / std::f64::consts::PI;
                let (fx, fy) = (u * w as f64 - 0.5, v * h as f64 - 0.5);
                let (x0, y0) = (fx.floor() as i64, fy.floor() as i64);
                let (a, b) = (fx - fx.floor(), fy - fy.floor());
                want += (texel(x0, y0) * (1.0 - a) + texel(x0 + 1, y0) * a) * (1.0 - b)
                    + (texel(x0, y0 + 1) * (1.0 - a) + texel(x0 + 1, y0 + 1) * a) * b;
            }
        }
        want *= f64::from(sky.milky_way_radiance) / f64::from(size.0 * size.1);
        for exposure in [1e5f32, 1e7] {
            let scene = Scene {
                camera: Some(camera),
                sky: Some(sky.clone()),
                look: Look {
                    exposure,
                    ..Default::default()
                },
                ..Default::default()
            };
            let image = radiance(&gpu, &mut renderer, &scene, size);
            let got = image
                .iter()
                .map(|p| f64::from(p[0] + p[1] + p[2]) / 3.0)
                .sum::<f64>()
                / image.len() as f64
                / f64::from(exposure);
            println!("{name}, exposure {exposure:e}: radiance {got:.4e}, the map's {want:.4e}");
            assert!((got / want - 1.0).abs() < 0.01, "{name}: {got} vs {want}");
        }
    }
}

/// GPU time of the whole sky at 1920×1080 and the sandbox's 50° view:
/// the Milky Way and 118,000 stars, toward the galactic center.
#[test]
#[ignore = "a measurement: run with --ignored --nocapture"]
fn sky_gpu_time() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue);
    let data = sky::wait().expect("data/sky reads");
    let t = Epoch::from_ymd(2045, 3, 1);
    let center = Place {
        name: "Sgr A*",
        ra: (17.0, 45.0, 40.0),
        dec: (-29.0, 0.0, 28.0),
        motion: (0.0, 0.0),
    };
    let d = equatorial_to_ecliptic() * center.at(0.0);
    for (what, sky) in [("nothing", None), ("the sky", Some(sky::sky(data, t)))] {
        let scene = Scene {
            camera: Some(Camera::looking_at(
                DVec3::ZERO,
                d,
                DVec3::Z,
                50f64.to_radians(),
            )),
            sky,
            // Earth's exposure + 19 stops: the Milky Way shown, and every
            // star.
            look: Look {
                exposure: 1e6,
                pixel: sandbox::look::PIXEL as f32,
                bloom: sandbox::look::BLOOM,
                ..Default::default()
            },
            ..Default::default()
        };
        for _ in 0..3 {
            if let Some(ms) = gpu.time_scene(&mut renderer, &scene, (1920, 1080)) {
                println!("{what}: {ms:.3} ms");
            }
        }
    }
}
