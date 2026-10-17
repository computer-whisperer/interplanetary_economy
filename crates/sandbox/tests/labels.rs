//! Labels, picking and the plan's panel (M1c.md, step 5 plan, item 4):
//! labels beside what they name and drawn where placed, none for what is
//! hidden; clicks picking what they land on; the panel's rows.

use damascene_core::prelude::*;
use damascene_wgpu::Runner;
use glam::{DVec3, Vec2};
use orbit::ephem::Ephemeris;
use orbit::mission::{Item, ItemKind, Plan, when};
use orbit::places::Places;
use orbit::time::Epoch;
use sandbox::app::Shell;
use sandbox::app::VIEWPORT_KEY;
use sandbox::bodies;
use sandbox::camera::MapCamera;
use sandbox::frame::{self, Output, View, shot};
use sandbox::frames::Frame;
use sandbox::labels::{self, Target, mps};
use sandbox::panel::{self, Tone};
use sandbox::paths::{SHIP_SETS, VIEW_POSE};
use sandbox::pick::Pick;
use sandbox::plan::{Loaded, Ship};
use sim::ships::Breach;

/// From the NRHO to low lunar orbit: designed in a fraction of a second.
const QUICK: &str = r#"
name = "Quick"

[[ship]]
name = "Lander"
start = { at = "nrho", epoch = "2045-03-05" }

[[ship.leg]]
to = "llo"
days = [0.5]
"#;

fn designed() -> Loaded {
    designed_from(QUICK)
}

fn designed_from(src: &str) -> Loaded {
    let eph = Ephemeris::standard();
    let mut plan = Loaded::start(Plan::parse(src, eph, Places::standard()).unwrap()).unwrap();
    plan.wait();
    assert!(plan.done());
    plan
}

/// How far `p` is from `r`, px.
fn from_rect(p: Vec2, r: damascene_core::Rect) -> f32 {
    let dx = (r.x - p.x).max(p.x - (r.x + r.w)).max(0.0);
    let dy = (r.y - p.y).max(p.y - (r.y + r.h)).max(0.0);
    dx.hypot(dy)
}

