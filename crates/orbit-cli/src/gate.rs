//! `orbit gate`: M1a's gate sweep (M1a.md, Gate and the step 10 plan).
//!
//! Four parts, each written as a CSV in the output directory:
//!
//! - `cold.csv`: every core leg planned from the playbook (`legs::plan`) at
//!   departures every 5 days, each leaving within one revolution of its
//!   station (5 days between low orbits);
//! - `routes.csv`: a standing route per leg, a departure each revolution,
//!   re-planned from the route's recent designs (`legs::replan`), and
//!   planned cold when that fails;
//! - `upkeep.csv`: each station's station-keeping cost for each year;
//! - `between.csv`: LEO to low Mars orbit (burning, and by aerocapture) and
//!   to each candidate asteroid, at departures every 5 days through each
//!   window, against patched conics.
//!
//! Station orbits are carried from the catalog in yearly pieces that
//! overlap by 60 days; a leg uses the piece covering its window and
//! duration.

use anyhow::{Context, Result, bail};
use orbit::ephem::{BodyId, Ephemeris};
use orbit::families::{Catalog, Member};
use orbit::gravity::Field;
use orbit::interplanetary;
use orbit::legs;
use orbit::reference::{Reference, continue_orbit};
use orbit::shooting::{Design, End, LowOrbit};
use orbit::stationkeeping::{Errors, upkeep};
use orbit::surface;
use orbit::time::{Epoch, SECONDS_PER_DAY};
use orbit::transfer::{Grid, Porkchop, default_tof_range};
use orbit::windows::find_windows;
use rayon::prelude::*;
use std::fmt::Write as _;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

const DAY: f64 = SECONDS_PER_DAY;

/// Days between departures in the cold sweep.
const CADENCE_DAYS: f64 = 5.0;

/// Reference pieces: a year each, overlapping the next by this many days.
const PIECE_DAYS: f64 = 365.25;
const OVERLAP_DAYS: f64 = 60.0;

/// How far inside a piece a leg keeps its times, days (`legs::plan`'s own
/// margin is 0.25).
const PIECE_MARGIN_DAYS: f64 = 0.5;

/// Designs a standing route re-plans from.
const HISTORY: usize = 8;

/// A re-plan this many times the median of the route's recent departures
/// is checked against a cold plan: it catches a jump to another family
/// (2× seen), while costs between station orbits vary ±25% with their
/// phasing.
const GUARD: f64 = 1.25;

/// Cold plans per leg timed alone.
const TIMED: usize = 6;

/// Monte Carlo runs per upkeep.
const UPKEEP_RUNS: u64 = 10;

/// The station orbits: name, catalog family and period wanted (days), as
/// in `tests/transfers.rs`.
const STATIONS: [(&str, &str, f64); 4] = [
    ("l1", "l1-halo-north", 12.0),
    ("l2", "l2-halo-north", 14.5),
    ("nrho", "l2-halo-south", 6.57),
    ("dro", "dro", 14.0),
];

pub struct Options {
    pub start: Epoch,
    pub end: Epoch,
    /// Only legs whose names contain this.
    pub only: Option<String>,
    pub parts: Vec<String>,
    /// Plan cold at every n-th date only.
    pub every: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Place {
    /// A low Earth orbit of this radius, free plane.
    Leo(f64),
    /// Polar low lunar orbit, 100 km.
    Llo,
    Station(usize),
    /// A hyperbola about Earth with this excess speed, periapsis at 200 km.
    Escape(f64),
    /// Low Mars orbit, 300 km.
    Lmo,
    Phobos,
}

/// Which burns a published value counts.
#[derive(Clone, Copy, Debug)]
enum Counted {
    All,
    First,
    AfterFirst,
    BeforeLast,
}

#[derive(Clone, Copy, Debug)]
struct Published {
    m_s: f64,
    counted: Counted,
    source: &'static str,
}

#[derive(Clone, Debug)]
struct LegSpec {
    name: String,
    from: Place,
    to: Place,
    /// Durations offered, days: the cheapest of them flies.
    days: Vec<f64>,
    published: Option<Published>,
}

impl LegSpec {
    fn mars(&self) -> bool {
        matches!(self.from, Place::Lmo | Place::Phobos)
    }

    fn station(&self) -> Option<usize> {
        match (self.from, self.to) {
            (Place::Station(s), _) | (_, Place::Station(s)) => Some(s),
            _ => None,
        }
    }

