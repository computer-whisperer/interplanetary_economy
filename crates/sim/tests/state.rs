//! The state and the loop (M1b.md, step 2 plan, parts 3, 5 and 7; step 3
//! plan, part 5): burns empty the tanks by the rocket equation, a station
//! stay burns its upkeep, hydrogen boils off and methane keeps, a ship
//! loaded short runs dry at the burn it cannot make, and the day's hash
//! covers the tanks; the checks of the tanks, and where a ship that ran
//! dry is; a tug refuels a depot it meets and buys at LEO.

use orbit::mission::{Item, ItemKind, MarkerKind};
use orbit::places::Places;
use orbit::time::Epoch;
use sim::game::{Game, world};
use sim::log::{Check, Log, Mode};
use sim::ships::Commodity;
use sim::state::{Bought, Course, Kind, With};
use std::sync::Arc;

fn pool(threads: usize) -> Arc<rayon::ThreadPool> {
    Arc::new(
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap(),
    )
}

fn game(src: &str, threads: usize) -> Game {
    let log = Log::parse(src, world().eph, Places::standard()).unwrap();
    let mut game = Game::new(log, pool(threads)).unwrap();
    game.wait();
    assert!(game.done());
    for ship in game.ships() {
        assert!(ship.failure.is_none(), "{}: {:?}", ship.name, ship.failure);
    }
    game
}

/// A lander's sortie from the NRHO to the lunar surface and back to low
/// lunar orbit, loaded with `propellant_t`.
fn sortie(propellant_t: f64) -> String {
    format!(
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
    )
}

/// The Δv an item burns, km/s: its markers' but a jump's (a leg's,
/// landing's or ascent's), or a station stay's upkeep.
fn burned(item: &Item) -> f64 {
    match item.kind {
        ItemKind::Leg | ItemKind::Landing | ItemKind::Ascent => item
            .markers
            .iter()
            .filter(|m| !matches!(m.kind, MarkerKind::Jump { .. }))
            .map(|m| m.dv)
            .sum(),
        ItemKind::Stay => item.dv,
        _ => 0.0,
    }
}

fn close(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * a.abs().max(b.abs())
}

/// The lander's sortie, methalox (no boil-off): each item burns
/// m(1 − e^(−Δv/vₑ)) of the ship as it begins (a leg's markers add up to
/// its Δv, and their burns, one after another, to its), and it ends at
/// m e^(−ΣΔv/vₑ), LOX and methane burned at 3.6:1.
#[test]
fn burns_follow_the_rocket_equation() {
    let game = game(&sortie(400.0), 24);
    let economy = game.economy();
    let class = economy.ships.iter().find(|c| c.id == "lander").unwrap();
    let spec = economy.propellant(class.propellant);
    let ship = &game.ships()[0];
    let state = &game.state().ships[0];
    let items: Vec<&Item> = ship.items().collect();
    for item in &items {
        if matches!(
            item.kind,
            ItemKind::Leg | ItemKind::Landing | ItemKind::Ascent
        ) {
            assert!(close(burned(item), item.dv, 1e-12), "{}", item.label);
        }
    }
    let m0 = class.dry + 400.0;
    // Each from the ship as it begins, the items before it paid.
    let mut m = m0;
    for (k, item) in items.iter().enumerate() {
        // 1 − e^(−x) as −expm1(−x): a stay's upkeep can be grams.
        let used = -m * (-burned(item) / spec.ve).exp_m1();
        assert!(
            close(state.used[k], used, 1e-12),
            "{}: {} {used}",
            item.label,
            state.used[k]
        );
        m -= state.used[k];
    }
    assert!(
        items
            .iter()
            .filter(|i| i.kind == ItemKind::Leg)
            .any(|i| i.markers.len() > 1),
        "a leg of more than one burn"
    );
    let total: f64 = items.iter().map(|i| burned(i)).sum();
    assert!(
        items.iter().any(|i| i.kind == ItemKind::Stay && i.dv > 0.0),
        "the NRHO stay keeps its station"
    );
    let end = state.load(items.last().unwrap().end).unwrap();
    let left = m0 * (-total / spec.ve).exp() - class.dry;
    assert!(
        close(end.propellant(), left, 1e-9),
        "{} {left}",
        end.propellant()
    );
    let all: f64 = state.used.iter().sum();
    assert!(close(all, 400.0 - left, 1e-9));
    // LOX and methane burned at 3.6:1.
    let s = spec.lox_share;
    assert!(close(400.0 * s - end.lox, all * s, 1e-9));
    assert!(close(400.0 * (1.0 - s) - end.fuel, all * (1.0 - s), 1e-9));
    assert!(state.dry.is_none());
}

