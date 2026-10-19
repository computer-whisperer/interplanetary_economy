//! Mission plans flown (M1c.md, step 1): every plan in `data/missions/`
//! parses; a chain of legs is continuous in time and in space, where
//! drawn; a leg leaves a low orbit from the ship's own orbit there, or
//! reports the jump; an aerocapture ends circular in its low orbit; a plan
//! edited goes on from the middle as a fresh design of it would.

use glam::DVec3;
use orbit::ephem::Ephemeris;
use orbit::gravity::Field;
use orbit::integrate::{Dop853, Tolerance, Trajectory};
use orbit::mission::{
    Checkpoint, Item, ItemKind, MarkerKind, Pin, Plan, Planner, Step, World, check, legs_value,
    parse_legs, reads, resume_point,
};
use orbit::places::Places;
use orbit::time::{Epoch, SECONDS_PER_DAY};

fn plan(file: &str) -> Plan {
    let path = format!("{}/../../data/missions/{file}", env!("CARGO_MANIFEST_DIR"));
    let src = std::fs::read_to_string(&path).unwrap();
    Plan::parse(&src, Ephemeris::standard(), Places::standard()).unwrap()
}

fn fly(world: &World, plan: &Plan, ship: &str) -> Vec<Item> {
    let ship = plan.ships.iter().find(|s| s.name == ship).unwrap();
    let mut planner = Planner::new(world, ship).unwrap();
    let mut items = Vec::new();
    while let Some(batch) = planner.step() {
        items.extend(batch);
    }
    for i in &items {
        println!(
            "{:?} {} {:.1} m/s, {:.2} d",
            i.kind,
            i.label,
            i.dv * 1e3,
            i.end.0 - i.start.0
        );
    }
    items
}

/// An item's first or last sample, relative to the solar-system
/// barycenter.
fn end_of(eph: &Ephemeris, item: &Item, last: bool) -> Option<DVec3> {
    let track = item.track.as_ref()?;
    let (t, s) = if last { track.last() } else { track.first() }?;
    let at = track.epoch.plus_days(t / SECONDS_PER_DAY);
    let center = track
        .center
        .map_or(DVec3::ZERO, |c| eph.barycentric(c, at).r);
    Some(DVec3::new(s[0], s[1], s[2]) + center)
}

/// Items follow each other in time, and drawn ones meet in space.
fn assert_continuous(eph: &Ephemeris, items: &[Item], km: f64) {
    for w in items.windows(2) {
        let gap_days = (w[1].start.0 - w[0].end.0).abs();
        assert!(
            gap_days < 1e-9,
            "{} → {}: {gap_days} days apart",
            w[0].label,
            w[1].label
        );
        if let (Some(a), Some(b)) = (end_of(eph, &w[0], true), end_of(eph, &w[1], false)) {
            // A stay on a surface or at a body is drawn at its center.
            let still = |i: &Item| i.track.as_ref().unwrap().rests();
            if still(&w[0]) || still(&w[1]) {
                continue;
            }
            let gap = (a - b).length();
            assert!(gap < km, "{} → {}: {gap} km apart", w[0].label, w[1].label);
        }
    }
}

/// A plan written out reads back as the same plan, and writes out the
/// same again.
fn assert_round_trip(p: &Plan) {
    let text = p.to_toml();
    let back = Plan::parse(&text, Ephemeris::standard(), Places::standard())
        .unwrap_or_else(|e| panic!("{e}\n{text}"));
    assert_eq!(back.to_toml(), text);
    assert_eq!(back.name, p.name);
    assert_eq!(back.ships.len(), p.ships.len());
    for (a, b) in p.ships.iter().zip(&back.ships) {
        assert_eq!(
            (a.id, &a.name, &a.start.label, a.epoch),
            (b.id, &b.name, &b.start.label, b.epoch)
        );
        assert_eq!(a.steps.len(), b.steps.len());
        for (x, y) in a.steps.iter().zip(&b.steps) {
            match (x, y) {
                (Step::Wait(x), Step::Wait(y)) => assert_eq!(x, y),
                (Step::Go(x), Step::Go(y)) => {
                    assert_eq!(
                        (&x.to.label, &x.days, x.window, x.pin, x.aero, &x.note),
                        (&y.to.label, &y.days, y.window, y.pin, y.aero, &y.note)
                    );
                }
                _ => panic!("{x:?} read back as {y:?}"),
            }
        }
    }
}

