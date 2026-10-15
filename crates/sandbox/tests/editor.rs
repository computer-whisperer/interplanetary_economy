//! Editing a ship (M1c.md, step 7 plan, items 6 and 9): the editor
//! opened from a ship's row; a leg's durations typed and committed with
//! Enter, the leg designed again; a wait added; a destination the
//! planner cannot reach saying why at once; undo; ships added and
//! removed.

use damascene_core::prelude::*;
use damascene_wgpu::Runner;
use orbit::ephem::Ephemeris;
use orbit::mission::{ItemKind, Plan, Step};
use orbit::places::Places;
use sandbox::app::Shell;
use sandbox::editor::Field;
use sandbox::frame::{self, Output, View};
use sandbox::panel;
use sandbox::plan::Loaded;

const PLAN: &str = r#"
name = "Edit"

[[ship]]
name = "Lander"
start = { at = "nrho", epoch = "2045-03-05" }

[[ship.leg]]
to = "llo"
days = [0.5]
"#;

/// Draws the app offscreen and drives it with clicks.
struct Harness {
    gpu: render::offscreen::Offscreen,
    ui: Runner,
    view: View,
    texture: wgpu::Texture,
    size: (f32, f32),
}

impl Harness {
    fn new(app: &Shell, size: (f32, f32)) -> Harness {
        let gpu = render::offscreen::Offscreen::new().expect("a GPU");
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: size.0 as u32,
                height: size.1 as u32,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: frame::OFFSCREEN_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let mut ui = Runner::new(&gpu.device, &gpu.queue, frame::OFFSCREEN_FORMAT);
        ui.set_animation_mode(damascene_core::AnimationMode::Settled);
        ui.set_theme(app.theme());
        ui.set_surface_size(size.0 as u32, size.1 as u32);
        let view = View::new(&gpu.device, &gpu.queue);
        Harness {
            gpu,
            ui,
            view,
            texture,
            size,
        }
    }

    fn draw(&mut self, app: &mut Shell) {
        app.poll();
        app.update_paths();
        let scene = app.scene(1.0);
        let target = self.texture.create_view(&Default::default());
        let out = Output {
            texture: &self.texture,
            view: &target,
            msaa: None,
        };
        let viewport = Rect::new(0.0, 0.0, self.size.0, self.size.1);
        frame::draw(
            &self.gpu.device,
            &self.gpu.queue,
            &mut self.ui,
            app,
            &mut self.view,
            &scene,
            out,
            viewport,
            1.0,
            None,
        );
    }

    fn click(&mut self, app: &mut Shell, (x, y): (f32, f32)) {
        let mut events = self
            .ui
            .pointer_down(Pointer::mouse(x, y, PointerButton::Primary));
        events.extend(
            self.ui
                .pointer_up(Pointer::mouse(x, y, PointerButton::Primary)),
        );
        let cx = EventCx::new().with_ui_state(self.ui.ui_state());
        for e in events {
            app.on_event(e, &cx);
        }
    }

    /// Writes the last frame where `OPTIONS_SHOT` says, if it does.
    fn shot(&mut self, app: &mut Shell, name: &str) {
        let Ok(dir) = std::env::var("OPTIONS_SHOT") else {
            return;
        };
        let s = frame::shot(
            &self.gpu,
            app,
            None,
            self.size.0 as u32,
            self.size.1 as u32,
            1.0,
        );
        let file = std::fs::File::create(format!("{dir}/{name}.png")).unwrap();
        let mut png = png::Encoder::new(std::io::BufWriter::new(file), s.width, s.height);
        png.set_color(png::ColorType::Rgba);
        png.set_depth(png::BitDepth::Eight);
        png.write_header()
            .unwrap()
            .write_image_data(&s.rgba)
            .unwrap();
    }
}

impl Harness {
    /// Types `text` into the focused field, as the host does: each key,
    /// then its text.
    fn type_text(&mut self, app: &mut Shell, text: &str) {
        for c in text.chars() {
            let mut events = self.ui.key_down(
                LogicalKey::Character(c.to_string()),
                PhysicalKey::Unidentified,
                KeyModifiers::default(),
                false,
            );
            events.extend(self.ui.text_input(c.to_string()));
            let cx = EventCx::new().with_ui_state(self.ui.ui_state());
            for e in events {
                app.on_event(e, &cx);
            }
            self.draw(app);
        }
    }

    fn key(&mut self, app: &mut Shell, key: NamedKey, modifiers: KeyModifiers) {
        let events = self.ui.key_down(
            LogicalKey::Named(key),
            PhysicalKey::Unidentified,
            modifiers,
            false,
        );
        let cx = EventCx::new().with_ui_state(self.ui.ui_state());
        for e in events {
            app.on_event(e, &cx);
        }
        self.draw(app);
    }

    fn click_key(&mut self, app: &mut Shell, key: &str) {
        let r = self
            .ui
            .rect_of_key(key)
            .unwrap_or_else(|| panic!("{key} is laid out"));
        self.click(app, (r.x + r.w / 2.0, r.y + r.h / 2.0));
        self.draw(app);
    }
}

