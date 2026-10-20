//! The game's state and the loop that advances it (M1b.md, step 2 plan,
//! part 3; step 3 plan, part 5): each ship's tanks and hold, emptied by
//! its burns and by its station stays' upkeep, its hydrogen boiling off;
//! what it gives to and takes from the vessels it meets, and buys from
//! Earth at LEO, as its legs' manifests say; and where it ran dry.
//!
//! The loop takes events in time order: each burn of a leg, landing or
//! ascent flown; each stop's gives, at the end of the step's last item,
//! and takes, at the start of the next leg, landing or ascent; and a tick
//! at each midnight. At one time: the tick, then the burns that end an
//! item (an arrival's), then gives, then takes, then the other burns (a
//! departure's), each by ship id. At each, a ship first pays what it owes
//! since the last: its station stays' upkeep (each stay's Δv spread
//! evenly over it) and its hydrogen's boil-off; so does the vessel it
//! transfers with. The loop runs from the first fitted ship's start to
//! the earliest time an unfinished ship's design has reached, where every
//! ship pays up and from which the state is pending; with every ship
//! designed, to the last item's end, beyond which nothing but boil-off
//! happens. Any change runs it again from the start.

use crate::params::{Economy, PROPELLANTS, PropellantSpec};
use crate::ships::{COMMODITIES, Commodity, Fitted, GRAM, Load, Manifest};
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
    /// Its stops, by step: what its legs' manifests say to have after
    /// each, and with whom; `None` for a wait, or a leg without one.
    pub stops: Vec<Option<Stop>>,
    /// Starts at LEO, where it is bought with its load (in either mode,
    /// so that a career log opened in the sandbox is the same game); a
    /// ship starting elsewhere is simply there.
    pub bought: bool,
}

/// A stop a leg's manifest speaks of (M1b.md, step 3 plan, parts 4 and
/// 5).
#[derive(Clone, Debug, PartialEq)]
pub struct Stop {
    pub with: With,
    /// The manifest standing when the ship arrives: what it carries above
    /// it is given.
    pub arrive: Manifest,
    /// The manifest as it stands when the ship leaves (a re-plan's `here`
    /// made while it stays changes it): what it carries below it is
    /// taken.
    pub leave: Manifest,
}

/// Whom a ship transfers with at a stop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum With {
    /// The vessel met, there after its leg `leg` (from its start, without
    /// one).
    Vessel { id: u64, leg: Option<usize> },
    /// Earth, at LEO: a take is bought.
    Earth,
    /// No vessel is met, and Earth does not sell here.
    NoOne,
}

/// What a ship gave or took at a stop, or tried to.
#[derive(Clone, Debug, PartialEq)]
pub struct Transfer {
    pub t: Epoch,
    /// The step whose manifest asked for it.
    pub step: usize,
    pub with: With,
    pub kind: Kind,
    pub commodity: Commodity,
    /// Asked for and got, t.
    pub asked: f64,
    pub got: f64,
    /// Why it fell short, if it did.
    pub short: Option<Short>,
    /// What a take at LEO cost, $M: launch at the time, and the goods.
    pub cost_musd: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Give,
    Take,
}

/// Why a transfer fell short.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Short {
    /// The vessel had less.
    Stock,
    /// The tanks or hold receiving it were full.
    Capacity,
    /// The vessel is not there yet.
    NotYet,
    /// The vessel has left.
    Gone,
    /// The vessel ran dry and drifts.
    Dry,
    /// No one to transfer with: no vessel met away from LEO, a vessel
    /// without a class, or a give at LEO, where Earth sells and does not
    /// buy.
    NoOne,
    /// Earth does not sell it (metal).
    NotSold,
}

impl std::fmt::Display for Short {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(match self {
            Short::Stock => "the vessel has less",
            Short::Capacity => "no room for it",
            Short::NotYet => "the vessel is not there yet",
            Short::Gone => "the vessel has left",
            Short::Dry => "the vessel ran dry",
            Short::NoOne => "no one to transfer with",
            Short::NotSold => "Earth does not sell it",
        })
    }
}

