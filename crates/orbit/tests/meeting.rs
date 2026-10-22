//! Meeting a vessel (M1b.md, step 3 plan, part 3): a leg to where a vessel
//! is after one of its legs arrives on the vessel's orbit at its phase,
//! the ship stays on that orbit and leaves from it; a vessel that is not
//! there, or is in a low orbit, fails the step.

use orbit::mission::{
    Checkpoint, Item, ItemKind, Meeting, Others, Plan, Planner, START, Ship, World,
};
use orbit::places::Places;
use orbit::time::{Epoch, SECONDS_PER_DAY};

/// A depot flown to the NRHO, and a tug that meets it there and comes
/// home; and the same tug flown to the NRHO on an orbit of its own.
const PLAN: &str = r#"
    name = "meeting"
    [[ship]]
    id = 0
    name = "Depot"
    start = { at = "leo", epoch = "2045-03-01" }
    [[ship.leg]]
    to = "nrho"
    days = [5.0]
    [[ship.leg]]
    wait = 60
    [[ship.leg]]
    to = "leo"
    days = [5.0]
    aero = true

    [[ship]]
    id = 1
    name = "Tug"
    start = { at = "leo", epoch = "2045-03-06" }
    [[ship.leg]]
    to = { ship = 0, leg = 0 }
    days = [5.0]
    [[ship.leg]]
    wait = 3
    [[ship.leg]]
    to = "leo"
    days = [5.0]
    aero = true

    [[ship]]
    id = 2
    name = "Tug alone"
    start = { at = "leo", epoch = "2045-03-06" }
    [[ship.leg]]
    to = "nrho"
    days = [5.0]
    [[ship.leg]]
    wait = 3
    [[ship.leg]]
    to = "leo"
    days = [5.0]
    aero = true
"#;

fn plan() -> Plan {
    Plan::parse(PLAN, World::standard().eph, Places::standard()).unwrap()
}

/// Every item of `ship`, and its checkpoint after each step; a leg that
/// meets a vessel is given `met`.
fn fly(world: &World, ship: &Ship, met: Option<&Checkpoint>) -> (Vec<Item>, Vec<Checkpoint>) {
    let mut planner = Planner::new(world, ship).unwrap();
    let (mut items, mut checkpoints) = (Vec::new(), vec![planner.checkpoint()]);
    loop {
        let batch = match planner.meets() {
            Some(_) => planner.meet(Ok(met.expect("the vessel met"))),
            None => planner.step(),
        };
        let Some(batch) = batch else { break };
        items.extend(batch);
        checkpoints.push(planner.checkpoint());
    }
    for i in &items {
        println!(
            "{:?} {} {} → {}, {:.1} m/s {}",
            i.kind,
            i.label,
            i.start.stamp(),
            i.end.stamp(),
            i.dv * 1e3,
            i.note.as_deref().unwrap_or("")
        );
    }
    (items, checkpoints)
}

/// Where `item`'s track puts the ship at `t`, km: a time within a
/// millisecond of the track's ends is at them (an item's times and its
/// track's are computed from different epochs).
fn position(item: &Item, t: Epoch) -> [f64; 3] {
    let track = item.track.as_ref().unwrap();
    let (first, last) = track.span().unwrap();
    let s = (t.0 - track.epoch.0) * SECONDS_PER_DAY;
    assert!(
        s >= first - 1e-3 && s <= last + 1e-3,
        "{} has no state at {}",
        item.label,
        t.stamp()
    );
    let s = track.at(s.clamp(first, last)).unwrap();
    [s[0], s[1], s[2]]
}

