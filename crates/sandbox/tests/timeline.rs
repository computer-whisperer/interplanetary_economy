//! The timeline (M1c.md, step 7 plan, items 1 and 9): bars drawn at
//! their items' times in their ships' lanes; a click on a bar selecting
//! its item, a press on the axis seeking the clock, and the wheel zooming
//! about the pointer.

use damascene_core::prelude::*;
use damascene_wgpu::Runner;
use orbit::ephem::Ephemeris;
use orbit::mission::Plan;
use orbit::places::Places;
use orbit::time::Epoch;
use sandbox::app::Shell;
use sandbox::frame::{self, Output, View};
use sandbox::plan::Loaded;
use sandbox::timeline::{self, AXIS_KEY, bar_key};

/// Two ships from the NRHO to low lunar orbit, a day apart: designed in
/// a fraction of a second.
const TWO: &str = r#"
name = "Two"

[[ship]]
name = "First"
start = { at = "nrho", epoch = "2045-03-05" }

[[ship.leg]]
to = "llo"
days = [0.5]

[[ship]]
name = "Second"
start = { at = "nrho", epoch = "2045-03-06" }

[[ship.leg]]
to = "llo"
days = [0.5]
"#;

#[test]
fn bars_at_their_times_and_clicks() {
    let gpu = render::offscreen::Offscreen::new().expect("a GPU");
    let (device, queue) = (&gpu.device, &gpu.queue);
    let eph = Ephemeris::standard();
    let mut plan = Loaded::start(Plan::parse(TWO, eph, Places::standard()).unwrap());
    plan.wait();
    assert!(plan.done() && plan.ships.iter().all(|s| s.failure.is_none()));
    for scale in [1.0f32, 2.0] {
        let mut app = Shell::new(plan.epoch());
        app.plan = Some(Loaded::start(
            Plan::parse(TWO, eph, Places::standard()).unwrap(),
        ));
        app.plan.as_mut().unwrap().wait();
        app.poll();

        let (w, h) = (1000.0, 700.0);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: (w * scale) as u32,
                height: (h * scale) as u32,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: frame::OFFSCREEN_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let target = texture.create_view(&Default::default());
        let mut ui = Runner::new(device, queue, frame::OFFSCREEN_FORMAT);
        ui.set_animation_mode(damascene_core::AnimationMode::Settled);
        ui.set_theme(app.theme());
        ui.set_surface_size((w * scale) as u32, (h * scale) as u32);
        let mut view = View::new(device, queue);
        let viewport = Rect::new(0.0, 0.0, w, h);
        let mut draw = |app: &mut Shell, ui: &mut Runner| {
            app.update_paths();
            let scene = app.scene(scale);
            let out = Output {
                texture: &texture,
                view: &target,
                msaa: None,
            };
            frame::draw(
                device, queue, ui, app, &mut view, &scene, out, viewport, scale, None,
            );
        };
        // The axis's width is read from the last layout.
        for _ in 0..3 {
            draw(&mut app, &mut ui);
        }
        let axis = ui.rect_of_key(AXIS_KEY).expect("the axis is laid out");
        let lanes = ui
            .rect_of_key(timeline::LANES_KEY)
            .expect("the lanes are laid out");
        assert!((lanes.x - axis.x).abs() < 0.01 && (lanes.w - axis.w).abs() < 0.01);
        let x_of = |app: &Shell, t: Epoch| axis.x + app.timeline.fraction(t) as f32 * axis.w;

        // Each bar spans its item's times (at least 2 px, about its
        // middle) in its ship's lane.
        let ships = app.plan.as_ref().unwrap().ships.clone();
        let mut worst = 0.0f32;
        for (s, ship) in ships.iter().enumerate() {
            for (k, item) in ship.items.iter().enumerate() {
                let r = ui.rect_of_key(&bar_key(s, k)).expect("a bar per item");
                let (x0, x1) = (x_of(&app, item.start), x_of(&app, item.end));
                let (want0, want1) = if x1 - x0 < 2.0 {
                    ((x0 + x1) / 2.0 - 1.0, (x0 + x1) / 2.0 + 1.0)
                } else {
                    (x0, x1)
                };
                worst = worst
                    .max((r.x - want0).abs())
                    .max((r.x + r.w - want1).abs());
                let lane = (lanes.y + s as f32 * 18.0, lanes.y + (s + 1) as f32 * 18.0);
                assert!(
                    r.y >= lane.0 && r.y + r.h <= lane.1,
                    "ship {s}'s bar {k} in its lane: {r:?}, {lane:?}"
                );
            }
        }
        println!("scale {scale}: bars within {worst:.4} px of their times");
        assert!(worst < 0.01);

        let click = |app: &mut Shell, ui: &mut Runner, (x, y): (f32, f32)| {
            let mut events = ui.pointer_down(Pointer::mouse(x, y, PointerButton::Primary));
            events.extend(ui.pointer_up(Pointer::mouse(x, y, PointerButton::Primary)));
            let cx = EventCx::new().with_ui_state(ui.ui_state());
            for e in events {
                app.on_event(e, &cx);
            }
        };

        // A click on the second ship's leg selects it and seeks there.
        let k = ships[1]
            .items
            .iter()
            .position(|i| i.kind == orbit::mission::ItemKind::Leg)
            .unwrap();
        let r = ui.rect_of_key(&bar_key(1, k)).unwrap();
        let at = (r.x + r.w / 2.0, r.y + r.h / 2.0);
        click(&mut app, &mut ui, at);
        assert_eq!(app.selected, Some((1, k)));
        let seconds =
            (app.clock.t.0 - app.timeline.at(f64::from((at.0 - axis.x) / axis.w)).0) * 86_400.0;
        assert!(seconds.abs() < 1e-3, "{seconds} s");

        // A click on the axis seeks the clock there.
        draw(&mut app, &mut ui);
        let x = axis.x + 0.25 * axis.w;
        click(&mut app, &mut ui, (x, axis.y + axis.h / 2.0));
        let f = f64::from((x - axis.x) / axis.w);
        assert!((app.clock.t.0 - app.timeline.at(f).0).abs() * 86_400.0 < 1e-3);
        assert_eq!(app.selected, Some((1, k)), "the axis selects nothing");

        // The wheel over the lanes zooms about the pointer: five lines.
        draw(&mut app, &mut ui);
        let (x, y) = (axis.x + 0.6 * axis.w, lanes.y + 1.0);
        let f = f64::from((x - axis.x) / axis.w);
        let (days, under) = (app.timeline.days(), app.timeline.at(f));
        let e = ui
            .pointer_wheel_event(x, y, 0.0, -250.0)
            .expect("a wheel event over the lanes");
        let cx = EventCx::new().with_ui_state(ui.ui_state());
        assert!(app.on_wheel_event(e, &cx), "the timeline takes the wheel");
        assert!(app.timeline.days() < days * 0.9);
        // A frame later, the clock left outside the span zoomed into.
        app.tick(0.0);
        assert!(app.timeline.fraction(app.clock.t) < 0.0);
        assert!((app.timeline.at(f).0 - under.0).abs() * 86_400.0 < 1e-3);

        // Dragging on the axis past its end holds the clock at the span's
        // end, frame after frame.
        draw(&mut app, &mut ui);
        let (x, y) = (axis.x + axis.w - 2.0, axis.y + axis.h / 2.0);
        let mut events = ui.pointer_down(Pointer::mouse(x, y, PointerButton::Primary));
        for k in 1..8 {
            events.extend(
                ui.pointer_moved(Pointer::moving(x + 10.0 * k as f32, y))
                    .events,
            );
        }
        let cx = EventCx::new().with_ui_state(ui.ui_state());
        for e in events {
            app.on_event(e, &cx);
            app.tick(0.0);
        }
        let up = ui.pointer_up(Pointer::mouse(x + 70.0, y, PointerButton::Primary));
        let cx = EventCx::new().with_ui_state(ui.ui_state());
        for e in up {
            app.on_event(e, &cx);
        }
        assert_eq!(app.clock.t, app.timeline.span.1);

        // A double click on empty lanes fits the plan again.
        assert!(!app.timeline.fitted);
        draw(&mut app, &mut ui);
        let lanes = ui.rect_of_key(timeline::LANES_KEY).unwrap();
        let empty = (lanes.x + 2.0, lanes.y + lanes.h - 2.0);
        click(&mut app, &mut ui, empty);
        click(&mut app, &mut ui, empty);
        app.tick(0.0);
        assert!(app.timeline.fitted);
        let span = app.plan.as_ref().unwrap().span();
        assert!(app.timeline.fraction(span.0) > 0.0 && app.timeline.fraction(span.1) < 1.0);
        // A press on a bar that moves off it selects nothing.
        app.select(None);
        draw(&mut app, &mut ui);
        let r = ui.rect_of_key(&bar_key(0, 0)).unwrap();
        let (x, y) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
        let mut events = ui.pointer_down(Pointer::mouse(x, y, PointerButton::Primary));
        events.extend(ui.pointer_moved(Pointer::moving(x, y + 200.0)).events);
        events.extend(ui.pointer_up(Pointer::mouse(x, y + 200.0, PointerButton::Primary)));
        let cx = EventCx::new().with_ui_state(ui.ui_state());
        for e in events {
            app.on_event(e, &cx);
        }
        assert_eq!(app.selected, None);
    }
}

