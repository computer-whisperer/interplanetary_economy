//! The game's state and the loop that advances it (M1b.md, step 2 plan,
//! part 3): each ship's tanks and hold, emptied by its burns and by its
//! station stays' upkeep, its hydrogen boiling off; and where it ran dry.
//!
//! The loop takes events in time order: each burn of a leg, landing or
//! ascent flown, and a tick at each midnight; at one time, the tick comes
//! first, then burns by ship id. At each, a ship first pays what it owes
//! since the last: its station stays' upkeep (each stay's Δv spread evenly
//! over it) and its hydrogen's boil-off. The loop runs from the first
//! fitted ship's start to the earliest time an unfinished ship's design
//! has reached, where every ship pays up and from which the state is
//! pending; with every ship designed, to the last item's end, beyond which
//! nothing but boil-off happens. Any change runs it again from the start.
//! Ships share nothing yet (depots are step 3).

use crate::params::{Economy, PROPELLANTS, PropellantSpec};
use crate::ships::{COMMODITIES, Commodity, Fitted, Load};
use orbit::mission::{Item, ItemKind, MarkerKind};
use orbit::time::Epoch;

/// A ship as the loop reads it.
pub struct Course<'a> {
    pub id: u64,
    /// `None`: massless.
    pub fitted: Option<Fitted>,
    pub epoch: Epoch,
    /// What it does, as far as it is designed (`ShipDesign::items`).
    pub items: Vec<&'a Item>,
    /// Designed to its end.
    pub done: bool,
}

/// A burn a ship's tanks could not make: the ship is stranded there, and
/// its design after it is not flown.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dry {
    pub t: Epoch,
    /// The item (its place in `ShipDesign::items`) and its marker; no
    /// marker for a station stay's upkeep (paid at `t`).
    pub item: usize,
    pub marker: Option<usize>,
    /// The burn, and what of it the tanks could not give, km/s.
    pub dv: f64,
    pub short: f64,
}

/// What the loop found of one ship.
#[derive(Clone, Debug, Default)]
pub struct ShipState {
    pub id: u64,
    /// Its load at its start, and after each burn made and each payment
    /// of upkeep, in time order; none for a massless ship.
    loads: Vec<(Epoch, Load)>,
    /// What of each part of its load a day leaves (boil-off).
    keep: Load,
    /// The propellant each item burned, t, by its place in
    /// `ShipDesign::items`: a leg's, landing's or ascent's burns, a
    /// station stay's upkeep.
    pub used: Vec<f64>,
    pub dry: Option<Dry>,
}

impl ShipState {
    /// What it carries at `t`, its events at `t` made: its last load
    /// boiled off since (and any upkeep since the last payment, at most a
    /// day's, not yet paid); `None` before its start, or for a massless
    /// ship.
    pub fn load(&self, t: Epoch) -> Option<Load> {
        let k = self.loads.partition_point(|(s, _)| s.0 <= t.0);
        let (at, load) = self.loads.get(k.checked_sub(1)?)?;
        Some(boiled(load, &self.keep, t.0 - at.0))
    }

    /// Records `load` at `t`, in time order: a burn a moment before the
    /// ship's start (the planner lets a leg leave up to a millisecond
    /// before the ship arrives) is recorded at its start.
    fn record(&mut self, t: Epoch, load: Load) {
        let last = self.loads.last().map_or(t.0, |(at, _)| at.0);
        self.loads.push((Epoch(t.0.max(last)), load));
    }

    /// Whether it has run dry by `t`.
    pub fn dry_by(&self, t: Epoch) -> Option<&Dry> {
        self.dry.as_ref().filter(|d| d.t.0 <= t.0)
    }
}

/// The state the loop reached.
#[derive(Clone, Debug)]
pub struct State {
    /// By the game's ships, in their order.
    pub ships: Vec<ShipState>,
    /// How far it ran.
    pub until: Epoch,
    /// Every ship was designed to its end: the state is known beyond
    /// `until`, rather than pending from it.
    pub done: bool,
}

impl Default for State {
    fn default() -> Self {
        State {
            ships: Vec::new(),
            until: Epoch(f64::NEG_INFINITY),
            done: true,
        }
    }
}

impl State {
    /// Whether the state at `t` is known: an unfinished ship's next step
    /// may add burns at `until` itself.
    pub fn known(&self, t: Epoch) -> bool {
        self.done || t.0 < self.until.0
    }
}