/// A wait in the NRHO burns its station-keeping, spread over the days
/// stayed: in full, by the ship's end.
#[test]
fn a_station_stay_burns_its_upkeep() {
    let src = sortie(400.0).replace(
        "[[ship.leg]]\nto = \"llo\"\ndays = [0.5]",
        "[[ship.leg]]\nwait = 20\n\n[[ship.leg]]\nto = \"llo\"\ndays = [0.5]",
    );
    let game = game(&src, 24);
    let economy = game.economy();
    let class = economy.ships.iter().find(|c| c.id == "lander").unwrap();
    let spec = economy.propellant(class.propellant);
    let items: Vec<&Item> = game.ships()[0].items().collect();
    let state = &game.state().ships[0];
    let stay = items
        .iter()
        .position(|i| i.kind == ItemKind::Stay && i.label.contains("nrho"))
        .unwrap();
    assert!(items[stay].end.0 - items[stay].start.0 > 19.0);
    assert!(items[stay].dv > 0.0);
    let m0 = class.dry + 400.0;
    // Before the leg leaves, the stay's whole days are paid.
    let whole = (items[stay].end.0 - items[stay].start.0).floor();
    let paid = items[stay].dv * whole / (items[stay].end.0 - items[stay].start.0);
    let before = state.load(items[stay].start.plus_days(whole)).unwrap();
    let left = m0 * (-paid / spec.ve).exp() - class.dry;
    assert!(close(before.propellant(), left, 1e-12), "{before:?} {left}");
    // By the end, all of it.
    let total: f64 = items.iter().map(|i| burned(i)).sum();
    let end = state
        .load(items.last().unwrap().end.plus_days(1.0))
        .unwrap();
    let left = m0 * (-total / spec.ve).exp() - class.dry;
    assert!(close(end.propellant(), left, 1e-9));
    // All of it before the leg leaves (found in review: the last
    // part-day was paid after the leg's burns).
    let upkeep = state.used[stay];
    let alone = m0 * (1.0 - (-items[stay].dv / spec.ve).exp());
    assert!(close(upkeep, alone, 1e-9), "{upkeep} {alone}");
}

/// Parked n days in LEO, a hydrolox tug keeps (1 − f)ⁿ of its hydrogen,
/// in its tanks and its hold, part-days and all, and all its LOX; a
/// methalox lander beside it keeps all its methane, and loses its
/// hydrogen cargo as the tug does.
#[test]
fn hydrogen_boils_off_and_methane_keeps() {
    let src = r#"
name = "parked"

[[ship]]
name = "Tug"
class = "tug-h"
start = { at = "leo", epoch = "2045-03-01T06:00", cargo = { lh2 = 5.0, ch4 = 3.0 } }

[[ship.leg]]
wait = 10

[[ship.leg]]
to = "llo"
days = [3.5]

[[ship]]
name = "Lander"
class = "lander"
start = { at = "leo", epoch = "2045-03-01", cargo = { lh2 = 2.0 } }

[[ship.leg]]
wait = 10

[[ship.leg]]
to = "llo"
days = [3.5]
"#;
    let game = game(src, 24);
    let economy = game.economy();
    let f = economy
        .propellant(sim::params::Propellant::Hydrolox)
        .fuel_boiloff_per_day;
    assert!(f > 0.0);
    let start = Epoch::from_ymd(2045, 3, 1);
    let leg = game.ships()[0]
        .items()
        .find(|i| i.kind == ItemKind::Leg)
        .unwrap()
        .clone();
    assert!(leg.start.0 > start.0 + 9.0, "it leaves {}", leg.start);
    let (tug, lander) = (&game.state().ships[0], &game.state().ships[1]);
    let tug_start = start.plus_days(0.25);
    let t0 = tug.load(tug_start).unwrap();
    for days in [0.75, 8.75] {
        let t = tug.load(tug_start.plus_days(days)).unwrap();
        let kept = (1.0 - f).powf(days);
        assert!(close(t.fuel, t0.fuel * kept, 1e-12), "{days}: {t:?}");
        assert_eq!(t.lox, t0.lox);
        assert!(close(t.cargo[Commodity::Lh2 as usize], 5.0 * kept, 1e-12));
        assert_eq!(t.cargo[Commodity::Ch4 as usize], 3.0);
    }
    let (l0, l9) = (
        lander.load(start).unwrap(),
        lander.load(start.plus_days(9.0)).unwrap(),
    );
    assert_eq!((l9.lox, l9.fuel), (l0.lox, l0.fuel));
    let lh2 = l9.cargo[Commodity::Lh2 as usize];
    assert!(close(lh2, 2.0 * (1.0 - f).powi(9), 1e-12), "{lh2}");
}

