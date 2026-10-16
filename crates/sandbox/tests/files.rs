//! Plans opened and saved (M1c.md, step 7 plan, item 3): the plan menu
//! lists `data/missions/` and opens what is picked; a plan saved reads
//! back as itself; a new plan gets a file of its own.

use damascene_core::prelude::*;
use damascene_wgpu::Runner;
use orbit::time::Epoch;
use sandbox::app::Shell;
use sandbox::frame::{self, Output, View};
use sandbox::plan::{Loaded, new_plan_path, plans_in};

#[test]
fn the_plan_menu_opens_plans() {
    let gpu = render::offscreen::Offscreen::new().expect("a GPU");
    let (device, queue) = (&gpu.device, &gpu.queue);
    let mut app = Shell::new(Epoch::from_ymd(2045, 3, 1));
    let (w, h) = (1000.0, 700.0);
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: w as u32,
            height: h as u32,
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
    ui.set_surface_size(w as u32, h as u32);
    let mut view = View::new(device, queue);
    let viewport = Rect::new(0.0, 0.0, w, h);
    let mut draw = |app: &mut Shell, ui: &mut Runner| {
        app.update_paths();
        let scene = app.scene(1.0);
        let out = Output {
            texture: &texture,
            view: &target,
            msaa: None,
        };
        frame::draw(
            device, queue, ui, app, &mut view, &scene, out, viewport, 1.0, None,
        );
    };
    let click = |app: &mut Shell, ui: &mut Runner, r: Rect| {
        let (x, y) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
        let mut events = ui.pointer_down(Pointer::mouse(x, y, PointerButton::Primary));
        events.extend(ui.pointer_up(Pointer::mouse(x, y, PointerButton::Primary)));
        let cx = EventCx::new().with_ui_state(ui.ui_state());
        for e in events {
            app.on_event(e, &cx);
        }
    };
    draw(&mut app, &mut ui);
    let trigger = ui
        .rect_of_key("plan-file")
        .expect("the plan menu's trigger");
    click(&mut app, &mut ui, trigger);
    draw(&mut app, &mut ui);
    let dir = sandbox::data_dir().join("missions");
    let names: Vec<String> = plans_in(&dir).into_iter().map(|(n, _)| n).collect();
    assert!(names.contains(&"gateway-rotation".to_string()), "{names:?}");
    let option = ui
        .rect_of_key("plan-file:option:file:gateway-rotation")
        .expect("the gateway rotation is offered");
    click(&mut app, &mut ui, option);
    let plan = app.plan.as_ref().expect("a plan is open");
    assert_eq!(plan.plan().name, "Gateway crew rotation");
    assert_eq!(
        plan.path(),
        Some(dir.join("gateway-rotation.toml").as_path())
    );
    assert_eq!(app.clock.t, Epoch::from_ymd(2045, 3, 1));
}

/// Saved, a plan reads back as itself; a new plan's file is not one
/// already there.
#[test]
fn a_plan_saved_reads_back() {
    let dir = std::env::temp_dir().join(format!("sandbox-files-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = new_plan_path(&dir);
    assert_eq!(path, dir.join("new-plan.toml"));
    let mut plan = Loaded::empty("New plan", path.clone());
    plan.wait();
    assert_eq!(plan.save().unwrap(), path);
    assert_eq!(new_plan_path(&dir), dir.join("new-plan-2.toml"));
    let back = Loaded::open(&path).unwrap();
    assert_eq!(back.plan().to_toml(), plan.plan().to_toml());

    // A plan with opening comments, opened and saved again.
    let rotation = dir.join("rotation.toml");
    std::fs::copy(
        sandbox::data_dir().join("missions/gateway-rotation.toml"),
        &rotation,
    )
    .unwrap();
    let mut plan = Loaded::open(&rotation).unwrap();
    assert_eq!(plan.save().unwrap(), rotation);
    let back = Loaded::open(&rotation).unwrap();
    assert_eq!(back.plan().to_toml(), plan.plan().to_toml());
    assert!(
        back.plan().to_toml().starts_with("# A crew rotation"),
        "the file's opening comments are kept"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A career game's log opens in the sandbox as a sandbox log, and can be
/// edited there (found in review: an edit had panicked).
#[test]
fn a_career_log_opens_in_the_sandbox() {
    let path = sandbox::data_dir().join("scenarios/llo-round-trip-replanned.toml");
    let mut plan = Loaded::open(&path).unwrap();
    plan.wait();
    assert_eq!(plan.game().mode(), sim::log::Mode::Sandbox);
    let mut ship = plan.plan().ships[0].clone();
    ship.steps.pop();
    plan.edit(0, ship);
    plan.wait();
    assert!(plan.game().log().replans.is_empty(), "folded into the ship");
    assert!(!plan.game().log().to_toml().contains("mode = \"career\""));
}
