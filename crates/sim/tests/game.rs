//! The game (M1b.md, step 1 plan, parts 4, 5 and 9): ships designed in
//! the background as the planner designs them; an edit designs only what
//! it changed, and an undo nothing; the threads change nothing; career
//! re-plans from now and keeps the past, refuses what reaches back into
//! it, and its log reads back, in career and in the sandbox, as the game
//! it was.

use orbit::mission::{Item, ItemKind, Manifest, Planner, Step};
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
    game.replan(id, 0, steps, None).unwrap();
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
    let why = game.replan(id, 0, steps, None).unwrap_err();
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
    game.replan(id, 1, longer, None).unwrap();
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
    game.replan(id, 1, vec![go("leo", 4.0)], None).unwrap();
    game.wait();
    game.advance("2045-03-02T00:00".parse().unwrap());
    game.replan(id, 0, vec![go("llo", 4.0), go("leo", 3.5)], None)
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

/// A re-plan in the log that cannot take effect (its leg had begun):
/// career, reading the log back, refuses it as the game would have when
/// it was made (M1b.md, step 3 plan, part 6), and the ship flies on as
/// before it, a later re-plan still taking effect; the sandbox passes it
/// over.
#[test]
fn a_replan_passed_over_does_not_stop_the_ship() {
    let mut log = round_trip_in_career("2045-03-04T00:00");
    let id = log.plan.ships[0].id;
    log.replans.push(sim::log::Replan {
        ship: id,
        at: "2045-03-04T00:00".parse().unwrap(),
        from: 0,
        steps: vec![go("llo", 4.0), go("leo", 3.5)],
        here: None,
    });
    let mut opened = Game::in_mode(log.clone(), Mode::Sandbox, pool(24)).unwrap();
    opened.wait();
    let ship = &opened.ships()[0];
    assert_eq!(ship.refused.len(), 1, "{:?}", ship.refused);
    assert!(
        ship.refused[0]
            .1
            .ends_with("changes a step whose leg had begun")
    );
    assert!(ship.failure.is_none() && ship.done);
    let Step::Go(out) = &opened.plan().ships[0].steps[0] else {
        panic!("a leg")
    };
    assert_eq!(out.days, vec![3.5], "the plan as it stands leaves it out");

    let mut game = Game::new(log, pool(24)).unwrap();
    game.wait();
    let why = game.refused().unwrap();
    assert!(
        why.starts_with("replan 1: ") && why.ends_with("changes a step whose leg had begun"),
        "{why}"
    );
    assert!(game.log().replans.is_empty());
    assert_eq!(game.now(), "2045-03-04T00:00".parse().unwrap());
    let ship = &game.ships()[0];
    assert!(ship.refused.is_empty() && ship.failure.is_none() && ship.done);
    game.replan(id, 1, vec![go("leo", 4.0)], None).unwrap();
    game.wait();
    assert_eq!(game.refused(), None);
    let home = game.ships()[0]
        .items()
        .filter(|i| i.kind == ItemKind::Leg)
        .nth(1)
        .unwrap()
        .clone();
    assert!((home.end.0 - home.start.0 - 4.0).abs() < 1e-6);
}

