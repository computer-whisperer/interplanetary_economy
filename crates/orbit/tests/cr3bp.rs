//! The CR3BP against JPL's periodic-orbit catalog: each orbit returns to its
//! start after one period, and keeps its Jacobi constant.

use orbit::cr3bp::Cr3bp;
use orbit::integrate::{Dop853, Tolerance};

struct Orbit {
    name: String,
    state: [f64; 6],
    jacobi: f64,
    period: f64,
}

/// The catalog's mass ratio, length unit (km) and orbits.
fn catalog() -> (f64, f64, Vec<Orbit>) {
    let path = format!(
        "{}/tests/fixtures/jpl_periodic_orbits.csv",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(&path).unwrap();
    let units = text
        .lines()
        .find(|l| l.starts_with("# mass ratio"))
        .unwrap();
    let words: Vec<&str> = units.split([' ', ',']).filter(|w| !w.is_empty()).collect();
    let (mu, lunit) = (words[3].parse().unwrap(), words[6].parse().unwrap());
    let orbits = text
        .lines()
        .filter(|l| !l.starts_with('#'))
        .map(|l| {
            let f: Vec<&str> = l.split(',').collect();
            let x: Vec<f64> = f[1..].iter().map(|v| v.parse().unwrap()).collect();
            Orbit {
                name: f[0].to_string(),
                state: [x[0], x[1], x[2], x[3], x[4], x[5]],
                jacobi: x[6],
                period: x[7],
            }
        })
        .collect();
    (mu, lunit, orbits)
}

#[test]
fn catalog_orbits_close() {
    let (mu, lunit, orbits) = catalog();
    let sys = Cr3bp { mu };
    let ode = sys.ode();
    let tol = Tolerance {
        rtol: 1e-12,
        atol: 1e-15,
    };
    for Orbit {
        name,
        state: s0,
        jacobi,
        period,
    } in orbits
    {
        assert!(
            (sys.jacobi(&s0) - jacobi).abs() < 1e-12,
            "{name}: Jacobi {}",
            sys.jacobi(&s0)
        );
        let mut run = Dop853::new(&ode, 0.0, s0, tol);
        run.advance_to(&ode, period).unwrap();
        let miss = (0..3)
            .map(|i| (run.y[i] - s0[i]).powi(2))
            .sum::<f64>()
            .sqrt()
            * lunit;
        let drift = (sys.jacobi(&run.y) - jacobi).abs();
        println!(
            "{name}: back within {:.3} mm after one period, Jacobi drift {drift:.1e}",
            miss * 1e6
        );
        assert!(miss < 1e-3, "{name}: {miss} km");
        assert!(drift < 1e-12, "{name}: {drift}");
    }
}

fn identity_with(s0: [f64; 6]) -> [f64; 42] {
    let mut y = [0.0; 42];
    y[..6].copy_from_slice(&s0);
    for i in 0..6 {
        y[6 + i * 7] = 1.0;
    }
    y
}

/// The state transition matrix matches central finite differences of the
/// flow over a quarter of the NRHO's period.
#[test]
fn stm_matches_finite_differences() {
    let (mu, _, orbits) = catalog();
    let sys = Cr3bp { mu };
    let nrho = orbits.iter().find(|o| o.name == "nrho").unwrap();
    let tol = Tolerance {
        rtol: 1e-13,
        atol: 1e-16,
    };
    let t1 = nrho.period / 4.0;
    let (ode, stm) = (sys.ode(), sys.ode_stm());
    let mut run = Dop853::new(&stm, 0.0, identity_with(nrho.state), tol);
    run.advance_to(&stm, t1).unwrap();
    let flow = |s: [f64; 6]| {
        let mut r = Dop853::new(&ode, 0.0, s, tol);
        r.advance_to(&ode, t1).unwrap();
        r.y
    };
    let eps = 1e-7;
    let mut worst: f64 = 0.0;
    for c in 0..6 {
        let (mut hi, mut lo) = (nrho.state, nrho.state);
        hi[c] += eps;
        lo[c] -= eps;
        let (a, b) = (flow(hi), flow(lo));
        for r in 0..6 {
            let fd = (a[r] - b[r]) / (2.0 * eps);
            let stm = run.y[6 + r * 6 + c];
            worst = worst.max((fd - stm).abs() / stm.abs().max(1.0));
        }
    }
    println!("STM against finite differences: worst relative difference {worst:.1e}");
    assert!(worst < 1e-5, "{worst}");
}

/// The monodromy matrix gives the catalog's stability index.
#[test]
fn stability_indices_match_catalog() {
    let path = format!(
        "{}/tests/fixtures/jpl_periodic_orbits.csv",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(&path).unwrap();
    let (mu, _, orbits) = catalog();
    let sys = Cr3bp { mu };
    let stm = sys.ode_stm();
    let tol = Tolerance {
        rtol: 1e-13,
        atol: 1e-16,
    };
    for o in orbits {
        let listed: f64 = text
            .lines()
            .find(|l| l.starts_with(&format!("{},", o.name)))
            .and_then(|l| l.split(',').nth(9))
            .unwrap()
            .parse()
            .unwrap();
        let mut run = Dop853::new(&stm, 0.0, identity_with(o.state), tol);
        run.advance_to(&stm, o.period).unwrap();
        let index = Cr3bp::stability_index(run.y[6..].try_into().unwrap());
        println!(
            "{}: stability index {index:.4} (catalog {listed:.4})",
            o.name
        );
        assert!(
            (index / listed - 1.0).abs() < 1e-3,
            "{}: {index} vs {listed}",
            o.name
        );
    }
}