/// Ships without an id get the next ones after the largest given, and
/// two ships may not share one. The sim engine's tables come back as
/// they were written, and legs written as a value read back as the same
/// steps.
#[test]
fn ids_and_the_engines_tables() {
    let src = r#"
        name = "ids"
        [game]
        mode = "career"
        [[ship]]
        name = "a"
        start = { at = "leo", epoch = "2045-03-01" }
        [[ship]]
        id = 7
        name = "b"
        start = { at = "leo", epoch = "2045-03-01" }
        [[ship.leg]]
        to = "nrho"
        days = [5.0]
        [[ship.leg]]
        wait = 2
        [[ship]]
        name = "a"
        start = { at = "leo", epoch = "2045-03-01" }
        [[replan]]
        ship = 7
        at = "2045-03-02T00:00"
    "#;
    let (eph, places) = (Ephemeris::standard(), Places::standard());
    let plan = Plan::parse(src, eph, places).unwrap();
    let ids: Vec<u64> = plan.ships.iter().map(|s| s.id).collect();
    assert_eq!(ids, [8, 7, 9]);
    assert_eq!(plan.next_id(), 10);
    assert_eq!(plan.game.as_ref().unwrap()["mode"].as_str(), Some("career"));
    assert_eq!(plan.replans.len(), 1);
    assert_round_trip(&plan);
    let back = Plan::parse(&plan.to_toml(), eph, places).unwrap();
    assert_eq!(back.game, plan.game);
    assert_eq!(back.replans, plan.replans);

    let steps = &plan.ships[1].steps;
    let again = parse_legs(legs_value(steps), eph, places).unwrap();
    assert_eq!(format!("{again:?}"), format!("{steps:?}"));
    let bad = toml::Value::try_from(vec![toml::toml! { wait = -1 }]).unwrap();
    assert_eq!(
        parse_legs(bad, eph, places).unwrap_err(),
        "leg 1: a wait is a number of days, not negative"
    );

    let dup = src.replace(
        "[[ship]]\n        name = \"a\"",
        "[[ship]]\n        id = 7\n        name = \"a\"",
    );
    assert_eq!(
        Plan::parse(&dup, eph, places).unwrap_err(),
        "two ships have id 7"
    );
}

#[test]
fn every_plan_parses() {
    let dir = format!("{}/../../data/missions", env!("CARGO_MANIFEST_DIR"));
    let mut n = 0;
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let src = std::fs::read_to_string(&path).unwrap();
        let p = Plan::parse(&src, Ephemeris::standard(), Places::standard())
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert!(!p.ships.is_empty());
        assert_round_trip(&p);
        for ship in &p.ships {
            assert_eq!(check(Ephemeris::standard(), ship), [], "{}", ship.name);
        }
        n += 1;
    }
    assert!(n >= 2);
    // Errors name the ship and leg.
    let bad = "name = \"x\"\n[[ship]]\nname = \"s\"\nstart = { at = \"leo\", epoch = \"2045-03-01\" }\n\
               [[ship.leg]]\nto = \"nowhere\"\n";
    let e = Plan::parse(bad, Ephemeris::standard(), Places::standard()).unwrap_err();
    assert!(
        e.contains("ship \"s\", leg 1") && e.contains("nowhere"),
        "{e}"
    );
    // Windows open before they close; waits and durations are positive.
    for leg in [
        "to = \"nrho\"\ndays = [5]\nwindow = [\"2045-03-02\", \"2045-03-01\"]",
        "wait = -1",
        "wait = nan",
        "to = \"nrho\"\ndays = [0]",
        "to = \"nrho\"\ndays = [inf]",
    ] {
        let bad = format!(
            "name = \"x\"\n[[ship]]\nname = \"s\"\nstart = {{ at = \"leo\", epoch = \"2045-03-01\" }}\n\
             [[ship.leg]]\n{leg}\n"
        );
        assert!(
            Plan::parse(&bad, Ephemeris::standard(), Places::standard()).is_err(),
            "{leg}"
        );
    }
}