/// Focused on Earth from 600,000 km, at 1 and 2 physical pixels to a
/// logical one: the Moon's anchor is where the Moon is drawn (its light's
/// centroid, within its phase's lean), its label stands its radius and 3 px off it, and the label
/// is drawn where it was placed (its backing darkens the scene there).
#[test]
fn labels_beside_what_they_name() {
    let gpu = render::offscreen::Offscreen::new().expect("a GPU");
    let eph = Ephemeris::standard();
    let t = Epoch::from_ymd(2045, 3, 1);
    let moon = eph.lookup("moon").unwrap();
    for scale in [1.0f32, 2.0] {
        let mut app = Shell::new(t);
        app.camera.set_altitude(eph, 600_000.0);
        app.update_paths();
        let mut scene = app.scene(scale);
        scene.sky = None;
        scene.background = [0.0, 0.5, 0.0];
        scene.look = render::Look::default();
        scene.paths.clear();
        let (w, h) = ((1000.0 * scale) as u32, (700.0 * scale) as u32);
        let s = shot(&gpu, &mut app, Some(scene), w, h, scale);
        let map = s.viewport.expect("the map view is laid out");
        let anchor = app
            .anchors()
            .iter()
            .find(|a| a.target == Target::Body(moon))
            .expect("the Moon is drawn")
            .clone();
        // In physical pixels: the Moon's light over the green background,
        // in its red channel; and the backing's green.
        let channel = |x: f32, y: f32, c: usize| {
            f64::from(s.rgba[4 * (y as usize * w as usize + x as usize) + c])
        };
        let (cx, cy) = ((map.x + anchor.at.x) * scale, (map.y + anchor.at.y) * scale);
        let (mut sum, mut moment) = (0.0, glam::DVec2::ZERO);
        let reach = (3.0 * scale) as i32;
        for dy in -reach..=reach {
            for dx in -reach..=reach {
                let (x, y) = ((cx as i32 + dx) as f32, (cy as i32 + dy) as f32);
                sum += channel(x, y, 0);
                moment +=
                    channel(x, y, 0) * glam::DVec2::new(f64::from(x) + 0.5, f64::from(y) + 0.5);
            }
        }
        let drawn = (moment / sum) / f64::from(scale)
            - glam::DVec2::new(f64::from(map.x), f64::from(map.y));
        let label = app
            .placed()
            .iter()
            .find(|p| p.target == Target::Body(moon))
            .expect("the Moon is labeled")
            .clone();
        let off = from_rect(anchor.at, label.rect);
        let r = label.rect;
        let y = (map.y + r.y + r.h / 2.0) * scale;
        // Beside the label on the side away from the Moon.
        let beside = if r.x + r.w / 2.0 < anchor.at.x {
            r.x - 3.0
        } else {
            r.x + r.w + 3.0
        };
        let (inside, outside) = (
            channel((map.x + r.x + 1.5) * scale, y, 1),
            channel((map.x + beside) * scale, y, 1),
        );
        println!(
            "scale {scale}: Moon anchored at {:?}, drawn at {drawn:?} ({:.3} px apart), radius {:.2} px; \
             label {:?}, {off:.2} px off; backing {inside}, beside it {outside}",
            anchor.at,
            (anchor.at.as_dvec2() - drawn).length(),
            anchor.radius,
            label.rect
        );
        // A disk's light leans toward its lit side, by up to 4r / 3π
        // (half lit); at 2 px to a logical one, the Moon is a disk.
        let phase = 4.0 * f64::from(anchor.radius) / (3.0 * std::f64::consts::PI);
        assert!(sum > 100.0 && (anchor.at.as_dvec2() - drawn).length() < phase + 0.05);
        assert!((off - (anchor.radius + 3.0)).abs() < 1.0);
        assert!(outside > 150.0 && inside < 0.75 * outside);
    }
}

/// No body is anchored behind another, behind the eye, or off the view.
#[test]
fn no_labels_for_the_hidden() {
    let eph = Ephemeris::standard();
    let t = Epoch::from_ymd(2045, 3, 1);
    let (earth, moon) = (eph.lookup("earth").unwrap(), eph.lookup("moon").unwrap());
    let (e, m) = (
        eph.barycentric_position(earth, t),
        eph.barycentric_position(moon, t),
    );
    let out = (e - m).normalize();
    let eye = e + out * 50_000.0;
    let side = out.cross(DVec3::Z).normalize();
    let anchored = |target: DVec3| {
        let view = labels::View {
            camera: render::Camera::looking_at(eye, target, DVec3::Z, 50f64.to_radians()),
            size: (800, 600),
            scale: 1.0,
        };
        let (_, anchors) = labels::bodies(eph, t, &bodies::drawn(eph), earth, &view);
        let has = |b| anchors.iter().any(|a| a.target == Target::Body(b));
        (has(earth), has(moon))
    };
    assert_eq!(anchored(e), (true, false), "the Moon behind Earth");
    assert_eq!(anchored(eye + out), (false, false), "behind the eye");
    assert_eq!(anchored(eye + side), (false, false), "off the view");
}

/// A ship at the Moon's center (resting there) is anchored where the Moon
/// is drawn as a point, which hides nothing, and hidden where it is drawn
/// as a disk: as the renderer draws its marker (once it was hidden from
/// afar too: found in review).
#[test]
fn a_ship_at_a_point_is_shown() {
    let eph = Ephemeris::standard();
    let t = Epoch::from_ymd(2045, 3, 1);
    let (earth, moon) = (eph.lookup("earth").unwrap(), eph.lookup("moon").unwrap());
    let m = eph.barycentric_position(moon, t);
    let shown = |distance: f64| {
        let eye = m + DVec3::new(0.6, 0.0, 0.8) * distance;
        let view = labels::View {
            camera: render::Camera::looking_at(eye, m, DVec3::Z, 50f64.to_radians()),
            size: (800, 600),
            scale: 1.0,
        };
        let (balls, anchors) = labels::bodies(eph, t, &bodies::drawn(eph), earth, &view);
        let radius = anchors
            .iter()
            .find(|a| a.target == Target::Body(moon))
            .unwrap()
            .radius;
        (
            radius,
            labels::ship(&view, &balls, 0, "Lander", m, 9.0).is_some(),
        )
    };
    let (far, near) = (shown(3e6), shown(2e4));
    println!(
        "from 3e6 km the Moon is {:.2} px, from 2e4 km {:.0} px",
        far.0, near.0
    );
    assert!(far.0 < 1.0 && far.1, "drawn as a point");
    assert!(near.0 > 1.0 && !near.1, "drawn as a disk");
}

