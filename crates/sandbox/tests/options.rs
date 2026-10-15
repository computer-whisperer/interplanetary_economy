//! The options panel (M1c.md, step 7 plan, items 5 and 9): a leg
//! between planets' porkchop over its window, a cell clicked pinning the
//! leg there and flying it at that cell; a leg within a system's offered
//! durations, one clicked pinning it.

use damascene_core::prelude::*;
use damascene_wgpu::Runner;
use orbit::ephem::Ephemeris;
use orbit::mission::{ItemKind, Plan, Step};
use orbit::places::Places;
use sandbox::app::Shell;
use sandbox::frame::{self, Output, View};
use sandbox::options::{CHART_KEY, SWEEP_KEY, UNPIN_KEY, point_key, swept_key};
use sandbox::plan::Loaded;

/// To Mars in the 2033 window, its date and duration left to the
/// autopilot; and a leg within Earth's system with three durations.
const PLAN: &str = r#"
name = "Options"

[[ship]]
name = "Hauler"
start = { at = "leo", epoch = "2033-01-01" }

[[ship.leg]]
to = { low = "mars", altitude = 400 }

[[ship]]
name = "Lander"
start = { at = "nrho", epoch = "2045-03-05" }

[[ship.leg]]
to = "llo"
days = [0.5, 1.0, 2.0]
"#;

/// Draws the app offscreen and drives it with clicks.
struct Harness {
    gpu: render::offscreen::Offscreen,
    ui: Runner,
    view: View,
    texture: wgpu::Texture,
    size: (f32, f32),
}

impl Harness {
    fn new(app: &Shell, size: (f32, f32)) -> Harness {
        let gpu = render::offscreen::Offscreen::new().expect("a GPU");
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: size.0 as u32,
                height: size.1 as u32,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: frame::OFFSCREEN_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let mut ui = Runner::new(&gpu.device, &gpu.queue, frame::OFFSCREEN_FORMAT);
        ui.set_animation_mode(damascene_core::AnimationMode::Settled);
        ui.set_theme(app.theme());
        ui.set_surface_size(size.0 as u32, size.1 as u32);
        let view = View::new(&gpu.device, &gpu.queue);
        Harness {
            gpu,
            ui,
            view,
            texture,
            size,
        }
    }

    fn draw(&mut self, app: &mut Shell) {
        app.poll();
        app.update_paths();
        let scene = app.scene(1.0);
        let target = self.texture.create_view(&Default::default());
        let out = Output {
            texture: &self.texture,
            view: &target,
            msaa: None,
        };
        let viewport = Rect::new(0.0, 0.0, self.size.0, self.size.1);
        frame::draw(
            &self.gpu.device,
            &self.gpu.queue,
            &mut self.ui,
            app,
            &mut self.view,
            &scene,
            out,
            viewport,
            1.0,
            None,
        );
    }

    fn click(&mut self, app: &mut Shell, (x, y): (f32, f32)) {
        let mut events = self
            .ui
            .pointer_down(Pointer::mouse(x, y, PointerButton::Primary));
        events.extend(
            self.ui
                .pointer_up(Pointer::mouse(x, y, PointerButton::Primary)),
        );
        let cx = EventCx::new().with_ui_state(self.ui.ui_state());
        for e in events {
            app.on_event(e, &cx);
        }
    }

    /// Writes the last frame where `OPTIONS_SHOT` says, if it does.
    fn shot(&mut self, app: &mut Shell, name: &str) {
        let Ok(dir) = std::env::var("OPTIONS_SHOT") else {
            return;
        };
        let s = frame::shot(
            &self.gpu,
            app,
            None,
            self.size.0 as u32,
            self.size.1 as u32,
            1.0,
        );
        let file = std::fs::File::create(format!("{dir}/{name}.png")).unwrap();
        let mut png = png::Encoder::new(std::io::BufWriter::new(file), s.width, s.height);
        png.set_color(png::ColorType::Rgba);
        png.set_depth(png::BitDepth::Eight);
        png.write_header()
            .unwrap()
            .write_image_data(&s.rgba)
            .unwrap();
    }
}