/// The crew rotation: LEO, the NRHO, a sortie to the south pole and back,
/// home by aerocapture. Every item flies, end to end.
#[test]
fn crew_rotation() {
    let world = World::standard();
    let eph = world.eph;
    let items = fly(&world, &plan("gateway-rotation.toml"), "Crew vehicle");
    assert!(!items.iter().any(|i| matches!(i.kind, ItemKind::Failed(_))));
    let kinds: Vec<&ItemKind> = items
        .iter()
        .filter(|i| i.kind != ItemKind::Stay)
        .map(|i| &i.kind)
        .collect();
    assert_eq!(
        kinds,
        [
            &ItemKind::Leg,
            &ItemKind::Leg,
            &ItemKind::Landing,
            &ItemKind::Ascent,
            &ItemKind::Leg,
            &ItemKind::Leg,
            &ItemKind::Capture
        ]
    );
    // Within a few percent of M1a's designs of the same legs.
    let legs: Vec<f64> = items
        .iter()
        .filter(|i| i.kind == ItemKind::Leg)
        .map(|i| i.dv * 1e3)
        .collect();
    for (got, want) in legs.iter().zip([3_485.0, 680.0, 700.0, 510.0]) {
        assert!((got / want - 1.0).abs() < 0.05, "{legs:?}");
    }
    // A low orbit left after an ascent has a free plane: no junction.
    assert_eq!(jumps(&items), 0);
    // 7 m: the stay in low lunar orbit, coasted back from the departure
    // and forward again over 8 days.
    assert_continuous(eph, &items, 0.05);
}

/// Arriving in a low orbit and leaving it again, the leg after leaves
/// from the ship's own orbit (6.9° from the plane it picks when free): at
/// once, paying for the plane, continuous with the stay before it.
#[test]
fn leaving_low_orbit_from_its_own_orbit() {
    let world = World::standard();
    let items = fly(
        &world,
        &plan("published.toml"),
        "Low lunar orbit round trip",
    );
    assert!(!items.iter().any(|i| matches!(i.kind, ItemKind::Failed(_))));
    assert_eq!(jumps(&items), 0);
    let back = items.iter().rfind(|i| i.kind == ItemKind::Leg).unwrap();
    let note = back.note.as_deref().unwrap();
    assert!(note.starts_with("from its own orbit, "), "{note}");
    assert_continuous(world.eph, &items, 1e-3);
}

/// Jumps in a low orbit the planner reports.
fn jumps(items: &[Item]) -> usize {
    items
        .iter()
        .flat_map(|i| &i.markers)
        .filter(|m| matches!(m.kind, MarkerKind::Jump { .. }))
        .count()
}

/// Leaving low lunar orbit as the ship arrives, the leg's free plane is
/// 166° from the ship's (it goes the other way): it waits in its orbit
/// for the planes to align. Between planets the date is not moved for the
/// plane: leaving LEO for Mars as the ship arrives there, 93° from its
/// plane, costs too much, and the jump is reported (found in review: the
/// jump had gone with the empty stay before it).
#[test]
fn waiting_for_the_plane_or_jumping() {
    let src = r#"
        name = "leaving on arrival"
        [[ship]]
        name = "moon, then Mars"
        start = { at = "leo", epoch = "2033-03-25" }
        [[ship.leg]]
        to = "llo"
        days = [3.5]
        [[ship.leg]]
        to = "leo"
        days = [3.5]
        [[ship.leg]]
        to = { low = "mars", altitude = 400 }
        window = ["2033-04-01", "2033-06-01"]
        days = [178]
    "#;
    let plan = Plan::parse(src, Ephemeris::standard(), Places::standard()).unwrap();
    let world = World::standard();
    let items = fly(&world, &plan, "moon, then Mars");
    let legs: Vec<&Item> = items.iter().filter(|i| i.kind == ItemKind::Leg).collect();
    assert_eq!(legs.len(), 3, "every leg flew");
    // Ready on arrival in low lunar orbit, it left 18.3 days later.
    let waited = legs[1].start.0 - legs[0].end.0;
    assert!(waited > 10.0, "{waited} days");
    let note = legs[1].note.as_deref().unwrap();
    assert!(note.contains("days later, when the planes align"), "{note}");
    let to_leo = items.iter().position(|i| std::ptr::eq(i, legs[1])).unwrap();
    assert_eq!(jumps(&items[..=to_leo]), 0);
    assert_continuous(world.eph, &items[..=to_leo], 1e-3);
    // The Mars leg leaves as the ship reaches LEO, and jumps.
    assert_eq!(legs[2].start.0, legs[1].end.0);
    assert_eq!(jumps(std::slice::from_ref(legs[2])), 1);
    let note = legs[2].note.as_deref().unwrap();
    assert!(note.starts_with("from its own orbit: "), "{note}");
}

