//! A log run headless (M1b.md, step 1 plan, part 8): designed to its end,
//! as one game, or each ship alone, one after another, so that each item's
//! time is its own; and a scenario's checks against it.

use crate::game::Game;
use crate::log::{Check, Log};
use std::sync::Arc;

/// `log` designed to its end: one game, or a game for each ship alone (its
/// re-plans and checks with it), each designed before the next starts;
/// why not, if the log is not a game (`Game::new`).
pub fn run(log: &Log, alone: bool, pool: &Arc<rayon::ThreadPool>) -> Result<Vec<Game>, String> {
    let logs: Vec<Log> = match alone {
        false => vec![log.clone()],
        true => log
            .plan
            .ships
            .iter()
            .map(|ship| {
                let mut one = log.clone();
                one.plan.ships = vec![ship.clone()];
                one.replans.retain(|r| r.ship == ship.id);
                one.checks.retain(|c| {
                    let (Check::At { ship: s, .. } | Check::Dv { ship: s, .. }) = c;
                    *s == ship.id
                });
                one
            })
            .collect(),
    };
    logs.into_iter()
        .map(|log| {
            let mut game = Game::new(log, pool.clone())?;
            game.wait();
            Ok(game)
        })
        .collect()
}

/// Each of `log`'s checks against the game of `games` that has its ship:
/// what it found, as held or not.
pub fn checks(log: &Log, games: &[Game]) -> Vec<Result<String, String>> {
    log.checks
        .iter()
        .map(|c| {
            let (Check::At { ship, .. } | Check::Dv { ship, .. }) = c;
            games
                .iter()
                .find(|g| g.index(*ship).is_some())
                .ok_or(format!("no ship has id {ship}"))?
                .check(c)
        })
        .collect()
}
