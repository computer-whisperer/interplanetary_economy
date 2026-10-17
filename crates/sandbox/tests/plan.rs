//! A plan in the sandbox (M1c.md, step 4 plan, items 4, 5 and 7; step 7
//! plan, item 2): designed in the background, its paths drawn in each
//! frame along its tracks, ships riding them, and events where they
//! happen; a ship edited designed again from the middle.

use orbit::ephem::Ephemeris;
use orbit::mission::{Item, ItemKind, Plan, Step};
use orbit::places::Places;
use orbit::time::Epoch;
use sandbox::app::Shell;
use sandbox::frames::Frame;
use sandbox::labels::Target;
use sandbox::paths::{self, SHIP_SETS, VIEW_POSE};
use sandbox::plan::{Loaded, on_track};
use std::sync::Arc;

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
    let mut plan = Loaded::start(Plan::parse(QUICK, eph, Places::standard()).unwrap()).unwrap();
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
        let built = paths::build(eph, &frame, moon, 0, ship);
        assert_eq!(built.set.paths.len(), 2, "{id}");
        let mut worst: f64 = 0.0;
        for (path, &k) in built.set.paths.iter().zip(&built.items) {
            let item = &ship.items[k];
            let track = item.track.as_ref().unwrap();
            for k in 1..400 {
                let t = Epoch(item.start.0 + (item.end.0 - item.start.0) * f64::from(k) / 400.0);
                let pose = frame.pose(eph, moon, t);
                let on_line = pose.to_world(path.at(t.seconds()).unwrap());
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
    let line = scene.paths[SHIP_SETS].paths.last().unwrap();
    let on_line = view.to_world(line.at(t.seconds()).unwrap());
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
        // The burns are at the line's ends, within rounding (the item's
        // times and its track's come from different epochs: the departure
        // fell 3e-7 s before the first sample once the Moon's pole moved).
        let (start, end) = (line.samples[0].t, line.samples.last().unwrap().t);
        let drawn = view.to_world(line.at(m.t.seconds().clamp(start, end)).unwrap());
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
    let mut plan = Loaded::start(Plan::parse(&early, eph, Places::standard()).unwrap()).unwrap();
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
    let ships = &scene.paths[SHIP_SETS..app.orbit_sets()];
    assert_eq!(ships.len(), 1);
    assert!(ships[0].paths.is_empty(), "no ship paths");
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
        let samples: usize = scene.paths[SHIP_SETS..app.orbit_sets()]
            .iter()
            .flat_map(|set| &set.paths)
            .map(|p| p.samples.len())
            .sum();
        match gpu.time_scene(&mut renderer, &scene, (1920, 1080)) {
            Some(ms) => println!("{name}: GPU {ms:.2} ms at 1920×1080, {samples} samples"),
            None => println!("no timestamps"),
        }
    }
}

/// Down from the NRHO to the south pole and back to low lunar orbit, and
/// a second ship that stays as it is.
const SORTIE: &str = r#"
name = "Sortie"

[[ship]]
name = "Lander"
start = { at = "nrho", epoch = "2045-03-05" }

[[ship.leg]]
to = "llo"
days = [0.5]

[[ship.leg]]
to = "moon-south-pole"

[[ship.leg]]
wait = 1

[[ship.leg]]
to = "llo"

[[ship]]
name = "Other"
start = { at = "nrho", epoch = "2045-03-06" }

[[ship.leg]]
to = "llo"
days = [0.5]
"#;

/// Items alike, to rounding.
fn assert_same(a: &[Item], b: &[Item]) {
    assert_eq!(a.len(), b.len());
    for (x, y) in a.iter().zip(b) {
        assert_eq!((x.step, &x.kind, &x.label), (y.step, &y.kind, &y.label));
        assert!((x.start.0 - y.start.0).abs() < 1e-9 && (x.end.0 - y.end.0).abs() < 1e-9);
        assert!(
            (x.dv - y.dv).abs() < 1e-9,
            "{}: {} against {}",
            x.label,
            x.dv,
            y.dv
        );
    }
}

/// A ship edited once designed, and one edited while still designing,
/// end as a fresh design of the edited plan; the other ship is left alone,
/// and what an older design sends is dropped.
#[test]
fn an_edited_ship_is_designed_again() {
    let eph = Ephemeris::standard();
    let parse = || Plan::parse(SORTIE, eph, Places::standard()).unwrap();
    let mut edited = parse();
    edited.ships[0].steps[2] = Step::Wait(3.0);
    let mut fresh = Loaded::start(edited.clone()).unwrap();
    fresh.wait();
    assert!(fresh.ships.iter().all(|s| s.failure.is_none()));

    let mut plan = Loaded::start(parse()).unwrap();
    plan.wait();
    let other = plan.ships[1].items.clone();
    let revisions = [plan.ships[0].revision, plan.ships[1].revision];
    plan.edit(0, edited.ships[0].clone());
    // Only the edited ship's lines are built again.
    assert!(plan.ships[0].revision != revisions[0]);
    assert_eq!(plan.ships[1].revision, revisions[1]);
    assert!(!plan.ships[0].done);
    assert!(
        plan.ships[0].items.iter().all(|i| i.step < 2),
        "the items before the edit are kept"
    );
    assert!(!plan.ships[0].items.is_empty());
    plan.wait();
    assert_same(&plan.ships[0].items, &fresh.ships[0].items);
    assert_same(&plan.ships[1].items, &other);

    // Edited at once, while its first design is under way.
    let mut plan = Loaded::start(parse()).unwrap();
    plan.edit(0, edited.ships[0].clone());
    plan.wait();
    assert_same(&plan.ships[0].items, &fresh.ships[0].items);
}