/// LEO to Mars by aerocapture on the handbook's 2033 dates: the ship
/// coasts from the pass to the low orbit's radius and circularizes there,
/// in Mars's frame, continuous with the leg in the barycentric frame.
#[test]
fn aerocapture_at_mars() {
    let world = World::standard();
    let eph = world.eph;
    let items = fly(&world, &plan("published.toml"), "Mars 2033 by aerocapture");
    let capture = items.iter().find(|i| i.kind == ItemKind::Capture).unwrap();
    let track = capture.track.as_ref().unwrap();
    let mars = eph.lookup("mars").unwrap();
    assert_eq!(track.center, Some(mars));
    let (_, s) = track.last().unwrap();
    let r = DVec3::new(s[0], s[1], s[2]).length();
    let want = eph.body(mars).radius + 400.0;
    assert!(
        (r - want).abs() < 10.0,
        "circularized at {r} km, not {want}"
    );
    let burn = capture.markers[0].dv * 1e3;
    assert!((burn - 87.0).abs() < 5.0, "{burn} m/s");
    assert_continuous(eph, &items, 1e-3);
    assert!(capture.start.0 > Epoch::from_ymd(2033, 9, 30).0);
}

/// A leg to a small body before its orbit is known (from 2040) fails with
/// the reason (found in M1c step 4's review: the planner asserted).
#[test]
fn small_body_before_its_orbit_is_known() {
    let src = r#"
name = "Early"

[[ship]]
name = "Too early"
start = { at = "leo", epoch = "2035-01-01" }

[[ship.leg]]
to = "2000-sg344"
"#;
    let plan = Plan::parse(src, Ephemeris::standard(), Places::standard()).unwrap();
    let items = fly(&World::standard(), &plan, "Too early");
    match &items[..] {
        [
            Item {
                kind: ItemKind::Failed(why),
                ..
            },
        ] => assert!(why.contains("known from 2040-01-01"), "{why}"),
        other => panic!("{other:?}"),
    }
}

/// Down from the NRHO to the south pole and back to low lunar orbit.
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
"#;

/// Every item, and the checkpoint before each step.
fn fly_with_checkpoints(
    world: &World,
    ship: &orbit::mission::Ship,
) -> (Vec<Item>, Vec<Checkpoint>) {
    let mut planner = Planner::new(world, ship).unwrap();
    let (mut items, mut checkpoints) = (Vec::new(), vec![planner.checkpoint()]);
    while let Some(batch) = planner.step() {
        items.extend(batch);
        checkpoints.push(planner.checkpoint());
    }
    (items, checkpoints)
}

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

/// The wait on the surface lengthened: going on from the checkpoint
/// before it gives what designing the edited plan afresh gives; each item
/// names its step.
#[test]
fn an_edit_goes_on_from_the_middle() {
    let world = World::standard();
    let plan = Plan::parse(SORTIE, world.eph, Places::standard()).unwrap();
    let ship = &plan.ships[0];
    let (items, checkpoints) = fly_with_checkpoints(&world, ship);
    let steps: Vec<usize> = items.iter().map(|i| i.step).collect();
    let kinds: Vec<&ItemKind> = items.iter().map(|i| &i.kind).collect();
    println!("{steps:?} {kinds:?}");
    assert!(
        items
            .iter()
            .all(|i| matches!(ship.steps[i.step], Step::Go(_)))
    );
    assert_eq!(
        checkpoints.iter().map(Checkpoint::next).collect::<Vec<_>>(),
        [0, 1, 2, 4]
    );

    let mut edited = ship.clone();
    edited.steps[2] = Step::Wait(3.0);
    let from = resume_point(ship, &checkpoints, 2).unwrap();
    assert_eq!(from.next(), 2, "after the landing: on the surface");
    let mut planner = Planner::resume(&world, &edited, from.clone());
    let mut resumed: Vec<Item> = items.iter().filter(|i| i.step < 2).cloned().collect();
    while let Some(batch) = planner.step() {
        resumed.extend(batch);
    }
    let (fresh, _) = fly_with_checkpoints(&world, &edited);
    assert_same(&resumed, &fresh);
    let stay = fresh
        .iter()
        .find(|i| i.label == "at moon-south-pole")
        .unwrap();
    assert!((stay.end.0 - stay.start.0 - 3.0).abs() < 1e-9);

    // An edit to the leg leaving a station goes on from the station: its
    // orbit is extended as the leg needs (M1b.md, step 3 plan, part 2).
    assert_eq!(
        resume_point(ship, &checkpoints, 0).map(Checkpoint::next),
        Some(0)
    );
    assert_eq!(
        resume_point(ship, &checkpoints, 1).map(Checkpoint::next),
        Some(1)
    );
}

