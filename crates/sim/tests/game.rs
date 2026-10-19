//! The game (M1b.md, step 1 plan, parts 4, 5 and 9): ships designed in
//! the background as the planner designs them; an edit designs only what
//! it changed, and an undo nothing; the threads change nothing; career
//! re-plans from now and keeps the past, refuses what reaches back into
//! it, and its log reads back, in career and in the sandbox, as the game
//! it was.

use orbit::mission::{Item, ItemKind, Planner, Step};
use orbit::places::Places;
use orbit::time::Epoch;
use sim::game::{Game, world};
use sim::log::{Log, Mode};
use std::sync::Arc;

fn pool(threads: usize) -> Arc<rayon::ThreadPool> {
    Arc::new(
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap(),
    )
}

fn parse(src: &str) -> Log {
    Log::parse(src, world().eph, Places::standard()).unwrap()
}

fn mission(file: &str) -> String {
    let path = format!("{}/../../data/missions/{file}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(path).unwrap()
}

/// Items as `Debug` shows them (floats to the bit), less the wall time
/// each took to design.
fn shown(items: &[Item]) -> Vec<String> {
    items
        .iter()
        .map(|i| {
            let mut i = i.clone();
            i.seconds = 0.0;
            format!("{i:?}")
        })
        .collect()
}

/// Every day's hash over the game's ships, from the first start to the
/// last item's end.
fn hashes(game: &Game) -> Vec<u64> {
    let start = game
        .plan()
        .ships
        .iter()
        .map(|s| s.epoch.0)
        .fold(f64::INFINITY, f64::min);
    let end = game
        .ships()
        .iter()
        .flat_map(|s| s.items().last())
        .map(|i| i.end.0)
        .fold(start, f64::max);
    game.day_hashes(Epoch(start), (end - start).ceil() as usize + 2)
}

/// A sortie from the NRHO to the lunar surface and back to low lunar
/// orbit: four quick steps. A hydrolox lander: the day's hash covers its
/// tanks, boil-off and all.
const SORTIE: &str = r#"
name = "sortie"

[[ship]]
name = "Lander"
class = "lander-h"
start = { at = "nrho", epoch = "2045-03-01" }

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

#[test]
fn a_game_designs_what_the_planner_does() {
    let src = mission("gateway-rotation.toml");
    let mut game = Game::new(parse(&src), pool(24)).unwrap();
    game.wait();
    let ship = &game.plan().ships[0];
    let mut planner = Planner::new(world(), ship).unwrap();
    let mut items = Vec::new();
    while let Some(batch) = planner.step() {
        items.extend(batch);
    }
    let designed = &game.ships()[0];
    assert!(designed.done && designed.failure.is_none());
    assert_eq!(
        shown(&designed.items().cloned().collect::<Vec<_>>()),
        shown(&items)
    );
    assert!(game.progress().ends_with("1 ships designed"));
}

#[test]
fn the_threads_change_nothing() {
    let mut one = Game::new(parse(SORTIE), pool(1)).unwrap();
    let mut many = Game::new(parse(SORTIE), pool(24)).unwrap();
    one.wait();
    many.wait();
    assert_eq!(
        shown(&one.ships()[0].items().cloned().collect::<Vec<_>>()),
        shown(&many.ships()[0].items().cloned().collect::<Vec<_>>())
    );
    assert_eq!(hashes(&one), hashes(&many));
}

/// The wait on the surface lengthened: the steps before it are there at
/// once, from the cache, and only the step it changes and those after it
/// are designed (here, the plan's end). The
/// game then is a fresh game of the edited plan. Undone, it designs
/// nothing and is the game it was.
#[test]
fn an_edit_designs_what_it_changed_and_an_undo_nothing() {
    let mut game = Game::new(parse(SORTIE), pool(24)).unwrap();
    game.wait();
    let (before, made) = (hashes(&game), game.designs_made());
    let items = game.ships()[0].items().count();

    let mut ship = game.plan().ships[0].clone();
    ship.steps[2] = Step::Wait(3.0);
    game.edit(ship.id, ship.clone()).unwrap();
    let kept = &game.ships()[0];
    assert!(!kept.done);
    assert!(
        kept.items().count() > 0 && kept.items().all(|i| i.step < 2),
        "the steps before the edit, at once"
    );
    game.wait();
    // The step it changes, and the end after it: keys are chained.
    assert_eq!(game.designs_made(), made + 2);

    let mut edited = parse(SORTIE);
    edited.plan.ships[0] = ship;
    let mut fresh = Game::new(edited, pool(24)).unwrap();
    fresh.wait();
    assert_eq!(hashes(&game), hashes(&fresh));
    assert_ne!(hashes(&game), before);

    let made = game.designs_made();
    assert!(game.undo());
    assert!(game.done(), "undone from the cache");
    assert_eq!(game.designs_made(), made);
    assert_eq!(game.ships()[0].items().count(), items);
    assert_eq!(hashes(&game), before);
}

/// The low lunar orbit round trip in career, now halfway through its wait
/// in LEO. Re-planned (its leg to the Moon longer), it keeps the past and
/// leaves no earlier than now. Once that leg has begun, a re-plan of it is
/// refused, as is any sandbox edit. Its log, written out and read back,
/// is the same game in career and in the sandbox.
#[test]
fn career_replans_from_now() {
    let published = mission("published.toml");
    let all = parse(&published);
    let mut log = Log::sandbox(all.plan.clone());
    log.plan
        .ships
        .retain(|s| s.name == "Low lunar orbit round trip");
    // A hydrolox tug: the hashes cover its tanks.
    log.plan.ships[0].class = Some("tug-h".into());
    let mut sandbox = Game::new(log.clone(), pool(24)).unwrap();
    sandbox.wait();
    let stay = sandbox.ships()[0].items().next().unwrap().clone();
    assert_eq!(stay.kind, ItemKind::Stay);
    let now = Epoch(0.5 * (stay.start.0 + stay.end.0));

    log.setup.mode = Mode::Career;
    log.setup.now = Some(now);
    let mut game = Game::new(log, pool(24)).unwrap();
    game.wait();
    let id = game.ships()[0].id;
    let before = hashes(&game);
    let past = (now.0 - stay.start.0).floor() as usize + 1;

    let mut steps = game.plan().ships[0].steps.clone();
    let Step::Go(go) = &mut steps[0] else {
        panic!("the first step is the leg to the Moon")
    };
    go.days = vec![4.0];
    game.replan(id, 0, steps).unwrap();
    game.wait();
    let after = hashes(&game);
    assert_eq!(after[..past], before[..past], "the past is kept");
    assert_ne!(after, before);
    let leg = game.ships()[0]
        .items()
        .find(|i| i.kind == ItemKind::Leg)
        .unwrap()
        .clone();
    assert!(leg.start.0 >= now.0, "it left {}", leg.start.stamp());

    // Past the leg's departure, a re-plan of it reaches back.
    let reached = game.advance(leg.start.plus_days(1.0));
    assert!(reached.0 > leg.start.0, "now {}", reached.stamp());
    let steps = game.plan().ships[0].steps.clone();
    let why = game.replan(id, 0, steps).unwrap_err();
    assert!(why.ends_with("changes a step whose leg had begun"), "{why}");
    let ship = game.plan().ships[0].clone();
    assert!(game.edit(id, ship).is_err(), "career does not edit");

    // Read back, in career and in the sandbox.
    let text = game.log().to_toml();
    let mut again = Game::new(parse(&text), pool(24)).unwrap();
    let mut opened = Game::in_mode(parse(&text), Mode::Sandbox, pool(24)).unwrap();
    again.wait();
    opened.wait();
    assert_eq!(hashes(&again), after);
    assert_eq!(hashes(&opened), after);
}

/// The crew rotation in career, 5 days into its 20-day wait in the NRHO:
/// the wait made twice as long is designed, its orbit extended as the wait
/// needs (M1b.md, step 3 plan, part 2), and the ship stays the 40 days.
#[test]
fn a_longer_station_visit_is_designed() {
    let src = mission("gateway-rotation.toml");
    let mut sandbox = Game::new(parse(&src), pool(24)).unwrap();
    sandbox.wait();
    let stay = sandbox.ships()[0]
        .items()
        .find(|i| i.kind == ItemKind::Stay && i.label.contains("nrho"))
        .unwrap()
        .clone();
    let mut log = parse(&src);
    log.setup.mode = Mode::Career;
    log.setup.now = Some(stay.start.plus_days(5.0));
    let mut game = Game::new(log, pool(24)).unwrap();
    game.wait();
    let id = game.ships()[0].id;
    let steps = game.plan().ships[0].steps.clone();

    let mut longer = steps[1..].to_vec();
    longer[0] = Step::Wait(40.0);
    game.replan(id, 1, longer).unwrap();
    game.wait();
    assert!(game.ships()[0].failure.is_none());
    let stayed = game.ships()[0]
        .items()
        .find(|i| i.kind == ItemKind::Stay && i.label.contains("nrho"))
        .unwrap()
        .clone();
    let days = stayed.end.0 - stayed.start.0;
    assert!(days >= 40.0 - 1e-9, "stayed {days} days");
    assert!(stayed.dv > 0.0, "the stay keeps station");
}

/// Every scenario in `data/scenarios/` runs headless, and its checks hold
/// (M1b.md, step 1 plan, part 8).
#[test]
fn every_scenario_holds() {
    let dir = format!("{}/../../data/scenarios", env!("CARGO_MANIFEST_DIR"));
    let mut ran = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "toml") {
            continue;
        }
        let log = parse(&std::fs::read_to_string(&path).unwrap());
        assert!(
            !log.checks.is_empty(),
            "{}: a scenario checks",
            path.display()
        );
        let games = sim::run::run(&log, false, &pool(24)).unwrap();
        for c in sim::run::checks(&log, &games) {
            match c {
                Ok(held) => println!("{}: held: {held}", path.display()),
                Err(failed) => panic!("{}: {failed}", path.display()),
            }
        }
        ran += 1;
    }
    assert!(ran > 0);
}

