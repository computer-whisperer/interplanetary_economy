//! Ships with mass in the sandbox (M1b.md, step 2 plan, part 4): the
//! panel's tanks and propellant, a ship that ran dry said where and drawn
//! as not flown after, and a new load that designs nothing but draws the
//! ship again.

use orbit::ephem::Ephemeris;
use orbit::mission::{ItemKind, Plan};
use orbit::places::Places;
use orbit::time::Epoch;
use sandbox::app::Shell;
use sandbox::frames::Frame;
use sandbox::panel::{self, Tone};
use sandbox::paths::{self, NOT_FLOWN_DIM};
use sandbox::plan::Loaded;

/// A lander's sortie from the NRHO to the lunar surface and back to low
/// lunar orbit, with `propellant_t`.
fn sortie(propellant_t: f64) -> Loaded {
    let src = format!(
        r#"
name = "sortie"

[[ship]]
name = "Lander"
class = "lander"
start = {{ at = "nrho", epoch = "2045-03-01", propellant_t = {propellant_t:?} }}

[[ship.leg]]
to = "llo"
days = [0.5]

[[ship.leg]]
to = "moon-south-pole"

[[ship.leg]]
wait = 1

[[ship.leg]]
to = "llo"
"#
    );
    let eph = Ephemeris::standard();
    let mut plan = Loaded::start(Plan::parse(&src, eph, Places::standard()).unwrap()).unwrap();
    plan.wait();
    assert!(plan.done());
    plan
}

fn lines(row: &panel::Row, tone: Tone) -> Vec<&str> {
    row.lines
        .iter()
        .filter(|(_, t)| *t == tone)
        .map(|(l, _)| l.as_str())
        .collect()
}

/// Loaded with 40 t, the lander runs dry at its landing: the panel says
/// so on the ship and the landing, what each burn took and left, and that
/// the ascent is not flown; its items after the landing are dimmed.
#[test]
fn a_ship_that_ran_dry_says_where() {
    let eph = Ephemeris::standard();
    let plan = sortie(40.0);
    let ship = &plan.ships[0];
    let dry = ship.dry().expect("it runs dry");
    let landing = ship
        .items
        .iter()
        .position(|i| i.kind == ItemKind::Landing)
        .unwrap();
    assert_eq!(dry.item, landing);
    let t = Epoch::from_ymd(2045, 3, 1);
    let rows = panel::rows(eph, &plan.ships, t, None);
    let muted = lines(&rows[0], Tone::Muted);
    assert!(
        muted
            .iter()
            .any(|l| l.starts_with("lander · LOX ") && l.contains(", CH4 ")),
        "{muted:?}"
    );
    let failed = lines(&rows[0], Tone::Failure);
    assert!(
        failed.iter().any(|l| l.starts_with("Ran dry ")),
        "{failed:?}"
    );
    let leg = ship
        .items
        .iter()
        .position(|i| i.kind == ItemKind::Leg)
        .unwrap();
    let leg_row = lines(&rows[1 + leg], Tone::Muted);
    assert!(
        leg_row
            .iter()
            .any(|l| l.starts_with("propellant ") && l.contains(" · left LOX ")),
        "{leg_row:?}"
    );
    assert!(
        lines(&rows[1 + landing], Tone::Failure)
            .iter()
            .any(|l| l.starts_with("Ran dry: the burn is ")),
    );
    let ascent = ship
        .items
        .iter()
        .position(|i| i.kind == ItemKind::Ascent)
        .unwrap();
    assert!(
        lines(&rows[1 + ascent], Tone::Muted).contains(&"Not flown: the ship ran dry"),
        "{:?}",
        rows[1 + ascent].lines
    );

    // Its colors: as the full ship's to its landing, dimmer after.
    let full = sortie(400.0);
    assert!(full.ships[0].dry().is_none());
    let (dimmed, colors) = (paths::colors(ship), paths::colors(&full.ships[0]));
    for (k, (a, b)) in dimmed.iter().zip(&colors).enumerate() {
        match k > landing {
            true => assert_eq!(*a, b.map(|x| x * NOT_FLOWN_DIM), "{k}"),
            false => assert_eq!(a, b, "{k}"),
        }
    }
}