fn km(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// The tug's leg arrives on the depot's orbit: its stay there is on the
/// depot's orbit to the bit, it leaves from that orbit, and the leg is
/// labelled by the vessel and the stay by the place.
#[test]
fn a_leg_that_meets_a_vessel_arrives_on_its_orbit() {
    let world = World::standard();
    let plan = plan();
    let (depot, checkpoints) = fly(&world, &plan.ships[0], None);
    let (tug, _) = fly(&world, &plan.ships[1], Some(&checkpoints[1]));
    assert!(!tug.iter().any(|i| matches!(i.kind, ItemKind::Failed(_))));
    let leg = tug.iter().find(|i| i.kind == ItemKind::Leg).unwrap();
    assert_eq!(leg.label, "leo → ship 0, leg 0");
    let stay = tug
        .iter()
        .find(|i| i.kind == ItemKind::Stay && i.start.0 > leg.start.0)
        .unwrap();
    assert_eq!(stay.label, "at nrho");
    assert!(stay.end.0 - stay.start.0 >= 3.0 - 1e-9);
    let depot_stay = depot
        .iter()
        .find(|i| i.kind == ItemKind::Stay && i.label == "at nrho")
        .unwrap();
    for k in 0..=6 {
        let t = Epoch(stay.start.0 + (stay.end.0 - stay.start.0) * k as f64 / 6.0);
        let gap = km(position(stay, t), position(depot_stay, t));
        assert!(gap < 1e-6, "{} km apart at {}", gap, t.stamp());
    }
    // The leg home leaves from the orbit.
    let home = tug
        .iter()
        .find(|i| i.kind == ItemKind::Leg && i.start.0 > stay.start.0)
        .unwrap();
    let gap = km(position(home, home.start), position(stay, stay.end));
    assert!(gap < 1.0, "{gap} km from the orbit at departure");

    // Against the tug flown to the NRHO on an orbit of its own: the
    // meeting fixes the phase, which costs or saves a little.
    let (alone, _) = fly(&world, &plan.ships[2], None);
    let cost = |items: &[Item]| -> f64 {
        items
            .iter()
            .filter(|i| i.kind == ItemKind::Leg)
            .map(|i| i.dv)
            .sum::<f64>()
            * 1e3
    };
    println!(
        "legs: meeting {:.1} m/s, alone {:.1} m/s",
        cost(&tug),
        cost(&alone)
    );
    assert!((cost(&tug) - cost(&alone)).abs() < 300.0);
}

/// A vessel that is not there fails the step, and so does one in a low
/// orbit (M1b.md, Scope).
#[test]
fn a_vessel_not_there_or_in_a_low_orbit_fails_the_step() {
    let world = World::standard();
    let plan = plan();
    let tug = &plan.ships[1];
    let mut planner = Planner::new(&world, tug).unwrap();
    assert_eq!(
        planner.meets(),
        Some(&Meeting {
            ship: 0,
            leg: Some(0)
        })
    );
    let items = planner
        .meet(Err(
            "ship 0 does not reach leg 0: no design converged".into()
        ))
        .unwrap();
    assert!(
        matches!(&items.last().unwrap().kind, ItemKind::Failed(why) if why == "ship 0, leg 0: ship 0 does not reach leg 0: no design converged"),
        "{:?}",
        items.last().unwrap().kind
    );
    assert!(planner.step().is_none(), "the plan stops at the failure");

    // The depot's start is a low orbit: met there, the step fails.
    let depot = Planner::new(&world, &plan.ships[0]).unwrap().checkpoint();
    let mut planner = Planner::new(&world, tug).unwrap();
    let items = planner.meet(Ok(&depot)).unwrap();
    assert!(
        matches!(&items.last().unwrap().kind, ItemKind::Failed(why) if why.contains("in a low orbit (leo) is not designed")),
        "{:?}",
        items.last().unwrap().kind
    );
}

/// A dock step is read but not designed yet (M1b.md, step 4 plan, part 3):
/// it says so, where the ship is after its leg.
#[test]
fn a_dock_step_is_not_designed_yet() {
    let world = World::standard();
    let src = PLAN.to_string()
        + r#"
    [[ship]]
    id = 4
    name = "Rider"
    start = { at = "leo", epoch = "2045-03-06" }
    [[ship.leg]]
    to = "nrho"
    days = [5.0]
    [[ship.leg]]
    dock = { ship = 0, leg = 0 }
    "#;
    let plan = Plan::parse(&src, world.eph, Places::standard()).unwrap();
    let depot = Planner::new(&world, &plan.ships[0]).unwrap().checkpoint();
    let mut rider = Planner::new(&world, &plan.ships[3]).unwrap();
    assert_eq!(rider.meets(), None);
    rider.step().expect("the leg to the NRHO");
    assert_eq!(
        rider.meets(),
        Some(&Meeting {
            ship: 0,
            leg: Some(0)
        })
    );
    let items = rider.meet(Ok(&depot)).unwrap();
    assert_eq!(items.len(), 1);
    assert!(
        matches!(&items[0].kind, ItemKind::Failed(why) if why == "a dock step is not designed yet (M1b.md, step 4 plan, part 3)"),
        "{:?}",
        items[0].kind
    );
    assert_eq!(items[0].label, "nrho docks to ship 0, leg 0");
    assert!(rider.step().is_none());
}

/// Riding (M1b.md, step 4 plan, part 2), by hand, with what the walk would
/// give the planner: a lander started docked to a hauler at LEO rides it to
/// the NRHO and leaves for low lunar orbit from the hauler's orbit, no
/// earlier than the hauler arrives and with no stay of its own; a hauler
/// that leaves first, or is not to be had, fails the leg; a start on a
/// vessel in flight fails. A leg that docks ends with the dock, arriving
/// while the vessel is there, and fails when the vessel leaves first.
#[test]
fn a_docked_ship_rides_and_leaves_from_its_vessel() {
    let world = World::standard();
    let src = r#"
    name = "riding"
    [[ship]]
    id = 0
    name = "Hauler"
    start = { at = "leo", epoch = "2045-03-01" }
    [[ship.leg]]
    to = "nrho"
    days = [5.0]
    [[ship.leg]]
    wait = 30
    [[ship.leg]]
    to = "leo"
    days = [5.0]
    aero = true

    [[ship]]
    id = 1
    name = "Lander"
    start = { at = { ship = 0 }, epoch = "2045-03-01" }
    [[ship.leg]]
    wait = 6
    [[ship.leg]]
    to = "llo"
    days = [1.0, 2.0]

    [[ship]]
    id = 2
    name = "Tug"
    start = { at = "leo", epoch = "2045-03-06" }
    [[ship.leg]]
    to = { ship = 0, leg = 0 }
    days = [5.0]
    dock = true
    "#;
    let plan = Plan::parse(src, world.eph, Places::standard()).unwrap();
    let (hauler, checkpoints) = fly(&world, &plan.ships[0], None);
    // The hauler's checkpoints: its start, after its leg out, after its
    // leg home.
    assert_eq!(checkpoints.len(), 3);
    let (at_leo, at_nrho) = (&checkpoints[0], &checkpoints[1]);
    let legs: Vec<&Item> = hauler.iter().filter(|i| i.kind == ItemKind::Leg).collect();
    let (out, home) = (legs[0], legs[1]);
    assert_eq!(at_nrho.since(), out.end);
    let nrho_stay = hauler
        .iter()
        .find(|i| i.kind == ItemKind::Stay && i.label == "at nrho")
        .unwrap();
    let labels =
        |items: &[Item]| -> Vec<String> { items.iter().map(|i| i.label.clone()).collect() };

    // The lander: docked at its start, where the hauler is until it leaves.
    let lander = &plan.ships[1];
    let host = |c: &Checkpoint, until: Epoch| Ok((c.clone(), Some(until)));
    let mut p = Planner::docked(&world, lander, host(at_leo, out.start)).unwrap();
    let docked = p.start_items();
    assert_eq!(labels(&docked), ["docked to ship 0"]);
    assert_eq!(docked[0].kind, ItemKind::Docked(0));
    assert_eq!(
        (docked[0].step, docked[0].start, docked[0].end),
        (START, lander.epoch, lander.epoch)
    );
    assert_eq!(p.docked_to(), Some(0));
    // Ready six days in, the hauler is at the NRHO: the leg leaves from
    // the hauler's orbit, before the hauler does.
    let items = p
        .next(Others {
            host: Some(host(at_nrho, home.start)),
            ..Others::default()
        })
        .unwrap();
    assert_eq!(labels(&items), ["nrho → llo"]);
    let leg = &items[0];
    assert_eq!(leg.kind, ItemKind::Leg);
    assert!(leg.start.0 >= lander.epoch.0 + 6.0 - 1e-9 && leg.start.0 >= at_nrho.since().0);
    assert!(leg.start.0 < home.start.0);
    let gap = km(position(leg, leg.start), position(nrho_stay, leg.start));
    assert!(gap < 1.0, "{gap} km from the hauler at departure");
    assert_eq!(p.docked_to(), None);
    assert!(p.step().is_none(), "the plan is done");

    // The hauler leaving as it arrives: the lander, ready after that,
    // cannot leave first.
    let mut p = Planner::docked(&world, lander, host(at_leo, out.start)).unwrap();
    let items = p
        .next(Others {
            host: Some(host(at_nrho, at_nrho.since())),
            ..Others::default()
        })
        .unwrap();
    let ItemKind::Failed(why) = &items[0].kind else {
        panic!("{}", items[0].label)
    };
    assert!(
        why.starts_with("ship 0 leaves nrho ") && why.contains(", before the leg can leave ("),
        "{why}"
    );
    // The hauler not to be had.
    let mut p = Planner::docked(&world, lander, host(at_leo, out.start)).unwrap();
    let items = p
        .next(Others {
            host: Some(Err("no design converged".into())),
            ..Others::default()
        })
        .unwrap();
    assert_eq!(labels(&items), ["docked to ship 0"]);
    assert!(
        matches!(&items[0].kind, ItemKind::Failed(why) if why == "ship 0: no design converged")
    );
    // A start on the hauler in flight, on no hauler, and by `new`.
    let why = Planner::docked(&world, lander, host(at_nrho, home.start))
        .err()
        .unwrap();
    assert!(
        why.starts_with("ship 0 is in flight at 2045-03-01 00:00, arriving nrho "),
        "{why}"
    );
    assert_eq!(
        Planner::docked(&world, lander, Err("no ship has id 0".into()))
            .err()
            .unwrap(),
        "ship 0: no ship has id 0"
    );
    assert_eq!(
        Planner::new(&world, lander).err().unwrap(),
        "a start docked to ship 0 is designed with `Planner::docked`"
    );

    // The tug docks to the hauler at the NRHO: the leg arrives no earlier
    // than the hauler and before it leaves, and the dock ends the items.
    let tug = &plan.ships[2];
    let mut p = Planner::new(&world, tug).unwrap();
    assert!(p.docks());
    assert_eq!(
        p.meets(),
        Some(&Meeting {
            ship: 0,
            leg: Some(0)
        })
    );
    let dock_to = |until: Epoch| Others {
        met: Some(Ok(at_nrho.clone())),
        until: Some(Ok(Some(until))),
        ..Others::default()
    };
    let items = p.next(dock_to(home.start)).unwrap();
    assert_eq!(
        labels(&items).last().map(String::as_str),
        Some("docked to ship 0")
    );
    let leg = items.iter().find(|i| i.kind == ItemKind::Leg).unwrap();
    let dock = items.last().unwrap();
    assert!(leg.end.0 >= at_nrho.since().0 && leg.end.0 <= home.start.0);
    assert_eq!(dock.kind, ItemKind::Docked(0));
    assert_eq!((dock.step, dock.start, dock.end), (0, leg.end, leg.end));
    assert_eq!(p.docked_to(), Some(0));
    assert!(p.next(Others::default()).is_none(), "the plan is done");
    // The hauler leaving half a day after it arrives: the tug, five days
    // out, cannot arrive first.
    let mut p = Planner::new(&world, tug).unwrap();
    let items = p.next(dock_to(at_nrho.since().plus_days(0.5))).unwrap();
    let ItemKind::Failed(why) = &items.last().unwrap().kind else {
        panic!("{}", items.last().unwrap().label)
    };
    assert!(
        why.starts_with("ship 0 leaves nrho ") && why.contains(", before the leg can arrive ("),
        "{why}"
    );
}