/// What a ship starting at LEO cost there: the ship, and each part of its
/// load (launch at the time, and the goods).
#[derive(Clone, Debug, PartialEq)]
pub struct Purchase {
    pub t: Epoch,
    pub what: Bought,
    pub tonnes: f64,
    pub musd: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bought {
    Ship,
    Commodity(Commodity),
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

impl Dry {
    /// Where it ran dry, `item` being its item: "at llo → moon-south-pole"
    /// (a burn), or "keeping station at nrho" (a stay's upkeep).
    pub fn place(&self, item: &Item) -> String {
        match self.marker {
            Some(_) => format!("at {}", item.label),
            None => format!("keeping station {}", item.label),
        }
    }
}

/// What the loop found of one ship.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShipState {
    pub id: u64,
    /// Its load at its start, and after each burn made, each payment of
    /// upkeep and each transfer, in time order; none for a massless ship.
    loads: Vec<(Epoch, Load)>,
    /// What of each part of its load a day leaves (boil-off).
    keep: Load,
    /// The propellant each item burned, t, by its place in
    /// `ShipDesign::items`: a leg's, landing's or ascent's burns, a
    /// station stay's upkeep.
    pub used: Vec<f64>,
    /// What it carried after each item's last burn made, by its place:
    /// none for an item that burned nothing.
    pub after: Vec<Option<Load>>,
    pub dry: Option<Dry>,
    /// What its manifests had it give and take, in time order.
    pub transfers: Vec<Transfer>,
    /// What it was bought for at its start, if it was.
    pub purchases: Vec<Purchase>,
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

/// $/kg of `c` bought from Earth, launch apart: propellant's and
/// equipment's prices; payload pays launch only; metal is not sold.
fn goods_usd_per_kg(economy: &Economy, c: Commodity) -> Option<f64> {
    match c {
        Commodity::Lox | Commodity::Ch4 | Commodity::Lh2 => Some(economy.propellant_usd_per_kg),
        Commodity::Equipment => Some(economy.equipment_usd_per_kg),
        Commodity::Payload => Some(0.0),
        Commodity::Metal => None,
    }
}

/// What `tonnes` of `c` bought at LEO at `t` cost, $M: launch at the time
/// and the goods; `None` for what Earth does not sell.
fn bought_musd(economy: &Economy, c: Commodity, tonnes: f64, t: Epoch) -> Option<f64> {
    let usd_per_kg = economy.launch_usd_per_kg_at(t) + goods_usd_per_kg(economy, c)?;
    Some(tonnes * 1e3 * usd_per_kg / 1e6)
}

/// A fitted ship as the loop runs it.
struct Running<'a> {
    dry: f64,
    spec: &'a PropellantSpec,
    /// The fuel it burns, and its tanks' and hold's capacity, t.
    fuel: Commodity,
    tank: f64,
    hold: f64,
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

    /// Room left in its tanks, or its hold, t.
    fn room(&self, place: Place) -> f64 {
        match place {
            Place::Tanks => (self.tank - self.load.propellant()).max(0.0),
            Place::Hold => (self.hold - self.load.cargo()).max(0.0),
        }
    }

    /// Where `c` goes aboard: LOX and the fuel it burns into its tanks,
    /// the rest into its hold.
    fn stows(&self, c: Commodity) -> Place {
        match c == Commodity::Lox || c == self.fuel {
            true => Place::Tanks,
            false => Place::Hold,
        }
    }

    /// What it carries of `c` in `place`.
    fn carried(&mut self, c: Commodity, place: Place) -> &mut f64 {
        match (place, c) {
            (Place::Tanks, Commodity::Lox) => &mut self.load.lox,
            (Place::Tanks, _) => &mut self.load.fuel,
            (Place::Hold, c) => &mut self.load.cargo[c as usize],
        }
    }
}

/// A ship's tanks or its hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Place {
    Tanks,
    Hold,
}

/// Where a manifest's targets are measured: a propellant target against
/// the tanks, LOX and fuel at the mixture ratio; a cargo target against
/// the hold. The amounts `kind` moves: what the ship carries above a
/// target (a give) or below one (a take), by commodity and place.
fn asks(r: &Running, m: &Manifest, kind: Kind) -> Vec<(Commodity, Place, f64)> {
    let mut out = Vec::new();
    let mut ask = |c: Commodity, place: Place, have: f64, target: f64| {
        let amount = match kind {
            Kind::Give => have - target,
            Kind::Take => target - have,
        };
        if amount > GRAM {
            out.push((c, place, amount));
        }
    };
    if let Some(p) = m.propellant {
        let s = r.spec.lox_share;
        ask(Commodity::Lox, Place::Tanks, r.load.lox, p * s);
        ask(r.fuel, Place::Tanks, r.load.fuel, p * (1.0 - s));
    }
    for (i, target) in m.cargo.iter().enumerate() {
        if let Some(target) = target {
            ask(COMMODITIES[i], Place::Hold, r.load.cargo[i], *target);
        }
    }
    out
}

/// When ship `c` arrives at the stop of its step `step`: the end of the
/// step's last item, if its leg was designed.
fn arrival(c: &Course, step: usize) -> Option<f64> {
    let last = c.items.iter().rev().find(|i| i.step == step)?;
    match last.kind {
        ItemKind::Failed(_) | ItemKind::Stay => None,
        _ => Some(last.end.0),
    }
}

/// When ship `c` leaves the stop of its step `step`: the start of its next
/// leg, landing or ascent (or its first burn, a moment earlier); `None` if
/// it has none.
fn departure(c: &Course, step: usize) -> Option<f64> {
    let next = c.items.iter().find(|i| i.step > step && burned(i))?;
    let first = next
        .markers
        .iter()
        .find(|m| !matches!(m.kind, MarkerKind::Jump { .. }))
        .map_or(f64::INFINITY, |m| m.t.0);
    Some(next.start.0.min(first))
}

/// Whether vessel `v` is at its stop after its leg `leg` (its start,
/// without one) at `t`, to transfer with: arrived, not yet left, and not
/// run dry; else why not.
fn there(v: &Course, s: &ShipState, leg: Option<usize>, t: f64) -> Result<(), Short> {
    let from = match leg {
        None => Some(v.epoch.0),
        Some(l) => arrival(v, l),
    };
    if from.is_none_or(|from| t < from) {
        return Err(Short::NotYet);
    }
    let to = leg.map_or(0, |l| l + 1);
    let gone = v
        .items
        .iter()
        .find(|i| i.step >= to && burned(i))
        .is_some_and(|next| {
            let first = next
                .markers
                .iter()
                .find(|m| !matches!(m.kind, MarkerKind::Jump { .. }))
                .map_or(f64::INFINITY, |m| m.t.0);
            next.start.0.min(first) < t
        });
    if gone {
        return Err(Short::Gone);
    }
    if s.dry_by(Epoch(t)).is_some() {
        return Err(Short::Dry);
    }
    Ok(())
}

