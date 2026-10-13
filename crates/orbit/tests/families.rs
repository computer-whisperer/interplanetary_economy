//! The orbit families: catalog members close, and members between catalog
//! samples are corrected into periodic orbits.

use orbit::cr3bp::{Cr3bp, Fix};
use orbit::families::{Catalog, PeriodicOrbit};
use orbit::integrate::{Dop853, Tolerance};

const TOL: Tolerance = Tolerance {
    rtol: 1e-13,
    atol: 1e-16,
};

/// How far an orbit is from its start after one period, CR3BP units.
fn miss(sys: &Cr3bp, o: &PeriodicOrbit) -> f64 {
    let ode = sys.ode();
    let mut run = Dop853::new(&ode, 0.0, o.state, TOL);
    run.advance_to(&ode, o.period).unwrap();
    (0..6)
        .map(|i| (run.y[i] - o.state[i]).powi(2))
        .sum::<f64>()
        .sqrt()
}

#[test]
fn catalog_members_close() {
    let cat = Catalog::standard();
    assert_eq!(cat.families.len(), 7);
    for f in &cat.families {
        let worst = f
            .members
            .iter()
            .step_by(4)
            .map(|m| miss(&cat.system, &m.orbit))
            .fold(0.0, f64::max);
        println!(
            "{}: {} members, worst miss after a period {worst:.1e}",
            f.id,
            f.members.len()
        );
        assert!(worst < 1e-6, "{}: {worst}", f.id);
    }
}

/// A perturbed NRHO corrects back onto the catalog member.
#[test]
fn corrector_recovers_the_nrho() {
    let cat = Catalog::standard();
    let family = cat.family("l2-halo-south").unwrap();
    let days = |m: &orbit::families::Member| m.orbit.period * cat.time_s / 86_400.0;
    let nrho = family
        .members
        .iter()
        .min_by(|a, b| (days(a) - 6.57).abs().total_cmp(&(days(b) - 6.57).abs()))
        .unwrap();
    let mut guess = nrho.orbit.state;
    guess[2] *= 1.001;
    guess[4] *= 0.999;
    let (state, period) = cat
        .system
        .correct_symmetric(guess, nrho.orbit.period * 1.01, Fix::X)
        .unwrap();
    let dz = (state[2] - nrho.orbit.state[2]).abs() * cat.length_km;
    println!(
        "recovered within {dz:.2e} km, period {:.6} vs {:.6}",
        period, nrho.orbit.period
    );
    assert!(dz < 1e-3, "{dz} km");
    assert!((period / nrho.orbit.period - 1.0).abs() < 1e-9);
}

/// Halfway between neighboring samples of a run, the corrected member
/// closes, with a period near its neighbors'. A few positions may not exist
/// where a family branches (the symmetric correction is singular there):
/// at most 1% of them, measured 1 of 491 (the smallest DROs, 2026-10-13).
/// The most unstable members close only to ~3e-8, as the catalog's own do
/// (L2 Lyapunov orbits, stability index up to ~700).
#[test]
fn members_between_samples_close() {
    let cat = Catalog::standard();
    let (mut tried, mut failed) = (0, Vec::new());
    for f in &cat.families {
        let mut worst: f64 = 0.0;
        for k in (0..f.members.len() - 1).filter(|&k| f.members[k].run == f.members[k + 1].run) {
            tried += 1;
            let Some(o) = cat.member(f, k as f64 + 0.5) else {
                failed.push(format!("{} at {k}.5", f.id));
                continue;
            };
            let (p, q) = (f.members[k].orbit.period, f.members[k + 1].orbit.period);
            let (lo, hi) = (p.min(q), p.max(q));
            assert!(
                o.period > lo - 0.05 * (hi - lo) - 1e-9 && o.period < hi + 0.05 * (hi - lo) + 1e-9,
                "{} at {k}.5: period {} not between {p} and {q}",
                f.id,
                o.period
            );
            worst = worst.max(miss(&cat.system, &o));
        }
        println!("{}: worst miss between samples {worst:.1e}", f.id);
        assert!(worst < 1e-7, "{}: {worst}", f.id);
    }
    println!("{} of {tried} positions failed: {failed:?}", failed.len());
    assert!(failed.len() * 100 <= tried, "{failed:?}");
}
