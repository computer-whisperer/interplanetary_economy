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
/// orbit: four quick steps.
const SORTIE: &str = r#"
name = "sortie"

[[ship]]
name = "Lander"
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
    let mut game = Game::new(parse(&src), pool(24));
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
    let mut one = Game::new(parse(SORTIE), pool(1));
    let mut many = Game::new(parse(SORTIE), pool(24));
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
    let mut game = Game::new(parse(SORTIE), pool(24));
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
    let mut fresh = Game::new(edited, pool(24));
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
    let mut sandbox = Game::new(log.clone(), pool(24));
    sandbox.wait();
    let stay = sandbox.ships()[0].items().next().unwrap().clone();
    assert_eq!(stay.kind, ItemKind::Stay);
    let now = Epoch(0.5 * (stay.start.0 + stay.end.0));

    log.setup.mode = Mode::Career;
    log.setup.now = Some(now);
    let mut game = Game::new(log, pool(24));
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
    let mut again = Game::new(parse(&text), pool(24));
    let mut opened = Game::in_mode(parse(&text), Mode::Sandbox, pool(24));
    again.wait();
    opened.wait();
    assert_eq!(hashes(&again), after);
    assert_eq!(hashes(&opened), after);
}

/// The crew rotation in career, 5 days into its 20-day wait in the NRHO:
/// the wait made twice as long needs a longer visit than its orbit was
/// built for, and is refused (until step 3); the same wait is not.
#[test]
fn a_longer_station_visit_is_refused() {
    let src = mission("gateway-rotation.toml");
    let mut sandbox = Game::new(parse(&src), pool(24));
    sandbox.wait();
    let stay = sandbox.ships()[0]
        .items()
        .find(|i| i.kind == ItemKind::Stay && i.label.contains("nrho"))
        .unwrap()
        .clone();
    let mut log = parse(&src);
    log.setup.mode = Mode::Career;
    log.setup.now = Some(stay.start.plus_days(5.0));
    let mut game = Game::new(log, pool(24));
    game.wait();
    let id = game.ships()[0].id;
    let steps = game.plan().ships[0].steps.clone();

    let mut longer = steps[1..].to_vec();
    longer[0] = Step::Wait(40.0);
    let why = game.replan(id, 1, longer).unwrap_err();
    assert!(
        why.starts_with("nrho's orbit was built for a visit until"),
        "{why}"
    );
    game.replan(id, 1, steps[1..].to_vec()).unwrap();
    game.wait();
    assert!(game.ships()[0].failure.is_none());
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
        let games = sim::run::run(&log, false, &pool(24));
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