/// An aerocapture into LEO: the leg's pass burns the rule's Δv, and the
/// capture's circularizing burn, which its note gives, burns nothing.
#[test]
fn a_capture_burns_nothing() {
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
    let game = game(src, 24);
    let items: Vec<&Item> = game.ships()[0].items().collect();
    let state = &game.state().ships[0];
    let capture = items
        .iter()
        .position(|i| i.kind == ItemKind::Capture)
        .unwrap();
    assert!(items[capture].markers.iter().any(|m| m.dv > 0.0));
    assert_eq!(state.used[capture], 0.0);
    let leg = items.iter().position(|i| i.kind == ItemKind::Leg).unwrap();
    assert!(
        items[leg]
            .markers
            .iter()
            .any(|m| m.kind == MarkerKind::Pass && m.dv > 0.0)
    );
    assert!(close(burned(items[leg]), items[leg].dv, 1e-12));
    assert!(state.used[leg] > 0.0);
}

/// Loaded with 40 t, the lander makes its leg to low lunar orbit but not
/// its landing: it runs dry there, short by what its tanks could not
/// give, and burns nothing after.
#[test]
fn a_ship_loaded_short_runs_dry() {
    let game = game(&sortie(40.0), 24);
    let economy = game.economy();
    let class = economy.ships.iter().find(|c| c.id == "lander").unwrap();
    let spec = economy.propellant(class.propellant);
    let items: Vec<&Item> = game.ships()[0].items().collect();
    let state = &game.state().ships[0];
    let landing = items
        .iter()
        .position(|i| i.kind == ItemKind::Landing)
        .unwrap();
    let dry = state.dry.unwrap();
    assert_eq!((dry.item, dry.marker), (landing, Some(0)));
    assert_eq!(dry.dv, items[landing].dv);
    let before = state.load(dry.t).unwrap();
    let m = class.dry + before.propellant();
    let could = spec.ve * (m / class.dry).ln();
    assert!(
        close(dry.short, items[landing].dv - could, 1e-12),
        "{dry:?}"
    );
    assert!(dry.short > 0.0);
    assert!(state.used[landing..].iter().all(|&u| u == 0.0));
    let end = state.load(items.last().unwrap().end).unwrap();
    assert_eq!(end, before, "nothing burned after");
}

/// Every day's hash over the game's ships, from the first start to past
/// the last item's end.
fn hashes(game: &Game) -> Vec<u64> {
    let start = Epoch::from_ymd(2045, 3, 1);
    let end = game
        .ships()
        .iter()
        .flat_map(|s| s.items().last())
        .map(|i| i.end.0)
        .fold(start.0, f64::max);
    game.day_hashes(start, (end - start.0).ceil() as usize + 2)
}

/// The day's hash covers the tanks: the same at 1 and 24 threads; a new
/// load changes it and designs nothing, and gives a fresh game's.
#[test]
fn hashes_cover_the_tanks() {
    let (one, many) = (game(&sortie(400.0), 1), game(&sortie(400.0), 24));
    assert_eq!(hashes(&one), hashes(&many));
    let mut game = many;
    let made = game.designs_made();
    let id = game.ships()[0].id;
    let mut lighter = game.plan().ships[0].clone();
    lighter.propellant_t = Some(300.0);
    game.edit(id, lighter).unwrap();
    game.wait();
    assert_eq!(game.designs_made(), made);
    let edited = hashes(&game);
    assert_ne!(edited, hashes(&one));
    assert_eq!(edited, hashes(&self::game(&sortie(300.0), 24)));
}