/// The low lunar orbit round trip in career, from `now`, a hydrolox tug.
fn round_trip_in_career(now: &str) -> Log {
    let all = parse(&mission("published.toml"));
    let mut log = Log::sandbox(all.plan.clone());
    log.plan
        .ships
        .retain(|s| s.name == "Low lunar orbit round trip");
    log.plan.ships[0].class = Some("tug-h".into());
    log.setup.mode = Mode::Career;
    log.setup.now = Some(now.parse().unwrap());
    log
}

/// A leg to `to` of `days`.
fn go(to: &str, days: f64) -> Step {
    let Step::Go(mut go) = parse(&mission("published.toml")).plan.ships[0].steps[0].clone() else {
        panic!("a leg")
    };
    go.to = orbit::mission::Spot::named(to, Places::standard()).unwrap();
    go.days = vec![days];
    go.note = None;
    Step::Go(go)
}

/// Two re-plans while the ship waits in LEO: the first changes its leg
/// home (taking effect in low lunar orbit), the second, made later, its
/// leg out and everything after. The second takes effect in LEO, before
/// the first would have (found in review: it had been passed over).
#[test]
fn a_later_replan_takes_effect_where_it_holds() {
    let mut game = Game::new(round_trip_in_career("2045-03-01T12:00"), pool(24)).unwrap();
    game.wait();
    let id = game.ships()[0].id;
    game.replan(id, 1, vec![go("leo", 4.0)]).unwrap();
    game.wait();
    game.advance("2045-03-02T00:00".parse().unwrap());
    game.replan(id, 0, vec![go("llo", 4.0), go("leo", 3.5)])
        .unwrap();
    game.wait();
    let legs: Vec<f64> = game.ships()[0]
        .items()
        .filter(|i| i.kind == ItemKind::Leg)
        .map(|i| i.end.0 - i.start.0)
        .collect();
    assert_eq!(legs.len(), 2);
    assert!(
        (legs[0] - 4.0).abs() < 1e-6 && (legs[1] - 3.5).abs() < 1e-6,
        "{legs:?}"
    );
    assert!(game.ships()[0].refused.is_empty());
    let text = game.log().to_toml();
    let mut again = Game::new(parse(&text), pool(24)).unwrap();
    again.wait();
    assert_eq!(hashes(&again), hashes(&game));
}

