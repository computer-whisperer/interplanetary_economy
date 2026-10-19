//! Designs are bit-identical however many threads design them, and a
//! ship going on from a checkpoint designs what a fresh design of its
//! edited plan does (M1b.md, step 1 plan, part 1): the engine keys
//! designs by their inputs and compares states by their bits.

use orbit::mission::{Checkpoint, Item, Plan, Planner, Ship, Step, World, reads, resume_point};
use orbit::places::Places;
use std::time::Instant;

fn plan(file: &str) -> Plan {
    let path = format!("{}/../../data/missions/{file}", env!("CARGO_MANIFEST_DIR"));
    let src = std::fs::read_to_string(&path).unwrap();
    Plan::parse(&src, World::standard().eph, Places::standard()).unwrap()
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

/// `f` run on a pool of `threads`.
fn on<T: Send>(threads: usize, f: impl FnOnce() -> T + Send) -> T {
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .unwrap()
        .install(f)
}

/// An item as bits: everything but the wall time it took to design.
fn bits(item: &Item) -> Vec<u64> {
    let mut w = vec![
        item.step as u64,
        item.start.0.to_bits(),
        item.end.0.to_bits(),
        item.dv.to_bits(),
    ];
    for m in &item.markers {
        w.extend([m.t.0.to_bits(), m.dv.to_bits()]);
        if let Some((_, km)) = m.pass {
            w.push(km.to_bits());
        }
    }
    for c in &item.options {
        w.extend([c.depart.0.to_bits(), c.days.to_bits()]);
        w.push(c.dv.map_or(u64::MAX, f64::to_bits));
    }
    if let Some((open, close)) = item.window {
        w.extend([open.0.to_bits(), close.0.to_bits()]);
    }
    if let Some(track) = &item.track {
        w.push(track.epoch.0.to_bits());
        w.push(track.revolution.map_or(u64::MAX, f64::to_bits));
        for (t, s) in track.held() {
            w.push(t.to_bits());
            w.extend(s.map(f64::to_bits));
        }
    }
    w
}

/// `a` and `b` the same to the bit.
fn assert_identical(what: &str, a: &[Item], b: &[Item]) {
    assert_eq!(a.len(), b.len(), "{what}: item counts");
    for (x, y) in a.iter().zip(b) {
        assert_eq!(
            (&x.kind, &x.label, &x.note),
            (&y.kind, &y.label, &y.note),
            "{what}"
        );
        assert_eq!(x.markers.len(), y.markers.len(), "{what}: {}", x.label);
        for (m, n) in x.markers.iter().zip(&y.markers) {
            assert_eq!(
                (m.kind, m.pass.map(|p| p.0)),
                (n.kind, n.pass.map(|p| p.0)),
                "{what}: {}",
                x.label
            );
        }
        let (u, v) = (bits(x), bits(y));
        if let Some(k) = (0..u.len().max(v.len())).find(|&k| u.get(k) != v.get(k)) {
            panic!(
                "{what}: {} differs at word {k} of {} ({:?} against {:?})",
                x.label,
                u.len(),
                u.get(k).map(|b| f64::from_bits(*b)),
                v.get(k).map(|b| f64::from_bits(*b)),
            );
        }
    }
}

/// Leaving low lunar orbit as it arrives, the ship waits for the planes
/// to align, then leaves LEO for Mars from its own orbit (as
/// `missions.rs` flies it).
const MOON_THEN_MARS: &str = r#"
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

/// Ships that take every parallel path in the planner: seed fans within
/// a system (to and from stations, low orbits and the surface), an
/// aerocapture, departures from the ship's own low orbit (one waiting for
/// the planes to align), and legs between planets (a date search over a
/// window and its durations, and their seeds).
fn ships() -> Vec<Ship> {
    let world = World::standard();
    let rotation = plan("gateway-rotation.toml");
    let mars = plan("mars-windows.toml");
    let moon_then_mars = Plan::parse(MOON_THEN_MARS, world.eph, Places::standard()).unwrap();
    vec![
        rotation.ships[0].clone(),
        mars.ships[0].clone(),
        moon_then_mars.ships[0].clone(),
    ]
}

/// Each ship designed on 24 threads, and on one (the ships side by side,
/// each on a pool of its own).
#[test]
fn designs_do_not_depend_on_the_threads() {
    let world = World::standard();
    let ships = ships();
    let t0 = Instant::now();
    let many: Vec<Vec<Item>> = ships
        .iter()
        .map(|ship| on(24, || fly(&world, ship)).0)
        .collect();
    let t1 = Instant::now();
    let one: Vec<Vec<Item>> = std::thread::scope(|scope| {
        let runs: Vec<_> = ships
            .iter()
            .map(|ship| scope.spawn(|| on(1, || fly(&world, ship)).0))
            .collect();
        runs.into_iter().map(|r| r.join().unwrap()).collect()
    });
    println!("24 threads {:.1?}, one {:.1?}", t1 - t0, t1.elapsed());
    for ((ship, one), many) in ships.iter().zip(&one).zip(&many) {
        println!("{}: {} items", ship.name, many.len());
        assert_identical(&ship.name, one, many);
    }
}

/// The crew rotation's wait in the NRHO lengthened, its stay on the
/// surface lengthened, and its last leg's duration changed: each going on
/// from where `resume_point` says gives the bits a fresh design gives.
#[test]
fn going_on_from_a_checkpoint_is_a_fresh_design() {
    let world = World::standard();
    let ship = plan("gateway-rotation.toml").ships[0].clone();
    let (items, checkpoints) = fly(&world, &ship);
    let edits: [(usize, Step); 3] = [
        (1, Step::Wait(23.0)),
        (4, Step::Wait(9.0)),
        (8, {
            let Step::Go(mut go) = ship.steps[8].clone() else {
                panic!("step 8 is the leg home")
            };
            go.days = vec![5.5];
            Step::Go(go)
        }),
    ];
    for (step, to) in edits {
        let mut edited = ship.clone();
        edited.steps[step] = to;
        let from = resume_point(&ship, &checkpoints, step).unwrap().clone();
        let mut resumed: Vec<Item> = items
            .iter()
            .filter(|i| i.step < from.next())
            .cloned()
            .collect();
        let mut planner = Planner::resume(&world, &edited, from);
        while let Some(batch) = planner.step() {
            resumed.extend(batch);
        }
        let (fresh, _) = fly(&world, &edited);
        assert_identical(&format!("step {step} edited"), &resumed, &fresh);
    }
}

/// A step's design depends on no step past those `reads` names: the crew
/// rotation's step from each checkpoint, designed again with the first
/// step past them changed, is the same to the bit.
#[test]
fn a_step_reads_what_reads_says() {
    let world = World::standard();
    let ship = plan("gateway-rotation.toml").ships[0].clone();
    let (items, checkpoints) = fly(&world, &ship);
    let mut checked = 0;
    for c in &checkpoints[..checkpoints.len() - 1] {
        let end = reads(&ship, c.next());
        let Some(past) = ship.steps.get(end) else {
            continue;
        };
        let mut changed = ship.clone();
        changed.steps[end] = match past {
            Step::Wait(days) => Step::Wait(days + 30.0),
            Step::Go(go) => {
                let mut go = go.clone();
                go.days = vec![go.longest() + 30.0];
                Step::Go(go)
            }
        };
        let step = |ship: &Ship| Planner::resume(&world, ship, c.clone()).step().unwrap();
        let (mine, again) = (step(&ship), step(&changed));
        let designed: Vec<Item> = items
            .iter()
            .filter(|i| mine.iter().any(|m| m.step == i.step))
            .cloned()
            .collect();
        assert_identical(&format!("from step {}", c.next()), &mine, &designed);
        assert_identical(&format!("step {end} changed"), &again, &mine);
        checked += 1;
    }
    assert!(checked >= 4, "{checked} steps checked");
}