/// The lander's sortie from LEO (`data/scenarios/lander-south-pole.toml`,
/// in career), loaded with `propellant_t`.
fn sortie_from_leo(propellant_t: f64) -> Game {
    let path = format!(
        "{}/../../data/scenarios/lander-south-pole.toml",
        env!("CARGO_MANIFEST_DIR")
    );
    let src = std::fs::read_to_string(path).unwrap();
    let mut log = Log::parse(&src, world().eph, Places::standard()).unwrap();
    log.plan.ships[0].propellant_t = Some(propellant_t);
    let mut game = Game::new(log, pool(24)).unwrap();
    game.wait();
    game
}

fn at(t: &str, place: &str) -> Check {
    Check::At {
        ship: 0,
        at: t.parse().unwrap(),
        place: place.into(),
    }
}

/// What the checks of a ship's tanks say when they fail (M1b.md, step 2
/// plan, part 5); and where a ship that ran dry is: stranded where its
/// failed burn would have left from, the surface or low lunar orbit, or in
/// flight.
#[test]
fn checks_say_where_a_ship_ran_dry() {
    // 200 t: it lands, and runs dry at the ascent.
    let game = sortie_from_leo(200.0);
    let ran_dry = "Lander ran dry 2045-03-09 14:40 at moon-south-pole → llo: 1317.2 m/s short, \
                   at step 3";
    assert_eq!(
        game.check(&Check::Dry {
            ship: 0,
            step: None
        }),
        Err(format!("{ran_dry}; expected: did not run dry"))
    );
    assert_eq!(
        game.check(&Check::Dry {
            ship: 0,
            step: Some(1)
        }),
        Err(format!("{ran_dry}; expected: ran dry at step 1"))
    );
    assert_eq!(
        game.check(&at("2045-03-12", "llo")),
        Err(
            "Lander on 2045-03-12: at moon-south-pole, stranded: ran dry 2045-03-09 14:40, \
             not at llo"
                .into()
        )
    );
    assert_eq!(
        game.check(&Check::Propellant {
            ship: 0,
            at: "2045-03-12".parse().unwrap(),
            t: [1.0, 2.0]
        }),
        Err("Lander on 2045-03-12: 6.680 t of propellant, not 1.000–2.000".into())
    );

    // 150 t: it runs dry at the descent, which leaves as it arrives in low
    // lunar orbit (no stay between).
    let game = sortie_from_leo(150.0);
    let items: Vec<&Item> = game.ships()[0].items().collect();
    let dry = game.state().ships[0].dry.unwrap();
    assert_eq!(items[dry.item].kind, ItemKind::Landing);
    assert_eq!(items[dry.item - 1].kind, ItemKind::Leg);
    assert_eq!(
        game.check(&at("2045-03-08", "llo")),
        Ok("Lander on 2045-03-08: at llo, stranded: ran dry 2045-03-06 12:16".into())
    );

    // 70 t: it runs dry at the leg's second burn, arriving at the Moon,
    // and flies on past it; before then, it is where it was designed to be.
    let game = sortie_from_leo(70.0);
    let items: Vec<&Item> = game.ships()[0].items().collect();
    let dry = game.state().ships[0].dry.unwrap();
    assert_eq!(
        (items[dry.item].kind.clone(), dry.marker),
        (ItemKind::Leg, Some(1))
    );
    let found = game.check(&at("2045-03-08", "llo")).unwrap_err();
    assert!(
        found.starts_with("Lander on 2045-03-08: ran dry 2045-03-06 12:16 at leo → llo: ")
            && found.ends_with(" short, not at llo"),
        "{found}"
    );
    assert_eq!(
        game.check(&at("2045-03-02", "leo")),
        Ok("Lander on 2045-03-02: at leo".into())
    );
}

