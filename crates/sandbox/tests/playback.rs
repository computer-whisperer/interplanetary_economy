//! Gate item 3 offscreen (M1c.md, step 8 plan): a plan played back at a
//! month a second, frame by frame as a window would draw it.

use damascene_core::prelude::*;
use damascene_wgpu::Runner;
use orbit::ephem::Ephemeris;
use orbit::time::Epoch;
use sandbox::app::Shell;
use sandbox::clock::RATES;
use sandbox::frame::{self, Output, View};
use sandbox::plan::Loaded;
use std::time::Instant;

/// The development machine's display: 3840×2160, two pixels to a logical
/// one, at 240 Hz.
const SIZE: (u32, u32) = (3840, 2160);
const SCALE: f32 = 2.0;
const REFRESH_HZ: f64 = 240.0;

/// Frames drawn from each date or at each rate, the first of them not
/// counted (the orbits' and ships' lines built for it).
const FRAMES: usize = 600;
const WARM: usize = 30;

/// Median, 95th percentile and the longest of `ms`.
fn spread(ms: &mut [f64]) -> (f64, f64, f64) {
    ms.sort_by(f64::total_cmp);
    let n = ms.len();
    (ms[n / 2], ms[n * 95 / 100], ms[n - 1])
}

/// Frames drawn offscreen as a window would draw them: the target, and
/// the UI's and the map's renderers.
struct Screen {
    gpu: render::offscreen::Offscreen,
    ui: Runner,
    view: View,
    texture: wgpu::Texture,
    texture_view: wgpu::TextureView,
}

impl Screen {
    fn new(app: &Shell) -> Screen {
        let gpu = render::offscreen::Offscreen::new().expect("a GPU");
        let (device, queue) = (&gpu.device, &gpu.queue);
        let (w, h) = SIZE;
        let mut ui = Runner::new(device, queue, frame::OFFSCREEN_FORMAT);
        ui.set_theme(app.theme());
        ui.set_surface_size(w, h);
        let view = View::new(device, queue);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("playback frame"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: frame::OFFSCREEN_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let texture_view = texture.create_view(&Default::default());
        Screen {
            gpu,
            ui,
            view,
            texture,
            texture_view,
        }
    }

    /// One frame 1/240 s on: its CPU time (the clock's tick, the scene,
    /// labels, the UI and drawing, to the submit), ms, and its GPU times
    /// (timestamps), the GPU waited for before the next, as a display's
    /// pacing would.
    fn frame(&mut self, app: &mut Shell) -> (f64, Vec<f64>) {
        let (device, queue) = (&self.gpu.device, &self.gpu.queue);
        let (w, h) = SIZE;
        let viewport = Rect::new(0.0, 0.0, w as f32 / SCALE, h as f32 / SCALE);
        let start = Instant::now();
        app.tick(1.0 / REFRESH_HZ);
        let scene = app.scene(SCALE);
        let out = Output {
            texture: &self.texture,
            view: &self.texture_view,
            msaa: None,
        };
        let drawn = frame::draw(
            device,
            queue,
            &mut self.ui,
            app,
            &mut self.view,
            &scene,
            out,
            viewport,
            SCALE,
            None,
        );
        let ms = start.elapsed().as_secs_f64() * 1e3;
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("the GPU finishes the frame");
        let gpu = drawn
            .timings
            .iter()
            .map(|t| f64::from(t.frame.unwrap_or(t.scene)))
            .collect();
        (ms, gpu)
    }
}

/// CPU and GPU times a frame, as their spreads.
fn report(cpu: &mut [f64], gpu: &mut [f64]) {
    let period = 1e3 / REFRESH_HZ;
    let (c50, c95, cmax) = spread(cpu);
    println!(
        "CPU a frame: median {c50:.2} ms, 95% {c95:.2} ms, longest {cmax:.2} ms over {} frames (the refresh: {period:.2} ms)",
        cpu.len()
    );
    if gpu.is_empty() {
        println!("no timestamps");
    } else {
        let (g50, g95, gmax) = spread(gpu);
        println!(
            "GPU a frame: median {g50:.2} ms, 95% {g95:.2} ms, longest {gmax:.2} ms over {} frames",
            gpu.len()
        );
    }
}

/// The Mars windows plan around the Sun from 4×10⁸ km, 60° above the
/// ecliptic, at a month a second from four dates across its years, each
/// frame 1/240 s (`cargo test --release -p sandbox --test playback --
/// --ignored --nocapture`; designing takes ~15 s on 24 cores).
#[test]
#[ignore]
fn mars_windows_at_a_month_a_second() {
    let eph = Ephemeris::standard();
    let mut plan = Loaded::open(&sandbox::data_dir().join("missions/mars-windows.toml")).unwrap();
    plan.wait();
    println!("{}", plan.progress());
    let mut app = Shell::new(plan.epoch());
    app.plan = Some(plan);
    app.clock.rate = rate("1 month");
    let mut camera = sandbox::camera::MapCamera::new(eph, eph.sun());
    camera.set_altitude(eph, 4e8);
    camera.set_direction(camera.azimuth, 60f64.to_radians());
    app.camera = camera;
    app.follow_focus();
    let mut screen = Screen::new(&app);
    let (mut cpu, mut gpu) = (Vec::new(), Vec::new());
    for year in [2042, 2052, 2062, 2072] {
        app.clock.seek(Epoch::from_ymd(year, 1, 1));
        for k in 0..FRAMES {
            let (ms, g) = screen.frame(&mut app);
            if k >= WARM {
                cpu.push(ms);
                gpu.extend(g);
            }
        }
    }
    report(&mut cpu, &mut gpu);
}

/// Into [`RATES`], by its name.
fn rate(name: &str) -> usize {
    RATES
        .iter()
        .position(|r| r.1 == name)
        .expect("a rate of the clock's")
}

/// The process's resident memory, MB (Linux).
fn resident_mb() -> f64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    status
        .lines()
        .find_map(|l| l.strip_prefix("VmRSS:"))
        .and_then(|v| v.trim().trim_end_matches("kB").trim().parse::<f64>().ok())
        .map_or(f64::NAN, |kb| kb / 1e3)
}

