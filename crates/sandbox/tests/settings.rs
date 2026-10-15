//! The settings menu, through damascene as the window delivers input, in
//! the window's first size: the gear opens it, its slider sets the sky's
//! lift by whole stops, and a click outside it or Escape closes it. The
//! lift reaches the sky and bodies' points of light alone.

use damascene_core::prelude::*;
use damascene_wgpu::Runner;
use glam::DVec3;
use orbit::time::Epoch;
use render::offscreen::Offscreen;
use sandbox::app::Shell;
use sandbox::camera::MapCamera;
use sandbox::frame::{self, Output, View};
use sandbox::look::{BACKGROUND_LIFT, MAX_LIFT};
use sandbox::settings;

#[test]
fn the_menu_sets_the_sky_lift() {
    let gpu = render::offscreen::Offscreen::new().expect("a GPU");
    let (device, queue) = (&gpu.device, &gpu.queue);
    for scale in [1.0f32, 2.0] {
        let mut app = Shell::new(Epoch::from_ymd(2045, 3, 1));
        let (w, h) = (1280.0, 800.0);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: (w * scale) as u32,
                height: (h * scale) as u32,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: frame::OFFSCREEN_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let target = texture.create_view(&Default::default());
        let mut ui = Runner::new(device, queue, frame::OFFSCREEN_FORMAT);
        ui.set_animation_mode(damascene_core::AnimationMode::Settled);
        ui.set_theme(app.theme());
        ui.set_surface_size((w * scale) as u32, (h * scale) as u32);
        let mut view = View::new(device, queue);
        let viewport = Rect::new(0.0, 0.0, w, h);
        let mut draw = |app: &mut Shell, ui: &mut Runner| {
            let scene = app.scene(scale);
            let out = Output {
                texture: &texture,
                view: &target,
                msaa: None,
            };
            frame::draw(device, queue, ui, app, &mut view, &scene, out, viewport, scale, None);
            scene
        };
        let deliver = |app: &mut Shell, ui: &Runner, events: Vec<UiEvent>| {
            let cx = EventCx::new().with_ui_state(ui.ui_state());
            for e in events {
                app.on_event(e, &cx);
            }
        };
        let click = |app: &mut Shell, ui: &mut Runner, (x, y): (f32, f32)| {
            let mut events = ui.pointer_down(Pointer::mouse(x, y, PointerButton::Primary));
            events.extend(ui.pointer_up(Pointer::mouse(x, y, PointerButton::Primary)));
            deliver(app, ui, events);
        };
        let center = |r: Rect| (r.x + r.w / 2.0, r.y + r.h / 2.0);

        let scene = draw(&mut app, &mut ui);
        let exposure = scene.look.exposure;
        assert_eq!(scene.look.sky_lift, 2f32.powi(BACKGROUND_LIFT));
        let gear = ui.rect_of_key(settings::KEY).expect("the gear is laid out");
        assert!(gear.x + gear.w < w, "the gear is in the window: {gear:?}");
        click(&mut app, &mut ui, center(gear));
        assert!(app.settings_open, "scale {scale}: the gear opens the menu");
        draw(&mut app, &mut ui);
        let lift = ui.rect_of_key(settings::LIFT_KEY).expect("the slider is laid out");
        println!("scale {scale}: gear {gear:?}, slider {lift:?}");
        // Its ends, then its middle; each a whole stop.
        for (x, want) in [
            (lift.x + 1.0, 0),
            (lift.x + lift.w - 1.0, MAX_LIFT),
            (lift.x + lift.w / 2.0, (MAX_LIFT as f32 / 2.0).round() as i32),
        ] {
            click(&mut app, &mut ui, (x, lift.y + lift.h / 2.0));
            assert_eq!(app.settings.background_lift, want, "scale {scale}, at x {x}");
            assert!(app.settings_open, "the slider leaves the menu open");
            let scene = draw(&mut app, &mut ui);
            assert_eq!(scene.look.sky_lift, 2f32.powi(want));
            assert_eq!(scene.look.exposure, exposure, "the lift leaves the scene's exposure");
        }
        // The arrow keys, on the slider focused by the clicks.
        let events = ui.key_down(
            LogicalKey::Named(NamedKey::ArrowRight),
            PhysicalKey::ArrowRight,
            KeyModifiers::default(),
            false,
        );
        deliver(&mut app, &ui, events);
        assert_eq!(app.settings.background_lift, MAX_LIFT / 2 + 1);

        // Outside it, on the map: closed, and the map takes no click.
        let focus = app.camera.focus;
        click(&mut app, &mut ui, (w / 2.0, h / 2.0));
        assert!(!app.settings_open, "a click outside closes the menu");
        assert_eq!(app.camera.focus, focus);
        draw(&mut app, &mut ui);
        click(&mut app, &mut ui, center(gear));
        assert!(app.settings_open);
        draw(&mut app, &mut ui);
        let events = ui.key_down(
            LogicalKey::Named(NamedKey::Escape),
            PhysicalKey::Escape,
            KeyModifiers::default(),
            false,
        );
        deliver(&mut app, &ui, events);
        assert!(!app.settings_open, "Escape closes the menu");
    }
}