/// A ship that runs dry keeping station drifts from its station, and runs
/// dry at the wait it was keeping station through (found in review: at
/// the leg after it, whose step the stay has); a ship without a class
/// carries nothing, and never runs dry.
#[test]
fn checks_of_a_drifting_ship_and_a_massless_one() {
    let src = r#"
name = "station"

[[ship]]
name = "Tug"
class = "tug-h"
start = { at = "nrho", epoch = "2045-03-01", propellant_t = 0.0 }

[[ship.leg]]
wait = 5

[[ship.leg]]
to = "llo"
days = [0.5]
"#;
    let drifting = game(src, 24);
    let found = drifting.check(&at("2045-03-03", "nrho")).unwrap_err();
    assert!(
        found.starts_with("Tug on 2045-03-03: ran dry 2045-03-0")
            && found.contains(" keeping station at nrho: ")
            && found.ends_with(" mm/s short, not at nrho"),
        "{found}"
    );
    let at_the_wait = drifting
        .check(&Check::Dry {
            ship: 0,
            step: Some(0),
        })
        .unwrap();
    assert!(
        at_the_wait.ends_with(" mm/s short, at step 0"),
        "{at_the_wait}"
    );
    let massless = game(
        &src.replace("class = \"tug-h\"\n", "")
            .replace(", propellant_t = 0.0", ""),
        24,
    );
    assert_eq!(
        massless.check(&at("2045-03-03", "nrho")),
        Ok("Tug on 2045-03-03: at nrho".into())
    );
    assert_eq!(
        massless.check(&Check::Propellant {
            ship: 0,
            at: "2045-03-03".parse().unwrap(),
            t: [0.0, 1.0]
        }),
        Err("Tug on 2045-03-03: no load (no class, or not yet started)".into())
    );
    assert_eq!(
        massless.check(&Check::Dry {
            ship: 0,
            step: None
        }),
        Ok("Tug did not run dry".into())
    );
}

