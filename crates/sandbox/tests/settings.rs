//! The settings menu, through damascene as the window delivers input, in
//! the window's first size: the gear opens it, its sliders set the
//! departures from realism by whole stops, its switches the lines drawn,
//! and a click outside it or Escape closes it. Each lift reaches what it
//! names alone; each class of lines hidden is left out alone.

use damascene_core::prelude::*;
use damascene_wgpu::Runner;
use glam::DVec3;
use orbit::time::Epoch;
use render::offscreen::Offscreen;
use sandbox::app::Shell;
use sandbox::camera::MapCamera;
use sandbox::frame::{self, Output, View};
use orbit::ephem::Ephemeris;
use orbit::mission::Plan;
use orbit::places::Places;
use sandbox::frames::Frame;
use sandbox::labels::Target;
use sandbox::look::{MAX_LIFT, MAX_NIGHT_LIGHTS};
use sandbox::paths::{ORBIT_SET, PLAN_SET};
use sandbox::plan::Loaded;
use sandbox::settings::{self, CONTROLS, Lines, Settings, TOGGLES};

#[test]
fn the_menu_sets_the_lifts() {
    let gpu = render::offscreen::Offscreen::new().expect("a GPU");
    let (device, queue) = (&gpu.device, &gpu.queue);
    for scale in [1.0f32, 2.0] {
        let mut app = Shell::new(Epoch::from_ymd(2045, 3, 1));
        let (w, h) = (1280.0, 800.0);
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
            let scene = app.scene(scale);
            let out = Output {
                texture: &texture,
                view: &target,
                msaa: None,
            };
            frame::draw(device, queue, ui, app, &mut view, &scene, out, viewport, scale, None);
            scene
        };
        let deliver = |app: &mut Shell, ui: &Runner, events: Vec<UiEvent>| {
            let cx = EventCx::new().with_ui_state(ui.ui_state());
            for e in events {
                app.on_event(e, &cx);
            }
        };
        let click = |app: &mut Shell, ui: &mut Runner, (x, y): (f32, f32)| {
            let mut events = ui.pointer_down(Pointer::mouse(x, y, PointerButton::Primary));
            events.extend(ui.pointer_up(Pointer::mouse(x, y, PointerButton::Primary)));
            deliver(app, ui, events);
        };
        let center = |r: Rect| (r.x + r.w / 2.0, r.y + r.h / 2.0);

        let lifts = |look: &render::Look| [look.sky_lift, look.body_lift, look.night_lift];
        let scene = draw(&mut app, &mut ui);
        let exposure = scene.look.exposure;
        assert_eq!(lifts(&scene.look), Settings::default().lifts());
        let gear = ui.rect_of_key(settings::KEY).expect("the gear is laid out");
        assert!(gear.x + gear.w < w, "the gear is in the window: {gear:?}");
        click(&mut app, &mut ui, center(gear));
        assert!(app.settings_open, "scale {scale}: the gear opens the menu");
        draw(&mut app, &mut ui);
        for (k, c) in CONTROLS.iter().enumerate() {
            let slider = ui.rect_of_key(c.key).expect("the slider is laid out");
            assert!(slider.y + slider.h < h, "{}'s slider is in the window", c.name);
            println!("scale {scale}: {} {slider:?}", c.name);
            // Its ends, then its middle; each a whole stop, and its own.
            let middle = (c.max as f32 / 2.0).round() as i32;
            for (x, want) in [
                (slider.x + 1.0, 0),
                (slider.x + slider.w - 1.0, c.max),
                (slider.x + slider.w / 2.0, middle),
            ] {
                let mut others = app.settings;
                click(&mut app, &mut ui, (x, slider.y + slider.h / 2.0));
                *(c.field)(&mut others) = want;
                assert_eq!(app.settings, others, "scale {scale}, {} at x {x}", c.name);
                assert!(app.settings_open, "the slider leaves the menu open");
                let scene = draw(&mut app, &mut ui);
                assert_eq!(lifts(&scene.look)[k], 2f32.powi(want));
                assert_eq!(lifts(&scene.look), app.settings.lifts());
                assert_eq!(scene.look.exposure, exposure, "the lifts leave the scene's exposure");
            }
            // The arrow keys, on the slider focused by the clicks.
            let events = ui.key_down(
                LogicalKey::Named(NamedKey::ArrowRight),
                PhysicalKey::ArrowRight,
                KeyModifiers::default(),
                false,
            );
            deliver(&mut app, &ui, events);
            assert_eq!(*(c.field)(&mut app.settings), middle + 1, "{}", c.name);
        }
        // Each switch, off and on again, alone.
        for t in &TOGGLES {
            let switch = ui.rect_of_key(t.key).expect("the switch is laid out");
            assert!(switch.y + switch.h < h, "{}'s switch is in the window", t.name);
            for on in [false, true] {
                let mut others = app.settings.lines;
                *(t.field)(&mut others) = on;
                click(&mut app, &mut ui, center(switch));
                assert_eq!(app.settings.lines, others, "scale {scale}, {}", t.name);
                assert!(app.settings_open, "the switch leaves the menu open");
                draw(&mut app, &mut ui);
            }
        }

        // Outside it, on the map: closed, and the map takes no click.
        let focus = app.camera.focus;
        click(&mut app, &mut ui, (w / 2.0, h / 2.0));
        assert!(!app.settings_open, "a click outside closes the menu");
        assert_eq!(app.camera.focus, focus);
        draw(&mut app, &mut ui);
        click(&mut app, &mut ui, center(gear));
        assert!(app.settings_open);
        draw(&mut app, &mut ui);
        let events = ui.key_down(
            LogicalKey::Named(NamedKey::Escape),
            PhysicalKey::Escape,
            KeyModifiers::default(),
            false,
        );
        deliver(&mut app, &ui, events);
        assert!(!app.settings_open, "Escape closes the menu");
    }
}

