//! Stops in the sandbox (M1b.md, step 3 plan, part 7): the panel shows
//! what each ship was bought for at LEO, what it gave and took at each
//! stop and with whom, what it bought from Earth and for how much, what
//! fell short and why, and a vessel's hold by commodity.

use orbit::ephem::Ephemeris;
use orbit::mission::{ItemKind, Plan};
use orbit::places::Places;
use orbit::time::Epoch;
use sandbox::panel::{self, Row, Tone};
use sandbox::plan::Loaded;

/// A hauler flown to the NRHO as a depot, and a hydrolox tug that gives
/// it all but 20 t of its propellant, stays three days, flies home by
/// aerocapture, fills its tanks from Earth and goes out again (the loop's
/// own test, `crates/sim/tests/state.rs`).
const DEPOT: &str = r#"
name = "depot"

[[ship]]
id = 0
name = "Depot"
class = "hauler"
start = { at = "leo", epoch = "2045-03-01" }

[[ship.leg]]
to = "nrho"
days = [5.0]

[[ship.leg]]
wait = 60

[[ship]]
id = 1
name = "Tug"
class = "tug-h"
start = { at = "leo", epoch = "2045-03-06" }

[[ship.leg]]
to = { ship = 0, leg = 0 }
days = [5.0]
propellant_t = 20.0

[[ship.leg]]
wait = 3

[[ship.leg]]
to = "leo"
days = [5.0]
aero = true
propellant_t = 150.0

[[ship.leg]]
wait = 2

[[ship.leg]]
to = "nrho"
days = [5.0]
"#;

fn designed(src: &str) -> Loaded {
    let eph = Ephemeris::standard();
    let mut plan = Loaded::start(Plan::parse(src, eph, Places::standard()).unwrap()).unwrap();
    plan.wait();
    assert!(plan.done());
    for ship in &plan.ships {
        assert!(ship.failure.is_none(), "{}: {:?}", ship.name, ship.failure);
    }
    plan
}

fn lines(row: &Row, tone: Tone) -> Vec<&str> {
    row.lines
        .iter()
        .filter(|(_, t)| *t == tone)
        .map(|(l, _)| l.as_str())
        .collect()
}

/// The line of `row` in `tone` that starts with `prefix`.
fn line<'a>(row: &'a Row, tone: Tone, prefix: &str) -> &'a str {
    lines(row, tone)
        .into_iter()
        .find(|l| l.starts_with(prefix))
        .unwrap_or_else(|| panic!("no line {prefix:?} in {:?}", row.lines))
}

#[test]
fn a_tug_refuels_a_depot_and_buys_at_leo() {
    let eph = Ephemeris::standard();
    let plan = designed(DEPOT);
    let (depot, tug) = (&plan.ships[0], &plan.ships[1]);
    let tug_row = 1 + depot.items.len();
    let rows = panel::rows(eph, &plan.ships, Epoch::from_ymd(2045, 3, 1), None);
    assert_eq!(rows[tug_row].label, "Tug");

    // Both bought at LEO: the ship, and its load by commodity (the
    // hauler burns methane, the tug hydrogen).
    for (r, fuel) in [(0, ", CH4 "), (tug_row, ", LH2 ")] {
        let bought = line(&rows[r], Tone::Muted, "Bought at LEO for $");
        assert!(bought.contains(": the ship $"), "{bought}");
        assert!(
            bought.contains(", LOX ") && bought.contains(fuel),
            "{bought}"
        );
    }

    // The tug's stop at the depot: the stay after its leg there, with
    // what it gave on arrival and took on leaving.
    let arrival = tug.items.iter().rposition(|i| i.step == 0).unwrap();
    assert_eq!(tug.items[arrival + 1].kind, ItemKind::Stay);
    let stop = &rows[tug_row + 1 + arrival + 1];
    let gave = line(stop, Tone::Muted, "gave LOX ");
    assert!(
        gave.contains(", LH2 ") && gave.ends_with(" to Depot"),
        "{gave}"
    );
    let took = line(stop, Tone::Muted, "took LOX ");
    assert!(
        took.contains(", LH2 ") && took.ends_with(" from Depot"),
        "{took}"
    );
    assert!(
        lines(&rows[tug_row + 1 + arrival], Tone::Muted)
            .iter()
            .all(|l| !l.starts_with("gave ")),
        "the leg itself shows no transfer"
    );

    // Home at LEO: bought from Earth, with its cost.
    let home = tug.items.iter().rposition(|i| i.step == 2).unwrap();
    assert_eq!(tug.items[home + 1].kind, ItemKind::Stay);
    let leo = &rows[tug_row + 1 + home + 1];
    let bought = line(leo, Tone::Muted, "bought LOX ");
    assert!(
        bought.contains(", LH2 ") && bought.contains(" from Earth for $") && bought.ends_with('M'),
        "{bought}"
    );

    // Nothing fell short, and no class warned.
    for row in &rows {
        assert_eq!(
            lines(row, Tone::Warning),
            Vec::<&str>::new(),
            "{:?}",
            row.label
        );
    }

    // The depot's hold, a day after the tug's visit: the hydrogen given.
    let after = Epoch(tug.items[arrival].end.0 + 1.0);
    let rows = panel::rows(eph, &plan.ships, after, None);
    let held = line(&rows[0], Tone::Muted, "hauler · LOX ");
    assert!(held.contains(" · hold LH2 "), "{held}");
}

/// A tug whose manifest at the NRHO has no one to give to: the engine
/// warns on the leg when the plan is read, and the stop says what fell
/// short.
#[test]
fn a_give_with_no_one_says_so() {
    let eph = Ephemeris::standard();
    let src = r#"
name = "alone"

[[ship]]
name = "Tug"
class = "tug-h"
start = { at = "leo", epoch = "2045-03-01" }

[[ship.leg]]
to = "nrho"
days = [5.0]
propellant_t = 20.0

[[ship.leg]]
wait = 1

[[ship.leg]]
to = "leo"
days = [5.0]
aero = true
"#;
    let plan = designed(src);
    let tug = &plan.ships[0];
    let rows = panel::rows(eph, &plan.ships, Epoch::from_ymd(2045, 3, 1), None);
    let arrival = tug.items.iter().rposition(|i| i.step == 0).unwrap();
    let leg = lines(&rows[1 + arrival], Tone::Warning);
    assert_eq!(leg, ["no vessel to transfer with at nrho"], "{leg:?}");
    let stop = &rows[1 + arrival + 1];
    let short = line(stop, Tone::Warning, "give of ");
    assert!(
        short.contains(" of LOX fell short: 0 g (no one to transfer with)"),
        "{short}"
    );
    assert!(
        lines(stop, Tone::Muted)
            .iter()
            .all(|l| !l.starts_with("gave "))
    );
}