/// Burns `dv` (km/s) from `load` on a ship of `dry` t burning `spec`: LOX
/// and fuel at its mixture ratio, m(1 − e^(−Δv/vₑ)) of the mass before it.
/// The propellant used, t; else, the burn not made, the Δv its tanks
/// could give, km/s (the scarcer of LOX and fuel bounds it).
fn burn(load: &mut Load, dry: f64, spec: &PropellantSpec, dv: f64) -> Result<f64, f64> {
    let m = dry + load.propellant() + load.cargo();
    let used = -m * (-dv / spec.ve).exp_m1();
    let s = spec.lox_share;
    let burnable = (load.lox / s).min(load.fuel / (1.0 - s));
    if used > burnable {
        return Err(spec.ve * (m / (m - burnable)).ln());
    }
    load.lox = (load.lox - used * s).max(0.0);
    load.fuel = (load.fuel - used * (1.0 - s)).max(0.0);
    Ok(used)
}

/// `load` after `days` of boil-off, `keep` a day's.
fn boiled(load: &Load, keep: &Load, days: f64) -> Load {
    let k = |x: f64, keep: f64| match keep == 1.0 {
        true => x,
        false => x * keep.powf(days),
    };
    Load {
        lox: k(load.lox, keep.lox),
        fuel: k(load.fuel, keep.fuel),
        cargo: std::array::from_fn(|c| k(load.cargo[c], keep.cargo[c])),
    }
}

/// What of a ship's load burning `fuel` a day leaves: of each fuel, less
/// its propellant's boil-off (hydrogen's; methane's is none); all else.
fn keep(economy: &Economy, fuel: Commodity) -> Load {
    let kept = |c: Commodity| {
        PROPELLANTS
            .iter()
            .find(|&&p| Commodity::fuel(p) == c)
            .map_or(1.0, |&p| 1.0 - economy.propellant(p).fuel_boiloff_per_day)
    };
    Load {
        lox: 1.0,
        fuel: kept(fuel),
        cargo: COMMODITIES.map(kept),
    }
}

/// Whether `item` is flown by burning its markers (but jumps, junctions
/// not flown): a leg's, landing's or ascent's. A capture's circularizing
/// burn is not: its leg's pass counts the rule's (`End::Atmosphere`'s
/// `after`).
fn burned(item: &Item) -> bool {
    matches!(
        item.kind,
        ItemKind::Leg | ItemKind::Landing | ItemKind::Ascent
    )
}

/// A fitted ship as the loop runs it.
struct Running<'a> {
    dry: f64,
    spec: &'a PropellantSpec,
    /// Its station stays with upkeep, in time order: their places in its
    /// items, start, end and Δv; and the first not yet paid in full.
    stays: Vec<(usize, f64, f64, f64)>,
    next: usize,
    /// What it carries, its upkeep and boil-off paid to `paid`.
    load: Load,
    paid: f64,
}

impl Running<'_> {
    /// Pays upkeep and boil-off to `t`, into `s`: whether it paid upkeep
    /// (or ran dry doing so). The upkeep burns from the load as it was at
    /// `paid`, before that while's boil-off (at most a day's, a few parts
    /// in ten thousand of the hydrogen).
    fn pay(&mut self, s: &mut ShipState, t: f64) -> bool {
        if t <= self.paid {
            return false;
        }
        let mut upkept = false;
        while s.dry.is_none()
            && let Some(&(i, start, end, dv)) = self.stays.get(self.next)
        {
            if start >= t {
                break;
            }
            let stayed = end.min(t) - start.max(self.paid);
            if stayed > 0.0 {
                let dv = dv * stayed / (end - start);
                upkept = true;
                match burn(&mut self.load, self.dry, self.spec, dv) {
                    Ok(used) => s.used[i] += used,
                    Err(could) => {
                        s.dry = Some(Dry {
                            t: Epoch(t),
                            item: i,
                            marker: None,
                            dv,
                            short: dv - could,
                        })
                    }
                }
            }
            if end > t {
                break;
            }
            self.next += 1;
        }
        self.load = boiled(&self.load, &s.keep, t - self.paid);
        self.paid = t;
        upkept
    }
}