/// A depot flown full to the NRHO, and a tug that meets it, gives it all
/// but 20 t of propellant, flies home and buys a full tank at LEO before
/// leaving again (M1b.md, step 3 plan, part 5): the loads change hands at
/// the arrival, the purchase costs the day's launch price and the goods,
/// the day's hashes cover both and are the same on 1 and 24 threads, and
/// the log reads back; in career the ships are bought at their starts.
#[test]
fn a_tug_refuels_a_depot_and_buys_at_leo() {
    let src = r#"
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
    let many = game(src, 24);
    let e = many.economy();
    let s_h = e.propellant(sim::params::Propellant::Hydrolox).lox_share;
    let (depot, tug) = (&many.state().ships[0], &many.state().ships[1]);
    let items: Vec<&Item> = many.ships()[1].items().collect();
    let arrived = items.iter().rev().find(|i| i.step == 0).unwrap().end;
    // Given on arrival: LOX into the depot's tanks, hydrogen into its hold.
    let gives: Vec<_> = tug
        .transfers
        .iter()
        .filter(|x| x.kind == Kind::Give)
        .collect();
    assert_eq!(gives.len(), 2, "{:?}", tug.transfers);
    for x in &gives {
        assert_eq!(x.t, arrived);
        assert_eq!(x.step, 0);
        assert_eq!(
            x.with,
            With::Vessel {
                id: 0,
                leg: Some(0)
            }
        );
        assert_eq!(x.short, None);
        assert!(x.got == x.asked && x.got > 0.0, "{x:?}");
    }
    assert_eq!(gives[0].commodity, Commodity::Lox);
    assert_eq!(gives[1].commodity, Commodity::Lh2);
    // Before: before the arrival burn too, so more than the give went.
    let before = tug.load(Epoch(arrived.0 - 1e-6)).unwrap();
    let after = tug.load(arrived).unwrap();
    assert!(close(after.propellant(), 20.0, 1e-9), "{after:?}");
    assert!(close(after.lox, 20.0 * s_h, 1e-9), "{after:?}");
    assert!(
        before.lox - after.lox > gives[0].got,
        "{before:?} {after:?}"
    );
    // The depot's tanks gain the give, less the station-keeping it pays
    // first to the transfer's time (part 5's order): at most a day's,
    // under 10 kg of a hauler's.
    let stock_before = depot.load(Epoch(arrived.0 - 1e-6)).unwrap();
    let stock = depot.load(arrived).unwrap();
    let gained = stock.lox - stock_before.lox;
    assert!(
        gained <= gives[0].got + 1e-9 && gained > gives[0].got - 0.01,
        "{gained} of {}",
        gives[0].got
    );
    assert!(close(
        stock.cargo[Commodity::Lh2 as usize],
        gives[1].got,
        1e-9
    ));
    // Taken on leaving, back up to 20 t: what three days of keeping the
    // depot's station burned (LOX from its tanks, hydrogen from its hold),
    // and the hydrogen that boiled off.
    let takes: Vec<_> = tug
        .transfers
        .iter()
        .filter(|x| x.kind == Kind::Take && x.step == 0)
        .collect();
    assert_eq!(takes.len(), 2, "{takes:?}");
    assert_eq!(takes[0].commodity, Commodity::Lox);
    assert_eq!(takes[1].commodity, Commodity::Lh2);
    for x in &takes {
        assert!(x.got == x.asked && x.got > 0.0 && x.got < 0.1, "{x:?}");
    }
    let left_depot = items
        .iter()
        .find(|i| i.step == 2 && i.kind == ItemKind::Leg)
        .unwrap();
    assert_eq!(takes[0].t, left_depot.start);
    // Bought at LEO before leaving again: launch at the day's price and
    // the propellant's.
    let bought: Vec<_> = tug.transfers.iter().filter(|x| x.step == 2).collect();
    assert_eq!(bought.len(), 2, "{bought:?}");
    let left = items
        .iter()
        .find(|i| i.step == 4 && i.kind == ItemKind::Leg)
        .unwrap();
    let launch = e.launch_usd_per_kg_at(left.start);
    for x in &bought {
        assert_eq!(x.with, With::Earth);
        assert_eq!(x.kind, Kind::Take);
        assert_eq!(x.t, left.start);
        assert!(close(
            x.cost_musd.unwrap(),
            x.got * 1e3 * (launch + e.propellant_usd_per_kg) / 1e6,
            1e-12
        ));
    }
    // What it had, and what it bought, fill its tanks (the departure burn
    // at the same time empties them again).
    let had = tug.load(Epoch(left.start.0 - 1e-6)).unwrap().propellant();
    let got: f64 = bought.iter().map(|x| x.got).sum();
    assert!((had + got - 150.0).abs() < 1e-6, "{had} + {got}");
    assert!(tug.dry.is_none() && depot.dry.is_none());
    // Both start at LEO, where they are bought: the ship, then its LOX
    // and fuel (a full tank).
    for s in [tug, depot] {
        assert_eq!(s.purchases.len(), 3, "{:?}", s.purchases);
        assert_eq!(s.purchases[0].what, Bought::Ship);
        assert!(s.purchases.iter().all(|p| p.musd > 0.0));
    }
    assert!(
        tug.purchases
            .iter()
            .all(|p| p.t == "2045-03-06".parse().unwrap())
    );

    // The checks a scenario writes of them (M1b.md, step 3 plan, part 8):
    // a transfer in full or short, and a hold by commodity.
    let transfer = |step, kind, commodity, short| Check::Transfer {
        ship: 1,
        step,
        kind,
        commodity,
        short,
    };
    let held = many
        .check(&transfer(0, Kind::Give, Commodity::Lox, false))
        .unwrap();
    assert!(
        held.starts_with("Tug, step 0: gave ")
            && held.contains(" of LOX to Depot (ship 0, leg 0) "),
        "{held}"
    );
    assert_eq!(
        many.check(&transfer(0, Kind::Give, Commodity::Lox, true)),
        Err(format!("{held}; expected: short"))
    );
    let bought = many
        .check(&transfer(2, Kind::Take, Commodity::Lh2, false))
        .unwrap();
    assert!(
        bought.starts_with("Tug, step 2: bought ")
            && bought.contains(" of LH2 from Earth ")
            && bought.contains(", $"),
        "{bought}"
    );
    assert_eq!(
        many.check(&transfer(1, Kind::Take, Commodity::Lh2, false)),
        Err("Tug, step 1: no take of LH2".into())
    );
    let day_after = Epoch(arrived.0 + 1.0);
    let held_lh2 = depot.load(day_after).unwrap().cargo[Commodity::Lh2 as usize];
    assert!(held_lh2 > 0.0);
    let cargo = |cargo| Check::Cargo {
        ship: 0,
        at: day_after,
        cargo,
    };
    let on = format!("Depot on {}", day_after.stamp());
    assert_eq!(
        many.check(&cargo(vec![(
            Commodity::Lh2,
            [held_lh2 - 0.01, held_lh2 + 0.01]
        )])),
        Ok(format!("{on}: LH2 {held_lh2:.3} t"))
    );
    assert_eq!(
        many.check(&cargo(vec![
            (Commodity::Lh2, [0.0, 0.001]),
            (Commodity::Ch4, [0.0, 0.0])
        ])),
        Err(format!(
            "{on}: LH2 {held_lh2:.3} t, CH4 0.000 t, LH2 not 0.000–0.001"
        ))
    );

    // The same on one thread, from the log written; and the same in
    // career.
    let text = many.log().to_toml();
    assert_eq!(hashes(&game(&text, 1)), hashes(&many));
    let mut log = Log::parse(&text, world().eph, Places::standard()).unwrap();
    log.setup.mode = Mode::Career;
    log.setup.now = Some("2045-03-01".parse().unwrap());
    let mut career = Game::new(log, pool(24)).unwrap();
    career.wait();
    assert_eq!(hashes(&career), hashes(&many));
}