/// A re-plan in the log that cannot take effect (its leg had begun) is
/// passed over: the ship flies on as before it, and a later re-plan still
/// takes effect.
#[test]
fn a_replan_passed_over_does_not_stop_the_ship() {
    let mut log = round_trip_in_career("2045-03-04T00:00");
    let id = log.plan.ships[0].id;
    log.replans.push(sim::log::Replan {
        ship: id,
        at: "2045-03-04T00:00".parse().unwrap(),
        from: 0,
        steps: vec![go("llo", 4.0), go("leo", 3.5)],
    });
    let mut game = Game::new(log, pool(24)).unwrap();
    game.wait();
    let ship = &game.ships()[0];
    assert_eq!(ship.refused.len(), 1, "{:?}", ship.refused);
    assert!(
        ship.refused[0]
            .1
            .ends_with("changes a step whose leg had begun")
    );
    assert!(ship.failure.is_none() && ship.done);
    let Step::Go(out) = &game.plan().ships[0].steps[0] else {
        panic!("a leg")
    };
    assert_eq!(out.days, vec![3.5], "the plan as it stands leaves it out");
    game.replan(id, 1, vec![go("leo", 4.0)]).unwrap();
    game.wait();
    let home = game.ships()[0]
        .items()
        .filter(|i| i.kind == ItemKind::Leg)
        .nth(1)
        .unwrap()
        .clone();
    assert!((home.end.0 - home.start.0 - 4.0).abs() < 1e-6);
}