/// With the sky left out, what each lift reaches, and nothing else (from
/// all at 0, true to life):
/// - the background lift: nothing here, pixel for pixel;
/// - the body lift: bodies' points alone, pixel for pixel away from them
///   (Earth with its air and the Moon's disk, the orbits' lines and the
///   station orbits' markers; Saturn with its rings), and by its stops
///   (Jupiter's point from beside Earth carries 2¹⁷ its flux at 17,
///   within 0.1%);
/// - the night lights: Earth's disk alone, its night side's lights by
///   their stops (what 10 and 20 stops add, over 2^stops − 1, the same
///   within 1%).
#[test]
fn each_lift_reaches_what_it_names() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = render::Renderer::new(&gpu.device, &gpu.queue);
    sandbox::maps::wait().expect("data/bodies reads");
    let size = (640, 400);
    let eph = orbit::ephem::Ephemeris::standard();
    let (sun, earth, saturn, jupiter) = (
        eph.sun(),
        eph.lookup("earth").unwrap(),
        eph.lookup("saturn").unwrap(),
        eph.lookup("jupiter").unwrap(),
    );
    let truth = Settings {
        background_lift: 0,
        body_lift: 0,
        night_lights: 0,
        ..Settings::default()
    };
    let mut shown = |t: Epoch, focus, camera: Option<render::Camera>, settings: Settings| {
        let mut app = Shell::new(t);
        app.camera = MapCamera::new(eph, focus);
        app.settings = settings;
        app.update_paths();
        let mut scene = app.scene(1.0);
        scene.sky = None;
        if camera.is_some() {
            scene.camera = camera;
        }
        let target = render::Target::new(&gpu.device, size.0, size.1);
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        renderer.render(&gpu.device, &gpu.queue, &mut encoder, &target, &scene, None);
        gpu.queue.submit(Some(encoder.finish()));
        (scene, gpu.read_rgba16f(&target.hdr))
    };
    let at = |i: usize| {
        glam::DVec2::new((i % size.0 as usize) as f64 + 0.5, (i / size.0 as usize) as f64 + 0.5)
    };
    // The pixels that differ, but for those within `reach` px of `near`.
    let differ = |a: &[[f32; 4]], b: &[[f32; 4]], near: &[glam::DVec2], reach: f64| {
        a.iter()
            .zip(b)
            .enumerate()
            .filter(|(i, (a, b))| a != b && near.iter().all(|p| p.distance(at(*i)) >= reach))
            .count()
    };

    let t = Epoch::from_ymd(2045, 3, 1);
    for (name, focus) in [("Earth", earth), ("Saturn", saturn)] {
        let (scene, true_view) = shown(t, focus, None, truth);
        let camera = scene.camera.unwrap();
        let pixel = 2.0 * (camera.fov_y / 2.0).tan() / f64::from(size.1);
        let radius_px = |s: &render::Sphere| {
            (s.radius / (s.center - camera.eye).length()).asin() / pixel
        };
        // Where bodies under 4 px in radius land (points, or fading from
        // them into their disks), and Earth's disk.
        let points: Vec<glam::DVec2> = scene
            .spheres
            .iter()
            .filter(|s| radius_px(s) < 4.0)
            .filter_map(|s| camera.project(s.center, size))
            .collect();
        let lit = true_view.iter().filter(|p| p[..3].iter().any(|&c| c > 0.0)).count();
        assert!(lit > 1000, "{name} shows little");
        let (_, sky) = shown(t, focus, None, Settings { background_lift: MAX_LIFT, ..truth });
        let (_, bodies) = shown(t, focus, None, Settings { body_lift: MAX_LIFT, ..truth });
        let (_, night) = shown(t, focus, None, Settings { night_lights: MAX_NIGHT_LIGHTS, ..truth });
        let earth_disk = scene
            .spheres
            .iter()
            .find(|s| (s.radius - eph.body(earth).radius).abs() < 1.0)
            .and_then(|s| Some((camera.project(s.center, size)?, radius_px(s))));
        let night_differ = match earth_disk {
            Some((c, r)) => differ(&true_view, &night, &[c], r + 2.0),
            None => differ(&true_view, &night, &[], 0.0),
        };
        let counts = (
            differ(&true_view, &sky, &[], 0.0),
            differ(&true_view, &bodies, &points, 8.0),
            night_differ,
        );
        println!(
            "{name}: {lit} pixels lit, {} points; differ: sky {}, bodies away from points {}, night lights off Earth {}",
            points.len(),
            counts.0,
            counts.1,
            counts.2
        );
        assert_eq!(counts, (0, 0, 0), "{name}");
    }

    // Jupiter near opposition, from beside Earth.
    let t = Epoch::from_ymd(2045, 9, 1);
    let pe = eph.barycentric_position(earth, t);
    let pj = eph.barycentric_position(jupiter, t);
    let eye = pe + (pj - pe).normalize() * 2e5;
    let camera = render::Camera::looking_at(eye, pj, DVec3::Z, 50f64.to_radians());
    let p = camera.project(pj, size).expect("Jupiter is in front");
    let (cx, cy) = (p.x as i64, p.y as i64);
    let flux = |image: &[[f32; 4]]| -> f64 {
        (cy - 4..=cy + 4)
            .flat_map(|y| (cx - 4..=cx + 4).map(move |x| (x, y)))
            .map(|(x, y)| f64::from(image[(y * i64::from(size.0) + x) as usize][1]))
            .sum()
    };
    let (_, true_view) = shown(t, earth, Some(camera), truth);
    let (_, lifted) = shown(t, earth, Some(camera), Settings { body_lift: 17, ..truth });
    let (_, sky) = shown(t, earth, Some(camera), Settings { background_lift: MAX_LIFT, ..truth });
    let ratio = flux(&lifted) / flux(&true_view);
    println!("Jupiter's point: {:.4e}, lifted {ratio:.1}×", flux(&true_view));
    assert!(flux(&true_view) > 0.0, "Jupiter's point is drawn");
    assert!((ratio / 2f64.powi(17) - 1.0).abs() < 1e-3, "{ratio}");
    assert_eq!(flux(&sky), flux(&true_view), "the sky's lift leaves bodies' points");

    // Earth's night side, from its antisolar side: what the lights add.
    let t = Epoch::from_ymd(2045, 3, 1);
    let pe = eph.barycentric_position(earth, t);
    let away = (pe - eph.barycentric_position(sun, t)).normalize();
    let side = away.cross(DVec3::Z).normalize();
    let eye = pe + (away + side * 0.2).normalize() * 30_000.0;
    let camera = render::Camera::looking_at(eye, pe, DVec3::Z, 50f64.to_radians());
    let total = |image: &[[f32; 4]]| -> f64 { image.iter().map(|p| f64::from(p[1])).sum() };
    let (_, dark) = shown(t, earth, Some(camera), truth);
    let added = |stops: i32, shown: &mut dyn FnMut(Settings) -> Vec<[f32; 4]>| {
        (total(&shown(Settings { night_lights: stops, ..truth })) - total(&dark))
            / (2f64.powi(stops) - 1.0)
    };
    let mut at_stops = |s: Settings| shown(t, earth, Some(camera), s).1;
    let (ten, twenty) = (added(10, &mut at_stops), added(20, &mut at_stops));
    println!("night lights: {ten:.4e} a unit at 10 stops, {twenty:.4e} at 20");
    assert!(ten > 0.0, "lights show on the night side");
    assert!((twenty / ten - 1.0).abs() < 0.01, "{ten} vs {twenty}");
}

