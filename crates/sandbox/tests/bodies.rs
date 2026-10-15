//! Bodies with their maps in the app's scene (M1c.md, step 6 plan,
//! item 4).

use orbit::time::Epoch;
use render::offscreen::Offscreen;
use sandbox::app::Shell;
use sandbox::camera::MapCamera;

/// GPU time at 1920×1080 of the whole scene with Earth filling the view,
/// with its maps (color, relief, clouds, night lights, water) and
/// without.
#[test]
#[ignore = "a measurement: run with --ignored --nocapture"]
fn textured_bodies_gpu_time() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = render::Renderer::new(&gpu.device, &gpu.queue);
    sandbox::maps::wait().expect("data/bodies reads");
    sandbox::sky::wait().expect("data/sky reads");
    let mut app = Shell::new(Epoch::from_ymd(2045, 3, 1));
    let earth = app.eph.lookup("earth").unwrap();
    let mut camera = MapCamera::new(app.eph, earth);
    camera.set_altitude(app.eph, 6_000.0);
    camera.set_direction(-0.5, 0.2);
    app.camera = camera;
    app.update_paths();
    let scene = app.scene(1.0);
    let mut plain = scene.clone();
    for s in &mut plain.spheres {
        s.maps = None;
    }
    for (what, scene) in [("with maps", &scene), ("without", &plain)] {
        for _ in 0..3 {
            if let Some(ms) = gpu.time_scene(&mut renderer, scene, (1920, 1080)) {
                println!("{what}: {ms:.3} ms");
            }
        }
    }
}
