//! A plan in the sandbox (M1c.md, step 4 plan, items 4, 5 and 7):
//! designed in the background, its paths drawn in each frame along its
//! tracks, ships riding them, and events where they happen.

use orbit::ephem::Ephemeris;
use orbit::mission::{ItemKind, Plan};
use orbit::places::Places;
use orbit::time::Epoch;
use sandbox::app::Shell;
use sandbox::frames::Frame;
use sandbox::paths::{self, PLAN_SET, VIEW_POSE};
use sandbox::plan::{Loaded, on_track};

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

fn designed() -> Loaded {
    let eph = Ephemeris::standard();
    let mut plan = Loaded::start(Plan::parse(QUICK, eph, Places::standard()).unwrap());
    plan.wait();
    assert!(plan.done());
    plan
}

/// In every frame, each item's line follows its track between samples:
/// mapped into the frame at the samples' times and back to the world now,
/// the curve stays within 100 m of the track (the interpolation in a
/// turning frame; the track's own is within centimeters in low orbit).
#[test]
fn lines_follow_tracks_in_every_frame() {
    let eph = Ephemeris::standard();
    let plan = designed();
    let ship = &plan.ships[0];
    let kinds: Vec<&ItemKind> = ship.items.iter().map(|i| &i.kind).collect();
    assert_eq!(kinds, [&ItemKind::Stay, &ItemKind::Leg], "{kinds:?}");
    let moon = eph.lookup("moon").unwrap();
    for (id, _, frame) in Frame::offered(eph) {
        let built = paths::build(eph, &frame, moon, &[], &plan.ships);
        assert_eq!(built.paths.len(), 2, "{id}");
        let mut worst: f64 = 0.0;
        for (path, owner) in built.paths.iter().zip(&built.owners) {
            let item = &ship.items[owner.unwrap().1];
            let track = item.track.as_ref().unwrap();
            for k in 1..400 {
                let t = Epoch(item.start.0 + (item.end.0 - item.start.0) * f64::from(k) / 400.0);
                let pose = frame.pose(eph, moon, t);
                let on_line = pose.to_world(path.at(paths::seconds(t)).unwrap());
                worst = worst.max((on_line - on_track(eph, track, t).unwrap()).length());
            }
        }
        println!("{id}: lines within {:.1} m of the tracks", worst * 1e3);
        assert!(worst < 0.1, "{id}: {worst} km");
    }
}

/// The ship's marker sits on its line in a turning frame, and the leg's
/// two burns are drawn where the line is at their times.
#[test]
fn ship_and_burns_on_the_line() {
    let eph = Ephemeris::standard();
    let mut app = Shell::new(Epoch::from_ymd(2045, 3, 5));
    app.plan = Some(designed());
    app.frame = Frame::offered(eph)[1].2;
    app.update_paths();
    let leg = app.plan.as_ref().unwrap().ships[0].items[1].clone();
    let t = Epoch(0.3 * leg.start.0 + 0.7 * leg.end.0);
    app.clock.seek(t);
    let scene = app.scene(1.0);
    let view = scene.poses[VIEW_POSE];
    let paths = &scene.paths[PLAN_SET];
    let line = paths.paths.last().unwrap();
    let on_line = view.to_world(line.at(paths::seconds(t)).unwrap());
    let ship = scene
        .markers
        .iter()
        .find(|m| m.shape == render::Shape::Disc)
        .expect("the ship is drawn");
    assert!(
        (ship.position - on_line).length() < 1e-6,
        "{}",
        (ship.position - on_line).length()
    );
    // And where the track puts it, within the turning frame's
    // interpolation.
    let tracked = on_track(eph, leg.track.as_ref().unwrap(), t).unwrap();
    assert!(
        (ship.position - tracked).length() < 1e-3,
        "{} km",
        (ship.position - tracked).length()
    );
    let burns: Vec<&render::Marker> = scene
        .markers
        .iter()
        .filter(|m| m.shape == render::Shape::Diamond)
        .collect();
    assert_eq!(burns.len(), 2);
    for (burn, m) in burns.iter().zip(&leg.markers) {
        let pose = app.frame.pose(eph, app.camera.focus, m.t);
        let at = view
            .to_world(pose.from_world(on_track(eph, leg.track.as_ref().unwrap(), m.t).unwrap()));
        // The arrival burn is at the line's end, within rounding.
        let end = line.samples.last().unwrap().t;
        let drawn = view.to_world(line.at(paths::seconds(m.t).min(end)).unwrap());
        println!(
            "burn at {}: marker {:.3} m from the line",
            m.t,
            (burn.position - drawn).length() * 1e3
        );
        assert!((burn.position - at).length() < 1e-6 && (burn.position - drawn).length() < 1e-3);
    }
    // Before the plan's start, the ship is not drawn.
    app.clock.seek(Epoch(leg.start.0 - 10.0));
    assert!(
        !app.scene(1.0)
            .markers
            .iter()
            .any(|m| m.shape == render::Shape::Disc)
    );
}