/// The loop, over `ships` in `economy`.
pub fn run(ships: &[Course], economy: &Economy) -> State {
    let reached = |c: &Course| c.items.last().map_or(c.epoch, |i| i.end).0;
    let done = ships.iter().all(|c| c.done);
    let until = match done {
        true => ships.iter().map(reached).fold(f64::NEG_INFINITY, f64::max),
        false => ships
            .iter()
            .filter(|c| !c.done)
            .map(reached)
            .fold(f64::INFINITY, f64::min),
    };
    let mut out: Vec<ShipState> = Vec::new();
    let mut runs: Vec<Option<Running>> = Vec::new();
    for c in ships {
        let mut s = ShipState {
            id: c.id,
            used: vec![0.0; c.items.len()],
            ..ShipState::default()
        };
        runs.push(c.fitted.map(|f| {
            let class = &economy.ships[f.class];
            s.keep = keep(economy, Commodity::fuel(class.propellant));
            s.loads.push((c.epoch, f.load));
            Running {
                dry: class.dry,
                spec: economy.propellant(class.propellant),
                stays: c
                    .items
                    .iter()
                    .enumerate()
                    .filter(|(_, i)| i.kind == ItemKind::Stay && i.dv > 0.0)
                    .filter(|(_, i)| i.end.0 > i.start.0)
                    .map(|(k, i)| (k, i.start.0, i.end.0, i.dv))
                    .collect(),
                next: 0,
                load: f.load,
                paid: c.epoch.0,
            }
        }));
        out.push(s);
    }

    // Burns, in time order, then by ship id; one ship's in its order.
    let mut burns: Vec<(Epoch, u64, usize, usize, usize)> = Vec::new();
    for (k, c) in ships.iter().enumerate() {
        if c.fitted.is_none() {
            continue;
        }
        for (i, item) in c.items.iter().enumerate().filter(|(_, i)| burned(i)) {
            for (j, m) in item.markers.iter().enumerate() {
                if !matches!(m.kind, MarkerKind::Jump { .. }) && m.t.0 <= until {
                    burns.push((m.t, c.id, k, i, j));
                }
            }
        }
    }
    burns.sort_by(|a, b| {
        a.0.0
            .total_cmp(&b.0.0)
            .then(a.1.cmp(&b.1))
            .then((a.3, a.4).cmp(&(b.3, b.4)))
    });

    let first = ships
        .iter()
        .filter(|c| c.fitted.is_some())
        .map(|c| c.epoch.0)
        .fold(f64::INFINITY, f64::min);
    // Midnight is half a day past a whole J2000 day; the first is after
    // the first start.
    let mut midnight = (first - 0.5).floor() + 1.5;
    let mut next = 0;
    loop {
        let tick = midnight <= until && burns.get(next).is_none_or(|b| midnight <= b.0.0);
        if tick {
            for ((c, s), r) in ships.iter().zip(&mut out).zip(&mut runs) {
                if let Some(r) = r
                    && c.epoch.0 < midnight
                    && r.pay(s, midnight)
                {
                    s.record(Epoch(midnight), r.load);
                }
            }
            midnight += 1.0;
        } else if let Some(&(t, _, k, i, j)) = burns.get(next) {
            next += 1;
            let (s, r) = (&mut out[k], runs[k].as_mut().expect("a fitted ship's burn"));
            let mut changed = r.pay(s, t.0);
            if s.dry.is_none() {
                let dv = ships[k].items[i].markers[j].dv;
                match burn(&mut r.load, r.dry, r.spec, dv) {
                    Ok(used) => {
                        s.used[i] += used;
                        changed = true;
                    }
                    Err(could) => {
                        s.dry = Some(Dry {
                            t,
                            item: i,
                            marker: Some(j),
                            dv,
                            short: dv - could,
                        })
                    }
                }
            }
            if changed {
                s.record(t, r.load);
            }
        } else {
            break;
        }
    }
    // Every ship pays up to where the loop stops.
    for ((c, s), r) in ships.iter().zip(&mut out).zip(&mut runs) {
        if let Some(r) = r
            && c.epoch.0 < until
            && r.pay(s, until)
        {
            s.record(Epoch(until), r.load);
        }
    }
    State {
        ships: out,
        until: Epoch(until),
        done,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::{Draw, EconomyFile, Propellant};
    use orbit::ephem::Ephemeris;
    use orbit::mission::Marker;
    use orbit::routes::RouteGraph;

    fn economy() -> Economy {
        EconomyFile::standard().draw(
            Ephemeris::standard(),
            RouteGraph::standard(),
            &mut Draw::Central,
        )
    }

    /// 2045-03-01 00:00, as J2000 days.
    fn day0() -> f64 {
        Epoch::from_ymd(2045, 3, 1).0
    }

    fn item(kind: ItemKind, start: f64, end: f64, dv: f64, burns: &[(f64, f64)]) -> Item {
        Item {
            step: 0,
            kind,
            label: String::new(),
            start: Epoch(day0() + start),
            end: Epoch(day0() + end),
            dv,
            track: None,
            markers: burns
                .iter()
                .map(|&(t, dv)| Marker {
                    t: Epoch(day0() + t),
                    kind: MarkerKind::Burn,
                    dv,
                    pass: None,
                })
                .collect(),
            options: Vec::new(),
            window: None,
            seconds: 0.0,
            note: None,
        }
    }

    /// A ship of class `id` with `propellant` t at the mixture ratio,
    /// starting `start` days after `day0()`.
    fn course<'a>(
        economy: &Economy,
        id: &str,
        propellant: f64,
        start: f64,
        items: Vec<&'a Item>,
    ) -> Course<'a> {
        let class = economy.ships.iter().position(|c| c.id == id).unwrap();
        let s = economy
            .propellant(economy.ships[class].propellant)
            .lox_share;
        Course {
            id: 0,
            fitted: Some(Fitted {
                class,
                load: Load {
                    lox: propellant * s,
                    fuel: propellant * (1.0 - s),
                    ..Load::default()
                },
            }),
            epoch: Epoch(day0() + start),
            items,
            done: true,
        }
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() <= 1e-12 * a.abs().max(b.abs())
    }

    /// Short of LOX or of fuel, a burn gives what the scarcer allows at
    /// the mixture ratio.
    #[test]
    fn the_scarcer_bounds_a_burn() {
        let e = economy();
        let spec = *e.propellant(Propellant::Hydrolox);
        let s = spec.lox_share;
        for (lox, fuel) in [(60.0, 2.0), (6.0, 20.0)] {
            let mut load = Load {
                lox,
                fuel,
                ..Load::default()
            };
            let burnable = (lox / s).min(fuel / (1.0 - s));
            let m = 18.0 + lox + fuel;
            let could = burn(&mut load, 18.0, &spec, 5.0).unwrap_err();
            assert!(close(could, spec.ve * (m / (m - burnable)).ln()), "{could}");
            assert_eq!((load.lox, load.fuel), (lox, fuel), "not made");
        }
    }

    /// Upkeep is paid before a burn: a stay's last part-day before the leg
    /// that follows (found in review: it was paid at the next midnight,
    /// after the leg's burns, and the ship ran dry in the stay it had
    /// left); and where the loop stops, a stay last and no midnight after
    /// it (found in a second review: untested).
    #[test]
    fn upkeep_is_paid_before_each_burn_and_at_the_end() {
        let e = economy();
        let lander = e.ships.iter().find(|c| c.id == "lander").unwrap();
        let ve = e.propellant(lander.propellant).ve;
        let stay = item(ItemKind::Stay, 0.0, 1.5, 0.2, &[]);
        let leg = item(ItemKind::Leg, 1.5, 1.95, 1.5, &[(1.5, 1.0), (1.9, 0.5)]);
        // Enough for 1.69 km/s: the stay and the first burn, not the
        // second.
        let p = lander.dry * ((1.69 / ve).exp() - 1.0);
        let state = run(&[course(&e, "lander", p, 0.0, vec![&stay, &leg])], &e);
        let dry = state.ships[0].dry.unwrap();
        assert_eq!((dry.item, dry.marker), (1, Some(1)), "{dry:?}");
        assert!(close(dry.short, 1.7 - 1.69), "{}", dry.short);

        // A stay from 06:00 to noon two days on, the game's last item.
        let stay = item(ItemKind::Stay, 0.25, 2.5, 0.1, &[]);
        let state = run(&[course(&e, "lander", 300.0, 0.25, vec![&stay])], &e);
        let m0 = lander.dry + 300.0;
        let end = state.ships[0].load(Epoch(day0() + 2.5)).unwrap();
        let left = m0 * (-0.1f64 / ve).exp() - lander.dry;
        assert!(close(end.propellant(), left), "{} {left}", end.propellant());
        assert!(close(
            state.ships[0].used[0],
            m0 * (1.0 - (-0.1f64 / ve).exp())
        ));
    }

    /// Parked ten days, a hydrolox tug's last burn, at the game's end, is
    /// made from what boil-off left (found in a second review: neither the
    /// loop's boil-off nor a burn at the end was tested).
    #[test]
    fn the_last_burn_is_made_from_what_boiled_off() {
        let e = economy();
        let tug = e.ships.iter().find(|c| c.id == "tug-h").unwrap();
        let spec = e.propellant(tug.propellant);
        let leg = item(ItemKind::Leg, 9.9, 10.0, 1.0, &[(10.0, 1.0)]);
        let state = run(&[course(&e, "tug-h", 150.0, 0.0, vec![&leg])], &e);
        let s = spec.lox_share;
        let kept = (1.0 - spec.fuel_boiloff_per_day).powi(10);
        let (lox, fuel) = (150.0 * s, 150.0 * (1.0 - s) * kept);
        let used = (tug.dry + lox + fuel) * (1.0 - (-1.0 / spec.ve).exp());
        assert!(
            close(state.ships[0].used[0], used),
            "{:?}",
            state.ships[0].used
        );
        let after = state.ships[0].load(Epoch(day0() + 10.0)).unwrap();
        assert!(close(after.lox, lox - used * s));
        assert!(close(after.fuel, fuel - used * (1.0 - s)));
    }

    /// A stay's upkeep the tanks cannot pay strands the ship in it, at the
    /// midnight it falls short; it keeps no station after.
    #[test]
    fn upkeep_can_run_a_ship_dry() {
        let e = economy();
        let ve = e.propellant(Propellant::Methalox).ve;
        let stay = item(ItemKind::Stay, 0.0, 10.0, 1.0, &[]);
        let later = item(ItemKind::Stay, 10.0, 20.0, 1.0, &[]);
        let state = run(&[course(&e, "lander", 2.0, 0.0, vec![&stay, &later])], &e);
        let s = &state.ships[0];
        // 0.179 km/s in the tanks: the first day's 0.1, not the second's.
        let dry = s.dry.unwrap();
        assert_eq!((dry.item, dry.marker), (0, None));
        assert_eq!(dry.t, Epoch(day0() + 2.0));
        assert!(
            close(dry.short, 0.2 - ve * (42.0f64 / 40.0).ln()),
            "{dry:?}"
        );
        assert_eq!(s.used[1], 0.0);
    }

    /// Hydrogen boils off from a ship's start, part-days and all, and on
    /// after the game's last item; one unfinished ship leaves the state
    /// pending from where its design has reached.
    #[test]
    fn hydrogen_boils_off_from_the_start_and_past_the_end() {
        let e = economy();
        let f = e.propellant(Propellant::Hydrolox).fuel_boiloff_per_day;
        let leg = item(ItemKind::Leg, 0.25, 0.3, 0.0, &[(0.25, 0.0)]);
        let state = run(&[course(&e, "tug-h", 150.0, 0.25, vec![&leg])], &e);
        let s = &state.ships[0];
        let f0 = s.load(Epoch(day0() + 0.25)).unwrap().fuel;
        for days in [0.75, 39.75] {
            let fuel = s.load(Epoch(day0() + 0.25 + days)).unwrap().fuel;
            assert!(close(fuel, f0 * (1.0 - f).powf(days)), "{days}: {fuel}");
        }
        assert!(state.known(Epoch(day0() + 40.0)));

        // Pending from the earlier of two unfinished ships' reach.
        let later = item(ItemKind::Leg, 4.0, 5.0, 0.0, &[(4.0, 0.0)]);
        let mut unfinished = [
            course(&e, "tug-h", 150.0, 0.25, vec![&leg]),
            course(&e, "tug-h", 150.0, 0.25, vec![&later]),
        ];
        for c in &mut unfinished {
            c.done = false;
        }
        let state = run(&unfinished, &e);
        assert_eq!(state.until, Epoch(day0() + 0.3));
        assert!(state.known(Epoch(day0() + 0.29)));
        assert!(
            !state.known(Epoch(day0() + 0.3)),
            "the next step may burn then"
        );
    }

    /// A burn a moment before the ship's start is recorded at it: nothing
    /// is known of the ship before it.
    #[test]
    fn a_burn_before_the_start_is_recorded_at_it() {
        let e = economy();
        let leg = item(ItemKind::Leg, -1e-9, 0.1, 0.5, &[(-1e-9, 0.5)]);
        let state = run(&[course(&e, "lander", 300.0, 0.0, vec![&leg])], &e);
        let s = &state.ships[0];
        assert!(s.load(Epoch(day0() - 5e-10)).is_none());
        assert!(s.load(Epoch(day0())).unwrap().propellant() < 300.0);
    }
}