/// A new load designs nothing, and the map draws the ship again: the
/// landing it now runs dry at is marked, a red block.
#[test]
fn a_new_load_draws_the_ship_again() {
    let eph = Ephemeris::standard();
    let mut app = Shell::new(Epoch::from_ymd(2045, 3, 1));
    app.plan = Some(sortie(400.0));
    app.frame = Frame::offered(eph)[1].2;
    let crosses = |app: &mut Shell| {
        app.update_paths();
        app.scene(1.0)
            .markers
            .iter()
            .filter(|m| m.shape == render::Shape::Block && m.color == paths::FAILURE_COLOR)
            .count()
    };
    assert_eq!(crosses(&mut app), 0);
    let plan = app.plan.as_mut().unwrap();
    let (made, revision) = (plan.game().designs_made(), plan.ships[0].revision);
    let mut short = plan.plan().ships[0].clone();
    short.propellant_t = Some(40.0);
    plan.edit(0, short).unwrap();
    plan.wait();
    assert_eq!(plan.game().designs_made(), made);
    assert_eq!(plan.ships[0].revision, revision);
    assert!(plan.ships[0].dry().is_some());
    assert_eq!(crosses(&mut app), 1, "the burn it ran dry at");
}

/// A ship that ran dry keeping station in the NRHO, before an aerocapture
/// home (its state set here): the stay, drawn a revolution about the
/// clock, is not split (found in review: both halves drew their
/// revolutions); the leg's pass, its own color, dimmed with the rest (found
/// in review: it was not); the panel says where, in mm/s.
#[test]
fn a_stay_that_ran_dry_keeps_its_line() {
    let eph = Ephemeris::standard();
    let src = r#"
name = "aerocapture"

[[ship]]
name = "Tug"
class = "tug-h"
start = { at = "nrho", epoch = "2045-03-01" }

[[ship.leg]]
to = "leo"
days = [5.0]
aero = true
"#;
    let mut plan = Loaded::start(Plan::parse(src, eph, Places::standard()).unwrap()).unwrap();
    plan.wait();
    let mut ship = plan.ships[0].clone();
    let full = paths::colors(&ship);
    let stay = &ship.items[0];
    assert_eq!(stay.kind, ItemKind::Stay);
    let n = ship.items.len();
    let mut state = sim::state::ShipState::default();
    state.used = vec![0.0; n];
    state.after = vec![None; n];
    state.dry = Some(sim::state::Dry {
        t: Epoch(0.5 * (stay.start.0 + stay.end.0)),
        item: 0,
        marker: None,
        dv: 1.76e-6,
        short: 1.76e-6,
    });
    let tanks = ship.tanks.as_mut().unwrap();
    tanks.state = state;
    tanks.pending_from = None;

    let (frame, focus) = (Frame::offered(eph)[1].2, eph.lookup("earth").unwrap());
    let built = paths::build(eph, &frame, focus, 0, &ship, ship.items[0].start);
    assert_eq!(built.items.iter().filter(|&&k| k == 0).count(), 1);
    let pass = built
        .events
        .iter()
        .find(|e| ship.items[e.item].markers[e.marker].kind == orbit::mission::MarkerKind::Pass)
        .expect("the aerocapture's pass");
    let own = paths::marker_style(orbit::mission::MarkerKind::Pass, full[pass.item]).1;
    assert_eq!(pass.color, own.map(|x| x * NOT_FLOWN_DIM));

    let start = ship.items[0].start;
    let rows = panel::rows(eph, &[ship], start, None);
    let failed = lines(&rows[0], Tone::Failure);
    assert!(
        failed
            .iter()
            .any(|l| l.contains(" keeping station at nrho: 1.8 mm/s short")),
        "{failed:?}"
    );
}