    fn uses(&self, station: usize) -> bool {
        [self.from, self.to].contains(&Place::Station(station))
    }
}

fn core_legs() -> Vec<LegSpec> {
    const WM: &str = "Whitley & Martinez 2016";
    let pub_ = |m_s: f64, counted: Counted, source: &'static str| {
        Some(Published {
            m_s,
            counted,
            source,
        })
    };
    let (l1, l2, nrho, dro) = (
        Place::Station(0),
        Place::Station(1),
        Place::Station(2),
        Place::Station(3),
    );
    let (leo, leo200, leo185) = (
        Place::Leo(6_778.0),
        Place::Leo(6_578.0),
        Place::Leo(6_563.0),
    );
    let leg =
        |name: &str, from: Place, to: Place, days: f64, published: Option<Published>| LegSpec {
            name: name.to_string(),
            from,
            to,
            days: vec![days],
            published,
        };
    let mut legs = vec![
        leg(
            "leo-l1",
            leo200,
            l1,
            4.1,
            pub_(3_660.0, Counted::All, "arXiv 2502.11694"),
        ),
        leg("l1-leo", l1, leo200, 4.1, None),
        leg("leo-l2", leo, l2, 6.5, None),
        leg("l2-leo", l2, leo, 6.5, None),
        leg(
            "leo-nrho",
            leo,
            nrho,
            5.1,
            pub_(428.5, Counted::AfterFirst, WM),
        ),
        leg(
            "nrho-leo",
            nrho,
            leo,
            5.0,
            pub_(411.5, Counted::BeforeLast, WM),
        ),
        leg(
            "leo-dro",
            leo,
            dro,
            9.0,
            pub_(344.0, Counted::AfterFirst, "Artemis I plan"),
        ),
        leg("dro-leo", dro, leo, 9.0, None),
        leg(
            "leo-llo",
            leo185,
            Place::Llo,
            4.4,
            pub_(3_142.0, Counted::First, "Lunar Prospector"),
        ),
        leg("llo-leo", Place::Llo, leo185, 4.4, None),
        leg(
            "nrho-llo",
            nrho,
            Place::Llo,
            0.5,
            pub_(730.0, Counted::All, WM),
        ),
        leg("llo-nrho", Place::Llo, nrho, 0.5, None),
        leg("l2-llo", l2, Place::Llo, 3.0, pub_(800.0, Counted::All, WM)),
        leg("llo-l2", Place::Llo, l2, 3.0, None),
        leg(
            "dro-llo",
            dro,
            Place::Llo,
            4.0,
            pub_(830.0, Counted::All, WM),
        ),
        leg("llo-dro", Place::Llo, dro, 4.0, None),
        leg("l1-llo", l1, Place::Llo, 3.0, None),
        leg("llo-l1", Place::Llo, l1, 3.0, None),
    ];
    // Between stations, the cheapest of 6 to 14 days: the two orbits'
    // relative phase repeats only every 12-400 days (the L2 halo's and the
    // DRO's periods nearly match), more than the window (a revolution of
    // the first), so a fixed duration finds some dates badly phased.
    for (a, &(an, _, _)) in STATIONS.iter().enumerate() {
        for (b, &(bn, _, _)) in STATIONS.iter().enumerate() {
            if a != b {
                legs.push(LegSpec {
                    days: vec![6.0, 8.0, 10.0, 12.0, 14.0],
                    ..leg(
                        &format!("{an}-{bn}"),
                        Place::Station(a),
                        Place::Station(b),
                        0.0,
                        None,
                    )
                });
            }
        }
    }
    // Escapes and captures, as long as the leg to LEO.
    let downs = [
        ("l1", l1, 4.1),
        ("l2", l2, 6.5),
        ("nrho", nrho, 5.0),
        ("dro", dro, 9.0),
        ("llo", Place::Llo, 4.4),
    ];
    for v_inf in [1.0, 3.0] {
        for &(name, place, days) in &downs {
            let esc = Place::Escape(v_inf);
            legs.push(leg(&format!("{name}-esc{v_inf}"), place, esc, days, None));
            legs.push(leg(&format!("esc{v_inf}-{name}"), esc, place, days, None));
        }
    }
    legs.push(leg("lmo-phobos", Place::Lmo, Place::Phobos, 0.0, None));
    legs.push(leg("phobos-lmo", Place::Phobos, Place::Lmo, 0.0, None));
    legs
}

/// One station's references, in yearly pieces from `first`.
struct Pieces {
    first: Epoch,
    refs: Vec<Option<Reference>>,
}

impl Pieces {
    /// The piece covering `from` to `until`.
    fn covering(&self, from: Epoch, until: Epoch) -> Option<&Reference> {
        let k = ((from.0 - self.first.0) / PIECE_DAYS).floor() as isize;
        [k, k - 1].into_iter().filter(|&k| k >= 0).find_map(|k| {
            let r = self.refs.get(k as usize)?.as_ref()?;
            (from.0 - r.epoch.0 >= PIECE_MARGIN_DAYS && r.end().0 - until.0 >= PIECE_MARGIN_DAYS)
                .then_some(r)
        })
    }
}

struct Sweep {
    eph: &'static Ephemeris,
    earth: BodyId,
    moon: BodyId,
    mars: BodyId,
    phobos: BodyId,
    cislunar: Field<'static>,
    cislunar_light: Field<'static>,
    mars_field: Field<'static>,
    pieces: Vec<Pieces>,
}

