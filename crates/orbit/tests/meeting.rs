//! Meeting a vessel (M1b.md, step 3 plan, part 3): a leg to where a vessel
//! is after one of its legs arrives on the vessel's orbit at its phase,
//! the ship stays on that orbit and leaves from it; a vessel that is not
//! there, or is in a low orbit, fails the step.

use orbit::mission::{Checkpoint, Item, ItemKind, Meeting, Plan, Planner, Ship, World};
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

/// Where `item`'s track puts the ship at `t`, km.
fn position(item: &Item, t: Epoch) -> [f64; 3] {
    let track = item.track.as_ref().unwrap();
    let s = track
        .at((t.0 - track.epoch.0) * SECONDS_PER_DAY)
        .unwrap_or_else(|| panic!("{} has no state at {}", item.label, t.stamp()));
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