/// A re-plan whose first step is past the steps the log has is refused,
/// and one of a step whose leg had begun: the log the game writes reads
/// back as the game it was (found in review: one from past the steps was
/// taken, and the log did not read back), its re-plans made again in
/// career.
#[test]
fn a_log_the_game_wrote_reads_back() {
    let mut game = Game::new(round_trip_in_career("2045-03-04T00:00"), pool(24)).unwrap();
    game.wait();
    let id = game.ships()[0].id;
    // In flight on the 4th: its leg out is not re-planned.
    let why = game.replan(id, 0, vec![go("llo", 4.0)], None).unwrap_err();
    assert!(why.ends_with("changes a step whose leg had begun"), "{why}");
    assert_eq!(game.plan().ships[0].steps.len(), 2);
    let why = game.replan(id, 3, vec![go("llo", 3.5)], None).unwrap_err();
    assert_eq!(why, "from step 3, but the ship has 2");
    game.replan(id, 1, vec![go("leo", 4.0)], None).unwrap();
    game.wait();
    assert_eq!(game.refused(), None);
    let mut again = Game::new(parse(&game.log().to_toml()), pool(24)).unwrap();
    again.wait();
    assert_eq!(again.refused(), None);
    assert_eq!(again.now(), game.now());
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
    game.replan(id, 0, vec![go("llo", 3.5)], None).unwrap();
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
        .replan(id, 1, vec![go("moon-south-pole", 0.1)], None)
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

/// A manifest edit designs nothing (M1b.md, step 3 plan, part 4): the
/// design, its revision and the day's hashes are kept, in a plain plan
/// and in a re-planned career log in the sandbox, where the manifest is
/// written to the re-plan's leg; a log with it reads back.
#[test]
fn a_manifest_edit_designs_nothing() {
    let mut game = Game::new(parse(SORTIE), pool(24)).unwrap();
    game.wait();
    let (made, revision) = (game.designs_made(), game.ships()[0].revision);
    let before = hashes(&game);
    let id = game.ships()[0].id;
    let mut lander = game.plan().ships[0].clone();
    let Step::Go(leg) = &mut lander.steps[0] else {
        panic!("a leg")
    };
    leg.manifest.propellant_t = Some(160.0);
    leg.manifest.cargo.insert("equipment".into(), 10.0);
    game.edit(id, lander).unwrap();
    game.wait();
    assert_eq!(game.designs_made(), made);
    assert_eq!(game.ships()[0].revision, revision);
    // The manifest's transfers are state (here, short: no one at LLO),
    // so the day's hashes change, though nothing was designed.
    assert_ne!(hashes(&game), before);
    let warned: Vec<String> = game.ships()[0]
        .breaches
        .iter()
        .map(|b| b.to_string())
        .collect();
    assert_eq!(
        warned,
        [
            "leg 1: a lander-h's tanks hold 150 t, not 160",
            "leg 1: no vessel to transfer with at llo",
        ]
    );
    let text = game.log().to_toml();
    assert!(text.contains("propellant_t = 160.0"), "{text}");
    let again = Game::new(parse(&text), pool(1)).unwrap();
    assert_eq!(again.ships()[0].breaches, game.ships()[0].breaches);
    assert!(game.undo());
    assert!(game.ships()[0].breaches.is_empty());
    assert_eq!(game.designs_made(), made);
    assert_eq!(hashes(&game), before);

    // A career log in the sandbox: the re-plan is kept, and the manifest
    // goes to its leg, the log's first steps untouched.
    let path = format!(
        "{}/../../data/scenarios/llo-round-trip-replanned.toml",
        env!("CARGO_MANIFEST_DIR")
    );
    let log = parse(&std::fs::read_to_string(path).unwrap());
    let mut game = Game::in_mode(log, Mode::Sandbox, pool(24)).unwrap();
    game.wait();
    let made = game.designs_made();
    let id = game.ships()[0].id;
    let mut tug = game.plan().ships[0].clone();
    tug.class = Some("tug-h".into());
    let Step::Go(home) = &mut tug.steps[1] else {
        panic!("a leg")
    };
    home.manifest.propellant_t = Some(10.0);
    game.edit(id, tug).unwrap();
    game.wait();
    assert_eq!(game.designs_made(), made);
    let log = game.log();
    assert_eq!(log.replans.len(), 1);
    let Step::Go(home) = &log.replans[0].steps[1] else {
        panic!("a leg")
    };
    assert_eq!(home.manifest.propellant_t, Some(10.0));
    let Step::Go(first) = &log.plan.ships[0].steps[1] else {
        panic!("a leg")
    };
    assert_eq!(first.manifest.propellant_t, None);
    let Step::Go(standing) = &game.plan().ships[0].steps[1] else {
        panic!("a leg")
    };
    assert_eq!(standing.manifest.propellant_t, Some(10.0));
}

/// Each check of a manifest, broken alone: refused in career, a warning
/// on the step in the sandbox. A career re-plan sets the manifest of the
/// stop the ship is at (`here`), within its class, and not at its start.
#[test]
fn career_refuses_a_manifest_it_cannot_fill_and_sets_one_here() {
    let cases = [
        (
            "leo",
            "propellant_t = 150.5",
            "leg 1: a tug-h's tanks hold 150 t, not 150.5",
        ),
        (
            "leo",
            "cargo = { equipment = 50.5 }",
            "leg 1: a tug-h's hold takes 50 t, not 50.5",
        ),
        (
            "llo",
            "cargo = { equipment = 20.0 }",
            "leg 1: no vessel to transfer with at llo",
        ),
    ];
    for (to, manifest, breach) in cases {
        let src = format!(
            r#"
name = "manifest"

[game]
mode = "career"
now = "2045-03-01"

[[ship]]
name = "Tug"
class = "tug-h"
start = {{ at = "leo", epoch = "2045-03-01" }}

[[ship.leg]]
to = "{to}"
days = [3.5]
{manifest}
"#
        );
        let why = Game::new(parse(&src), pool(1)).err().unwrap();
        assert_eq!(why, format!("ship \"Tug\": {breach}"));
        let game = Game::in_mode(parse(&src), Mode::Sandbox, pool(1)).unwrap();
        let warned: Vec<String> = game.ships()[0]
            .breaches
            .iter()
            .map(|b| b.to_string())
            .collect();
        assert_eq!(warned, [breach]);
    }

    let src = r#"
name = "here"

[game]
mode = "career"
now = "2045-03-01"

[[ship]]
id = 0
name = "Depot"
class = "hauler"
start = { at = "leo", epoch = "2045-03-01" }

[[ship.leg]]
to = "nrho"
days = [5.0]

[[ship.leg]]
wait = 10

[[ship]]
id = 1
name = "Tug"
class = "tug-h"
start = { at = "leo", epoch = "2045-03-02" }

[[ship.leg]]
to = { ship = 0, leg = 0 }
days = [5.0]
propellant_t = 40.0

[[ship.leg]]
wait = 3

[[ship.leg]]
to = "leo"
days = [5.0]
aero = true
"#;
    let mut game = Game::new(parse(src), pool(24)).unwrap();
    // Made before the tug arrives, it is what the tug gives down to on
    // arrival (M1b.md, step 3 plan, part 5): what it has to give.
    let here = Manifest {
        propellant_t: Some(30.0),
        cargo: Default::default(),
    };
    let why = game.replan(1, 0, vec![], Some(here.clone())).unwrap_err();
    assert_eq!(
        why,
        "`here` sets the manifest of the stop the ship is at, but no leg before step 0 arrives \
         at one"
    );
    let too_much = Manifest {
        propellant_t: Some(151.0),
        cargo: Default::default(),
    };
    let why = game
        .replan(1, 1, vec![go("leo", 5.0)], Some(too_much))
        .unwrap_err();
    assert_eq!(why, "replan 1: here: a tug-h's tanks hold 150 t, not 151");
    assert!(game.log().replans.is_empty());
    // An action waits for the ships' designs, the plan's own being judged
    // (M1b.md, step 3 plan, part 6).
    let why = game
        .replan(1, 1, vec![go("leo", 5.0)], Some(here.clone()))
        .unwrap_err();
    assert_eq!(why, "an action is pending: the plan's ships");
    game.wait();
    game.replan(1, 1, vec![go("leo", 5.0)], Some(here.clone()))
        .unwrap();
    assert_eq!(game.pending(), Some("ship \"Tug\" re-planned from step 1"));
    game.wait();
    assert_eq!((game.pending(), game.refused()), (None, None));
    assert_eq!(game.log().replans[0].here, Some(here.clone()));
    let Step::Go(out) = &game.plan().ships[1].steps[0] else {
        panic!("a leg")
    };
    assert_eq!(out.manifest, here);
    // Read back, the re-plan is made again (M1b.md, step 3 plan, part 6).
    let mut again = Game::new(parse(&game.log().to_toml()), pool(24)).unwrap();
    assert!(again.log().replans.is_empty());
    again.wait();
    assert_eq!(again.refused(), None);
    assert_eq!(again.log().replans[0].here, Some(here.clone()));
    let Step::Go(out) = &again.plan().ships[1].steps[0] else {
        panic!("a leg")
    };
    assert_eq!(out.manifest, here);
}

/// Career against the projection (M1b.md, step 3 plan, part 6): an action
/// is pending until the ships are designed, the clock held; one that has
/// a transfer fall short, or a vessel run dry, where none did before is
/// refused and nothing of it stays; one that does not is accepted, and the
/// log read back makes it again and is the same game. A log whose action
/// is refused is refused, headless too; a ship added that runs dry is
/// refused, and a log whose ships do.
#[test]
fn career_refuses_what_runs_dry_or_short() {
    let src = r#"
name = "depot"

[game]
mode = "career"
now = "2045-03-01"

[[ship]]
id = 0
name = "Depot"
class = "hauler"
start = { at = "leo", epoch = "2045-03-01", propellant_t = 500.0 }

[[ship.leg]]
to = "nrho"
days = [5.0]

[[ship.leg]]
wait = 20

[[ship]]
id = 1
name = "Tug"
class = "tug-h"
start = { at = "leo", epoch = "2045-03-06" }

[[ship.leg]]
to = { ship = 0, leg = 0 }
days = [5.0]
propellant_t = 50.0

[[ship.leg]]
wait = 3

[[ship.leg]]
to = "leo"
days = [5.0]
aero = true
"#;
    let mut game = Game::new(parse(src), pool(24)).unwrap();
    assert_eq!(game.pending(), Some("the plan's ships"));
    game.wait();
    assert_eq!((game.pending(), game.refused()), (None, None));
    assert_eq!(game.now(), "2045-03-01".parse().unwrap());
    let items = |g: &Game| shown(&g.ships()[1].items().cloned().collect::<Vec<_>>());
    let (before, made) = (items(&game), game.designs_made());
    let hydrogen_short = Manifest {
        propellant_t: Some(150.0),
        cargo: Default::default(),
    };
    // Taking a full tank at the depot: its hold has only the hydrogen the
    // tug gave. Refused once designed; the clock does not move meanwhile.
    let home = || {
        let Step::Go(mut go) = go("leo", 5.0) else {
            panic!("a leg")
        };
        go.aero = true;
        Step::Go(go)
    };
    game.replan(1, 1, vec![home()], Some(hydrogen_short))
        .unwrap();
    assert_eq!(game.pending(), Some("ship \"Tug\" re-planned from step 1"));
    assert_eq!(game.log().replans.len(), 1);
    let far = "2045-04-01".parse().unwrap();
    assert_eq!(game.advance(far), "2045-03-01".parse().unwrap());
    let why = game.replan(1, 1, vec![home()], None).unwrap_err();
    assert_eq!(
        why,
        "an action is pending: ship \"Tug\" re-planned from step 1"
    );
    game.wait();
    let why = game.refused().unwrap().to_string();
    assert!(
        why.starts_with("ship \"Tug\"'s take of ") && why.contains(" of LH2 at ship 0, leg 0 on "),
        "{why}"
    );
    assert!(why.ends_with("(the vessel has less)"), "{why}");
    assert!(game.log().replans.is_empty());
    assert_eq!(items(&game), before);
    assert!(
        game.designs_made() > made,
        "the refused re-plan was designed"
    );
    // Headless, a log with it is refused.
    let mut log = game.log();
    log.replans.push(sim::log::Replan {
        ship: 1,
        at: "2045-03-01".parse().unwrap(),
        from: 1,
        steps: vec![home()],
        here: Some(Manifest {
            propellant_t: Some(150.0),
            cargo: Default::default(),
        }),
    });
    let why = sim::run::run(&log, false, &pool(24)).err().unwrap();
    assert!(
        why.starts_with("refused: replan 1: ship \"Tug\"'s take of "),
        "{why}"
    );
    // Within the depot's stock: accepted, and the log read back makes it
    // again, to the same game, its clock at the log's now.
    let little = Manifest {
        propellant_t: Some(55.0),
        cargo: Default::default(),
    };
    game.replan(1, 1, vec![home()], Some(little)).unwrap();
    game.wait();
    assert_eq!(game.refused(), None);
    assert_eq!(game.log().replans.len(), 1);
    assert!(game.advance(far).0 > "2045-03-01".parse::<Epoch>().unwrap().0);
    let text = game.log().to_toml();
    let mut again = Game::new(parse(&text), pool(24)).unwrap();
    again.wait();
    assert_eq!(again.refused(), None);
    assert_eq!(again.log().replans.len(), 1);
    assert_eq!(again.now(), game.now());
    assert_eq!(hashes(&again), hashes(&game));
    // A ship added that runs dry is refused once designed, and is gone.
    let short = parse(&format!(
        "{}\n[[ship]]\nname = \"Lander\"\nclass = \"lander\"\nstart = {{ at = \"leo\", epoch = \
         \"2045-04-05\", propellant_t = 200.0 }}\n{}",
        "name = \"lander\"",
        "[[ship.leg]]\nto = \"llo\"\ndays = [3.5]\n[[ship.leg]]\nto = \"moon-south-pole\"\n\
         [[ship.leg]]\nwait = 3\n[[ship.leg]]\nto = \"llo\"\n"
    ));
    let lander = short.plan.ships[0].clone();
    game.add_ship(lander.clone()).unwrap();
    assert_eq!(game.pending(), Some("ship \"Lander\" added"));
    assert_eq!(game.plan().ships.len(), 3);
    game.wait();
    let why = game.refused().unwrap();
    assert!(why.starts_with("ship \"Lander\" ran dry 2045-04-"), "{why}");
    assert_eq!(game.plan().ships.len(), 2);
    // A log whose ships run dry is refused as a career game.
    let mut log = short;
    log.setup.mode = Mode::Career;
    log.setup.now = Some("2045-03-01".parse().unwrap());
    let mut game = Game::new(log, pool(24)).unwrap();
    game.wait();
    let why = game.refused().unwrap();
    assert!(why.starts_with("ship \"Lander\" ran dry 2045-04-"), "{why}");
    assert!(!game.ships()[0].done || game.ships()[0].items().count() > 0);
}

/// A depot flown to the NRHO and home, and a tug that meets it there
/// (M1b.md, step 3 plan, part 3).
const MEETING: &str = r#"
name = "meeting"

[[ship]]
id = 0
name = "Depot"
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
"#;

/// Where `item`'s track puts the ship at `t`, km.
fn position(item: &Item, t: Epoch) -> [f64; 3] {
    let track = item.track.as_ref().unwrap();
    let s = track
        .at((t.0 - track.epoch.0) * orbit::time::SECONDS_PER_DAY)
        .unwrap_or_else(|| panic!("{} has no state at {}", item.label, t.stamp()));
    [s[0], s[1], s[2]]
}

/// Ship `s`'s stay at the NRHO.
fn nrho_stay(game: &Game, s: usize) -> Item {
    game.ships()[s]
        .items()
        .find(|i| i.kind == ItemKind::Stay && i.label == "at nrho")
        .unwrap()
        .clone()
}

/// The tug's stay is on the depot's orbit, to the bit, at every time
/// both are there.
fn assert_on_the_depots_orbit(game: &Game) {
    let (depot, tug) = (nrho_stay(game, 0), nrho_stay(game, 1));
    assert!(tug.start.0 >= depot.start.0 && tug.end.0 <= depot.end.0);
    for k in 0..=6 {
        let t = Epoch(tug.start.0 + (tug.end.0 - tug.start.0) * k as f64 / 6.0);
        let (a, b) = (position(&tug, t), position(&depot, t));
        assert_eq!(a, b, "apart at {}", t.stamp());
    }
}

/// The tug's walk waits for the depot's, and arrives on its orbit; on one
/// thread as on 24; and the log reads back as the game it was.
#[test]
fn a_meeting_is_designed_on_the_vessels_orbit_on_any_threads() {
    let mut one = Game::new(parse(MEETING), pool(1)).unwrap();
    let mut many = Game::new(parse(MEETING), pool(24)).unwrap();
    one.wait();
    many.wait();
    for game in [&one, &many] {
        for s in game.ships() {
            assert!(s.done && s.failure.is_none(), "{}: {:?}", s.name, s.failure);
        }
        assert_on_the_depots_orbit(game);
    }
    let tug = |g: &Game| shown(&g.ships()[1].items().cloned().collect::<Vec<_>>());
    assert_eq!(tug(&one), tug(&many));
    assert_eq!(hashes(&one), hashes(&many));
    let leg = many.ships()[1]
        .items()
        .find(|i| i.kind == ItemKind::Leg)
        .unwrap()
        .clone();
    assert_eq!(leg.label, "leo → ship 0, leg 0");

    let text = many.log().to_toml();
    assert!(
        text.contains("[ship.leg.to]\nleg = 0\nship = 0\n"),
        "{text}"
    );
    let mut again = Game::new(parse(&text), pool(24)).unwrap();
    again.wait();
    assert_eq!(hashes(&again), hashes(&many));
}

/// An edit of the depot after the leg the tug meets designs nothing of
/// the tug; one at that leg designs the tug again from its meeting, and
/// nothing before it; undone, nothing is designed and the game is what it
/// was.
#[test]
fn an_edit_of_the_vessel_designs_its_visitors_from_their_meetings() {
    let mut game = Game::new(parse(MEETING), pool(24)).unwrap();
    game.wait();
    let before = hashes(&game);
    let tug = |g: &Game| shown(&g.ships()[1].items().cloned().collect::<Vec<_>>());
    let tug_before = tug(&game);
    let leg_days = |ship: &mut orbit::mission::Ship, step: usize, days: f64| {
        let Step::Go(go) = &mut ship.steps[step] else {
            panic!("step {step} is a leg")
        };
        go.days = vec![days];
    };

    // The depot's leg home: its step and the end after it.
    let made = game.designs_made();
    let mut depot = game.plan().ships[0].clone();
    leg_days(&mut depot, 2, 4.5);
    game.edit(0, depot).unwrap();
    game.wait();
    assert_eq!(
        game.designs_made() - made,
        2,
        "the depot's leg home and its end"
    );
    assert_eq!(tug(&game), tug_before);

    // The depot a day later: its four steps (its orbit begins a day
    // later), and the tug's from the meeting (the meeting, the leg home,
    // the end), not its start.
    let made = game.designs_made();
    let mut depot = game.plan().ships[0].clone();
    depot.epoch = depot.epoch.plus_days(1.0);
    game.edit(0, depot).unwrap();
    game.wait();
    assert_eq!(game.designs_made() - made, 7);
    for s in game.ships() {
        assert!(s.done && s.failure.is_none(), "{}: {:?}", s.name, s.failure);
    }
    assert_ne!(tug(&game), tug_before);
    assert_on_the_depots_orbit(&game);

    let made = game.designs_made();
    assert!(game.undo() && game.undo());
    game.wait();
    assert_eq!(game.designs_made(), made);
    assert_eq!(tug(&game), tug_before);
    assert_eq!(hashes(&game), before);
}

/// A depot that never gets there fails the tug's step with why.
#[test]
fn a_vessel_that_never_gets_there_fails_its_visitor() {
    let src = r#"
name = "stuck"

[[ship]]
id = 0
name = "Stuck"
start = { at = "leo", epoch = "2045-03-01" }

[[ship.leg]]
to = "moon-south-pole"

[[ship]]
id = 1
name = "Tug"
start = { at = "leo", epoch = "2045-03-06" }

[[ship.leg]]
to = { ship = 0, leg = 0 }
days = [5.0]
"#;
    let mut game = Game::new(parse(src), pool(24)).unwrap();
    game.wait();
    let why = game.ships()[1].failure.clone().unwrap();
    assert!(
        why.starts_with("ship 0, leg 0: ship 0 does not reach leg 0: a surface is reached"),
        "{why}"
    );
}

/// A vessel without a leg named is met where it starts: a depot that
/// starts at the NRHO, on the orbit it starts on.
#[test]
fn a_vessel_is_met_where_it_starts() {
    let src = r#"
name = "depot"

[[ship]]
id = 0
name = "Depot"
start = { at = "nrho", epoch = "2045-03-01" }

[[ship.leg]]
wait = 30

[[ship.leg]]
to = "leo"
days = [5.0]
aero = true

[[ship]]
id = 1
name = "Tug"
start = { at = "leo", epoch = "2045-03-01" }

[[ship.leg]]
to = { ship = 0 }
days = [5.0]

[[ship.leg]]
wait = 2

[[ship.leg]]
to = "leo"
days = [5.0]
aero = true
"#;
    let mut game = Game::new(parse(src), pool(24)).unwrap();
    game.wait();
    for s in game.ships() {
        assert!(s.done && s.failure.is_none(), "{}: {:?}", s.name, s.failure);
    }
    assert_on_the_depots_orbit(&game);
}

/// What the log refuses of meetings (M1b.md, step 3 plan, part 3): a ship
/// not in the plan, the ship itself, a wait, a step the ship has not, a
/// vessel in a low orbit, meetings that wait on each other, a start at a
/// vessel, and a re-plan replacing a leg another ship's re-plan meets it
/// after.
#[test]
fn what_the_log_refuses_of_meetings() {
    let with = |to: &str| MEETING.replace("to = { ship = 0, leg = 0 }", &format!("to = {to}"));
    let refused = |src: &str| Log::parse(src, world().eph, Places::standard()).unwrap_err();
    let cases = [
        ("{ ship = 5 }", "ship \"Tug\", step 0: no ship has id 5"),
        (
            "{ ship = 1, leg = 0 }",
            "ship \"Tug\", step 0: a ship does not meet itself",
        ),
        (
            "{ ship = 0, leg = 1 }",
            "step 1 of ship 0 is a wait, not a leg",
        ),
        ("{ ship = 0, leg = 7 }", "ship 0 has 3 steps, no leg 7"),
        (
            "{ ship = 0 }",
            "meeting ship 0 in a low orbit is not designed",
        ),
        (
            "{ ship = 0, leg = 0, foo = 1 }",
            "unknown key \"foo\" for a meeting",
        ),
        ("{ ship = -1 }", "ship must be a whole number"),
    ];
    for (to, why) in cases {
        let got = refused(&with(to));
        assert!(got.contains(why), "{to}: {got}");
    }
    let cycle = with("{ ship = 0, leg = 2 }").replace(
        "to = \"leo\"\ndays = [5.0]\naero = true\n\n[[ship]]",
        "to = { ship = 1, leg = 0 }\ndays = [5.0]\n\n[[ship]]",
    );
    let got = refused(&cycle);
    assert!(got.contains("meetings wait on each other"), "{got}");
    let start = MEETING.replace(
        "start = { at = \"leo\", epoch = \"2045-03-06\" }",
        "start = { at = { ship = 0, leg = 0 }, epoch = \"2045-03-06\" }",
    );
    let got = refused(&start);
    assert!(
        got.contains("a ship starts at a place, not at a vessel"),
        "{got}"
    );

    let replanned = format!(
        "{}\n{}",
        with("\"nrho\"").replace(
            "name = \"meeting\"",
            "name = \"meeting\"\n[game]\nmode = \"career\"\nnow = \"2045-03-01T00:10\""
        ),
        r#"
[[replan]]
ship = 1
at = "2045-03-01T00:05"
from = 0

[[replan.leg]]
to = { ship = 0, leg = 0 }
days = [5.0]

[[replan]]
ship = 0
at = "2045-03-01T00:10"
from = 0

[[replan.leg]]
to = "nrho"
days = [4.0]
"#
    );
    let got = refused(&replanned);
    assert_eq!(
        got,
        "replan 2: replaces leg 0 of ship 0, which ship 1 meets it after (step 0 of its plan)"
    );
}

/// Career keeps a leg another ship's plan meets its ship after: a re-plan
/// of the depot from that leg is refused, one from after it is not.
#[test]
fn career_keeps_a_leg_another_ship_meets() {
    let mut log = parse(MEETING);
    log.setup.mode = Mode::Career;
    log.setup.now = Some("2045-03-01".parse().unwrap());
    let mut game = Game::new(log, pool(24)).unwrap();
    let steps = game.plan().ships[0].steps.clone();
    let why = game.replan(0, 0, steps.clone(), None).unwrap_err();
    assert_eq!(
        why,
        "ship \"Tug\" meets it after leg 0 (step 0 of its plan), which the re-plan replaces"
    );
    game.wait();
    game.replan(0, 1, steps[1..].to_vec(), None).unwrap();
}