impl Sweep {
    fn end<'a>(&'a self, place: Place, reference: Option<&'a Reference>) -> End<'a> {
        let eph = self.eph;
        match place {
            Place::Leo(radius) => End::Low(LowOrbit {
                body: self.earth,
                radius,
                inclination: None,
            }),
            Place::Llo => End::Low(LowOrbit {
                body: self.moon,
                radius: eph.body(self.moon).radius + 100.0,
                inclination: Some(90f64.to_radians()),
            }),
            Place::Station(_) => End::Orbit(reference.expect("a reference")),
            Place::Escape(v_inf) => End::Hyperbola {
                body: self.earth,
                radius: eph.body(self.earth).radius + 200.0,
                v_inf,
            },
            Place::Lmo => End::Low(LowOrbit {
                body: self.mars,
                radius: eph.body(self.mars).radius + 300.0,
                inclination: None,
            }),
            Place::Phobos => End::Body(self.phobos),
        }
    }

    /// The leg's durations, s: Hohmann's between low Mars orbit and Phobos.
    fn durations(&self, leg: &LegSpec) -> Vec<f64> {
        if leg.mars() {
            let gm = self.eph.body(self.mars).gm;
            let a = (self.eph.body(self.mars).radius + 300.0 + 9_376.0) / 2.0;
            vec![std::f64::consts::PI * (a.powi(3) / gm).sqrt()]
        } else {
            leg.days.iter().map(|d| d * DAY).collect()
        }
    }

    /// The cheapest design of `leg` leaving within `window` (s after
    /// `date`), over its durations.
    fn plan(&self, leg: &LegSpec, date: Epoch, from: End, to: End, window: f64) -> Option<Design> {
        let (field, light) = self.fields(leg);
        legs::plan(
            field,
            light,
            date,
            from,
            to,
            (0.0, window),
            &self.durations(leg),
        )
        .into_iter()
        .flatten()
        .min_by(|a, b| a.dv.total_cmp(&b.dv))
    }

    /// A leg's window at `date` (s: a revolution of the station it leaves,
    /// or else reaches; 5 days between low orbits; a day in the Mars
    /// system) and its ends; `None` without a reference piece covering a
    /// station end.
    fn setup(&self, leg: &LegSpec, date: Epoch) -> Option<(f64, End<'_>, End<'_>)> {
        // A piece covering the longest revolution (15 days) and the leg
        // after it.
        let longest = self.durations(leg).into_iter().fold(0.0, f64::max);
        let until = date.plus_days(15.0 + longest / DAY);
        let piece = |place: Place| match place {
            Place::Station(s) => self.pieces[s].covering(date, until).map(Some),
            _ => Some(None),
        };
        let (a, b) = (piece(leg.from)?, piece(leg.to)?);
        let window = match a.or(b) {
            Some(r) => r.period_s(),
            None if leg.mars() => DAY,
            None => CADENCE_DAYS * DAY,
        };
        Some((window, self.end(leg.from, a), self.end(leg.to, b)))
    }

    fn fields(&self, leg: &LegSpec) -> (&Field<'static>, &Field<'static>) {
        if leg.mars() {
            (&self.mars_field, &self.mars_field)
        } else {
            (&self.cislunar, &self.cislunar_light)
        }
    }

    fn cold(&self, leg: &LegSpec, date: Epoch) -> Option<(f64, Option<Design>, f64)> {
        let (window, from, to) = self.setup(leg, date)?;
        let clock = Instant::now();
        let d = self.plan(leg, date, from, to, window);
        Some((window, d, clock.elapsed().as_secs_f64()))
    }
}

/// `f`'s result, or `None` if it panicked: a bug in one design is a row
/// of the sweep, not the end of it (its message still goes to stderr).
fn guarded<T>(f: impl FnOnce() -> T) -> Option<T> {
    catch_unwind(AssertUnwindSafe(f)).ok()
}

fn counted(d: &Design, counted: Counted) -> f64 {
    let b: Vec<f64> = d.burns.iter().map(|b| b.dv.length()).collect();
    match counted {
        Counted::All => b.iter().sum(),
        Counted::First => b[0],
        Counted::AfterFirst => b[1..].iter().sum(),
        Counted::BeforeLast => b[..b.len() - 1].iter().sum(),
    }
}

fn nearest<'a>(cat: &'a Catalog, family: &str, days: f64) -> &'a Member {
    let to_days = |m: &Member| m.orbit.period * cat.time_s / DAY;
    cat.family(family)
        .expect("a catalog family")
        .members
        .iter()
        .min_by(|a, b| {
            (to_days(a) - days)
                .abs()
                .total_cmp(&(to_days(b) - days).abs())
        })
        .expect("members")
}