/// With a ship on its way to low lunar orbit, clicks pick: the Moon at its
/// center; the ship beside its marker; a marker's item at its label; the
/// leg on its line; nothing on empty space. Picking the ship selects the item it is in,
/// and empty space clears the selection. With labels hidden, none is
/// placed, and the rest picks as before.
#[test]
fn clicks_pick_what_they_land_on() {
    let gpu = render::offscreen::Offscreen::new().expect("a GPU");
    // The anchors are compared between frames: a shape read between them
    // would widen its body's (`bodies::extent`).
    sandbox::maps::wait().expect("data/bodies reads");
    let eph = Ephemeris::standard();
    let moon = eph.lookup("moon").unwrap();
    let mut app = Shell::new(Epoch::from_ymd(2045, 3, 5));
    app.plan = Some(designed());
    app.frame = Frame::offered(eph)[1].2;
    app.camera = MapCamera::new(eph, moon);
    app.camera.set_altitude(eph, 40_000.0);
    app.update_paths();
    let ship = app.plan.as_ref().unwrap().ships[0].clone();
    let k = ship
        .items
        .iter()
        .position(|i| i.kind == ItemKind::Leg)
        .unwrap();
    let leg = &ship.items[k];
    let t = Epoch(0.25 * leg.start.0 + 0.75 * leg.end.0);
    app.clock.seek(t);
    shot(&gpu, &mut app, None, 1000, 700, 1.0);
    let scene = app.scene(1.0);
    let anchors = app.anchors().to_vec();
    let at = |target: Target| anchors.iter().find(|a| a.target == target).map(|a| a.at);

    let center = at(Target::Body(moon)).expect("the Moon is drawn");
    assert_eq!(app.pick(center), Some(Pick::Body(moon)));

    let marker = at(Target::Ship(0)).expect("the ship is drawn");
    assert_eq!(app.pick(marker + Vec2::new(3.0, 0.0)), Some(Pick::Ship(0)));

    let label = app
        .placed()
        .iter()
        .find(|p| matches!(p.target, Target::Marker { .. }))
        .expect("a marker is labeled")
        .clone();
    let Target::Marker { item, .. } = label.target else {
        unreachable!()
    };
    let r = label.rect;
    assert_eq!(
        app.pick(Vec2::new(r.x + r.w / 2.0, r.y + r.h / 2.0)),
        Some(Pick::Item(0, item))
    );

    // A point on the leg's line on the view, clear of anchors and labels.
    let line = scene.paths[SHIP_SETS].paths.last().unwrap();
    let view = app.view().unwrap();
    let on_line = (5..70)
        .filter_map(|i| {
            let f = f64::from(i) / 100.0;
            let at = Epoch((1.0 - f) * leg.start.0 + f * leg.end.0);
            let p = scene.poses[VIEW_POSE].to_world(line.at(at.seconds())?);
            Some(view.project(p)?.as_vec2() + Vec2::new(0.0, 2.5))
        })
        .find(|&p| {
            let e = view.extent();
            p.x > 0.0
                && p.y > 0.0
                && p.x < e.x
                && p.y < e.y
                && anchors.iter().all(|a| a.at.distance(p) > 10.0)
                && app.placed().iter().all(|l| {
                    let r = l.rect;
                    p.x < r.x || p.y < r.y || p.x > r.x + r.w || p.y > r.y + r.h
                })
        })
        .expect("the leg is on the view");
    assert_eq!(app.pick(on_line), Some(Pick::Item(0, k)));

    let empty = Vec2::new(app.view().unwrap().extent().x - 10.0, 5.0);
    println!("the Moon at {center}, the ship at {marker}, a label at {r:?}, the line at {on_line}");
    assert!(anchors.iter().all(|a| a.at.distance(empty) > 30.0));
    assert_eq!(app.pick(empty), None);

    app.clicked_map(marker);
    assert_eq!(app.selected, Some((0, panel::current(&ship, t).unwrap())));
    app.clicked_map(empty);
    assert_eq!(app.selected, None);

    // Labels hidden: none placed, and what is drawn still picks.
    app.settings.labels = false;
    shot(&gpu, &mut app, None, 1000, 700, 1.0);
    assert!(app.placed().is_empty());
    assert_eq!(app.anchors(), &anchors[..]);
    assert_eq!(app.pick(center), Some(Pick::Body(moon)));
    assert_eq!(app.pick(marker + Vec2::new(3.0, 0.0)), Some(Pick::Ship(0)));
    assert_eq!(app.pick(on_line), Some(Pick::Item(0, k)));
}