/// A ship parked a year in LEO and one a year in the NRHO, about Earth
/// from 30,000 km, the clock running through their stays at an hour, a
/// day and a week a second (M1b.md, step 2 plan, parts 6 and 8): the
/// resident memory once designed, and the frames' times; the first frame
/// at each rate, which builds the lines, and the longest, which may build
/// a stay's again (`cargo test --release -p sandbox --test playback --
/// parked --ignored --nocapture`).
#[test]
#[ignore]
fn parked_a_year() {
    let eph = Ephemeris::standard();
    let src = r#"
name = "parked"

[[ship]]
name = "Parked in LEO"
start = { at = "leo", epoch = "2045-03-01" }

[[ship.leg]]
wait = 365

[[ship.leg]]
to = "llo"
days = [3.5]

[[ship]]
name = "Parked in the NRHO"
start = { at = "nrho", epoch = "2045-03-01" }

[[ship.leg]]
wait = 365

[[ship.leg]]
to = "llo"
days = [0.5]
"#;
    let before = resident_mb();
    let plan = orbit::mission::Plan::parse(src, eph, orbit::places::Places::standard()).unwrap();
    let mut plan = Loaded::start(plan).unwrap();
    plan.wait();
    println!(
        "{}; resident {:.0} MB, {:.0} MB before",
        plan.progress(),
        resident_mb(),
        before
    );
    let mut app = Shell::new(plan.epoch());
    app.plan = Some(plan);
    let mut camera = sandbox::camera::MapCamera::new(eph, eph.lookup("earth").unwrap());
    camera.set_altitude(eph, 3e4);
    app.camera = camera;
    app.follow_focus();
    let mut screen = Screen::new(&app);
    for name in ["1 h", "1 day", "1 week"] {
        app.clock.rate = rate(name);
        app.clock.seek(Epoch::from_ymd(2045, 6, 1));
        let (mut cpu, mut gpu) = (Vec::new(), Vec::new());
        let mut first = 0.0;
        for k in 0..FRAMES {
            let (ms, g) = screen.frame(&mut app);
            match k {
                0 => first = ms,
                k if k >= WARM => {
                    cpu.push(ms);
                    gpu.extend(g);
                }
                _ => {}
            }
        }
        println!("{name} a second, the first frame {first:.2} ms:");
        report(&mut cpu, &mut gpu);
    }
    println!("resident {:.0} MB after", resident_mb());
}
