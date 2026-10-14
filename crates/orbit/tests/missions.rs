//! Mission plans flown (M1c.md, step 1): every plan in `data/missions/`
//! parses; a chain of legs is continuous in time and in space, where
//! drawn; junctions in a low orbit the planner cannot fly are reported;
//! an aerocapture ends circular in its low orbit.

use glam::DVec3;
use orbit::ephem::Ephemeris;
use orbit::mission::{Item, ItemKind, MarkerKind, Plan, Planner, World};
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
    let k = if last { track.times.len() - 1 } else { 0 };
    let at = track.epoch.plus_days(track.times[k] / SECONDS_PER_DAY);
    let s = track.states[k];
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
            let still = |i: &Item| {
                i.track
                    .as_ref()
                    .unwrap()
                    .states
                    .iter()
                    .all(|s| s == &[0.0; 6])
            };
            if still(&w[0]) || still(&w[1]) {
                continue;
            }
            let gap = (a - b).length();
            assert!(gap < km, "{} → {}: {gap} km apart", w[0].label, w[1].label);
        }
    }
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
        "to = \"nrho\"\ndays = [0]",
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
    assert!(
        !items
            .iter()
            .flat_map(|i| &i.markers)
            .any(|m| matches!(m.kind, MarkerKind::Jump { .. }))
    );
    // 7 m: the stay in low lunar orbit, coasted back from the departure
    // and forward again over 8 days.
    assert_continuous(eph, &items, 0.05);
}

/// Arriving in a low orbit and leaving it again, the leg after picks its
/// own plane: the planner reports the jump.
#[test]
fn junction_in_low_orbit_is_reported() {
    let world = World::standard();
    let items = fly(
        &world,
        &plan("published.toml"),
        "Low lunar orbit round trip",
    );
    let jumps: Vec<(f64, f64)> = items
        .iter()
        .flat_map(|i| &i.markers)
        .filter_map(|m| match m.kind {
            MarkerKind::Jump { km, plane_deg } => Some((km, plane_deg)),
            _ => None,
        })
        .collect();
    assert_eq!(jumps.len(), 1, "{jumps:?}");
    assert!(jumps[0].0 > 1.0, "{jumps:?}");
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
    let s = track.states.last().unwrap();
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

/// A leg leaving at the moment the ship arrived in a low orbit (no stay
/// between) still reports its jump. Found in review: the jump went with
/// the empty stay.
#[test]
fn junction_when_leaving_on_arrival() {
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
        window = ["2033-04-11", "2033-04-11"]
        days = [178]
    "#;
    let plan = Plan::parse(src, Ephemeris::standard(), Places::standard()).unwrap();
    let world = World::standard();
    let items = fly(&world, &plan, "moon, then Mars");
    let legs: Vec<&Item> = items.iter().filter(|i| i.kind == ItemKind::Leg).collect();
    assert_eq!(legs.len(), 3, "every leg flew");
    // The precondition: the Mars leg leaves when the ship reaches LEO.
    assert_eq!(legs[2].start.0, legs[1].end.0);
    let jumps = |i: &Item| {
        i.markers
            .iter()
            .filter(|m| matches!(m.kind, MarkerKind::Jump { .. }))
            .count()
    };
    assert_eq!((jumps(legs[1]), jumps(legs[2])), (1, 1));
}