/// A leg pinned (`pin`) flies at its date and duration, as the published
/// leg pinned by a window of one day does; pinned before the ship is
/// ready, or a departure within a system, it is refused.
#[test]
fn a_pinned_departure_flies_then() {
    let world = World::standard();
    let published = plan("published.toml");
    let windowed = fly(&world, &published, "Mars 2031, Type I");
    let src = r#"
name = "Pinned"

[[ship]]
name = "Mars 2031, Type I"
start = { at = "leo", epoch = "2031-01-28" }

[[ship.leg]]
to = { low = "mars", altitude = 400 }
pin = { depart = "2031-01-28", days = 190 }
"#;
    let pinned = Plan::parse(src, world.eph, Places::standard()).unwrap();
    assert_round_trip(&pinned);
    let items = fly(&world, &pinned, "Mars 2031, Type I");
    let (a, b) = (
        windowed.iter().find(|i| i.kind == ItemKind::Leg).unwrap(),
        items.iter().find(|i| i.kind == ItemKind::Leg).unwrap(),
    );
    assert_eq!(b.start, Epoch::from_ymd(2031, 1, 28));
    assert!((a.dv - b.dv).abs() < 1e-9 && (a.end.0 - b.end.0).abs() < 1e-9);

    for (leg, why) in [
        (
            "to = { low = \"mars\", altitude = 400 }\npin = { depart = \"2031-01-27\" }",
            "before the ship is ready",
        ),
        (
            "to = \"nrho\"\ndays = [5]\npin = { depart = \"2031-01-29\" }",
            "between planets",
        ),
    ] {
        let src = format!(
            "name = \"x\"\n[[ship]]\nname = \"s\"\nstart = {{ at = \"leo\", epoch = \"2031-01-28\" }}\n\
             [[ship.leg]]\n{leg}\n"
        );
        let p = Plan::parse(&src, world.eph, Places::standard()).unwrap();
        match &fly(&world, &p, "s")[..] {
            [
                Item {
                    kind: ItemKind::Failed(e),
                    ..
                },
            ] => assert!(e.contains(why), "{e}"),
            other => panic!("{other:?}"),
        }
    }
}

/// Legs that cannot be flown are found without designing, each with the
/// planner's own words, which it says on reaching them.
#[test]
fn checks_say_what_the_planner_says() {
    let world = World::standard();
    let src = r#"
name = "Bad"

[[ship]]
name = "s"
start = { at = "leo", epoch = "2045-03-01" }

[[ship.leg]]
to = "moon-south-pole"

[[ship.leg]]
to = "nrho"

[[ship.leg]]
to = { low = "mars", altitude = 400 }
"#;
    let plan = Plan::parse(src, world.eph, Places::standard()).unwrap();
    let problems = check(world.eph, &plan.ships[0]);
    println!("{problems:?}");
    assert_eq!(problems.iter().map(|p| p.0).collect::<Vec<_>>(), [0, 1, 2]);
    assert!(problems[0].1.contains("landing from a low orbit"));
    assert!(problems[1].1.contains("left by ascent"));
    assert!(problems[2].1.contains("station orbit"));
    match &fly(&world, &plan, "s")[..] {
        [
            Item {
                kind: ItemKind::Failed(why),
                ..
            },
        ] => assert_eq!(why, &problems[0].1),
        other => panic!("{other:?}"),
    }
}