/// From the NRHO to low lunar orbit: designed in a fraction of a second.
const QUICK: &str = r#"
name = "Quick"

[[ship]]
name = "Lander"
start = { at = "nrho", epoch = "2045-03-05" }

[[ship.leg]]
to = "llo"
days = [0.5]
"#;

/// Each class of lines hidden is left out alone, and shown again comes
/// back: the ships' paths with their events' markers and labels (the
/// ships' own markers stay), the station orbits with their labels, and
/// the bodies' orbits (`orbits`' own test checks each class). Near the
/// Moon, in the Earth–Moon frame, where the lander's events are; and
/// focused on Earth, where the station orbits are labeled.
#[test]
fn hidden_lines_are_left_out() {
    let eph = Ephemeris::standard();
    let mut plan = Loaded::start(Plan::parse(QUICK, eph, Places::standard()).unwrap());
    plan.wait();
    assert!(plan.done());
    let mut app = Shell::new(Epoch::from_ymd(2045, 3, 5));
    app.plan = Some(plan);
    app.frame = Frame::offered(eph)[1].2;
    let moon = eph.lookup("moon").unwrap();
    app.camera = MapCamera::new(eph, moon);
    app.camera.set_altitude(eph, 40_000.0);
    // The plan's paths, the bodies' orbits, the markers, and the events'
    // and station orbits' labels.
    let seen = |app: &mut Shell| {
        app.update_paths();
        let scene = app.scene(1.0);
        app.place_labels(&scene, Some((1000, 700)), 1.0);
        let labeled = |f: fn(&Target) -> bool| app.anchors().iter().filter(|a| f(&a.target)).count();
        [
            scene.paths[PLAN_SET].paths.len(),
            scene.paths[ORBIT_SET].paths.len(),
            scene.markers.len(),
            labeled(|t| matches!(t, Target::Marker { .. })),
            labeled(|t| matches!(t, Target::Loop(_))),
        ]
    };
    let lines = |f: fn(&mut Lines)| {
        let mut l = Lines::default();
        f(&mut l);
        l
    };
    let all = seen(&mut app);
    let ships = app.plan.as_ref().unwrap().ships.len();
    println!("near the Moon, all shown: {all:?}");
    assert!(all[..4].iter().all(|&n| n > 0), "{all:?}");

    app.settings.lines = lines(|l| l.ships = false);
    let no_ships = seen(&mut app);
    app.settings.lines = lines(|l| l.stations = false);
    let no_stations = seen(&mut app);
    app.settings.lines = lines(|l| (l.planets, l.moons, l.small_bodies) = (false, false, false));
    let no_orbits = seen(&mut app);
    println!("no ships' paths {no_ships:?}, no station orbits {no_stations:?}, no bodies' orbits {no_orbits:?}");
    // The plan's set holds the station loops, then the ships' paths.
    let loops = no_ships[0];
    assert!(loops > 0 && loops < all[0]);
    assert_eq!(no_ships, [loops, all[1], ships, 0, all[4]]);
    assert_eq!(no_stations, [all[0] - loops, all[1], all[2], all[3], 0]);
    assert_eq!(no_orbits, [all[0], 0, all[2], all[3], all[4]]);
    app.settings.lines = Lines::default();
    assert_eq!(seen(&mut app), all);

    // Focused on Earth by default, on a day its view shows them.
    let mut app = Shell::new(Epoch::from_ymd(2045, 3, 1));
    let all = seen(&mut app);
    app.settings.lines = lines(|l| l.stations = false);
    let no_stations = seen(&mut app);
    println!("focused on Earth: all {all:?}, no station orbits {no_stations:?}");
    assert!(all[4] > 0, "the station orbits are labeled");
    assert_eq!(no_stations[4], 0);
}