pub fn run(opts: &Options, out: &Path) -> Result<()> {
    std::fs::create_dir_all(out).with_context(|| format!("creating {}", out.display()))?;
    let eph = Ephemeris::standard();
    let id = |s: &str| eph.lookup(s).context(format!("no body {s}"));
    let (earth, moon, mars, phobos) = (id("earth")?, id("moon")?, id("mars")?, id("phobos")?);
    let bodies = [eph.sun(), earth, moon];
    let cislunar = Field::new(eph, Some(earth), &bodies).with_oblateness();
    let wants = |part: &str| opts.parts.iter().any(|p| p == part);
    let mut legs = core_legs();
    if let Some(only) = &opts.only {
        legs.retain(|l| l.name.contains(only.as_str()));
    }
    // References for the stations any chosen leg (or the upkeep) uses.
    let needed: Vec<bool> = (0..STATIONS.len())
        .map(|s| wants("upkeep") || legs.iter().any(|l| l.uses(s)))
        .collect();
    let first = opts.start.plus_days(-31.0);
    let count = ((opts.end.0 - first.0) / PIECE_DAYS).ceil() as usize;
    let clock = Instant::now();
    let cat = Catalog::standard();
    let jobs: Vec<(usize, usize)> = (0..STATIONS.len())
        .filter(|&s| needed[s])
        .flat_map(|s| (0..count).map(move |k| (s, k)))
        .collect();
    let built: Vec<(usize, usize, Option<Reference>)> = jobs
        .par_iter()
        .map(|&(s, k)| {
            let (_, family, days) = STATIONS[s];
            let m = nearest(cat, family, days);
            let revs = ((PIECE_DAYS + OVERLAP_DAYS) / days).ceil() as usize;
            // A start that does not converge is retried a day or two later.
            let r = (0..3).find_map(|late| {
                let start = first.plus_days(k as f64 * PIECE_DAYS + late as f64);
                continue_orbit(eph, &cislunar, cat, m.orbit, start, revs, 16).ok()
            });
            (s, k, r)
        })
        .collect();
    let mut pieces: Vec<Pieces> = (0..STATIONS.len())
        .map(|_| Pieces {
            first,
            refs: (0..count).map(|_| None).collect(),
        })
        .collect();
    let mut missing = Vec::new();
    for (s, k, r) in built {
        if r.is_none() {
            missing.push(format!("{} piece {k}", STATIONS[s].0));
        }
        pieces[s].refs[k] = r;
    }
    eprintln!(
        "references: {} pieces in {:.0} s; not continuous: {missing:?}",
        jobs.len(),
        clock.elapsed().as_secs_f64()
    );
    let sweep = Sweep {
        eph,
        earth,
        moon,
        mars,
        phobos,
        cislunar_light: Field::new(eph, Some(earth), &bodies),
        mars_field: Field::new(eph, Some(mars), &[eph.sun(), mars, id("jupiter")?])
            .with_oblateness(),
        cislunar,
        pieces,
    };
    let mut report = String::new();
    if wants("cold") {
        cold(&sweep, &legs, opts, out, &mut report)?;
    }
    if wants("routes") {
        routes(&sweep, &legs, opts, out, &mut report)?;
    }
    if wants("upkeep") {
        upkeeps(&sweep, out, &mut report)?;
    }
    if wants("between") {
        between(&sweep, opts, out, &mut report)?;
    }
    print!("{report}");
    std::fs::write(out.join("summary.txt"), &report)?;
    Ok(())
}

fn dates(opts: &Options) -> Vec<Epoch> {
    let n = ((opts.end.0 - opts.start.0) / CADENCE_DAYS).floor() as usize;
    (0..n)
        .map(|k| opts.start.plus_days(k as f64 * CADENCE_DAYS))
        .collect()
}

fn quantile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    let i = ((sorted.len() - 1) as f64 * q).round() as usize;
    sorted[i]
}

fn sorted(mut v: Vec<f64>) -> Vec<f64> {
    v.sort_by(f64::total_cmp);
    v
}

fn burns_ms(d: &Design) -> String {
    d.burns
        .iter()
        .map(|b| format!("{:.1}", b.dv.length() * 1e3))
        .collect::<Vec<_>>()
        .join(";")
}

struct ColdRow {
    leg: usize,
    date: Epoch,
    /// None without a reference piece.
    result: Option<(f64, Option<Design>, f64)>,
    panicked: bool,
}