/// A pin a few seconds before the ship is ready (pins are kept to the
/// minute) leaves when it is ready; minutes before, it is refused.
/// Unpinned, the leg flies its own durations again.
#[test]
fn pins_hold_to_ready_and_come_off() {
    let world = World::standard();
    let src = r#"
name = "x"
[[ship]]
name = "s"
start = { at = "nrho", epoch = "2045-03-05" }
[[ship.leg]]
to = "llo"
days = [0.5]
[[ship.leg]]
to = "moon-south-pole"
"#;
    let mut plan = Plan::parse(src, world.eph, Places::standard()).unwrap();
    let items = fly(&world, &plan, "s");
    // The landing leaves when the ship reaches low lunar orbit, at no
    // whole minute.
    let landing = items.iter().find(|i| i.kind == ItemKind::Landing).unwrap();
    let ready = landing.start;
    println!(
        "ready {ready:?}: {:.1} s past the minute",
        (ready.0 * 1_440.0).fract() * 60.0
    );
    for (early_s, flies) in [(20.0, true), (120.0, false)] {
        let Step::Go(go) = &mut plan.ships[0].steps[1] else {
            unreachable!()
        };
        go.pin = Some(Pin {
            depart: Some(ready.plus_days(-early_s / 86_400.0)),
            days: None,
        });
        let items = fly(&world, &plan, "s");
        let landing = items
            .iter()
            .find(|i| matches!(i.kind, ItemKind::Landing | ItemKind::Failed(_)));
        match (landing.map(|i| &i.kind), flies) {
            (Some(ItemKind::Landing), true) => assert_eq!(landing.unwrap().start, ready),
            (Some(ItemKind::Failed(why)), false) => {
                assert!(why.contains("before the ship is ready"))
            }
            other => panic!("{early_s} s early: {other:?}"),
        }
    }
    // A duration pinned over those offered, then taken off.
    let Step::Go(go) = &mut plan.ships[0].steps[0] else {
        unreachable!()
    };
    go.pin = Some(Pin {
        depart: None,
        days: Some(1.0),
    });
    assert_eq!(go.durations(), [1.0]);
    assert_eq!(go.days, [0.5], "the offered durations stay");
    let items = fly(&world, &plan, "s");
    let leg = items.iter().find(|i| i.kind == ItemKind::Leg).unwrap();
    assert!((leg.end.0 - leg.start.0 - 1.0).abs() < 1e-6);
    let Step::Go(go) = &mut plan.ships[0].steps[0] else {
        unreachable!()
    };
    go.pin = None;
    let leg = fly(&world, &plan, "s")
        .into_iter()
        .find(|i| i.kind == ItemKind::Leg)
        .unwrap();
    assert!((leg.end.0 - leg.start.0 - 0.5).abs() < 1e-6);
}

/// Each duration a leg within a system offers is designed alone, so its
/// option is what pinning it flies, and the cheapest flies. (Designed in
/// one call, from the NRHO to low lunar orbit, 2 and 4 days cost hundreds
/// of m/s more than alone, and pinned they flew cheaper than offered.) The
/// station orbit's window is as long pinned as offered: sized by the
/// longest duration designed, 8 days followed the NRHO longer than a pin
/// of 0.5, which flew 111 m/s from its option (found in review).
#[test]
fn offered_durations_fly_as_pinned() {
    let world = World::standard();
    let src = r#"
name = "x"
[[ship]]
name = "s"
start = { at = "nrho", epoch = "2045-03-01" }
[[ship.leg]]
to = "llo"
days = [0.5, 2.0, 8.0]
"#;
    let mut plan = Plan::parse(src, world.eph, Places::standard()).unwrap();
    let leg = fly(&world, &plan, "s")
        .into_iter()
        .find(|i| i.kind == ItemKind::Leg)
        .unwrap();
    assert_eq!(leg.options.len(), 3);
    let cheapest = leg
        .options
        .iter()
        .filter_map(|c| c.dv)
        .fold(f64::MAX, f64::min);
    assert_eq!(leg.dv, cheapest);
    for option in &leg.options {
        let Step::Go(go) = &mut plan.ships[0].steps[0] else {
            unreachable!()
        };
        go.pin = Some(Pin {
            depart: None,
            days: Some(option.days),
        });
        let pinned = fly(&world, &plan, "s")
            .into_iter()
            .find(|i| i.kind == ItemKind::Leg)
            .unwrap();
        println!(
            "{} days: offered {:.1} m/s, pinned {:.1}",
            option.days,
            option.dv.unwrap() * 1e3,
            pinned.dv * 1e3
        );
        assert_eq!(Some(pinned.dv), option.dv, "{} days", option.days);
    }
}