/// A re-plan whose first step is past the steps the log has, its
/// re-plans passed over among them, is refused: the log the game writes
/// reads back as the game it was (found in review: it was taken, and the
/// log did not read back).
#[test]
fn a_log_the_game_wrote_reads_back() {
    let mut game = Game::new(round_trip_in_career("2045-03-04T00:00"), pool(24)).unwrap();
    let id = game.ships()[0].id;
    // Its step not yet designed, it is taken; designed, it is passed over.
    game.replan(id, 0, vec![go("llo", 4.0)]).unwrap();
    game.wait();
    assert_eq!(game.ships()[0].refused.len(), 1);
    assert_eq!(game.plan().ships[0].steps.len(), 2);
    let why = game.replan(id, 2, vec![go("llo", 3.5)]).unwrap_err();
    assert_eq!(why, "replan 2: from step 2, but the ship has 1");
    game.replan(id, 1, vec![go("leo", 4.0)]).unwrap();
    game.wait();
    let mut again = Game::new(parse(&game.log().to_toml()), pool(24)).unwrap();
    again.wait();
    assert_eq!(hashes(&again), hashes(&game));
}

/// A ship in LEO with no legs has flown nothing: re-planned, what it does
/// begins at now, and every day before now is as it was.
#[test]
fn a_ship_with_no_legs_begins_its_stay_at_now() {
    let mut log = round_trip_in_career("2045-03-10T00:00");
    log.plan.ships[0].steps.clear();
    let mut game = Game::new(log, pool(24)).unwrap();
    game.wait();
    assert_eq!(game.ships()[0].items().count(), 0);
    let start = Epoch::from_ymd(2045, 3, 1);
    let before = game.day_hashes(start, 9);
    let id = game.ships()[0].id;
    game.replan(id, 0, vec![go("llo", 3.5)]).unwrap();
    game.wait();
    assert_eq!(game.day_hashes(start, 9), before);
    // Its plane free from now, it leaves at once.
    let first = game.ships()[0].items().next().unwrap().clone();
    assert_eq!(first.start, game.now(), "{:?} {}", first.kind, first.label);
}

