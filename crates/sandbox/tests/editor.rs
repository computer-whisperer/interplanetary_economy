//! Editing a ship (M1c.md, step 7 plan, items 6 and 9): the editor
//! opened from a ship's row; a leg's durations typed and committed with
//! Enter, the leg designed again; a wait added; a destination the
//! planner cannot reach saying why at once; undo; ships added and
//! removed. Its class and load (M1b.md, step 2 plan, part 4); a leg that
//! meets a vessel, and its manifest (M1b.md, step 3 plan, part 7).

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
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
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

    /// Writes the frame as the harness's own runner draws it (menus and
    /// scroll positions as they stand) where `OPTIONS_SHOT` says, if it
    /// does.
    fn shot(&mut self, app: &mut Shell, name: &str) {
        let Ok(dir) = std::env::var("OPTIONS_SHOT") else {
            return;
        };
        self.draw(app);
        let rgba = self.gpu.read(&self.texture);
        let (width, height) = (self.size.0 as u32, self.size.1 as u32);
        let file = std::fs::File::create(format!("{dir}/{name}.png")).unwrap();
        let mut png = png::Encoder::new(std::io::BufWriter::new(file), width, height);
        png.set_color(png::ColorType::Rgba);
        png.set_depth(png::BitDepth::Eight);
        png.write_header().unwrap().write_image_data(&rgba).unwrap();
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

    /// Scrolls the view keyed `scroll` until `key` lies within it, a
    /// wheel line at a time as the host does: offered to the app, then
    /// the runner's own scroll.
    fn scroll_to(&mut self, app: &mut Shell, scroll: &str, key: &str) {
        for _ in 0..100 {
            let view = self.ui.rect_of_key(scroll).expect("the scroll view");
            let r = self
                .ui
                .rect_of_key(key)
                .unwrap_or_else(|| panic!("{key} is laid out"));
            if r.y >= view.y && r.y + r.h <= view.y + view.h {
                return;
            }
            let (x, y) = (view.x + view.w / 2.0, view.y + view.h / 2.0);
            let dy = if r.y + r.h > view.y + view.h {
                sandbox::host::WHEEL_LINE
            } else {
                -sandbox::host::WHEEL_LINE
            };
            let taken = self.ui.pointer_wheel_event(x, y, 0.0, dy).is_some_and(|e| {
                let cx = EventCx::new().with_ui_state(self.ui.ui_state());
                app.on_wheel_event(e, &cx)
            });
            if !taken {
                self.ui.pointer_wheel(x, y, dy);
            }
            self.draw(app);
        }
        panic!("{key} did not come into {scroll}");
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
    app.plan.as_ref().unwrap().plan().ships[0].steps.clone()
}

/// Opens the editor on ship `s` by its row, found down the panel's list
/// (damascene does not index keyed nodes in a virtual list's rows).
fn open_editor_on(h: &mut Harness, app: &mut Shell, s: usize) {
    let list = h.ui.rect_of_key(panel::LIST_KEY).unwrap();
    let x = list.x + list.w / 2.0;
    let y = (0..250)
        .map(|i| list.y + i as f32 * 4.0)
        .find(|&y| {
            let down =
                h.ui.pointer_down(Pointer::mouse(x, y, PointerButton::Primary));
            h.ui.pointer_up(Pointer::mouse(x, y, PointerButton::Primary));
            down.iter()
                .any(|e| e.route() == Some(panel::ship_key(s).as_str()))
        })
        .expect("the ship's row");
    h.click(app, (x, y));
    h.draw(app);
    assert_eq!(app.editor.as_ref().map(|e| e.ship), Some(s));
}

/// Opens the editor on the first ship.
fn open_editor(h: &mut Harness, app: &mut Shell) {
    open_editor_on(h, app, 0);
}

/// Selects all of the focused field's text.
fn select_all(h: &mut Harness, app: &mut Shell) {
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
}

#[test]
fn ships_edited() {
    let eph = Ephemeris::standard();
    let mut plan = Loaded::start(Plan::parse(PLAN, eph, Places::standard()).unwrap()).unwrap();
    plan.wait();
    let mut app = Shell::new(plan.epoch());
    app.load(plan);
    let mut h = Harness::new(&app, (1400.0, 1000.0));
    h.draw(&mut app);
    open_editor(&mut h, &mut app);
    h.shot(&mut app, "editor");

    // The leg's durations: 1.5 days, typed over what was there; the
    // period does not warp time while typing.
    let rate = app.clock.rate;
    h.click_key(&mut app, &Field::Days(0).key());
    assert!(app.editor.as_ref().unwrap().typing);
    select_all(&mut h, &mut app);
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
    let ship = &app.plan.as_ref().unwrap().plan().ships[0];
    let problems = orbit::mission::check(eph, ship);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(problems[0].0, 3);
    assert!(problems[0].1.contains("left by ascent"));

    // Undo, twice: the last leg, then the destination.
    let ctrl = KeyModifiers {
        ctrl: true,
        ..KeyModifiers::default()
    };
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
    assert_eq!(app.plan.as_ref().unwrap().plan().ships.len(), 2);
    assert_eq!(app.editor.as_ref().map(|e| e.ship), Some(1));
    assert_eq!(app.plan.as_ref().unwrap().ships[1].name, "Ship 2");
    h.click_key(&mut app, &Field::RemoveShip.key());
    assert_eq!(app.plan.as_ref().unwrap().plan().ships.len(), 1);
    assert!(app.editor.is_none());
    app.plan.as_mut().unwrap().wait();
}

/// A class from its menu, a load typed (M1b.md, step 2 plan, part 4):
/// nothing designed again; a hold overfilled, a warning; a negative load,
/// refused; "none", a massless ship carrying nothing. A load typed, then
/// the class undone: the draft goes with it (found in review: committed
/// to the classless ship, it panicked).
#[test]
fn class_and_load_edited() {
    let eph = Ephemeris::standard();
    let mut plan = Loaded::start(Plan::parse(PLAN, eph, Places::standard()).unwrap()).unwrap();
    plan.wait();
    let mut app = Shell::new(plan.epoch());
    app.load(plan);
    let mut h = Harness::new(&app, (1400.0, 1000.0));
    h.draw(&mut app);
    open_editor(&mut h, &mut app);
    let made = app.plan.as_ref().unwrap().game().designs_made();
    let revision = app.plan.as_ref().unwrap().ships[0].revision;
    let ship = |app: &Shell| app.plan.as_ref().unwrap().plan().ships[0].clone();

    h.click_key(&mut app, &Field::Class.key());
    h.shot(&mut app, "class-menu");
    h.click_key(&mut app, &format!("{}:option:lander", Field::Class.key()));
    assert_eq!(ship(&app).class.as_deref(), Some("lander"));
    assert_eq!(app.plan.as_ref().unwrap().ships[0].revision, revision);

    let typed = |h: &mut Harness, app: &mut Shell, field: Field, text: &str| {
        h.click_key(app, &field.key());
        select_all(h, app);
        h.type_text(app, text);
        h.key(app, NamedKey::Enter, KeyModifiers::default());
    };
    typed(&mut h, &mut app, Field::Propellant, "300");
    typed(
        &mut h,
        &mut app,
        Field::Cargo(sim::ships::Commodity::Equipment),
        "120",
    );
    assert_eq!(ship(&app).propellant_t, Some(300.0));
    assert_eq!(ship(&app).cargo.get("equipment"), Some(&120.0));
    app.plan.as_mut().unwrap().wait();
    h.draw(&mut app);
    let view = &app.plan.as_ref().unwrap().ships[0];
    assert_eq!(view.warnings.len(), 1, "{:?}", view.warnings);
    assert_eq!(view.warnings[0].why, "a lander's hold takes 100 t, not 120");
    assert!(view.tanks.is_some());
    h.shot(&mut app, "load");

    typed(&mut h, &mut app, Field::Propellant, "-1");
    assert_eq!(ship(&app).propellant_t, Some(300.0));
    assert!(app.editor.as_ref().unwrap().refused.is_some());

    h.click_key(&mut app, &Field::Class.key());
    h.click_key(&mut app, &format!("{}:option:none", Field::Class.key()));
    assert_eq!(ship(&app).class, None);
    assert!(ship(&app).propellant_t.is_none() && ship(&app).cargo.is_empty());

    h.click_key(&mut app, &Field::Class.key());
    h.click_key(&mut app, &format!("{}:option:lander", Field::Class.key()));
    h.click_key(&mut app, &Field::Propellant.key());
    select_all(&mut h, &mut app);
    h.type_text(&mut app, "100");
    let ctrl = KeyModifiers {
        ctrl: true,
        ..KeyModifiers::default()
    };
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
    h.draw(&mut app);
    assert_eq!(ship(&app).class, None);
    assert!(app.editor.as_ref().unwrap().drafts.is_empty());
    h.click_key(&mut app, &Field::Done.key());
    assert!(app.editor.is_none());
    assert!(ship(&app).class.is_none() && ship(&app).propellant_t.is_none());
    assert_eq!(app.plan.as_ref().unwrap().game().designs_made(), made);
}

/// A leg's destination chosen among the plan's vessels, and its manifest
/// typed (M1b.md, step 3 plan, part 7): the tug meets the depot where it
/// starts, the trigger names it, and once designed the panel shows what
/// it gave on arrival and what both were bought for at LEO.
#[test]
fn a_leg_meets_a_vessel_with_a_manifest() {
    const MEET: &str = r#"
name = "Meet"

[[ship]]
id = 0
name = "Depot"
class = "hauler"
start = { at = "nrho", epoch = "2045-03-01" }

[[ship.leg]]
wait = 30

[[ship]]
id = 1
name = "Tug"
class = "tug-h"
start = { at = "leo", epoch = "2045-03-01" }

[[ship.leg]]
to = "nrho"
days = [5.0]
"#;
    let eph = Ephemeris::standard();
    let mut plan = Loaded::start(Plan::parse(MEET, eph, Places::standard()).unwrap()).unwrap();
    plan.wait();
    let mut app = Shell::new(plan.epoch());
    app.load(plan);
    let mut h = Harness::new(&app, (1400.0, 1000.0));
    h.draw(&mut app);
    open_editor_on(&mut h, &mut app, 1);
    let tug = |app: &Shell| app.plan.as_ref().unwrap().plan().ships[1].clone();

    h.click_key(&mut app, &Field::To(0).key());
    h.shot(&mut app, "vessel-menu");
    h.click_key(&mut app, &format!("{}:option:meet:0", Field::To(0).key()));
    assert_eq!(app.editor.as_ref().unwrap().refused, None);
    let Step::Go(go) = &tug(&app).steps[0] else {
        unreachable!()
    };
    assert_eq!(
        go.to.meeting,
        Some(orbit::mission::Meeting { ship: 0, leg: None })
    );
    assert_eq!(
        sandbox::editor::destination(&tug(&app), &app.plan.as_ref().unwrap().plan().ships, 0),
        "Depot, start"
    );
    assert!(app.editor.as_ref().unwrap().refused.is_none());

    // Under the editor's fold: scrolled to, as the wheel would.
    let field = Field::LegPropellant(0).key();
    h.scroll_to(&mut app, sandbox::editor::SCROLL_KEY, &field);
    h.click_key(&mut app, &field);
    assert!(
        app.editor.as_ref().unwrap().typing,
        "the field has the keyboard"
    );
    select_all(&mut h, &mut app);
    h.type_text(&mut app, "20");
    h.key(&mut app, NamedKey::Enter, KeyModifiers::default());
    let Step::Go(go) = &tug(&app).steps[0] else {
        unreachable!()
    };
    assert_eq!(go.manifest.propellant_t, Some(20.0));
    h.shot(&mut app, "manifest");

    app.plan.as_mut().unwrap().wait();
    h.draw(&mut app);
    let plan = app.plan.as_ref().unwrap();
    assert!(
        plan.ships[1].failure.is_none(),
        "{:?}",
        plan.ships[1].failure
    );
    let rows = panel::rows(eph, &plan.ships, app.clock.t, None);
    let tug_row = 1 + plan.ships[0].items.len();
    let starts = |row: &panel::Row, prefix: &str| {
        assert!(
            row.lines.iter().any(|(l, _)| l.starts_with(prefix)),
            "no line {prefix:?} in {:?}",
            row.lines
        );
    };
    // The tug, starting at LEO, is bought there; the depot, starting at
    // the NRHO, is simply there.
    starts(&rows[tug_row], "Bought at LEO for $");
    assert!(
        rows[0].lines.iter().all(|(l, _)| !l.starts_with("Bought")),
        "{:?}",
        rows[0].lines
    );
    // The tug's plan ends at the depot: its arrival is the stop. The
    // depot's tanks are full (its start names no load), so the hydrogen
    // goes into its hold and the LOX has no room.
    let arrival = plan.ships[1].items.len() - 1;
    let stop = &rows[tug_row + 1 + arrival];
    let line = |prefix: &str| {
        stop.lines
            .iter()
            .find(|(l, _)| l.starts_with(prefix))
            .unwrap_or_else(|| panic!("no line {prefix:?} in {:?}", stop.lines))
            .clone()
    };
    let gave = line("gave LH2 ");
    assert_eq!(gave.1, panel::Tone::Muted);
    assert!(gave.0.ends_with(" to Depot"), "{}", gave.0);
    // (less the few kilograms its station-keeping since arriving made
    // room for).
    let short = line("give of ");
    assert_eq!(short.1, panel::Tone::Warning);
    assert!(
        short.0.contains(" of LOX fell short: ") && short.0.ends_with("(no room for it)"),
        "{}",
        short.0
    );
}

/// Docking in the editor (M1b.md, step 4 plan, part 6): the tug's leg to
/// the depot docks by its switch; the module's start is set to the tug from
/// the start's menu, a dock step is added and pointed at the depot from its
/// menu; once designed, the panel shows the module's docks by name and the
/// tug's leg carrying it.
#[test]
fn a_module_is_docked_in_the_editor() {
    use orbit::mission::{ItemKind, Meeting};
    const ASSEMBLY: &str = r#"
name = "Assembly"

[[ship]]
id = 0
name = "Depot"
class = "hauler"
start = { at = "nrho", epoch = "2045-03-01" }

[[ship.leg]]
wait = 30

[[ship]]
id = 1
name = "Tug"
class = "tug-h"
start = { at = "leo", epoch = "2045-03-01" }

[[ship.leg]]
to = { ship = 0 }
days = [5.0]

[[ship]]
id = 2
name = "Hab"
class = "module"
start = { at = "leo", epoch = "2045-03-01" }
"#;
    let eph = Ephemeris::standard();
    let mut plan = Loaded::start(Plan::parse(ASSEMBLY, eph, Places::standard()).unwrap()).unwrap();
    plan.wait();
    let mut app = Shell::new(plan.epoch());
    app.load(plan);
    let mut h = Harness::new(&app, (1400.0, 1000.0));
    h.draw(&mut app);
    let ship = |app: &Shell, s: usize| app.plan.as_ref().unwrap().plan().ships[s].clone();

    // The tug's leg to the depot docks.
    open_editor_on(&mut h, &mut app, 1);
    h.click_key(&mut app, &Field::Dock(0).key());
    assert_eq!(app.editor.as_ref().unwrap().refused, None);
    assert!(matches!(&ship(&app, 1).steps[0], Step::Go(g) if g.dock));
    h.shot(&mut app, "dock-switch");
    h.click_key(&mut app, &Field::Done.key());
    assert!(app.editor.is_none());

    // The module starts docked to the tug, and docks to the depot.
    open_editor_on(&mut h, &mut app, 2);
    h.click_key(&mut app, &Field::Start.key());
    h.shot(&mut app, "start-menu");
    h.click_key(&mut app, &format!("{}:option:meet:1", Field::Start.key()));
    assert_eq!(app.editor.as_ref().unwrap().refused, None);
    assert_eq!(
        ship(&app, 2).start.meeting,
        Some(Meeting { ship: 1, leg: None })
    );
    h.click_key(&mut app, &Field::AddDock.key());
    assert!(matches!(&ship(&app, 2).steps[0], Step::Dock(_)));
    h.click_key(&mut app, &Field::DockTo(0).key());
    h.click_key(
        &mut app,
        &format!("{}:option:meet:0", Field::DockTo(0).key()),
    );
    assert_eq!(app.editor.as_ref().unwrap().refused, None);
    let Step::Dock(d) = &ship(&app, 2).steps[0] else {
        unreachable!()
    };
    assert_eq!(d.to.meeting, Some(Meeting { ship: 0, leg: None }));
    h.shot(&mut app, "dock-step");

    app.plan.as_mut().unwrap().wait();
    h.draw(&mut app);
    let plan = app.plan.as_ref().unwrap();
    for s in &plan.ships {
        assert!(s.failure.is_none(), "{}: {:?}", s.name, s.failure);
    }
    let kinds: Vec<ItemKind> = plan.ships[2].items.iter().map(|i| i.kind.clone()).collect();
    assert_eq!(kinds, [ItemKind::Docked(1), ItemKind::Docked(0)]);
    let rows = panel::rows(eph, &plan.ships, app.clock.t, None);
    let labels: Vec<&str> = rows.iter().map(|r| r.label.as_str()).collect();
    assert!(
        labels.contains(&"to Tug") && labels.contains(&"to Depot"),
        "{labels:?}"
    );
    let docks: Vec<&str> = rows
        .iter()
        .filter(|r| r.kind == "docked")
        .map(|r| r.label.as_str())
        .collect();
    assert_eq!(docks, ["to Depot", "to Tug", "to Depot"]);
    let tug_leg = rows
        .iter()
        .find(|r| r.label == "leo → ship 0")
        .expect("the tug's leg");
    assert!(
        tug_leg
            .lines
            .iter()
            .any(|(l, _)| l.contains("with 20") && l.contains("docked")),
        "{:?}",
        tug_leg.lines
    );
    h.shot(&mut app, "assembly-panel");
}
