//! The whole frame drawn offscreen (M1c.md, step 2 plan, item 7): the
//! scene shows inside the map view's rect, the UI around it, and the
//! scene's target matches the map view's size.

use sandbox::app::Shell;
use sandbox::frame::shot;

/// sRGB's encoding of a linear value, as an 8-bit byte.
fn srgb(linear: f32) -> f32 {
    let v = if linear <= 0.003_130_8 {
        12.92 * linear
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    };
    v * 255.0
}

#[test]
fn scene_shows_in_the_map_view() {
    let gpu = render::offscreen::Offscreen::new().expect("a GPU");
    let mut app = Shell::new(orbit::time::Epoch::from_ymd(2045, 3, 1));
    let scene = render::Scene {
        background: [0.0, 0.5, 0.0],
        ..Default::default()
    };
    let (w, h) = (640, 400);
    let s = shot(&gpu, &mut app, Some(scene), w, h, 1.0);
    let map = s.viewport.expect("the map view is laid out");
    println!("map view {map:?}, target {:?}", s.target);
    assert_eq!(s.target, Some((map.w.round() as u32, map.h.round() as u32)));
    assert_eq!(map.w, w as f32, "the map view spans the window");
    assert!(
        map.h > 0.6 * h as f32 && map.h < h as f32,
        "bars above and below"
    );
    let pixel = |x: f32, y: f32| {
        let i = 4 * (y as usize * w as usize + x as usize);
        [s.rgba[i], s.rgba[i + 1], s.rgba[i + 2]]
    };
    // Inside: the scene's linear color, encoded once.
    let inside = pixel(map.x + map.w / 2.0, map.y + map.h / 2.0);
    let green = srgb(0.5);
    assert!(
        inside[0] == 0 && inside[2] == 0 && (inside[1] as f32 - green).abs() <= 1.0,
        "{inside:?}, wanted green {green:.1}"
    );
    // Outside, in the bars: the UI.
    for y in [
        map.y / 2.0,
        map.y + map.h + (h as f32 - map.y - map.h) / 2.0,
    ] {
        let bar = pixel(w as f32 / 2.0, y);
        assert!(bar[1] < 64, "the bar at y {y} shows the scene: {bar:?}");
    }
}