/// A plan stopped by a failure is not reached by an edit after it; a
/// small body's leg whose ship is ready before its orbit is known (2040)
/// but whose window opens after is designed, not a panic.
#[test]
fn failures_and_small_bodies_before_2040() {
    let world = World::standard();
    let src = r#"
name = "x"
[[ship]]
name = "s"
start = { at = "leo", epoch = "2039-12-01" }
[[ship.leg]]
wait = 1
[[ship.leg]]
to = "2000-sg344"
window = ["2039-12-15", "2040-03-01"]
"#;
    let plan = Plan::parse(src, world.eph, Places::standard()).unwrap();
    let ship = &plan.ships[0];
    let mut planner = Planner::new(&world, ship).unwrap();
    let mut checkpoints = vec![planner.checkpoint()];
    let mut items = Vec::new();
    while let Some(batch) = planner.step() {
        items.extend(batch);
        checkpoints.push(planner.checkpoint());
    }
    let kinds: Vec<&ItemKind> = items.iter().map(|i| &i.kind).collect();
    println!("{kinds:?}");
    // The window opens before 2040: the search prices what it can.
    assert!(
        items
            .iter()
            .any(|i| matches!(i.kind, ItemKind::Leg | ItemKind::Failed(_)))
    );
    // After a failure, an edit to a later step does not reach it: the
    // failed step's design reads only its own steps.
    let src = src.replace(
        r#"window = ["2039-12-15", "2040-03-01"]"#,
        r#"window = ["2035-01-01", "2035-01-02"]"#,
    );
    let plan = Plan::parse(&src, world.eph, Places::standard()).unwrap();
    let mut planner = Planner::new(&world, &plan.ships[0]).unwrap();
    let mut checkpoints = vec![planner.checkpoint()];
    while planner.step().is_some() {
        checkpoints.push(planner.checkpoint());
    }
    assert_eq!(checkpoints.len(), 2, "the start, and after the failed leg");
    assert_eq!(reads(&plan.ships[0], 0), 2, "the wait and the leg");
    assert!(resume_point(&plan.ships[0], &checkpoints, 2).is_none());
}

/// The low lunar orbit round trip's stays in LEO and in low lunar orbit
/// keep four states a day (M1b.md, step 2 plan, part 6), those of one
/// integration from their first state to the bit, and are where it puts
/// them between, within a meter: 0.4 mm in LEO, 89 mm in low lunar orbit
/// (2026-10-16), where integrating again in Earth's frame is that noisy.
#[test]
fn a_stay_is_where_one_integration_puts_it() {
    let world = World::standard();
    let eph = world.eph;
    let items = fly(
        &world,
        &plan("published.toml"),
        "Low lunar orbit round trip",
    );
    let (earth, moon) = (eph.lookup("earth").unwrap(), eph.lookup("moon").unwrap());
    let field = Field::new(eph, Some(earth), &[eph.sun(), earth, moon]).with_oblateness();
    let tol = Tolerance {
        rtol: 1e-12,
        atol: 1e-9,
    };
    let stays: Vec<&Item> = items
        .iter()
        .filter(|i| i.kind == ItemKind::Stay && i.track.as_ref().unwrap().revolution.is_some())
        .collect();
    assert_eq!(stays.len(), 2, "in LEO and in low lunar orbit");
    for stay in stays {
        let track = stay.track.as_ref().unwrap();
        let (first, last) = track.span().unwrap();
        let days = (last - first) / SECONDS_PER_DAY;
        assert!(
            track.held().len() as f64 <= 4.0 * days + 3.0,
            "{} kept",
            track.held().len()
        );
        let (t0, s0) = track.first().unwrap();
        let ode = field.ode(track.epoch.plus_days(t0 / SECONDS_PER_DAY));
        let mut run = Dop853::new(&ode, 0.0, s0, tol);
        let mut steps = Vec::new();
        run.advance_dense(&ode, last - t0, &mut steps).unwrap();
        let one = Trajectory::new(steps);
        for (t, s) in track.held() {
            assert_eq!(
                s.map(f64::to_bits),
                one.at(t - t0).map(f64::to_bits),
                "kept at {t} s"
            );
        }
        let worst = (0..=500)
            .map(|k| {
                let t = first + (last - first) * f64::from(k) / 500.0;
                let (a, b) = (track.at(t).unwrap(), one.at(t - t0));
                (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f64>().sqrt()
            })
            .fold(0.0, f64::max);
        println!(
            "{}: {:.3} mm at worst over {days:.2} days",
            stay.label,
            worst * 1e6
        );
        assert!(worst < 1e-3, "{}: {worst} km", stay.label);
    }
}