/// From 6,000 km over the Moon, with a ship then waiting in low lunar
/// orbit: a click on that stay's line where it passes in front of the
/// Moon's disk picks the stay (once it picked the Moon: found in review);
/// a click on the disk away from lines and markers, the Moon. Seen from
/// the first of a few directions where the line crosses the disk.
#[test]
fn a_line_over_a_disk_is_picked() {
    let gpu = render::offscreen::Offscreen::new().expect("a GPU");
    let eph = Ephemeris::standard();
    let moon = eph.lookup("moon").unwrap();
    let mut app = Shell::new(Epoch::from_ymd(2045, 3, 5));
    let back = "[[ship.leg]]\nwait = 0.1\n\n[[ship.leg]]\nto = \"nrho\"\ndays = [0.5]\n";
    app.plan = Some(designed_from(&format!("{QUICK}\n{back}")));
    app.frame = Frame::offered(eph)[1].2;
    app.camera = MapCamera::new(eph, moon);
    app.camera.set_altitude(eph, 6_000.0);
    let k = 2;
    let stay = app.plan.as_ref().unwrap().ships[0].items[k].clone();
    assert_eq!(stay.kind, ItemKind::Stay);
    let t = Epoch(0.5 * stay.start.0 + 0.5 * stay.end.0);
    app.clock.seek(t);
    for (azimuth, elevation) in [
        (0.0, 0.3),
        (1.6, 0.3),
        (3.1, 0.3),
        (4.7, 0.3),
        (0.0, 1.2),
        (0.0, -1.2),
    ] {
        app.camera.set_direction(azimuth, elevation);
        app.update_paths();
        shot(&gpu, &mut app, None, 1000, 700, 1.0);
        let scene = app.scene(1.0);
        let view = app.view().unwrap();
        let anchors = app.anchors().to_vec();
        let (balls, _) = labels::bodies(eph, t, &bodies::drawn(eph), moon, &view);
        let disk = anchors
            .iter()
            .find(|a| a.target == Target::Body(moon))
            .unwrap()
            .clone();
        // The stay's line: the one in the view's frame within it, about
        // the clock (built from samples made there).
        let line = scene.paths[SHIP_SETS]
            .paths
            .iter()
            .find(|p| {
                let (first, last) = (p.samples[0].t, p.samples.last().unwrap().t);
                p.pose == VIEW_POSE
                    && first >= stay.start.seconds() - 1.0
                    && last <= stay.end.seconds() + 1.0
                    && (first..=last).contains(&t.seconds())
            })
            .expect("the stay is drawn");
        let clear = |q: Vec2| {
            anchors
                .iter()
                .all(|a| a.target == disk.target || a.at.distance(q) > 10.0)
                && app.placed().iter().all(|l| {
                    let r = l.rect;
                    q.x < r.x - 5.0
                        || q.y < r.y - 5.0
                        || q.x > r.x + r.w + 5.0
                        || q.y > r.y + r.h + 5.0
                })
        };
        // What is drawn of it: a revolution about the clock.
        let drawn = line.drawn(t.seconds());
        let (from, to) = (line.samples[drawn.start].t, line.samples[drawn.end].t);
        assert!(
            to - from < 3.0 * 3_600.0,
            "{} h drawn",
            (to - from) / 3_600.0
        );
        let on_line: Vec<Vec2> = (0..=2000)
            .filter_map(|i| {
                let at = from + (to - from) * f64::from(i) / 2000.0;
                let p = scene.poses[VIEW_POSE].to_world(line.at(at)?);
                let q = view.project(p)?.as_vec2();
                (!labels::hidden(view.camera.eye, p, &balls, None)).then_some(q)
            })
            .collect();
        let Some(over_disk) = on_line
            .iter()
            .copied()
            .find(|&q| q.distance(disk.at) < disk.radius - 10.0 && clear(q))
        else {
            continue;
        };
        println!(
            "from azimuth {azimuth}, elevation {elevation}: the Moon at {} ({:.0} px), the stay over it at {over_disk}",
            disk.at, disk.radius
        );
        assert_eq!(app.pick(over_disk), Some(Pick::Item(0, k)));
        // On the disk, away from every line drawn there.
        let away = (0..400)
            .map(|i| {
                let (a, r) = (
                    i as f32 * 2.399,
                    (i as f32 / 400.0).sqrt() * (disk.radius - 10.0),
                );
                disk.at + Vec2::new(a.cos(), a.sin()) * r
            })
            .find(|&q| clear(q) && on_line.iter().all(|l| l.distance(q) > 30.0))
            .expect("a clear spot on the disk");
        assert_eq!(app.pick(away), Some(Pick::Body(moon)));
        return;
    }
    panic!("the stay crosses the Moon's disk from none of the directions");
}

