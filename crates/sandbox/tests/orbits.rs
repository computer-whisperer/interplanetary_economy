//! The bodies' orbits in the app's scene (M1c.md, step 6 plan, items 3
//! and 7): in every frame, each orbit's line passes through a body now.

use orbit::time::Epoch;
use sandbox::app::Shell;
use sandbox::frames::Frame;
use sandbox::paths::ORBIT_SET;

#[test]
fn every_orbit_passes_through_its_body_in_every_frame() {
    let t = Epoch::from_ymd(2046, 7, 9).plus_days(0.61);
    let mut app = Shell::new(t);
    let eph = app.eph;
    let bodies: Vec<_> = eph
        .ids()
        .filter(|&b| eph.covers(b, t))
        .map(|b| eph.barycentric_position(b, t))
        .collect();
    for (id, _, frame) in Frame::offered(eph) {
        app.set_frame(frame);
        app.update_paths();
        let scene = app.scene(1.0);
        let orbits = &scene.paths[ORBIT_SET];
        assert!(
            orbits.paths.len() >= 12,
            "{id}: {} orbits",
            orbits.paths.len()
        );
        for path in &orbits.paths {
            let pose = scene.poses[path.pose];
            let at = pose.to_world(path.at(t.seconds()).expect("now is in the window"));
            let nearest = bodies
                .iter()
                .map(|b| (*b - at).length())
                .fold(f64::INFINITY, f64::min);
            // Hermite at a 384th of a period or 10 days, within 1e-8 of
            // the orbit's radius (`orbits`' own test checks it in pixels).
            let radius = (at - pose.origin).length();
            assert!(
                nearest < 1e-8 * radius,
                "{id}: {nearest} km off, radius {radius} km"
            );
        }
    }
}