/// With the clock set before the plan, the timeline still opens on the
/// plan.
#[test]
fn the_plan_is_fitted_with_the_clock_elsewhere() {
    let eph = Ephemeris::standard();
    let mut app = Shell::new(Epoch::from_ymd(2044, 1, 1));
    app.plan = Some(Loaded::start(
        Plan::parse(TWO, eph, Places::standard()).unwrap(),
    ));
    app.tick(0.0);
    app.plan.as_mut().unwrap().wait();
    app.tick(0.0);
    let span = app.plan.as_ref().unwrap().span();
    assert!(app.timeline.fitted);
    assert!(app.timeline.fraction(span.0) > 0.0 && app.timeline.fraction(span.1) < 1.0);
}

/// The timeline's CPU time with the published plan (18 ships) at
/// 1920×1080: building its elements, and the UI's build and layout
/// (`App::build` and damascene's `prepare`) with it, the clock stepping
/// an hour a frame (`cargo test --release -p sandbox --test timeline --
/// --ignored --nocapture`).
#[test]
#[ignore]
fn timeline_cpu_time() {
    let gpu = render::offscreen::Offscreen::new().expect("a GPU");
    let (device, queue) = (&gpu.device, &gpu.queue);
    let eph = Ephemeris::standard();
    let mut plan = Loaded::open(&sandbox::data_dir().join("missions/published.toml")).unwrap();
    plan.wait();
    let mut app = Shell::new(Epoch::from_ymd(2045, 3, 1));
    app.plan = Some(plan);
    app.poll();
    let ships = app.plan.as_ref().unwrap().ships.clone();
    let items: usize = ships.iter().map(|s| s.items.len()).sum();
    let n = 300;
    let clock = std::time::Instant::now();
    for i in 0..n {
        let t = app.timeline.at(i as f64 / n as f64);
        let windows = Some(&app.plan.as_ref().unwrap().windows);
        std::hint::black_box(timeline::strip(
            eph,
            &app.timeline,
            1700.0,
            &ships,
            windows,
            t,
            None,
            None,
        ));
    }
    let build = clock.elapsed().as_secs_f64() * 1e3 / n as f64;

    let (w, h) = (1920u32, 1080u32);
    let mut ui = Runner::new(device, queue, frame::OFFSCREEN_FORMAT);
    ui.set_animation_mode(damascene_core::AnimationMode::Settled);
    ui.set_theme(app.theme());
    ui.set_surface_size(w, h);
    let viewport = Rect::new(0.0, 0.0, w as f32, h as f32);
    let theme = app.theme();
    let mut times = Vec::new();
    for i in 0..n {
        app.clock
            .seek(Epoch::from_ymd(2045, 3, 1).plus_days(i as f64 / 24.0));
        app.poll();
        let clock = std::time::Instant::now();
        let tree = {
            let cx = BuildCx::new(&theme)
                .with_ui_state(ui.ui_state())
                .with_viewport(viewport.w, viewport.h);
            app.build(&cx)
        };
        ui.prepare(device, queue, tree, viewport, 1.0);
        times.push(clock.elapsed().as_secs_f64() * 1e3);
    }
    times.sort_by(f64::total_cmp);
    println!(
        "{} ships, {items} items: the timeline's elements built in {build:.3} ms; \
         the UI's build and layout median {:.2} ms, 95% {:.2} ms",
        ships.len(),
        times[n / 2],
        times[n * 95 / 100]
    );
}