fn cold(
    sweep: &Sweep,
    legs: &[LegSpec],
    opts: &Options,
    out: &Path,
    report: &mut String,
) -> Result<()> {
    let all = dates(opts);
    let jobs: Vec<(usize, Epoch)> = legs
        .iter()
        .enumerate()
        .flat_map(|(i, _)| all.iter().step_by(opts.every.max(1)).map(move |&d| (i, d)))
        .collect();
    let done = AtomicUsize::new(0);
    let clock = Instant::now();
    let rows: Vec<ColdRow> = jobs
        .par_iter()
        .map(|&(leg, date)| {
            let outcome = guarded(|| sweep.cold(&legs[leg], date));
            let panicked = outcome.is_none();
            let result = outcome.unwrap_or(Some((0.0, None, 0.0)));
            let n = done.fetch_add(1, Ordering::Relaxed) + 1;
            if n.is_multiple_of(500) {
                eprintln!(
                    "cold: {n}/{} in {:.0} s",
                    jobs.len(),
                    clock.elapsed().as_secs_f64()
                );
            }
            ColdRow {
                leg,
                date,
                result,
                panicked,
            }
        })
        .collect();
    // Design times from plans run one at a time (the sweep's own run many
    // at once): a few dates spread over the span.
    let span = opts.end.0 - opts.start.0;
    let timed: Vec<Vec<f64>> = legs
        .iter()
        .map(|leg| {
            (0..TIMED)
                .filter_map(|k| {
                    let date = opts.start.plus_days(
                        ((k as f64 + 0.5) * span / TIMED as f64 / CADENCE_DAYS).floor()
                            * CADENCE_DAYS,
                    );
                    guarded(|| sweep.cold(leg, date)).flatten().map(|r| r.2)
                })
                .collect()
        })
        .collect();
    let mut csv = String::from(
        "leg,date,window_days,converged,dv_m_s,counted_m_s,leaves_days,duration_days,seconds,burns_m_s\n",
    );
    for r in &rows {
        let leg = &legs[r.leg];
        match &r.result {
            None => writeln!(csv, "{},{},,no reference,,,,,,", leg.name, r.date)?,
            Some((window, d, s)) => match d {
                Some(d) => {
                    let leaves = (d.path.epoch.0 - r.date.0) + d.departure() / DAY;
                    let c = leg.published.map_or(d.dv, |p| counted(d, p.counted));
                    writeln!(
                        csv,
                        "{},{},{:.3},yes,{:.1},{:.1},{:.3},{:.3},{:.3},{}",
                        leg.name,
                        r.date,
                        window / DAY,
                        d.dv * 1e3,
                        c * 1e3,
                        leaves,
                        d.duration() / DAY,
                        s,
                        burns_ms(d)
                    )?
                }
                None => writeln!(
                    csv,
                    "{},{},{:.3},{},,,,,{s:.3},",
                    leg.name,
                    r.date,
                    window / DAY,
                    if r.panicked { "panic" } else { "no" }
                )?,
            },
        }
    }
    std::fs::write(out.join("cold.csv"), csv)?;
    writeln!(
        report,
        "\nCold plans (departures every {} days):",
        CADENCE_DAYS * opts.every.max(1) as f64
    )?;
    writeln!(
        report,
        "{:<14} {:>5} {:>7} {:>8} {:>8} {:>8}  {:>9} {:>7}  {:>6} {:>6} {:>6}",
        "leg", "n", "conv%", "min", "median", "max", "published", "within", "t50", "t95", "tmax"
    )?;
    writeln!(
        report,
        "(m/s, as the published value counts them; times in s from {TIMED} plans run alone)"
    )?;
    for (i, leg) in legs.iter().enumerate() {
        let mine: Vec<&ColdRow> = rows.iter().filter(|r| r.leg == i).collect();
        let tried: Vec<&(f64, Option<Design>, f64)> =
            mine.iter().filter_map(|r| r.result.as_ref()).collect();
        let designs: Vec<&Design> = tried.iter().filter_map(|r| r.1.as_ref()).collect();
        let values = sorted(
            designs
                .iter()
                .map(|d| leg.published.map_or(d.dv, |p| counted(d, p.counted)) * 1e3)
                .collect(),
        );
        let times = sorted(timed[i].clone());
        let (published, within) = match leg.published {
            Some(p) => (
                format!("{:.0}", p.m_s),
                format!(
                    "{:.0}%",
                    100.0
                        * values
                            .iter()
                            .filter(|v| (*v / p.m_s - 1.0).abs() <= 0.10)
                            .count() as f64
                        / values.len().max(1) as f64
                ),
            ),
            None => ("-".into(), "-".into()),
        };
        writeln!(
            report,
            "{:<14} {:>5} {:>6.1}% {:>8.0} {:>8.0} {:>8.0}  {:>9} {:>7}  {:>6.2} {:>6.2} {:>6.2}{}",
            leg.name,
            tried.len(),
            100.0 * designs.len() as f64 / tried.len().max(1) as f64,
            quantile(&values, 0.0),
            quantile(&values, 0.5),
            quantile(&values, 1.0),
            published,
            within,
            quantile(&times, 0.5),
            quantile(&times, 0.95),
            quantile(&times, 1.0),
            if mine.len() > tried.len() {
                format!("  ({} without a reference)", mine.len() - tried.len())
            } else {
                String::new()
            },
        )?;
    }
    let total: usize = rows.iter().filter(|r| r.result.is_some()).count();
    let converged = rows
        .iter()
        .filter(|r| matches!(r.result, Some((_, Some(_), _))))
        .count();
    writeln!(
        report,
        "all legs: {converged} of {total} converged ({:.2}%), {:.0} s",
        100.0 * converged as f64 / total.max(1) as f64,
        clock.elapsed().as_secs_f64()
    )?;
    for leg in legs {
        if let Some(p) = leg.published {
            writeln!(
                report,
                "  {}: published {:.0} m/s ({})",
                leg.name, p.m_s, p.source
            )?;
        }
    }
    Ok(())
}

struct RouteRow {
    date: Epoch,
    /// "replan", "checked" (re-planned, cheaper than a cold plan made because
    /// it cost more than usual), "cold", "failed" or "no reference".
    how: &'static str,
    design: Option<Design>,
    replan_s: f64,
    cold_s: f64,
}