/// Two of `v`, to change together.
fn two<T>(v: &mut [T], a: usize, b: usize) -> (&mut T, &mut T) {
    assert_ne!(a, b, "a ship does not transfer with itself");
    if a < b {
        let (x, y) = v.split_at_mut(b);
        (&mut x[a], &mut y[0])
    } else {
        let (x, y) = v.split_at_mut(a);
        (&mut y[0], &mut x[b])
    }
}

/// Whom a transfer is made with.
enum Partner {
    Vessel(usize),
    Earth,
    NoOne(Short),
}

/// The loop's ships as it runs them.
struct Loop<'a> {
    ships: &'a [Course<'a>],
    economy: &'a Economy,
    /// By ship, what the loop found; and each fitted ship as it runs.
    out: Vec<ShipState>,
    runs: Vec<Option<Running<'a>>>,
}

impl Loop<'_> {
    /// The gives or takes (`kind`) of ship `k`'s stop at `step`, at `t`:
    /// both vessels paid up to `t`, then each amount the manifest asks
    /// moved as far as it can be, and recorded (M1b.md, step 3 plan, part
    /// 5). A ship that ran dry is stranded and transfers nothing.
    fn transfer(&mut self, t: f64, k: usize, step: usize, kind: Kind) {
        let (ships, economy) = (self.ships, self.economy);
        let (out, runs) = (self.out.as_mut_slice(), self.runs.as_mut_slice());
        if out[k].dry.is_some() {
            return;
        }
        let c = &ships[k];
        let stop = c.stops[step].as_ref().expect("a stop's event");
        let manifest = match kind {
            Kind::Give => &stop.arrive,
            Kind::Take => &stop.leave,
        };
        let mut changed = runs[k]
            .as_mut()
            .expect("a fitted ship's stop")
            .pay(&mut out[k], t);
        let wanted = asks(runs[k].as_ref().unwrap(), manifest, kind);
        let partner = match stop.with {
            With::Vessel { id, leg } => match ships.iter().position(|v| v.id == id) {
                Some(v) if runs[v].is_some() => match there(&ships[v], &out[v], leg, t) {
                    Ok(()) => Partner::Vessel(v),
                    Err(why) => Partner::NoOne(why),
                },
                _ => Partner::NoOne(Short::NoOne),
            },
            With::Earth => Partner::Earth,
            With::NoOne => Partner::NoOne(Short::NoOne),
        };
        let mut vessel_changed = false;
        if let Partner::Vessel(v) = partner {
            let (vs, vr) = (&mut out[v], runs[v].as_mut().unwrap());
            vessel_changed = vr.pay(vs, t);
        }
        for (commodity, place, asked) in wanted {
            let mut got = 0.0;
            let mut short = None;
            let mut cost_musd = None;
            match (&partner, kind) {
                (Partner::Vessel(v), Kind::Give) => {
                    let (r, vr) = two(runs, k, *v);
                    let (r, vr) = (r.as_mut().unwrap(), vr.as_mut().unwrap());
                    let into = vr.stows(commodity);
                    got = asked.min(vr.room(into));
                    *r.carried(commodity, place) -= got;
                    *vr.carried(commodity, into) += got;
                    if got < asked - GRAM {
                        short = Some(Short::Capacity);
                    }
                }
                (Partner::Vessel(v), Kind::Take) => {
                    let (r, vr) = two(runs, k, *v);
                    let (r, vr) = (r.as_mut().unwrap(), vr.as_mut().unwrap());
                    // From the vessel's hold first, then its tanks.
                    let in_hold = vr.load.cargo[commodity as usize];
                    let in_tanks = match vr.stows(commodity) {
                        Place::Tanks => *vr.carried(commodity, Place::Tanks),
                        Place::Hold => 0.0,
                    };
                    let room = r.room(place);
                    got = asked.min(in_hold + in_tanks).min(room);
                    let from_hold = got.min(in_hold);
                    vr.load.cargo[commodity as usize] -= from_hold;
                    if got > from_hold {
                        *vr.carried(commodity, Place::Tanks) -= got - from_hold;
                    }
                    *r.carried(commodity, place) += got;
                    if in_hold + in_tanks < asked - GRAM {
                        short = Some(Short::Stock);
                    } else if room < asked - GRAM {
                        short = Some(Short::Capacity);
                    }
                }
                (Partner::Earth, Kind::Take) => {
                    let r = runs[k].as_mut().unwrap();
                    match bought_musd(economy, commodity, 1.0, Epoch(t)) {
                        None => short = Some(Short::NotSold),
                        Some(per_tonne) => {
                            got = asked.min(r.room(place));
                            *r.carried(commodity, place) += got;
                            cost_musd = Some(got * per_tonne);
                            if got < asked - GRAM {
                                short = Some(Short::Capacity);
                            }
                        }
                    }
                }
                (Partner::Earth, Kind::Give) => short = Some(Short::NoOne),
                (Partner::NoOne(why), _) => short = Some(*why),
            }
            if got > 0.0 {
                changed = true;
                vessel_changed |= matches!(partner, Partner::Vessel(_));
            }
            out[k].transfers.push(Transfer {
                t: Epoch(t),
                step,
                with: stop.with,
                kind,
                commodity,
                asked,
                got,
                short,
                cost_musd,
            });
        }
        if changed {
            let load = runs[k].as_ref().unwrap().load;
            out[k].record(Epoch(t), load);
        }
        if let Partner::Vessel(v) = partner
            && vessel_changed
        {
            let load = runs[v].as_ref().unwrap().load;
            out[v].record(Epoch(t), load);
        }
    }
}