fn item(kind: ItemKind, dv: f64) -> Item {
    Item {
        step: 0,
        kind,
        label: "leo → nrho".into(),
        start: Epoch::from_ymd(2045, 3, 1),
        end: Epoch::from_ymd(2045, 3, 6),
        dv,
        track: None,
        markers: Vec::new(),
        options: Vec::new(),
        window: None,
        seconds: 0.0,
        note: None,
    }
}

/// A designed plan's rows carry each item's Δv, start and duration; a
/// ship refused, and one whose leg failed, say why.
#[test]
fn panel_rows_carry_dv_and_failures() {
    let eph = Ephemeris::standard();
    let plan = designed();
    let items = &plan.ships[0].items;
    let rows = panel::rows(eph, &plan.ships, items[0].start, Some((0, 1)));
    assert_eq!(rows.len(), 1 + items.len());
    assert_eq!(rows[0].label, "Lander");
    for (row, item) in rows[1..].iter().zip(items) {
        println!("{} {} {} {:?}", row.kind, row.label, row.dv, row.lines);
        if item.dv >= 0.5e-3 {
            assert_eq!(row.dv, mps(item.dv));
        }
        assert!(row.lines[0].0.starts_with(&when(item.start)));
    }
    assert!(rows[1].current && !rows[2].current);
    assert!(rows[2].selected && !rows[1].selected);

    let why = "no design converged in the window";
    let ships = [
        Ship {
            id: 1,
            name: "Refused".into(),
            items: Vec::new(),
            done: true,
            failure: Some(why.into()),
            first_leg: 0,
            revision: 0,
            tanks: None,
            warnings: Vec::new(),
        },
        Ship {
            id: 2,
            name: "Failed".into(),
            items: vec![
                item(ItemKind::Leg, 3.1),
                item(ItemKind::Failed(why.into()), 0.0),
            ],
            done: true,
            failure: Some(why.into()),
            first_leg: 0,
            revision: 0,
            tanks: None,
            warnings: Vec::new(),
        },
    ];
    let panicked = "designing it panicked: no message";
    let ships = [
        ships[0].clone(),
        ships[1].clone(),
        // Refused after its first items (its design panicked): no failed
        // item says why, so its row does (once nothing did: found in
        // review).
        Ship {
            id: 3,
            name: "Panicked".into(),
            items: vec![item(ItemKind::Stay, 0.0), item(ItemKind::Leg, 3.1)],
            done: true,
            failure: Some(panicked.into()),
            first_leg: 0,
            revision: 0,
            tanks: None,
            warnings: Vec::new(),
        },
    ];
    let rows = panel::rows(eph, &ships, Epoch::from_ymd(2045, 3, 2), None);
    let failure = (why.to_string(), Tone::Failure);
    assert!(rows[0].lines.contains(&failure));
    assert!(!rows[1].lines.contains(&failure), "said by its failed item");
    assert!(
        rows[4]
            .lines
            .contains(&(panicked.to_string(), Tone::Failure))
    );
    assert_eq!(rows[2].dv, "3,100 m/s");
    assert_eq!(rows[3].kind, "failed");
    assert!(rows[3].lines.contains(&failure));
}