fn route(sweep: &Sweep, leg: &LegSpec, opts: &Options) -> Vec<RouteRow> {
    let (field, _) = sweep.fields(leg);
    let mut history: Vec<Design> = Vec::new();
    let mut rows = Vec::new();
    let mut date = opts.start;
    while date.0 < opts.end.0 {
        let Some((window, from, to)) = sweep.setup(leg, date) else {
            rows.push(RouteRow {
                date,
                how: "no reference",
                design: None,
                replan_s: 0.0,
                cold_s: 0.0,
            });
            date = date.plus_days(CADENCE_DAYS);
            continue;
        };
        // Every 5 days for a leg with no station (and a day's window in the
        // Mars system); a revolution otherwise, so the windows tile.
        let cadence = if leg.station().is_some() {
            window
        } else {
            CADENCE_DAYS * DAY
        };
        let clock = Instant::now();
        let replanned = if history.is_empty() {
            None
        } else {
            guarded(|| legs::replan(field, from, to, &history, date, (0.0, window)).ok()).flatten()
        };
        // Time spent re-planning, successful or not (none at the start).
        let replan_s = clock.elapsed().as_secs_f64();
        // A re-plan keeps its seed's family of paths, which can be a worse
        // one (seen after a reference piece's boundary: NRHO to low lunar
        // orbit at twice its usual Δv, kept by every re-plan after). One
        // more than `GUARD` times the median of the route's recent
        // departures is checked against a cold plan, and the cheaper flies.
        let usual = quantile(&sorted(history.iter().map(|d| d.dv).collect()), 0.5);
        let (how, design, cold_s) = match replanned {
            Some(d) if d.dv <= GUARD * usual => ("replan", Some(d), 0.0),
            warm => {
                let clock = Instant::now();
                let cold = guarded(|| sweep.plan(leg, date, from, to, window)).flatten();
                let s = clock.elapsed().as_secs_f64();
                match (warm, cold) {
                    (Some(w), Some(c)) if w.dv <= c.dv => ("checked", Some(w), s),
                    (Some(w), None) => ("checked", Some(w), s),
                    (_, Some(c)) => ("cold", Some(c), s),
                    (None, None) => ("failed", None, s),
                }
            }
        };
        if let Some(d) = &design {
            history.push(d.clone());
            if history.len() > HISTORY {
                history.remove(0);
            }
        }
        rows.push(RouteRow {
            date,
            how,
            design,
            replan_s,
            cold_s,
        });
        date = date.plus_days(cadence / DAY);
    }
    rows
}

fn routes(
    sweep: &Sweep,
    legs: &[LegSpec],
    opts: &Options,
    out: &Path,
    report: &mut String,
) -> Result<()> {
    let clock = Instant::now();
    let done = AtomicUsize::new(0);
    let all: Vec<Vec<RouteRow>> = legs
        .par_iter()
        .map(|leg| {
            let rows = route(sweep, leg, opts);
            let n = done.fetch_add(1, Ordering::Relaxed) + 1;
            eprintln!(
                "routes: {} done ({n}/{}) in {:.0} s",
                leg.name,
                legs.len(),
                clock.elapsed().as_secs_f64()
            );
            rows
        })
        .collect();
    let mut csv =
        String::from("leg,date,how,dv_m_s,counted_m_s,replan_seconds,cold_seconds,burns_m_s\n");
    writeln!(
        report,
        "\nStanding routes (a departure each revolution, or every 5 days):"
    )?;
    writeln!(
        report,
        "{:<14} {:>5} {:>8} {:>8} {:>6} {:>7}  {:>8} {:>8}  {:>7} {:>7}",
        "leg", "n", "replan%", "checked%", "cold%", "failed", "dv50", "cold50", "r50", "r95"
    )?;
    let mut every_replan = Vec::new();
    for (leg, rows) in legs.iter().zip(&all) {
        for r in rows {
            let (dv, c, burns) = match &r.design {
                Some(d) => (
                    format!("{:.1}", d.dv * 1e3),
                    format!(
                        "{:.1}",
                        leg.published.map_or(d.dv, |p| counted(d, p.counted)) * 1e3
                    ),
                    burns_ms(d),
                ),
                None => Default::default(),
            };
            writeln!(
                csv,
                "{},{},{},{dv},{c},{:.4},{:.3},{burns}",
                leg.name, r.date, r.how, r.replan_s, r.cold_s
            )?;
        }
        let tried: Vec<&RouteRow> = rows.iter().filter(|r| r.how != "no reference").collect();
        let n = tried.len().max(1) as f64;
        let share = |how: &str| 100.0 * tried.iter().filter(|r| r.how == how).count() as f64 / n;
        let dv_of = |how: &str| {
            sorted(
                tried
                    .iter()
                    .filter(|r| r.how == how)
                    .filter_map(|r| r.design.as_ref())
                    .map(|d| d.dv * 1e3)
                    .collect(),
            )
        };
        let replan_times = sorted(
            tried
                .iter()
                .filter(|r| r.how == "replan")
                .map(|r| r.replan_s)
                .collect(),
        );
        every_replan.extend(replan_times.iter().copied());
        writeln!(
            report,
            "{:<14} {:>5} {:>7.1}% {:>7.1}% {:>5.1}% {:>7}  {:>8.0} {:>8.0}  {:>7.3} {:>7.3}",
            leg.name,
            tried.len(),
            share("replan"),
            share("checked"),
            share("cold"),
            tried.iter().filter(|r| r.how == "failed").count(),
            quantile(&dv_of("replan"), 0.5),
            quantile(&dv_of("cold"), 0.5),
            quantile(&replan_times, 0.5),
            quantile(&replan_times, 0.95),
        )?;
    }
    let every = sorted(every_replan);
    writeln!(
        report,
        "all re-plans: {}, median {:.3} s, 95th percentile {:.3} s; {:.0} s",
        every.len(),
        quantile(&every, 0.5),
        quantile(&every, 0.95),
        clock.elapsed().as_secs_f64()
    )?;
    std::fs::write(out.join("routes.csv"), csv)?;
    Ok(())
}