/// A measurement (M1b.md, step 2 plan, parts 3 and 8): the published plan,
/// every ship a hauler, designed; the loop alone, and a new load (which
/// designs nothing and runs the loop again from the start). Each leg's,
/// landing's and ascent's markers add up to its Δv. Then the loop alone
/// over 50 ships of 2,000 items each, as long (found in review: each tick
/// read every item, 2 s here).
/// `cargo test --release -p sim --test state -- --ignored --nocapture`
#[test]
#[ignore]
fn the_published_plan_runs_again_quickly() {
    let path = format!(
        "{}/../../data/missions/published.toml",
        env!("CARGO_MANIFEST_DIR")
    );
    let src = std::fs::read_to_string(path)
        .unwrap()
        .replace("[[ship]]\n", "[[ship]]\nclass = \"hauler\"\n");
    let log = Log::parse(&src, world().eph, Places::standard()).unwrap();
    let clock = std::time::Instant::now();
    let mut game = Game::new(log, pool(24)).unwrap();
    game.wait();
    println!("designed in {:.1} s", clock.elapsed().as_secs_f64());
    for ship in game.ships() {
        for item in ship.items() {
            if matches!(
                item.kind,
                ItemKind::Leg | ItemKind::Landing | ItemKind::Ascent
            ) {
                assert!(close(burned(item), item.dv, 1e-12), "{}", item.label);
            }
        }
    }
    let state = game.state();
    println!(
        "{} ships, {:.0} days, {} ran dry",
        state.ships.len(),
        state.until.0 - Epoch::from_ymd(2031, 1, 28).0,
        state.ships.iter().filter(|s| s.dry.is_some()).count()
    );
    let loop_alone = || {
        let courses: Vec<Course> = game
            .ships()
            .iter()
            .zip(&game.plan().ships)
            .map(|(d, ship)| Course {
                id: d.id,
                fitted: sim::ships::read(ship, game.economy()).unwrap(),
                epoch: ship.epoch,
                items: d.items().collect(),
                done: d.done,
                stops: Vec::new(),
                bought: false,
            })
            .collect();
        let clock = std::time::Instant::now();
        sim::state::run(&courses, &[], game.economy());
        clock.elapsed().as_secs_f64() * 1e3
    };
    let alone: Vec<f64> = (0..5).map(|_| loop_alone()).collect();
    println!("the loop alone: {alone:.2?} ms");
    let (id, made) = (game.ships()[0].id, game.designs_made());
    let mut times = Vec::new();
    for t in [1200.0, 1100.0, 1000.0, 900.0, 1200.0] {
        let mut ship = game.plan().ships[0].clone();
        ship.propellant_t = Some(t);
        let clock = std::time::Instant::now();
        game.edit(id, ship).unwrap();
        times.push(clock.elapsed().as_secs_f64() * 1e3);
    }
    assert_eq!(game.designs_made(), made);
    println!("a new load, run again: {times:.2?} ms");

    // 50 ships, each a station stay and a leg of two burns, 1,000 times
    // over the same 13,379 days.
    let start = Epoch::from_ymd(2031, 1, 28);
    let span = 13_379.0 / 1_000.0;
    let items: Vec<Item> = (0..1_000)
        .flat_map(|k| {
            let t = start.plus_days(k as f64 * span);
            let mut stay = game.ships()[0].items().next().unwrap().clone();
            (stay.kind, stay.start, stay.end, stay.dv) =
                (ItemKind::Stay, t, t.plus_days(span - 1.0), 1e-6);
            stay.markers.clear();
            let mut leg = stay.clone();
            (leg.kind, leg.start, leg.end, leg.dv) =
                (ItemKind::Leg, stay.end, t.plus_days(span), 2e-5);
            leg.markers = (0..2)
                .map(|b| orbit::mission::Marker {
                    t: leg.start.plus_days(b as f64 * 0.5),
                    kind: MarkerKind::Burn,
                    dv: 1e-5,
                    pass: None,
                })
                .collect();
            [stay, leg]
        })
        .collect();
    let tug = game
        .economy()
        .ships
        .iter()
        .position(|c| c.id == "tug-h")
        .unwrap();
    let fitted = sim::ships::Fitted {
        class: tug,
        load: sim::ships::Load {
            lox: 120.0,
            fuel: 20.0,
            ..Default::default()
        },
    };
    let courses: Vec<Course> = (0..50)
        .map(|id| Course {
            id,
            fitted: Some(fitted),
            epoch: start,
            items: items.iter().collect(),
            done: true,
            stops: Vec::new(),
            bought: false,
        })
        .collect();
    let times: Vec<f64> = (0..3)
        .map(|_| {
            let clock = std::time::Instant::now();
            let state = sim::state::run(&courses, &[], game.economy());
            assert!(state.ships.iter().all(|s| s.dry.is_none()));
            clock.elapsed().as_secs_f64() * 1e3
        })
        .collect();
    println!("50 ships of 2,000 items: {times:.1?} ms");
}