#[test]
fn a_cell_picked_flies_there() {
    let eph = Ephemeris::standard();
    let mut plan = Loaded::start(Plan::parse(PLAN, eph, Places::standard()).unwrap());
    plan.wait();
    assert!(plan.ships.iter().all(|s| s.failure.is_none()));
    let mut app = Shell::new(plan.start);
    app.load(plan);
    let mut h = Harness::new(&app, (1400.0, 900.0));
    h.draw(&mut app);

    // The Mars leg selected: its porkchop over its window.
    let ships = &app.plan.as_ref().unwrap().ships;
    let k = ships[0]
        .items
        .iter()
        .position(|i| i.kind == ItemKind::Leg)
        .unwrap();
    let flown = ships[0].items[k].clone();
    app.select(Some((0, k)));
    assert_eq!(app.leg, Some((0, 0)));
    for _ in 0..3 {
        h.draw(&mut app);
    }
    h.shot(&mut app, "porkchop");
    let chart = h.ui.rect_of_key(CHART_KEY).expect("the porkchop is drawn");
    println!(
        "chart {chart:?}; flown {} for {:.1} d, {:.3} km/s",
        flown.start,
        flown.end.0 - flown.start.0,
        flown.dv
    );

    // A cell well off the one flown: three quarters across, a third up;
    // the pointer over it first.
    let at = (chart.x + 0.75 * chart.w, chart.y + 0.67 * chart.h);
    app.pointer = Some(at);
    h.draw(&mut app);
    h.draw(&mut app);
    h.shot(&mut app, "hover");
    h.click(&mut app, at);
    let go = match &app.plan.as_ref().unwrap().plan.ships[0].steps[0] {
        Step::Go(go) => go.clone(),
        _ => unreachable!(),
    };
    let (depart, days) = (go.depart.expect("pinned"), go.days.clone());
    println!("pinned {depart} for {days:?}");
    assert_eq!(days.len(), 1);
    assert!(
        (depart.0 - flown.start.0).abs() > 30.0,
        "a cell away from the one flown"
    );
    app.plan.as_mut().unwrap().wait();
    h.draw(&mut app);
    h.shot(&mut app, "pinned");
    let ship = &app.plan.as_ref().unwrap().ships[0];
    assert!(ship.failure.is_none(), "{:?}", ship.failure);
    let leg = ship.items.iter().find(|i| i.kind == ItemKind::Leg).unwrap();
    println!(
        "flies {} for {:.2} d, {:.3} km/s",
        orbit::mission::when(leg.start),
        leg.end.0 - leg.start.0,
        leg.dv
    );
    assert!(
        (leg.start.0 - depart.0).abs() * 1_440.0 < 1.0,
        "leaves at the pin"
    );
    assert!(
        (leg.end.0 - leg.start.0 - days[0]).abs() < 1e-6,
        "takes the cell's duration"
    );
    assert_eq!(app.leg, Some((0, 0)), "the panel keeps the leg");

    // Unpinned, it flies the autopilot's choice again.
    h.draw(&mut app);
    let unpin = h.ui.rect_of_key(UNPIN_KEY).expect("an unpin button");
    h.click(&mut app, (unpin.x + unpin.w / 2.0, unpin.y + unpin.h / 2.0));
    app.plan.as_mut().unwrap().wait();
    let leg = app.plan.as_ref().unwrap().ships[0]
        .items
        .iter()
        .find(|i| i.kind == ItemKind::Leg)
        .unwrap()
        .clone();
    assert!((leg.start.0 - flown.start.0).abs() < 1e-9 && (leg.dv - flown.dv).abs() < 1e-9);

    // The leg within Earth's system: its durations, one clicked pinned.
    let lander = app.plan.as_ref().unwrap().ships[1].clone();
    let k = lander
        .items
        .iter()
        .position(|i| i.kind == ItemKind::Leg)
        .unwrap();
    app.select(Some((1, k)));
    h.draw(&mut app);
    h.draw(&mut app);
    h.shot(&mut app, "durations");
    let options = lander.items[k].options.clone();
    let i = options
        .iter()
        .position(|c| c.dv.is_some() && (c.days - 2.0).abs() < 1e-9)
        .expect("the 2-day design converged");
    let point = h.ui.rect_of_key(&point_key(i)).expect("its point is drawn");
    h.click(&mut app, (point.x + point.w / 2.0, point.y + point.h / 2.0));
    match &app.plan.as_ref().unwrap().plan.ships[1].steps[0] {
        Step::Go(go) => assert_eq!(go.days, [2.0]),
        _ => unreachable!(),
    }
    app.plan.as_mut().unwrap().wait();
    let leg = app.plan.as_ref().unwrap().ships[1]
        .items
        .iter()
        .find(|i| i.kind == ItemKind::Leg)
        .unwrap();
    assert!((leg.end.0 - leg.start.0 - 2.0).abs() < 1e-6);

    // Swept about the 2 days now offered: 1 to 4 days, each designed
    // alone, shown beside it, the plan as it was; one clicked pinned flies
    // as swept.
    h.draw(&mut app);
    let sweep = h.ui.rect_of_key(SWEEP_KEY).expect("a sweep button");
    h.click(&mut app, (sweep.x + sweep.w / 2.0, sweep.y + sweep.h / 2.0));
    let before = app.plan.as_ref().unwrap().plan.to_toml();
    app.plan.as_mut().unwrap().wait_sweeps();
    assert_eq!(app.plan.as_ref().unwrap().plan.to_toml(), before);
    let swept = app.plan.as_ref().unwrap().sweeps[&(1, 0)].clone();
    let options = swept.options.clone().unwrap().unwrap();
    let converged: Vec<_> = options.iter().filter(|c| c.dv.is_some()).collect();
    println!(
        "swept {:?}: {} of {} converged",
        swept.durations,
        converged.len(),
        options.len()
    );
    assert!(converged.len() * 2 >= options.len());
    h.draw(&mut app);
    h.draw(&mut app);
    h.shot(&mut app, "sweep");
    // The longest: where designs of several durations at once land
    // hundreds of m/s from one alone.
    let i = options.iter().rposition(|c| c.dv.is_some()).unwrap();
    let point =
        h.ui.rect_of_key(&swept_key(i))
            .expect("a swept point is drawn");
    h.click(&mut app, (point.x + point.w / 2.0, point.y + point.h / 2.0));
    match &app.plan.as_ref().unwrap().plan.ships[1].steps[0] {
        Step::Go(go) => assert_eq!(go.days, [options[i].days]),
        _ => unreachable!(),
    }
    app.plan.as_mut().unwrap().wait();
    let leg = app.plan.as_ref().unwrap().ships[1]
        .items
        .iter()
        .find(|i| i.kind == ItemKind::Leg)
        .unwrap();
    println!(
        "swept {:?}; flies {} d at {:.6} km/s",
        options[i],
        leg.end.0 - leg.start.0,
        leg.dv
    );
    assert!(
        (leg.dv - options[i].dv.unwrap()).abs() < 1e-9,
        "flies as swept"
    );
}