fn steps(app: &Shell) -> Vec<Step> {
    app.plan.as_ref().unwrap().plan.ships[0].steps.clone()
}

#[test]
fn ships_edited() {
    let eph = Ephemeris::standard();
    let mut plan = Loaded::start(Plan::parse(PLAN, eph, Places::standard()).unwrap());
    plan.wait();
    let mut app = Shell::new(plan.epoch());
    app.load(plan);
    let mut h = Harness::new(&app, (1400.0, 1000.0));
    h.draw(&mut app);

    // The ship's row, found down the panel's list (damascene does not
    // index keyed nodes in a virtual list's rows).
    let list = h.ui.rect_of_key(panel::LIST_KEY).unwrap();
    let x = list.x + list.w / 2.0;
    let y = (0..100)
        .map(|i| list.y + i as f32 * 4.0)
        .find(|&y| {
            let down =
                h.ui.pointer_down(Pointer::mouse(x, y, PointerButton::Primary));
            h.ui.pointer_up(Pointer::mouse(x, y, PointerButton::Primary));
            down.iter()
                .any(|e| e.route() == Some(panel::ship_key(0).as_str()))
        })
        .expect("the ship's row");
    h.click(&mut app, (x, y));
    h.draw(&mut app);
    assert_eq!(app.editor.as_ref().map(|e| e.ship), Some(0));
    h.shot(&mut app, "editor");

    // The leg's durations: 1.5 days, typed over what was there; the
    // period does not warp time while typing.
    let rate = app.clock.rate;
    h.click_key(&mut app, &Field::Days(0).key());
    assert!(app.editor.as_ref().unwrap().typing);
    let ctrl = KeyModifiers {
        ctrl: true,
        ..KeyModifiers::default()
    };
    let events = h.ui.key_down(
        LogicalKey::Character("a".into()),
        PhysicalKey::Unidentified,
        ctrl,
        false,
    );
    let cx = EventCx::new().with_ui_state(h.ui.ui_state());
    for e in events {
        app.on_event(e, &cx);
    }
    h.type_text(&mut app, "1.5");
    assert_eq!(app.clock.rate, rate, "no warp while typing");
    assert!(
        matches!(&steps(&app)[0], Step::Go(g) if g.days == [0.5]),
        "not yet committed"
    );
    h.key(&mut app, NamedKey::Enter, KeyModifiers::default());
    assert!(
        matches!(&steps(&app)[0], Step::Go(g) if g.days == [1.5]),
        "{:?}",
        steps(&app)
    );
    app.plan.as_mut().unwrap().wait();
    let leg = app.plan.as_ref().unwrap().ships[0]
        .items
        .iter()
        .find(|i| i.kind == ItemKind::Leg)
        .unwrap()
        .clone();
    assert!((leg.end.0 - leg.start.0 - 1.5).abs() < 1e-6);

    // A wait added, then a leg to the south pole from orbit (fine), then
    // its destination changed to the NRHO from the surface: the planner's
    // own words, at once.
    h.click_key(&mut app, &Field::AddWait.key());
    assert!(matches!(steps(&app)[1], Step::Wait(d) if d == 1.0));
    h.click_key(&mut app, &Field::AddLeg.key());
    h.click_key(&mut app, &Field::To(2).key());
    h.click_key(
        &mut app,
        &format!("{}:option:moon-south-pole", Field::To(2).key()),
    );
    assert!(matches!(&steps(&app)[2], Step::Go(g) if g.to.label == "moon-south-pole"));
    h.click_key(&mut app, &Field::AddLeg.key());
    h.shot(&mut app, "problems");
    let ship = &app.plan.as_ref().unwrap().plan.ships[0];
    let problems = orbit::mission::check(eph, ship);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(problems[0].0, 3);
    assert!(problems[0].1.contains("left by ascent"));

    // Undo, twice: the last leg, then the destination.
    let ctrl_z = |h: &mut Harness, app: &mut Shell| {
        let events = h.ui.key_down(
            LogicalKey::Character("z".into()),
            PhysicalKey::Unidentified,
            ctrl,
            false,
        );
        let cx = EventCx::new().with_ui_state(h.ui.ui_state());
        for e in events {
            app.on_event(e, &cx);
        }
        h.draw(app);
    };
    ctrl_z(&mut h, &mut app);
    assert_eq!(steps(&app).len(), 3);
    ctrl_z(&mut h, &mut app);
    assert!(matches!(&steps(&app)[2], Step::Go(g) if g.to.label == "nrho"));

    // A ship added, edited at once; removed.
    h.click_key(&mut app, panel::ADD_SHIP_KEY);
    assert_eq!(app.plan.as_ref().unwrap().plan.ships.len(), 2);
    assert_eq!(app.editor.as_ref().map(|e| e.ship), Some(1));
    assert_eq!(app.plan.as_ref().unwrap().ships[1].name, "Ship 2");
    h.click_key(&mut app, &Field::RemoveShip.key());
    assert_eq!(app.plan.as_ref().unwrap().plan.ships.len(), 1);
    assert!(app.editor.is_none());
    app.plan.as_mut().unwrap().wait();
}
