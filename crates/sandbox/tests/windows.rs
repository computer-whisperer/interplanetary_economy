//! The window lanes (M1c.md, step 7 plan, items 4 and 9): one per kind
//! of leg between planets in the plan, each leg in its lane; their
//! cheapest days at the Mars windows.

use orbit::mission::Step;
use orbit::time::Epoch;
use sandbox::plan::Loaded;

/// The published plan's legs between planets: from LEO to low Mars orbit,
/// directly and by aerocapture, and to 2000 SG344, each in its lane; the
/// Mars lane's cheapest day about each handbook date the plan pins lies
/// near it.
#[test]
fn lanes_and_their_windows() {
    let mut plan = Loaded::open(&sandbox::data_dir().join("missions/published.toml")).unwrap();
    plan.windows.wait();
    let labels: Vec<&str> = plan
        .windows
        .lanes
        .iter()
        .map(|l| l.label.as_str())
        .collect();
    println!("{labels:?}");
    assert_eq!(
        labels,
        [
            "leo → mars r 3790 km",
            "leo → mars r 3790 km (aero)",
            "leo → 2000-sg344"
        ]
    );
    // Each leg between planets is in the lane of its ends; the rest in
    // none.
    let mut legs = 0;
    for (s, ship) in plan.plan().ships.iter().enumerate() {
        for (k, step) in ship.steps.iter().enumerate() {
            let Step::Go(go) = step else { continue };
            let between = go.to.label.starts_with("mars") || go.to.label == "2000-sg344";
            match plan.windows.legs.get(&(s, k)) {
                Some(&l) => {
                    let aero = if go.aero { " (aero)" } else { "" };
                    assert_eq!(
                        labels[l],
                        format!("{} → {}{aero}", ship.start.label, go.to.label)
                    );
                    legs += 1;
                }
                None => assert!(!between, "{}: {}", ship.name, go.to.label),
            }
        }
    }
    assert_eq!(legs, plan.windows.legs.len());
    assert!(legs >= 6, "{legs} legs between planets");
    let mars = &plan.windows.lanes[0];
    for (date, ship) in [
        (Epoch::from_ymd(2031, 1, 28), "Mars 2031, Type I"),
        (Epoch::from_ymd(2033, 4, 6), "Mars 2033, Type I"),
        (Epoch::from_ymd(2043, 11, 15), "Mars 2043, Type II"),
    ] {
        let s = plan
            .plan()
            .ships
            .iter()
            .position(|s| s.name == ship)
            .unwrap();
        assert!(
            plan.windows.legs.contains_key(&(s, 0)),
            "{ship}'s leg is in a lane"
        );
        let days = mars.cheapest.as_ref().unwrap();
        let k0 = (date.0 - 90.0 - mars.first.0) as usize;
        let best = (k0..k0 + 181)
            .min_by(|&a, &b| days[a].total_cmp(&days[b]))
            .unwrap();
        let off = mars.first.0 + best as f64 - date.0;
        let there = days[(date.0 - mars.first.0) as usize];
        println!(
            "{ship}: cheapest {:.3} km/s, {off:+.0} days from the handbook's date ({there:.3} there)",
            days[best]
        );
        // The handbook's dates are the least C3 leaving Earth; the lane's
        // least Δv counts the capture at Mars too.
        assert!(off.abs() <= 45.0 && there < 1.1 * days[best]);
        // The lane's least over the window is its days' least there.
        let near = mars.least_between(date.plus_days(-90.0), date.plus_days(91.0));
        assert_eq!(near, days[best]);
    }
}

/// Lanes are their legs' exact ends: two orbits that read alike are two
/// lanes, one orbit written two ways is one.
#[test]
fn lanes_are_their_exact_ends() {
    let src = r#"
name = "x"
[[ship]]
name = "a"
start = { at = "leo", epoch = "2033-01-01" }
[[ship.leg]]
to = { low = "mars", radius = 3790.2 }
[[ship]]
name = "b"
start = { at = "leo", epoch = "2033-01-01" }
[[ship.leg]]
to = { low = "mars", radius = 3790.4 }
[[ship]]
name = "c"
start = { at = "leo", epoch = "2033-01-01" }
[[ship.leg]]
to = { low = "mars", radius = 3790.2 }
"#;
    let plan = orbit::mission::Plan::parse(
        src,
        orbit::ephem::Ephemeris::standard(),
        orbit::places::Places::standard(),
    )
    .unwrap();
    let mut windows = sandbox::windows::Windows::default();
    let pool = rayon::ThreadPoolBuilder::new().build().unwrap();
    windows.update(&plan, &pool);
    assert_eq!(windows.lanes.len(), 2);
    assert_eq!(
        windows.lanes[0].label, windows.lanes[1].label,
        "they read alike"
    );
    assert_eq!(windows.legs[&(0, 0)], windows.legs[&(2, 0)]);
    assert_ne!(windows.legs[&(0, 0)], windows.legs[&(1, 0)]);
}