/// With the sky left out, what the lift reaches: bodies drawn as points
/// of light, by its stops (Jupiter from beside Earth, its point's flux
/// 2¹⁷× at a lift of 17, within 1%), and nothing else: at lifts 0 and
/// [`MAX_LIFT`], pixel for pixel away from bodies' points, Earth with its
/// air (and the Moon's disk), the orbits' lines and the station orbits'
/// markers, and Saturn with its rings.
#[test]
fn the_lift_reaches_the_sky_and_points_alone() {
    let gpu = Offscreen::new().expect("a GPU");
    let mut renderer = render::Renderer::new(&gpu.device, &gpu.queue);
    sandbox::maps::wait().expect("data/bodies reads");
    let size = (640, 400);
    let eph = orbit::ephem::Ephemeris::standard();
    let (earth, saturn, jupiter) = (
        eph.lookup("earth").unwrap(),
        eph.lookup("saturn").unwrap(),
        eph.lookup("jupiter").unwrap(),
    );
    let mut shown = |t: Epoch, focus, camera: Option<render::Camera>, lift: i32| {
        let mut app = Shell::new(t);
        app.camera = MapCamera::new(eph, focus);
        app.settings.background_lift = lift;
        app.update_paths();
        let mut scene = app.scene(1.0);
        scene.sky = None;
        if camera.is_some() {
            scene.camera = camera;
        }
        let target = render::Target::new(&gpu.device, size.0, size.1);
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        renderer.render(&gpu.device, &gpu.queue, &mut encoder, &target, &scene, None);
        gpu.queue.submit(Some(encoder.finish()));
        (scene, gpu.read_rgba16f(&target.hdr))
    };
    let t = Epoch::from_ymd(2045, 3, 1);
    for (name, focus) in [("Earth", earth), ("Saturn", saturn)] {
        let (scene, true_sky) = shown(t, focus, None, 0);
        let (_, lifted) = shown(t, focus, None, MAX_LIFT);
        // Where bodies under 4 px in radius land (points, or fading
        // from them into their disks).
        let camera = scene.camera.unwrap();
        let pixel = 2.0 * (camera.fov_y / 2.0).tan() / f64::from(size.1);
        let points: Vec<glam::DVec2> = scene
            .spheres
            .iter()
            .filter(|s| (s.radius / (s.center - camera.eye).length()).asin() / pixel < 4.0)
            .filter_map(|s| camera.project(s.center, size))
            .collect();
        let (mut lit, mut differ) = (0, 0);
        for (i, (a, b)) in true_sky.iter().zip(&lifted).enumerate() {
            let at = glam::DVec2::new(
                (i % size.0 as usize) as f64 + 0.5,
                (i / size.0 as usize) as f64 + 0.5,
            );
            if points.iter().any(|p| p.distance(at) < 8.0) {
                continue;
            }
            lit += usize::from(a[..3].iter().any(|&c| c > 0.0));
            differ += usize::from(a != b);
        }
        println!("{name}: {} points; elsewhere {lit} pixels lit, {differ} differ", points.len());
        assert!(lit > 1000, "{name} shows little");
        assert_eq!(differ, 0, "{name}: the lift reaches past the sky and points");
    }

    // Jupiter near opposition, from beside Earth.
    let t = Epoch::from_ymd(2045, 9, 1);
    let pe = eph.barycentric_position(earth, t);
    let pj = eph.barycentric_position(jupiter, t);
    let eye = pe + (pj - pe).normalize() * 2e5;
    let camera = render::Camera::looking_at(eye, pj, DVec3::Z, 50f64.to_radians());
    let p = camera.project(pj, size).expect("Jupiter is in front");
    let (cx, cy) = (p.x as i64, p.y as i64);
    let flux = |image: &[[f32; 4]]| -> f64 {
        (cy - 4..=cy + 4)
            .flat_map(|y| (cx - 4..=cx + 4).map(move |x| (x, y)))
            .map(|(x, y)| f64::from(image[(y * i64::from(size.0) + x) as usize][1]))
            .sum()
    };
    let (_, true_sky) = shown(t, earth, Some(camera), 0);
    let (_, lifted) = shown(t, earth, Some(camera), 17);
    let ratio = flux(&lifted) / flux(&true_sky);
    println!("Jupiter's point: {:.4e}, lifted {ratio:.1}×", flux(&true_sky));
    assert!(flux(&true_sky) > 0.0, "Jupiter's point is drawn");
    assert!((ratio / 2f64.powi(17) - 1.0).abs() < 0.01, "{ratio}");
}
