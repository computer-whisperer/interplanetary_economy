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

/// Frames drawn from each date, the first of them not counted (the
/// orbits' and ships' lines built for it).
const FRAMES: usize = 600;
const WARM: usize = 30;

/// Median, 95th percentile and the longest of `ms`.
fn spread(ms: &mut [f64]) -> (f64, f64, f64) {
    ms.sort_by(f64::total_cmp);
    let n = ms.len();
    (ms[n / 2], ms[n * 95 / 100], ms[n - 1])
}

/// The Mars windows plan around the Sun from 4×10⁸ km, 60° above the
/// ecliptic, at a month a second from four dates across its years, each
/// frame 1/240 s: its CPU time (the clock's tick, the scene, labels, the
/// UI and drawing, to the submit) and GPU time (timestamps). Each frame
/// waits for the GPU before the next, as a display's pacing would
/// (`cargo test --release -p sandbox --test playback -- --ignored
/// --nocapture`; designing takes ~15 s on 24 cores).
#[test]
#[ignore]
fn mars_windows_at_a_month_a_second() {
    let eph = Ephemeris::standard();
    let mut plan = Loaded::open(&sandbox::data_dir().join("missions/mars-windows.toml")).unwrap();
    plan.wait();
    println!("{}", plan.progress());
    let gpu = render::offscreen::Offscreen::new().expect("a GPU");
    let (device, queue) = (&gpu.device, &gpu.queue);
    let mut app = Shell::new(plan.epoch());
    app.plan = Some(plan);
    app.clock.rate = RATES
        .iter()
        .position(|r| r.1 == "1 month")
        .expect("a month a second");
    let mut camera = sandbox::camera::MapCamera::new(eph, eph.sun());
    camera.set_altitude(eph, 4e8);
    camera.set_direction(camera.azimuth, 60f64.to_radians());
    app.camera = camera;
    app.follow_focus();

    let (w, h) = SIZE;
    let mut ui = Runner::new(device, queue, frame::OFFSCREEN_FORMAT);
    ui.set_theme(app.theme());
    ui.set_surface_size(w, h);
    let mut view = View::new(device, queue);
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
    let viewport = Rect::new(0.0, 0.0, w as f32 / SCALE, h as f32 / SCALE);

    let (mut cpu, mut gpu_ms) = (Vec::new(), Vec::new());
    for year in [2042, 2052, 2062, 2072] {
        app.clock.seek(Epoch::from_ymd(year, 1, 1));
        for k in 0..FRAMES {
            let start = Instant::now();
            app.tick(1.0 / REFRESH_HZ);
            let scene = app.scene(SCALE);
            let out = Output {
                texture: &texture,
                view: &texture_view,
                msaa: None,
            };
            let drawn = frame::draw(
                device, queue, &mut ui, &mut app, &mut view, &scene, out, viewport, SCALE, None,
            );
            let ms = start.elapsed().as_secs_f64() * 1e3;
            device
                .poll(wgpu::PollType::wait_indefinitely())
                .expect("the GPU finishes the frame");
            if k >= WARM {
                cpu.push(ms);
                gpu_ms.extend(
                    drawn
                        .timings
                        .iter()
                        .map(|t| f64::from(t.frame.unwrap_or(t.scene))),
                );
            }
        }
    }
    let period = 1e3 / REFRESH_HZ;
    let (c50, c95, cmax) = spread(&mut cpu);
    println!(
        "CPU a frame: median {c50:.2} ms, 95% {c95:.2} ms, longest {cmax:.2} ms over {} frames (the refresh: {period:.2} ms)",
        cpu.len()
    );
    if gpu_ms.is_empty() {
        println!("no timestamps");
    } else {
        let (g50, g95, gmax) = spread(&mut gpu_ms);
        println!(
            "GPU a frame: median {g50:.2} ms, 95% {g95:.2} ms, longest {gmax:.2} ms over {} frames",
            gpu_ms.len()
        );
    }
}