/// Focused on a small body (its orbit known from 2040) with a plan before
/// 2040, the paths leave out what the frame cannot place, and the clock
/// stays where the focus is known (found in review: both panicked).
#[test]
fn small_body_focus_before_2040() {
    let eph = Ephemeris::standard();
    let early = QUICK.replace("2045-03-05", "2039-12-30");
    let mut plan = Loaded::start(Plan::parse(&early, eph, Places::standard()).unwrap());
    plan.wait();
    assert!(plan.ships[0].failure.is_none());
    let bennu = eph.lookup("bennu").unwrap();
    let start = eph.span_of(bennu).0;
    let mut app = Shell::new(Epoch(start.0 + 1.0));
    app.plan = Some(plan);
    app.camera.focus_on(eph, bennu, app.clock.t);
    app.follow_focus();
    app.update_paths();
    let scene = app.scene(1.0);
    let loops = scene.paths[PLAN_SET]
        .paths
        .iter()
        .filter(|p| p.pose == paths::LOOP_POSE)
        .count();
    assert_eq!(scene.paths[PLAN_SET].paths.len(), loops, "no ship paths");
    app.clock.seek(Epoch::from_ymd(2035, 1, 1));
    assert_eq!(app.clock.t, start);
    app.tick(0.1);
    let _ = app.scene(1.0);
}

/// The published plan's paths, measured (run with `--release --ignored`;
/// designing takes ~20 s): GPU time in the Earth–Moon frame from above,
/// and around the Sun.
#[test]
#[ignore]
fn published_plan_gpu_time() {
    let eph = Ephemeris::standard();
    let path = format!(
        "{}/../../data/missions/published.toml",
        env!("CARGO_MANIFEST_DIR")
    );
    let mut plan = Loaded::open(std::path::Path::new(&path)).unwrap();
    plan.wait();
    println!("{}", plan.progress());
    let gpu = render::offscreen::Offscreen::new().expect("a GPU");
    let mut renderer = render::Renderer::new(&gpu.device, &gpu.queue);
    let mut app = Shell::new(Epoch::from_ymd(2045, 3, 14));
    app.plan = Some(plan);
    let id = |s: &str| eph.lookup(s).unwrap();
    for (name, focus, frame, date, altitude) in [
        (
            "Earth–Moon frame, from 700,000 km",
            id("earth"),
            1,
            (2045, 3, 14),
            7e5,
        ),
        (
            "around the Sun, from 2.2 au",
            eph.sun(),
            0,
            (2033, 8, 1),
            3.3e8,
        ),
    ] {
        app.frame = Frame::offered(eph)[frame].2;
        app.clock.seek(Epoch::from_ymd(date.0, date.1, date.2));
        let mut camera = sandbox::camera::MapCamera::new(eph, focus);
        camera.set_altitude(eph, altitude);
        camera.set_direction(-1.6, 1.5);
        app.camera = camera;
        app.follow_focus();
        app.update_paths();
        let scene = app.scene(1.0);
        let samples: usize = scene.paths[PLAN_SET]
            .paths
            .iter()
            .map(|p| p.samples.len())
            .sum();
        match gpu.time_scene(&mut renderer, &scene, (1920, 1080)) {
            Some(ms) => println!("{name}: GPU {ms:.2} ms at 1920×1080, {samples} samples"),
            None => println!("no timestamps"),
        }
    }
}