/// What a ship's class does not allow (M1b.md, step 2 plan, part 2): a
/// warning on the step's first item past its stay (here a landing, which
/// a step to a surface designs in place of a leg: found in a screenshot);
/// on the ship's row, its start's and those of steps not designed yet.
#[test]
fn panel_rows_carry_class_warnings() {
    let breach = |step, why: &str| Breach {
        step,
        why: why.into(),
    };
    let (mut stay, mut landing) = (item(ItemKind::Stay, 0.0), item(ItemKind::Landing, 2.1));
    (stay.step, landing.step) = (1, 1);
    let ship = Ship {
        id: 1,
        name: "Tug".into(),
        items: vec![stay, landing],
        done: false,
        failure: None,
        first_leg: 0,
        revision: 0,
        tanks: None,
        warnings: vec![
            breach(None, "a tug-h's hold takes 50 t, not 60"),
            breach(Some(1), "a tug-h can't land at moon-south-pole"),
            breach(Some(2), "a tug-h can't fly between planets"),
        ],
    };
    let rows = panel::rows(
        Ephemeris::standard(),
        &[ship],
        Epoch::from_ymd(2045, 3, 2),
        None,
    );
    let warned = |r: usize| -> Vec<&str> {
        rows[r]
            .lines
            .iter()
            .filter(|(_, tone)| *tone == Tone::Warning)
            .map(|(line, _)| line.as_str())
            .collect()
    };
    assert_eq!(
        warned(0),
        [
            "a tug-h's hold takes 50 t, not 60",
            "leg 3: a tug-h can't fly between planets"
        ]
    );
    assert!(warned(1).is_empty());
    assert_eq!(warned(2), ["a tug-h can't land at moon-south-pole"]);
}