fn upkeeps(sweep: &Sweep, out: &Path, report: &mut String) -> Result<()> {
    let clock = Instant::now();
    let jobs: Vec<(usize, usize)> = (0..STATIONS.len())
        .flat_map(|s| (0..sweep.pieces[s].refs.len()).map(move |k| (s, k)))
        .collect();
    let rows: Vec<(usize, usize, Option<f64>)> = jobs
        .par_iter()
        .map(|&(s, k)| {
            let u = sweep.pieces[s].refs[k].as_ref().and_then(|r| {
                guarded(|| upkeep(&sweep.cislunar, r, Errors::TYPICAL, UPKEEP_RUNS)).flatten()
            });
            (s, k, u.map(|u| u.dv_per_year_kms * 1e3))
        })
        .collect();
    let mut csv = String::from("station,piece_start,m_s_per_year\n");
    writeln!(report, "\nUpkeep (m/s per year, by yearly piece):")?;
    for (s, &(name, _, _)) in STATIONS.iter().enumerate() {
        let mine: Vec<&(usize, usize, Option<f64>)> = rows.iter().filter(|r| r.0 == s).collect();
        for &&(_, k, u) in &mine {
            let start = sweep.pieces[s].refs[k]
                .as_ref()
                .map(|r| r.epoch.to_string())
                .unwrap_or_default();
            writeln!(
                csv,
                "{name},{start},{}",
                u.map_or(String::new(), |u| format!("{u:.3}"))
            )?;
        }
        let values = sorted(mine.iter().filter_map(|r| r.2).collect());
        writeln!(
            report,
            "{name:<6} {} of {} pieces held: min {:.2}, median {:.2}, max {:.2}",
            values.len(),
            mine.len(),
            quantile(&values, 0.0),
            quantile(&values, 0.5),
            quantile(&values, 1.0)
        )?;
    }
    writeln!(report, "{:.0} s", clock.elapsed().as_secs_f64())?;
    std::fs::write(out.join("upkeep.csv"), csv)?;
    Ok(())
}