/// Renamed, then undone: the old name is back, in the plan and the ship.
#[test]
fn an_undone_rename_takes_the_old_name() {
    let mut game = Game::new(parse(SORTIE), pool(24)).unwrap();
    game.wait();
    let id = game.ships()[0].id;
    game.rename(id, "Other");
    assert_eq!(game.ships()[0].name, "Other");
    assert!(game.undo());
    assert_eq!(game.ships()[0].name, "Lander");
    assert_eq!(game.plan().ships[0].name, "Lander");
}

/// A ship's class in the sandbox: what it does not allow is a warning on
/// the step, and a new class or load designs nothing (M1b.md, step 2
/// plan, part 2).
#[test]
fn a_class_warns_in_the_sandbox_and_a_new_one_designs_nothing() {
    let mut game = Game::new(parse(SORTIE), pool(24)).unwrap();
    game.wait();
    assert!(game.ships()[0].breaches.is_empty());
    let (made, revision) = (game.designs_made(), game.ships()[0].revision);
    let id = game.ships()[0].id;
    let mut tug = game.plan().ships[0].clone();
    tug.class = Some("tug-h".into());
    tug.cargo.insert("equipment".into(), 60.0);
    game.edit(id, tug).unwrap();
    game.wait();
    let warned: Vec<String> = game.ships()[0]
        .breaches
        .iter()
        .map(|b| b.to_string())
        .collect();
    assert_eq!(
        warned,
        [
            "a tug-h's hold takes 50 t, not 60",
            "leg 2: a tug-h can't land at moon-south-pole",
        ]
    );
    assert_eq!(game.designs_made(), made);
    assert_eq!(game.ships()[0].revision, revision);
    assert!(game.undo());
    assert!(game.ships()[0].breaches.is_empty());
    let mut unknown = game.plan().ships[0].clone();
    unknown.class = Some("barge".into());
    let why = game.edit(id, unknown).unwrap_err();
    assert!(
        why.starts_with("ship \"Lander\": no class \"barge\""),
        "{why}"
    );
    assert_eq!(game.plan().ships[0].class.as_deref(), Some("lander-h"));
}

/// A career log in the sandbox, its ship given a class: its re-plan is
/// kept, and nothing is designed again (found in review: the edit folded
/// the re-plan, and the ship flew from its start without it).
#[test]
fn a_new_class_keeps_the_replans() {
    let path = format!(
        "{}/../../data/scenarios/llo-round-trip-replanned.toml",
        env!("CARGO_MANIFEST_DIR")
    );
    let log = parse(&std::fs::read_to_string(path).unwrap());
    let mut game = Game::in_mode(log, Mode::Sandbox, pool(24)).unwrap();
    game.wait();
    let items = |g: &Game| shown(&g.ships()[0].items().cloned().collect::<Vec<_>>());
    let (made, before) = (game.designs_made(), items(&game));
    let id = game.ships()[0].id;
    let mut tug = game.plan().ships[0].clone();
    tug.class = Some("tug-h".into());
    game.edit(id, tug).unwrap();
    game.wait();
    assert_eq!(game.log().replans.len(), 1);
    assert_eq!(game.designs_made(), made);
    // The same path (the day's hash now covers the tanks it adds).
    assert_eq!(items(&game), before);
    assert_eq!(game.plan().ships[0].class.as_deref(), Some("tug-h"));
}