/// Clicks through damascene, as the window delivers them, at 1 and 2
/// physical pixels to a logical one: on a marker's label, the map view
/// gets the click (labels do not take it) and its item is selected; on
/// the leg's line, the leg is; on an item's row in the panel, the clock
/// seeks to the item's start and selects it.
#[test]
fn clicks_reach_the_map_and_the_panel() {
    let gpu = render::offscreen::Offscreen::new().expect("a GPU");
    let (device, queue) = (&gpu.device, &gpu.queue);
    let eph = Ephemeris::standard();
    let moon = eph.lookup("moon").unwrap();
    let plan = designed();
    for scale in [1.0f32, 2.0] {
        let mut app = Shell::new(Epoch::from_ymd(2045, 3, 5));
        app.plan = Some(designed());
        app.frame = Frame::offered(eph)[1].2;
        app.camera = MapCamera::new(eph, moon);
        app.camera.set_altitude(eph, 40_000.0);
        let k = 1;
        let leg = plan.ships[0].items[k].clone();
        app.clock.seek(Epoch(0.25 * leg.start.0 + 0.75 * leg.end.0));
        app.update_paths();

        let (w, h) = (1000.0, 700.0);
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
            frame::draw(
                device, queue, ui, app, &mut view, &scene, out, viewport, scale, None,
            );
            scene
        };
        let mut scene = draw(&mut app, &mut ui);
        for _ in 0..2 {
            scene = draw(&mut app, &mut ui);
        }
        let click = |app: &mut Shell, ui: &mut Runner, (x, y): (f32, f32)| {
            let mut events = ui.pointer_down(Pointer::mouse(x, y, PointerButton::Primary));
            events.extend(ui.pointer_up(Pointer::mouse(x, y, PointerButton::Primary)));
            let routes: Vec<_> = events
                .iter()
                .map(|e| (e.kind, e.route().map(str::to_string)))
                .collect();
            let cx = EventCx::new().with_ui_state(ui.ui_state());
            for e in events {
                app.on_event(e, &cx);
            }
            routes
        };

        let map = ui.rect_of_key(VIEWPORT_KEY).unwrap();
        let label = app
            .placed()
            .iter()
            .find(|p| matches!(p.target, Target::Marker { .. }))
            .expect("a marker is labeled")
            .clone();
        let Target::Marker { item, .. } = label.target else {
            unreachable!()
        };
        let r = label.rect;
        let routes = click(
            &mut app,
            &mut ui,
            (map.x + r.x + r.w / 2.0, map.y + r.y + r.h / 2.0),
        );
        println!("scale {scale}, on the label: {routes:?}");
        assert!(routes.contains(&(UiEventKind::Click, Some(VIEWPORT_KEY.to_string()))));
        assert_eq!(app.selected, Some((0, item)));

        // On the leg's line, clear of anchors and labels.
        let line = scene.paths[SHIP_SETS].paths.last().unwrap();
        let v = app.view().unwrap();
        let anchors = app.anchors().to_vec();
        let on_line = (5..70)
            .filter_map(|i| {
                let f = f64::from(i) / 100.0;
                let at = Epoch((1.0 - f) * leg.start.0 + f * leg.end.0);
                let p = scene.poses[VIEW_POSE].to_world(line.at(at.seconds())?);
                Some(v.project(p)?.as_vec2() + Vec2::new(0.0, 2.5))
            })
            .find(|&p| {
                let e = v.extent();
                p.x > 0.0
                    && p.y > 0.0
                    && p.x < e.x
                    && p.y < e.y
                    && anchors.iter().all(|a| a.at.distance(p) > 10.0)
                    && app.placed().iter().all(|l| {
                        let r = l.rect;
                        p.x < r.x || p.y < r.y || p.x > r.x + r.w || p.y > r.y + r.h
                    })
            })
            .expect("the leg is on the view");
        click(&mut app, &mut ui, (map.x + on_line.x, map.y + on_line.y));
        assert_eq!(app.selected, Some((0, k)));

        draw(&mut app, &mut ui);
        // Down the panel's list to the first item's row (damascene's
        // `rect_of_key` does not index keyed nodes in a virtual list's
        // rows).
        let list = ui
            .rect_of_key(panel::LIST_KEY)
            .expect("the panel is laid out");
        let key = panel::item_key(0, 0);
        let x = list.x + list.w / 2.0;
        let y = (0..100)
            .map(|i| list.y + i as f32 * 4.0)
            .find(|&y| {
                let down = ui.pointer_down(Pointer::mouse(x, y, PointerButton::Primary));
                ui.pointer_up(Pointer::mouse(x, y, PointerButton::Primary));
                down.iter().any(|e| e.route() == Some(key.as_str()))
            })
            .expect("the first item's row is in the list");
        let routes = click(&mut app, &mut ui, (x, y + 4.0));
        println!("scale {scale}, on the row: {routes:?}");
        assert_eq!(app.selected, Some((0, 0)));
        assert_eq!(app.clock.t, plan.ships[0].items[0].start);
    }
}