/// LEO to low Mars orbit (burning, and by aerocapture) and to each
/// candidate asteroid, at departures every 5 days through each window at
/// the porkchop's best time of flight, against patched conics.
fn between(sweep: &Sweep, opts: &Options, out: &Path, report: &mut String) -> Result<()> {
    let eph = sweep.eph;
    let earth = sweep.earth;
    let global = Field::all(eph, None).with_oblateness();
    let locals = vec![
        Field::all(eph, Some(earth)).with_oblateness(),
        Field::all(eph, Some(sweep.mars)).with_oblateness(),
    ];
    let leo = LowOrbit {
        body: earth,
        radius: 6_778.0,
        inclination: None,
    };
    let gm_e = eph.body(earth).gm;
    let burn = |gm: f64, r: f64, v: f64| (v * v + 2.0 * gm / r).sqrt() - (gm / r).sqrt();
    let mars = sweep.mars;
    let (gm_m, r_m) = (eph.body(mars).gm, eph.body(mars).radius);
    let lmo = LowOrbit {
        body: mars,
        radius: r_m + 400.0,
        inclination: None,
    };
    let rules = surface::rules(eph, mars)
        .atmosphere
        .context("Mars's atmosphere")?;
    let pass = r_m + rules.capture_altitude;
    let after = surface::after_aerocapture(gm_m, pass, lmo.radius);
    // Targets: Mars, both ways of arriving, and the asteroids.
    let mut targets: Vec<(String, BodyId)> = vec![("mars".into(), mars)];
    targets.extend(
        eph.ids()
            .filter(|&b| matches!(eph.body(b).kind, orbit::ephem::BodyKind::SmallBody { .. }))
            .map(|b| (eph.body(b).id.clone(), b)),
    );
    if let Some(only) = &opts.only {
        targets.retain(|(n, _)| {
            n.contains(only.as_str()) || format!("leo-{n}").contains(only.as_str())
        });
    }
    // Departure dates every 5 days through each window, each at the
    // porkchop's best time of flight for that day.
    let mut jobs: Vec<(String, BodyId, Epoch, f64)> = Vec::new();
    for (name, body) in &targets {
        let (lo, hi) = default_tof_range(eph, earth, *body, opts.start);
        let grid = Grid::covering(opts.start, opts.end, 1.0, lo, hi, 2.0);
        let pc = Porkchop::compute(eph, earth, *body, grid);
        let synodic = eph.synodic_period_days(earth, *body, opts.start);
        let best = pc.best_by_departure();
        for w in find_windows(&pc, synodic, 0.5) {
            let mut d = w.open;
            while d.0 <= w.close.0 {
                let i = ((d.0 - grid.depart(0).0) / grid.depart_step).round() as usize;
                if let Some(Some(t)) = best.get(i) {
                    jobs.push((name.clone(), *body, t.depart, t.tof_days));
                }
                d = d.plus_days(CADENCE_DAYS);
            }
        }
    }
    let clock = Instant::now();
    let done = AtomicUsize::new(0);
    let rows: Vec<String> = jobs
        .par_iter()
        .flat_map(|(name, body, depart, tof)| {
            let arrive = depart.plus_days(*tof);
            let arc = orbit::transfer::Transfer::compute(eph, earth, *body, *depart, *tof);
            let out_pc = arc.map(|a| burn(gm_e, leo.radius, a.v_inf_depart));
            let mut variants: Vec<(String, End, Option<f64>)> = Vec::new();
            if *body == mars {
                let into = arc.map(|a| burn(gm_m, lmo.radius, a.v_inf_arrive));
                variants.push((
                    "leo-lmo".into(),
                    End::Low(lmo),
                    out_pc.zip(into).map(|(a, b)| a + b),
                ));
                variants.push((
                    "leo-mars-aero".into(),
                    End::Atmosphere {
                        body: mars,
                        radius: pass,
                        after,
                        max_speed: rules.max_speed,
                    },
                    out_pc.map(|a| a + after),
                ));
            } else {
                let into = arc.map(|a| a.v_inf_arrive);
                variants.push((
                    format!("leo-{name}"),
                    End::Body(*body),
                    out_pc.zip(into).map(|(a, b)| a + b),
                ));
            }
            let lines: Vec<String> = variants
                .into_iter()
                .map(|(leg, to, patched)| {
                    let c = Instant::now();
                    let d = guarded(|| {
                        interplanetary::plan(&global, &locals, End::Low(leo), to, *depart, arrive)
                    })
                    .flatten();
                    let s = c.elapsed().as_secs_f64();
                    let p = patched.map_or(String::new(), |p| format!("{:.1}", p * 1e3));
                    match d {
                        Some(d) => format!(
                            "{leg},{depart},{tof:.0},yes,{:.1},{p},{s:.2},{}",
                            d.dv * 1e3,
                            burns_ms(&d)
                        ),
                        None => format!("{leg},{depart},{tof:.0},no,,{p},{s:.2},"),
                    }
                })
                .collect();
            let n = done.fetch_add(1, Ordering::Relaxed) + 1;
            if n.is_multiple_of(50) {
                eprintln!(
                    "between: {n}/{} in {:.0} s",
                    jobs.len(),
                    clock.elapsed().as_secs_f64()
                );
            }
            lines
        })
        .collect();
    let mut csv =
        String::from("leg,date,tof_days,converged,dv_m_s,patched_m_s,seconds,burns_m_s\n");
    for r in &rows {
        csv.push_str(r);
        csv.push('\n');
    }
    std::fs::write(out.join("between.csv"), &csv)?;
    // Summary per leg kind.
    writeln!(
        report,
        "\nBetween systems (departures every 5 days through each window):"
    )?;
    writeln!(
        report,
        "{:<18} {:>5} {:>7}  {:>8} {:>8} {:>8}  {:>6} {:>6}",
        "leg", "n", "conv%", "r_min", "r_med", "r_max", "t50", "tmax"
    )?;
    let mut names: Vec<String> = rows
        .iter()
        .map(|r| r.split(',').next().unwrap().to_string())
        .collect();
    names.sort();
    names.dedup();
    for name in names {
        let mine: Vec<Vec<&str>> = rows
            .iter()
            .map(|r| r.split(',').collect::<Vec<_>>())
            .filter(|f| f[0] == name)
            .collect();
        let ok: Vec<&Vec<&str>> = mine.iter().filter(|f| f[3] == "yes").collect();
        let ratios = sorted(
            ok.iter()
                .filter_map(|f| Some(f[4].parse::<f64>().ok()? / f[5].parse::<f64>().ok()?))
                .collect(),
        );
        let times = sorted(mine.iter().filter_map(|f| f[6].parse().ok()).collect());
        writeln!(
            report,
            "{:<18} {:>5} {:>6.1}%  {:>8.3} {:>8.3} {:>8.3}  {:>6.1} {:>6.1}",
            name,
            mine.len(),
            100.0 * ok.len() as f64 / mine.len().max(1) as f64,
            quantile(&ratios, 0.0),
            quantile(&ratios, 0.5),
            quantile(&ratios, 1.0),
            quantile(&times, 0.5),
            quantile(&times, 1.0),
        )?;
    }
    writeln!(
        report,
        "(r: Δv over patched conics; {:.0} s)",
        clock.elapsed().as_secs_f64()
    )?;
    if rows.is_empty() {
        bail!("no windows found");
    }
    Ok(())
}