/// Career refuses what a ship's class does not allow when it is made: a
/// ship added, a re-plan, or a log that has one; and a ship starts at LEO.
#[test]
fn career_refuses_what_a_class_does_not_allow() {
    let src = r#"
name = "tugs"

[game]
mode = "career"
now = "2045-03-01"

[[ship]]
name = "Tug"
class = "tug-h"
start = { at = "leo", epoch = "2045-03-01" }

[[ship.leg]]
to = "llo"
days = [3.5]
"#;
    let mut game = Game::new(parse(src), pool(24)).unwrap();
    let id = game.ships()[0].id;
    // Its legs counted as the re-plan's, as a log reads them (found in
    // review: they were counted as the ship's steps).
    let why = game
        .replan(id, 1, vec![go("moon-south-pole", 0.1)])
        .unwrap_err();
    assert_eq!(
        why,
        "replan 1: leg 1: a tug-h can't land at moon-south-pole"
    );
    assert!(game.log().replans.is_empty());

    let mut lander = game.plan().ships[0].clone();
    lander.name = "Lander".into();
    lander.class = Some("lander".into());
    lander.steps = vec![go("llo", 3.5), go("leo", 3.5)];
    let Step::Go(home) = &mut lander.steps[1] else {
        panic!("a leg")
    };
    home.aero = true;
    let why = game.add_ship(lander.clone()).unwrap_err();
    assert_eq!(
        why,
        "ship \"Lander\": leg 2: a lander can't arrive through an atmosphere"
    );
    let mut at_nrho = lander.clone();
    at_nrho.start = orbit::mission::Spot::named("nrho", Places::standard()).unwrap();
    at_nrho.steps.clear();
    let why = game.add_ship(at_nrho).unwrap_err();
    assert_eq!(
        why,
        "ship \"Lander\": a ship starts at LEO, where it is bought, not nrho"
    );
    assert_eq!(game.ships().len(), 1);

    // A log that has one is not a career game; in the sandbox, it warns.
    let landing = format!("{src}\n[[ship.leg]]\nto = \"moon-south-pole\"\n");
    let why = Game::new(parse(&landing), pool(24)).err().unwrap();
    assert_eq!(
        why,
        "ship \"Tug\": leg 2: a tug-h can't land at moon-south-pole"
    );
    let game = Game::in_mode(parse(&landing), Mode::Sandbox, pool(24)).unwrap();
    assert_eq!(game.ships()[0].breaches.len(), 1);
}

/// Each check of a ship's class, broken alone by a ship from LEO: refused
/// in career, a warning on the step in the sandbox (M1b.md, step 2 plan,
/// part 7).
#[test]
fn each_class_check_is_refused_in_career_and_warned_in_the_sandbox() {
    let llo = "\n[[ship.leg]]\nto = \"llo\"\ndays = [3.5]\n";
    let cases = [
        (
            "tug-h",
            ", propellant_t = 150.5",
            String::new(),
            "a tug-h's tanks hold 150 t, not 150.5",
        ),
        (
            "tug-h",
            ", cargo = { equipment = 50.5 }",
            String::new(),
            "a tug-h's hold takes 50 t, not 50.5",
        ),
        (
            "tug-h",
            "",
            format!("{llo}\n[[ship.leg]]\nto = \"moon-south-pole\"\n"),
            "leg 2: a tug-h can't land at moon-south-pole",
        ),
        (
            "lander",
            "",
            format!("{llo}\n[[ship.leg]]\nto = \"leo\"\ndays = [3.5]\naero = true\n"),
            "leg 2: a lander can't arrive through an atmosphere",
        ),
        (
            "tug-h",
            "",
            "\n[[ship.leg]]\nto = \"lmo\"\n".to_string(),
            "leg 1: a tug-h can't leave Earth's system",
        ),
    ];
    for (class, load, legs, breach) in cases {
        let src = format!(
            r#"
name = "breach"

[game]
mode = "career"
now = "2045-03-01"

[[ship]]
name = "Ship"
class = "{class}"
start = {{ at = "leo", epoch = "2045-03-01"{load} }}
{legs}"#
        );
        let why = Game::new(parse(&src), pool(1)).err().unwrap();
        assert_eq!(why, format!("ship \"Ship\": {breach}"));
        let game = Game::in_mode(parse(&src), Mode::Sandbox, pool(1)).unwrap();
        let warned: Vec<String> = game.ships()[0]
            .breaches
            .iter()
            .map(|b| b.to_string())
            .collect();
        assert_eq!(warned, [breach]);
    }
}