/// An event of the loop but the tick, in the order events at one time are
/// taken.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Phase {
    /// A burn at the end of its item: an arrival's.
    Arrival,
    Gives,
    Takes,
    /// Any other burn: a departure's, or one in flight.
    Burn,
}

/// A burn's marker is at its item's end within this, days (0.1 ms): the
/// planner's rounding.
const AT_THE_END_DAYS: f64 = 1e-9;

/// What an event does.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum What {
    /// Item and marker.
    Burn(usize, usize),
    /// A stop's gives or takes: the step.
    Transfer(usize),
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
            after: vec![None; c.items.len()],
            ..ShipState::default()
        };
        runs.push(c.fitted.map(|f| {
            let class = &economy.ships[f.class];
            s.keep = keep(economy, Commodity::fuel(class.propellant));
            s.loads.push((c.epoch, f.load));
            if c.bought {
                s.purchases.push(Purchase {
                    t: c.epoch,
                    what: Bought::Ship,
                    tonnes: 0.0,
                    musd: class.price_musd,
                });
                let fuel = Commodity::fuel(class.propellant);
                let parts = [(Commodity::Lox, f.load.lox), (fuel, f.load.fuel)]
                    .into_iter()
                    .chain(COMMODITIES.iter().map(|&c| (c, f.load.cargo[c as usize])));
                for (commodity, tonnes) in parts.filter(|(_, t)| *t > 0.0) {
                    // Metal is not sold: career refuses a load of it
                    // (`game`); the sandbox's pays its launch.
                    let goods = goods_usd_per_kg(economy, commodity).unwrap_or(0.0);
                    let musd = tonnes * 1e3 * (economy.launch_usd_per_kg_at(c.epoch) + goods) / 1e6;
                    s.purchases.push(Purchase {
                        t: c.epoch,
                        what: Bought::Commodity(commodity),
                        tonnes,
                        musd,
                    });
                }
            }
            Running {
                dry: class.dry,
                spec: economy.propellant(class.propellant),
                fuel: Commodity::fuel(class.propellant),
                tank: class.tank,
                hold: class.cargo,
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
    let mut l = Loop {
        ships,
        economy,
        out,
        runs,
    };

    // Events in time order, then by phase and ship id; one ship's burns
    // in its order.
    let mut events: Vec<(f64, Phase, u64, usize, What)> = Vec::new();
    for (k, c) in ships.iter().enumerate() {
        if c.fitted.is_none() {
            continue;
        }
        for (i, item) in c.items.iter().enumerate().filter(|(_, i)| burned(i)) {
            for (j, m) in item.markers.iter().enumerate() {
                if matches!(m.kind, MarkerKind::Jump { .. }) || m.t.0 > until {
                    continue;
                }
                let phase = match m.t.0 >= item.end.0 - AT_THE_END_DAYS {
                    true => Phase::Arrival,
                    false => Phase::Burn,
                };
                events.push((m.t.0, phase, c.id, k, What::Burn(i, j)));
            }
        }
        for (step, stop) in c.stops.iter().enumerate() {
            let Some(stop) = stop else { continue };
            if !stop.arrive.is_empty()
                && let Some(t) = arrival(c, step)
                && t <= until
            {
                events.push((t, Phase::Gives, c.id, k, What::Transfer(step)));
            }
            if !stop.leave.is_empty()
                && let Some(t) = departure(c, step)
                && t <= until
            {
                events.push((t, Phase::Takes, c.id, k, What::Transfer(step)));
            }
        }
    }
    events.sort_by(|a, b| {
        a.0.total_cmp(&b.0)
            .then(a.1.cmp(&b.1))
            .then(a.2.cmp(&b.2))
            .then(a.4.cmp(&b.4))
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
        let tick = midnight <= until && events.get(next).is_none_or(|e| midnight <= e.0);
        if tick {
            for ((c, s), r) in ships.iter().zip(&mut l.out).zip(&mut l.runs) {
                if let Some(r) = r
                    && c.epoch.0 < midnight
                    && r.pay(s, midnight)
                {
                    s.record(Epoch(midnight), r.load);
                }
            }
            midnight += 1.0;
        } else if let Some(&(t, phase, _, k, what)) = events.get(next) {
            next += 1;
            match what {
                What::Burn(i, j) => {
                    let (s, r) = (
                        &mut l.out[k],
                        l.runs[k].as_mut().expect("a fitted ship's burn"),
                    );
                    let mut changed = r.pay(s, t);
                    if s.dry.is_none() {
                        let dv = ships[k].items[i].markers[j].dv;
                        match burn(&mut r.load, r.dry, r.spec, dv) {
                            Ok(used) => {
                                s.used[i] += used;
                                s.after[i] = Some(r.load);
                                changed = true;
                            }
                            Err(could) => {
                                s.dry = Some(Dry {
                                    t: Epoch(t),
                                    item: i,
                                    marker: Some(j),
                                    dv,
                                    short: dv - could,
                                })
                            }
                        }
                    }
                    if changed {
                        s.record(Epoch(t), r.load);
                    }
                }
                What::Transfer(step) => {
                    let kind = match phase {
                        Phase::Gives => Kind::Give,
                        _ => Kind::Take,
                    };
                    l.transfer(t, k, step, kind);
                }
            }
        } else {
            break;
        }
    }
    // Every ship pays up to where the loop stops.
    for ((c, s), r) in ships.iter().zip(&mut l.out).zip(&mut l.runs) {
        if let Some(r) = r
            && c.epoch.0 < until
            && r.pay(s, until)
        {
            s.record(Epoch(until), r.load);
        }
    }
    State {
        ships: l.out,
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
            stops: Vec::new(),
            bought: false,
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
        assert_eq!(state.ships[0].after[0], Some(after));
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
    /// A manifest of `propellant` t and `cargo` by commodity.
    fn manifest(propellant: Option<f64>, cargo: &[(Commodity, f64)]) -> Manifest {
        let mut m = Manifest {
            propellant,
            ..Manifest::default()
        };
        for &(c, t) in cargo {
            m.cargo[c as usize] = Some(t);
        }
        m
    }

    /// A stop with vessel 1 from its start, giving to `arrive` and taking
    /// to `leave`.
    fn stop(arrive: Manifest, leave: Manifest) -> Option<Stop> {
        Some(Stop {
            with: With::Vessel { id: 1, leg: None },
            arrive,
            leave,
        })
    }

    /// A tug (id 0) that arrives at the depot's stop on day 2 (a leg of
    /// two burns, `arrival_dv` km/s at the end), stays, and leaves on day 5
    /// (a leg of two burns, `departure_dv` at its start).
    fn visit(arrival_dv: f64, departure_dv: f64) -> [Item; 3] {
        [
            item(
                ItemKind::Leg,
                1.0,
                2.0,
                arrival_dv,
                &[(1.0, 0.0), (2.0, arrival_dv)],
            ),
            {
                let mut stay = item(ItemKind::Stay, 2.0, 5.0, 0.0, &[]);
                stay.step = 1;
                stay
            },
            {
                let mut leg = item(
                    ItemKind::Leg,
                    5.0,
                    6.0,
                    departure_dv,
                    &[(5.0, departure_dv), (6.0, 0.0)],
                );
                leg.step = 1;
                leg
            },
        ]
    }

    /// The transfers of `s` as (commodity, kind, asked, got, short).
    fn moved(s: &ShipState) -> Vec<(Commodity, Kind, f64, f64, Option<Short>)> {
        s.transfers
            .iter()
            .map(|x| (x.commodity, x.kind, x.asked, x.got, x.short))
            .collect()
    }

    /// A give fills the vessel's tanks with what it burns and its hold with
    /// the rest, to the manifest's targets by commodity at the mixture
    /// ratio; a take drains its hold first, then its tanks (M1b.md, step 3
    /// plan, part 5).
    #[test]
    fn a_give_fills_the_tanks_then_the_hold_and_a_take_drains_them_back() {
        let e = economy();
        let s_h = e.propellant(Propellant::Hydrolox).lox_share;
        let s_m = e.propellant(Propellant::Methalox).lox_share;
        let staying = [item(ItemKind::Stay, 0.0, 30.0, 0.0, &[])];
        let mut depot = course(&e, "hauler", 100.0, 0.0, staying.iter().collect());
        depot.id = 1;
        let legs = visit(0.0, 0.0);
        let mut tug = course(&e, "tug-h", 140.0, 0.0, legs.iter().collect());
        tug.fitted.as_mut().unwrap().load.cargo[Commodity::Equipment as usize] = 10.0;
        tug.stops = vec![
            stop(
                manifest(Some(40.0), &[(Commodity::Equipment, 0.0)]),
                manifest(Some(100.0), &[(Commodity::Equipment, 5.0)]),
            ),
            None,
        ];
        let state = run(&[tug, depot], &e);
        let (tug, depot) = (&state.ships[0], &state.ships[1]);
        let (day2, day5) = (Epoch(day0() + 2.0), Epoch(day0() + 5.0));
        // Given on arrival: LOX into the depot's tanks, hydrogen (which
        // the depot does not burn, and of which two days boiled off) and
        // equipment into its hold.
        let given_lox = 140.0 * s_h - 40.0 * s_h;
        let gives = &moved(tug)[..3];
        let given_lh2 = gives[1].3;
        assert_eq!(
            gives,
            [
                (Commodity::Lox, Kind::Give, given_lox, given_lox, None),
                (Commodity::Lh2, Kind::Give, given_lh2, given_lh2, None),
                (Commodity::Equipment, Kind::Give, 10.0, 10.0, None),
            ]
        );
        let whole = 140.0 * (1.0 - s_h) - 40.0 * (1.0 - s_h);
        assert!(given_lh2 < whole && given_lh2 > whole - 0.05, "{given_lh2}");
        let after = tug.load(day2).unwrap();
        assert!(close(after.lox, 40.0 * s_h), "{after:?}");
        assert!(close(after.fuel, 40.0 * (1.0 - s_h)), "{after:?}");
        assert_eq!(after.cargo(), 0.0);
        let stock = depot.load(day2).unwrap();
        assert!(close(stock.lox, 100.0 * s_m + given_lox), "{stock:?}");
        assert!(close(stock.fuel, 100.0 * (1.0 - s_m)), "{stock:?}");
        assert!(close(stock.cargo[Commodity::Lh2 as usize], given_lh2));
        assert_eq!(stock.cargo[Commodity::Equipment as usize], 10.0);
        // Taken on leaving, to the targets: hydrogen from the hold (what
        // boiled off in three days too), LOX from the tanks, equipment.
        let takes = &moved(tug)[3..];
        assert_eq!(takes.len(), 3);
        assert_eq!(
            takes.iter().map(|x| (x.0, x.1, x.4)).collect::<Vec<_>>(),
            [
                (Commodity::Lox, Kind::Take, None),
                (Commodity::Lh2, Kind::Take, None),
                (Commodity::Equipment, Kind::Take, None),
            ]
        );
        assert!(close(takes[0].2, 60.0 * s_h), "{takes:?}");
        assert!(takes[1].2 > 60.0 * (1.0 - s_h), "{takes:?}");
        assert_eq!(takes[2].2, 5.0);
        assert!(takes.iter().all(|x| x.2 == x.3));
        let after = tug.load(day5).unwrap();
        assert!(close(after.lox, 100.0 * s_h), "{after:?}");
        assert!(close(after.fuel, 100.0 * (1.0 - s_h)), "{after:?}");
        assert_eq!(after.cargo[Commodity::Equipment as usize], 5.0);
        let stock = depot.load(day5).unwrap();
        assert!(
            close(stock.lox, 100.0 * s_m + given_lox - takes[0].3),
            "{stock:?}"
        );
        assert!(stock.cargo[Commodity::Lh2 as usize] < given_lh2 - takes[1].3);
        assert_eq!(stock.cargo[Commodity::Equipment as usize], 5.0);
        assert!(tug.dry.is_none() && depot.dry.is_none());
    }

    /// At one time: the tick, an arrival's burn, then gives, then takes,
    /// then a departure's burn. A tug arriving burns before it gives, so
    /// it keeps its target; one leaving takes before it burns, so it can
    /// make a burn its tanks could not have.
    #[test]
    fn events_at_one_time_go_tick_arrival_gives_takes_departure() {
        let e = economy();
        let s_h = e.propellant(Propellant::Hydrolox).lox_share;
        let staying = [item(ItemKind::Stay, 0.0, 30.0, 0.0, &[])];
        let mut depot = course(&e, "hauler", 0.0, 0.0, staying.iter().collect());
        depot.id = 1;
        // Arrives on day 2 with a 0.5 km/s burn, and gives down to 10 t.
        let arriving = visit(0.5, 0.0);
        let mut a = course(&e, "tug-h", 150.0, 0.0, arriving.iter().collect());
        a.stops = vec![stop(manifest(Some(10.0), &[]), Manifest::default()), None];
        // Arrived on day 1, leaves on day 2 with a 1 km/s burn, taking up
        // to 100 t first: its 20 t could not make the burn.
        let leaving = [
            item(ItemKind::Leg, 0.0, 1.0, 0.0, &[(0.0, 0.0), (1.0, 0.0)]),
            {
                let mut stay = item(ItemKind::Stay, 1.0, 2.0, 0.0, &[]);
                stay.step = 1;
                stay
            },
            {
                let mut leg = item(ItemKind::Leg, 2.0, 3.0, 1.0, &[(2.0, 1.0), (3.0, 0.0)]);
                leg.step = 1;
                leg
            },
        ];
        let mut b = course(&e, "tug-h", 20.0, 0.0, leaving.iter().collect());
        b.id = 2;
        b.stops = vec![stop(Manifest::default(), manifest(Some(100.0), &[])), None];
        let state = run(&[a, b, depot], &e);
        let (a, b, depot) = (&state.ships[0], &state.ships[1], &state.ships[2]);
        let day2 = Epoch(day0() + 2.0);
        assert!(a.dry.is_none() && b.dry.is_none() && depot.dry.is_none());
        let after = a.load(day2).unwrap();
        assert!(close(after.propellant(), 10.0), "{after:?}");
        let burned = a.used[0];
        assert!(burned > 15.0 && burned < 20.0, "{burned}");
        // What it had before the burn (its hydrogen boiled off two days),
        // less the burn, less its target.
        let before = a.load(Epoch(day2.0 - 1e-9)).unwrap().propellant();
        let given: f64 = a.transfers.iter().map(|x| x.got).sum();
        assert!((given - (before - burned - 10.0)).abs() < 1e-9, "{given}");
        // 80 t and the two days' boil-off of its hydrogen.
        let taken: f64 = b.transfers.iter().map(|x| x.got).sum();
        assert!(taken > 80.0 && taken < 80.05, "{:?}", b.transfers);
        assert!(b.transfers.iter().all(|x| x.short.is_none()));
        let after = b.load(day2).unwrap();
        assert!(close(after.propellant(), 100.0 - b.used[2]), "{after:?}");
        assert!(b.used[2] > 20.0, "{}", b.used[2]);
        let lox_given = a
            .transfers
            .iter()
            .find(|x| x.commodity == Commodity::Lox)
            .unwrap()
            .got;
        let stock = depot.load(day2).unwrap();
        assert!(close(stock.lox, lox_given - 80.0 * s_h), "{stock:?}");
    }

    /// A transfer that cannot be made in full is made as far as it can be
    /// and says why: the vessel not there yet, gone, or run dry; its stock;
    /// the capacity receiving it; no one to transfer with; what Earth does
    /// not sell.
    #[test]
    fn a_transfer_short_says_why() {
        let e = economy();
        let staying = [item(ItemKind::Stay, 0.0, 30.0, 0.0, &[])];
        let legs = visit(0.0, 0.0);
        let shorts = |depot: Course, stops: Vec<Option<Stop>>| {
            let mut tug = course(&e, "tug-h", 140.0, 0.0, legs.iter().collect());
            tug.fitted.as_mut().unwrap().load.cargo[Commodity::Equipment as usize] = 10.0;
            tug.stops = stops;
            let state = run(&[tug, depot], &e);
            moved(&state.ships[0])
        };
        fn depot<'a>(e: &Economy, propellant: f64, start: f64, items: Vec<&'a Item>) -> Course<'a> {
            let mut d = course(e, "hauler", propellant, start, items);
            d.id = 1;
            d
        }
        let give_40 = stop(manifest(Some(40.0), &[]), Manifest::default());
        let lox = |m: &Vec<(Commodity, Kind, f64, f64, Option<Short>)>| {
            m.iter().find(|x| x.0 == Commodity::Lox).copied().unwrap()
        };
        // Not there yet: the depot starts on day 3.
        let m = shorts(
            depot(&e, 100.0, 3.0, staying.iter().collect()),
            vec![give_40.clone(), None],
        );
        assert_eq!(lox(&m).3, 0.0);
        assert_eq!(lox(&m).4, Some(Short::NotYet));
        // Gone: the depot leaves on day 1.5.
        let left = [
            item(ItemKind::Stay, 0.0, 1.5, 0.0, &[]),
            item(ItemKind::Leg, 1.5, 3.0, 0.0, &[(1.5, 0.0), (3.0, 0.0)]),
        ];
        let m = shorts(
            depot(&e, 100.0, 0.0, left.iter().collect()),
            vec![give_40.clone(), None],
        );
        assert_eq!(lox(&m).4, Some(Short::Gone));
        // Run dry: the depot's upkeep is more than its tanks hold.
        let keeping = [item(ItemKind::Stay, 0.0, 30.0, 1.0, &[])];
        let m = shorts(
            depot(&e, 0.001, 0.0, keeping.iter().collect()),
            vec![give_40.clone(), None],
        );
        assert_eq!(lox(&m).4, Some(Short::Dry));
        // Stock: a take of more than the depot has.
        let m = shorts(
            depot(&e, 10.0, 0.0, staying.iter().collect()),
            vec![stop(Manifest::default(), manifest(Some(150.0), &[])), None],
        );
        let s_m = e.propellant(Propellant::Methalox).lox_share;
        assert!(close(lox(&m).3, 10.0 * s_m), "{m:?}");
        assert_eq!(lox(&m).4, Some(Short::Stock));
        // Capacity: a give to full tanks, and a take beyond the hold (the
        // tug's 50 t, 10 of equipment aboard).
        let mut full = depot(&e, 1200.0, 0.0, staying.iter().collect());
        full.fitted.as_mut().unwrap().load.cargo[Commodity::Equipment as usize] = 100.0;
        let m = shorts(
            full,
            vec![
                stop(
                    manifest(Some(40.0), &[]),
                    manifest(None, &[(Commodity::Equipment, 60.0)]),
                ),
                None,
            ],
        );
        assert_eq!(lox(&m).3, 0.0);
        assert_eq!(lox(&m).4, Some(Short::Capacity));
        let equipment = m.iter().find(|x| x.1 == Kind::Take).unwrap();
        assert_eq!(
            *equipment,
            (
                Commodity::Equipment,
                Kind::Take,
                50.0,
                40.0,
                Some(Short::Capacity)
            )
        );
        // No one: a stop without a vessel, a vessel without a class, and
        // a give at LEO.
        let no_one = |with: With| {
            Some(Stop {
                with,
                arrive: manifest(Some(40.0), &[]),
                leave: Manifest::default(),
            })
        };
        let m = shorts(
            depot(&e, 100.0, 0.0, staying.iter().collect()),
            vec![no_one(With::NoOne), None],
        );
        assert_eq!(lox(&m).4, Some(Short::NoOne));
        let mut massless = depot(&e, 100.0, 0.0, staying.iter().collect());
        massless.fitted = None;
        let m = shorts(massless, vec![give_40.clone(), None]);
        assert_eq!(lox(&m).4, Some(Short::NoOne));
        let m = shorts(
            depot(&e, 100.0, 0.0, staying.iter().collect()),
            vec![no_one(With::Earth), None],
        );
        assert_eq!(lox(&m).4, Some(Short::NoOne));
        // Not sold: metal from Earth.
        let m = shorts(
            depot(&e, 100.0, 0.0, staying.iter().collect()),
            vec![
                Some(Stop {
                    with: With::Earth,
                    arrive: Manifest::default(),
                    leave: manifest(None, &[(Commodity::Metal, 1.0)]),
                }),
                None,
            ],
        );
        assert_eq!(
            m,
            [(Commodity::Metal, Kind::Take, 1.0, 0.0, Some(Short::NotSold))]
        );
    }

    /// A take at LEO is bought: launch at the day's price, geometric
    /// between the start's and the end's, plus the goods; payload pays
    /// launch only. A career ship is bought at its start with its load.
    #[test]
    fn a_take_at_leo_is_bought_at_the_days_launch_price() {
        let e = economy();
        let [l0, l1] = e.launch_usd_per_kg;
        assert_eq!(e.launch_usd_per_kg_at(e.start), l0);
        assert_eq!(e.launch_usd_per_kg_at(Epoch(e.start.0 - 100.0)), l0);
        assert!(close(e.launch_usd_per_kg_at(e.end), l1));
        assert!(close(e.launch_usd_per_kg_at(Epoch(e.end.0 + 100.0)), l1));
        let mid = Epoch((e.start.0 + e.end.0) / 2.0);
        assert!(close(e.launch_usd_per_kg_at(mid), (l0 * l1).sqrt()));
        assert!(l0 > l1 && e.launch_usd_per_kg_at(Epoch(day0())) < l0);

        let s_h = e.propellant(Propellant::Hydrolox).lox_share;
        let legs = visit(0.0, 0.0);
        let mut tug = course(&e, "tug-h", 100.0, 0.0, legs.iter().collect());
        tug.stops = vec![
            Some(Stop {
                with: With::Earth,
                arrive: Manifest::default(),
                leave: manifest(
                    Some(150.0),
                    &[(Commodity::Equipment, 10.0), (Commodity::Payload, 5.0)],
                ),
            }),
            None,
        ];
        tug.bought = true;
        let state = run(&[tug], &e);
        let tug = &state.ships[0];
        let day5 = Epoch(day0() + 5.0);
        let launch = e.launch_usd_per_kg_at(day5);
        let musd = |tonnes: f64, goods: f64| tonnes * 1e3 * (launch + goods) / 1e6;
        let bought: Vec<(Commodity, f64, f64)> = tug
            .transfers
            .iter()
            .map(|x| (x.commodity, x.got, x.cost_musd.unwrap()))
            .collect();
        assert_eq!(bought.len(), 4);
        assert_eq!(bought[0].0, Commodity::Lox);
        assert!(close(bought[0].1, 50.0 * s_h));
        assert!(close(
            bought[0].2,
            musd(50.0 * s_h, e.propellant_usd_per_kg)
        ));
        assert_eq!(bought[1].0, Commodity::Lh2);
        assert!(
            bought[1].1 > 50.0 * (1.0 - s_h),
            "boiled off: {:?}",
            bought[1]
        );
        assert!(close(
            bought[1].2,
            musd(bought[1].1, e.propellant_usd_per_kg)
        ));
        assert_eq!(bought[2].0, Commodity::Equipment);
        assert!(close(bought[2].2, musd(10.0, e.equipment_usd_per_kg)));
        assert_eq!(bought[3].0, Commodity::Payload);
        assert!(close(bought[3].2, musd(5.0, 0.0)));
        let after = tug.load(day5).unwrap();
        assert!(close(after.propellant(), 150.0));
        assert_eq!(after.cargo(), 15.0);
        // Bought at its start: the ship, its LOX and its hydrogen.
        let launch = e.launch_usd_per_kg_at(Epoch(day0()));
        let tug_h = e.ships.iter().find(|c| c.id == "tug-h").unwrap();
        assert_eq!(tug.purchases.len(), 3);
        assert_eq!(
            (tug.purchases[0].what, tug.purchases[0].musd),
            (Bought::Ship, tug_h.price_musd)
        );
        assert_eq!(tug.purchases[1].what, Bought::Commodity(Commodity::Lox));
        assert!(close(tug.purchases[1].tonnes, 100.0 * s_h));
        assert!(close(
            tug.purchases[1].musd,
            100.0 * s_h * 1e3 * (launch + e.propellant_usd_per_kg) / 1e6
        ));
        assert_eq!(tug.purchases[2].what, Bought::Commodity(Commodity::Lh2));
        assert!(tug.purchases.iter().all(|p| p.t == Epoch(day0())));
    }

    /// A station keeps no reserve: drained by a visitor, it runs dry
    /// keeping station.
    #[test]
    fn a_drained_depot_runs_dry_keeping_station() {
        let e = economy();
        let keeping = [item(ItemKind::Stay, 0.0, 30.0, 0.01, &[])];
        let mut depot = course(&e, "hauler", 50.0, 0.0, keeping.iter().collect());
        depot.id = 1;
        let legs = visit(0.0, 0.0);
        let mut tug = course(&e, "tug-h", 10.0, 0.0, legs.iter().collect());
        tug.stops = vec![stop(Manifest::default(), manifest(Some(150.0), &[])), None];
        let state = run(&[tug, depot], &e);
        let (tug, depot) = (&state.ships[0], &state.ships[1]);
        let taken: Vec<_> = moved(tug);
        assert_eq!(taken.len(), 2);
        assert_eq!(taken[0].4, Some(Short::Stock));
        assert!(taken[0].3 > 35.0 && taken[0].3 < 40.0, "{taken:?}");
        let dry = depot.dry.unwrap();
        assert_eq!(dry.marker, None);
        assert!(dry.t.0 > day0() + 5.0 && dry.t.0 <= day0() + 6.0, "{dry:?}");
        assert!(tug.dry.is_none());
    }
}