/// The assembled station's scenario (M1b.md, step 4 plan, part 7),
/// measured: designed on 24 threads and on one (the module's walk waits on
/// the tug's, which waits on the depot's), the loop alone (docks, the
/// assembly's transfers), and a new load run again.
/// `cargo test --release -p sim --test state -- --ignored --nocapture`
#[test]
#[ignore]
fn the_assembled_station_runs_again_quickly() {
    let path = format!(
        "{}/../../data/scenarios/station-assembled.toml",
        env!("CARGO_MANIFEST_DIR")
    );
    let src = std::fs::read_to_string(path).unwrap();
    let parse = || Log::parse(&src, world().eph, Places::standard()).unwrap();
    for threads in [24, 1] {
        let clock = std::time::Instant::now();
        let games = sim::run::run(&parse(), false, &pool(threads)).unwrap();
        println!(
            "designed on {threads} threads in {:.1} s, {} designs",
            clock.elapsed().as_secs_f64(),
            games[0].designs_made()
        );
    }
    let mut game = Game::in_mode(parse(), Mode::Sandbox, pool(24)).unwrap();
    game.wait();
    let loop_alone = || {
        let courses: Vec<Course> = game
            .ships()
            .iter()
            .zip(&game.plan().ships)
            .map(|(d, ship)| Course {
                id: d.id,
                fitted: sim::ships::read(ship, game.economy()).unwrap(),
                epoch: ship.epoch,
                items: d.items().collect(),
                done: d.done,
                stops: Vec::new(),
                bought: false,
            })
            .collect();
        let clock = std::time::Instant::now();
        sim::state::run(&courses, &[], game.economy());
        clock.elapsed().as_secs_f64() * 1e3
    };
    let alone: Vec<f64> = (0..5).map(|_| loop_alone()).collect();
    println!("the loop alone, without the stops: {alone:.3?} ms");
    let (id, made) = (game.ships()[0].id, game.designs_made());
    let mut times = Vec::new();
    for t in [215.0, 214.0, 213.0, 212.0, 215.0] {
        let mut ship = game.plan().ships[0].clone();
        ship.propellant_t = Some(t);
        let clock = std::time::Instant::now();
        game.edit(id, ship).unwrap();
        times.push(clock.elapsed().as_secs_f64() * 1e3);
    }
    assert_eq!(game.designs_made(), made);
    println!("a new load, run again with its docks and transfers: {times:.3?} ms");
}
