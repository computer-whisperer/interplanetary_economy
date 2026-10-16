//! Career's rule in the planner (M1b.md, step 1 plan, part 3): a ship
//! re-planned while it stays somewhere goes on from where it is at now,
//! leaving no earlier, its waits counted from its arrival. A free plane is
//! fixed by the stay flown until now, and a station's orbit built for a
//! shorter visit refuses steps that need a longer one.

use orbit::mission::{Checkpoint, Item, ItemKind, Plan, Planner, Ship, Step, World};
use orbit::places::Places;
use orbit::time::{Epoch, SECONDS_PER_DAY};

fn ship(file: &str, name: &str) -> Ship {
    let path = format!("{}/../../data/missions/{file}", env!("CARGO_MANIFEST_DIR"));
    let src = std::fs::read_to_string(&path).unwrap();
    let plan = Plan::parse(&src, World::standard().eph, Places::standard()).unwrap();
    plan.ships.into_iter().find(|s| s.name == name).unwrap()
}

/// Every item from `planner` on.
fn rest(mut planner: Planner) -> Vec<Item> {
    let mut items = Vec::new();
    while let Some(batch) = planner.step() {
        items.extend(batch);
    }
    items
}

/// Every item, and the checkpoint before each step.
fn fly(world: &World, ship: &Ship) -> (Vec<Item>, Vec<Checkpoint>) {
    let mut planner = Planner::new(world, ship).unwrap();
    let (mut items, mut checkpoints) = (Vec::new(), vec![planner.checkpoint()]);
    while let Some(batch) = planner.step() {
        items.extend(batch);
        checkpoints.push(planner.checkpoint());
    }
    (items, checkpoints)
}

fn show(items: &[Item]) {
    for i in items {
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
}

/// The low lunar orbit round trip waits in LEO, its plane free, for its
/// first leg. Re-planned halfway through that wait (its leg longer), it
/// keeps the orbit it flew to the bit until now, and leaves no earlier,
/// from that orbit.
#[test]
fn a_ship_parked_in_leo_keeps_what_it_flew() {
    let world = World::standard();
    let ship = ship("published.toml", "Low lunar orbit round trip");
    let (items, checkpoints) = fly(&world, &ship);
    show(&items);
    let stay = &items[0];
    assert_eq!(stay.kind, ItemKind::Stay);
    assert!(stay.end.0 - stay.start.0 > 0.1, "it waits in LEO");
    let now = Epoch(0.5 * (stay.start.0 + stay.end.0));

    let mut edited = ship.clone();
    let Step::Go(go) = &mut edited.steps[0] else {
        panic!("the first step is the leg to the Moon")
    };
    go.days = vec![4.0];
    let from = checkpoints[0].staying(now, Some(stay));
    let again = rest(Planner::resume(&world, &edited, from));
    println!("re-planned at {}:", now.stamp());
    show(&again);

    let (old, new) = (
        stay.track.as_ref().unwrap(),
        again[0].track.as_ref().unwrap(),
    );
    assert_eq!(again[0].start, stay.start);
    assert_eq!(new.epoch, old.epoch);
    let before = |t: f64| old.epoch.plus_days(t / SECONDS_PER_DAY).0 < now.0;
    let flown = old.times.iter().take_while(|&&t| before(t)).count();
    assert!(flown > 100, "{flown} samples before now");
    for k in 0..flown {
        assert_eq!(new.times[k].to_bits(), old.times[k].to_bits(), "sample {k}");
        for (a, b) in new.states[k].iter().zip(&old.states[k]) {
            assert_eq!(a.to_bits(), b.to_bits(), "sample {k}");
        }
    }
    let leg = again.iter().find(|i| i.kind == ItemKind::Leg).unwrap();
    assert!(leg.start.0 >= now.0, "it left {}", leg.start.stamp());
    let note = leg.note.as_deref().unwrap_or("");
    assert!(note.starts_with("from its own orbit"), "{note}");
}

/// Re-planned in low lunar orbit with a wait shorter than it has already
/// stayed, the ship leaves no earlier than now; with a longer one, when
/// that wait from its arrival ends.
#[test]
fn waits_count_from_the_arrival() {
    let world = World::standard();
    let ship = ship("published.toml", "Low lunar orbit round trip");
    let (items, checkpoints) = fly(&world, &ship);
    let arrived = items.iter().find(|i| i.kind == ItemKind::Leg).unwrap().end;
    let now = arrived.plus_days(2.0);
    for (wait, earliest) in [(1.0, now), (3.0, arrived.plus_days(3.0))] {
        let mut edited = ship.clone();
        edited.steps.insert(1, Step::Wait(wait));
        let from = checkpoints[1].staying(now, None);
        assert_eq!(from.next(), 1);
        assert_eq!(from.since(), arrived);
        let again = rest(Planner::resume(&world, &edited, from));
        show(&again);
        let leg = again.iter().find(|i| i.kind == ItemKind::Leg).unwrap();
        assert!(
            leg.start.0 >= earliest.0 - 1e-9,
            "waiting {wait} days: left {}",
            leg.start.stamp()
        );
        assert!(!again.iter().any(|i| matches!(i.kind, ItemKind::Failed(_))));
    }
}

/// The crew rotation waits 20 days in the NRHO. Re-planned during that
/// wait, the same wait goes on in the orbit built for it, and one twice as
/// long is refused: the orbit was built for a shorter visit.
#[test]
fn a_station_visit_built_shorter_refuses() {
    let world = World::standard();
    let ship = ship("gateway-rotation.toml", "Crew vehicle");
    let (items, checkpoints) = fly(&world, &ship);
    let stay = items
        .iter()
        .find(|i| i.kind == ItemKind::Stay && i.label.contains("nrho"))
        .unwrap();
    let now = stay.start.plus_days(5.0);
    let at_nrho = &checkpoints[1];
    assert_eq!(at_nrho.next(), 1);

    let same = Planner::resume(&world, &ship, at_nrho.staying(now, Some(stay)));
    assert_eq!(same.outstays(), None);
    let mut longer = ship.clone();
    longer.steps[1] = Step::Wait(40.0);
    let planner = Planner::resume(&world, &longer, at_nrho.staying(now, Some(stay)));
    let why = planner.outstays().expect("refused");
    println!("{why}");
    assert!(
        why.starts_with("nrho's orbit was built for a visit until "),
        "{why}"
    );
    let items = rest(planner);
    assert!(matches!(&items[0].kind, ItemKind::Failed(w) if *w == why));
}
