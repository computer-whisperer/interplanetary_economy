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