/// Each ship's lines are a set of their own: an edit builds the edited
/// ship's again and keeps the other's, and so does removing the last
/// ship; a ship moved by a removal is built again, its events by its new
/// place.
#[test]
fn an_edit_builds_its_own_ships_lines_alone() {
    let eph = Ephemeris::standard();
    let parse = || Plan::parse(SORTIE, eph, Places::standard()).unwrap();
    let shell = || {
        let mut plan = Loaded::start(parse()).unwrap();
        plan.wait();
        let mut app = Shell::new(Epoch::from_ymd(2045, 3, 6));
        app.plan = Some(plan);
        app.frame = Frame::offered(eph)[1].2;
        let moon = eph.lookup("moon").unwrap();
        app.camera = sandbox::camera::MapCamera::new(eph, moon);
        app.camera.set_altitude(eph, 40_000.0);
        app
    };
    let sets = |app: &mut Shell| {
        app.update_paths();
        app.scene(1.0).paths[SHIP_SETS..app.orbit_sets()].to_vec()
    };
    let mut app = shell();
    let before = sets(&mut app);
    assert_eq!(before.len(), 2);
    assert!(before.iter().all(|set| !set.paths.is_empty()));
    let mut edited = parse().ships[0].clone();
    edited.steps[2] = Step::Wait(3.0);
    app.plan.as_mut().unwrap().edit(0, edited);
    let after = sets(&mut app);
    assert!(
        !Arc::ptr_eq(&after[0], &before[0]),
        "the edited ship's built again"
    );
    assert!(Arc::ptr_eq(&after[1], &before[1]), "the other's kept");
    app.plan.as_mut().unwrap().remove_ship(1);
    let left = sets(&mut app);
    assert_eq!(left.len(), 1);
    assert!(
        Arc::ptr_eq(&left[0], &after[0]),
        "kept after the last's removal"
    );

    let mut app = shell();
    let before = sets(&mut app);
    app.plan.as_mut().unwrap().remove_ship(0);
    let left = sets(&mut app);
    assert_eq!(left.len(), 1);
    assert!(!Arc::ptr_eq(&left[0], &before[1]), "moved: built again");
    let scene = app.scene(1.0);
    app.place_labels(&scene, Some((1000, 700)), 1.0);
    let markers: Vec<usize> = app
        .anchors()
        .iter()
        .filter_map(|a| match a.target {
            Target::Marker { ship, .. } => Some(ship),
            _ => None,
        })
        .collect();
    assert!(
        !markers.is_empty(),
        "the remaining ship's markers are labeled"
    );
    assert!(markers.iter().all(|&s| s == 0), "{markers:?}");
}

/// The jumps between markers follow the plan with the ships' lines
/// hidden (found in review: their times stayed the old plan's).
#[test]
fn jumps_follow_the_plan_with_its_lines_hidden() {
    let mut app = Shell::new(Epoch::from_ymd(2045, 3, 5));
    app.settings.lines.ships = false;
    app.update_paths();
    assert!(app.event_times().is_empty());
    app.load(designed());
    app.update_paths();
    let times = app.plan.as_ref().unwrap().events();
    assert!(!times.is_empty());
    assert_eq!(app.event_times(), &times[..]);
}

/// A ship's start changed to a station, then its first leg edited: it is
/// designed from the new start (the old design's checkpoints dropped),
/// as a fresh design of the edited plan is.
#[test]
fn a_new_start_drops_the_old_checkpoints() {
    let eph = Ephemeris::standard();
    let src = SORTIE.replace(
        r#"start = { at = "nrho", epoch = "2045-03-05" }"#,
        r#"start = { at = "llo", epoch = "2045-03-05" }"#,
    );
    let mut plan = Loaded::start(Plan::parse(&src, eph, Places::standard()).unwrap()).unwrap();
    // From low lunar orbit, the first leg goes to low lunar orbit: change
    // it to the south pole's, so the plan designs.
    let mut ship = plan.plan().ships[0].clone();
    ship.steps.remove(0);
    plan.edit(0, ship);
    plan.wait();
    // The start to the NRHO, and the first leg there back.
    let mut fresh_plan = Plan::parse(SORTIE, eph, Places::standard()).unwrap();
    let mut ship = plan.plan().ships[0].clone();
    ship.start = fresh_plan.ships[0].start.clone();
    ship.steps.insert(0, fresh_plan.ships[0].steps[0].clone());
    plan.edit(0, ship.clone());
    plan.wait();
    // Then the first leg's durations: the edit that resumed from the old
    // start (stepping back from the station's checkpoint onto it).
    if let Step::Go(go) = &mut ship.steps[0] {
        go.days = vec![1.0];
    }
    plan.edit(0, ship.clone());
    plan.wait();
    fresh_plan.ships[0] = ship;
    let mut fresh = Loaded::start(fresh_plan).unwrap();
    fresh.wait();
    assert!(
        plan.ships[0].failure.is_none(),
        "{:?}",
        plan.ships[0].failure
    );
    assert_same(&plan.ships[0].items, &fresh.ships[0].items);
}
