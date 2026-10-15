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
                sky: gain,
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
            // Its flux as shown, at this look and at the Sun's (exposure
            // 2.6e-5, the sandbox's sky): the green channel's pixels × the
            // sky each subtends. Found in review: at the Sun's, spots were
            // clipped at f16's largest before exposure, every star
            // brighter than V ~4 alike.
            let sun_look = Look {
                exposure: 2.6e-5,
                sky: 7.5e5,
                ..Default::default()
            };
            for look in [scene.look, sun_look] {
                let image = radiance(
                    &gpu,
                    &mut renderer,
                    &Scene {
                        look,
                        ..scene.clone()
                    },
                    size,
                );
                let tan_half = (fov / 2.0).tan();
                let side = 2.0 * tan_half / f64::from(size.1);
                let mut flux = 0.0;
                for y in cy - 6..=cy + 6 {
                    for x in cx - 6..=cx + 6 {
                        let ndc = DVec2::new(
                            (x as f64 + 0.5) / f64::from(size.0) * 2.0 - 1.0,
                            1.0 - (y as f64 + 0.5) / f64::from(size.1) * 2.0,
                        ) * tan_half;
                        let r = DVec3::new(ndc.x, ndc.y, -1.0);
                        let sky = side * side / r.length().powi(3);
                        flux += f64::from(image[(y * size.0 as i64 + x) as usize][1]) * sky;
                    }
                }
                // Times the gain the look gives a spot this bright (by
                // its peak pixel where it lands: pixel² cos³θ of sky), and
                // the exposure.
                let star = data.stars[0];
                let cos = -camera.rotate(d).z;
                let peak = f64::from(star.flux) / (side * side * cos.powi(3))
                    * f64::from(render::scene::SPOT_PEAK);
                let shown = look.point_gain(peak as f32) * look.exposure;
                let want = f64::from(star.flux) * f64::from((star.color >> 8) & 255) / 255.0
                    * f64::from(shown);
                println!(
                    "Sirius at exposure {}: flux {flux:.4e}, wanted {want:.4e}",
                    look.exposure
                );
                assert!((flux / want - 1.0).abs() < 1e-2, "{flux} vs {want}");
            }
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
            look: Look {
                sky: sandbox::look::SKY,
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
