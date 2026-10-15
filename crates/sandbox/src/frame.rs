//! One frame, the same in a window and offscreen (M1c.md, step 2 plan,
//! item 3): build and prepare the UI, size the scene's target to the map
//! view, render the scene, then the UI, which composites it.

use crate::app::{Shell, VIEWPORT_KEY};
use damascene_core::prelude::*;
use damascene_core::{HostDiagnostics, paint::rgba_f32_in};
use damascene_wgpu::Runner;

/// What the scene is drawn with.
pub struct View {
    pub renderer: render::Renderer,
    pub target: Option<render::Target>,
    pub timer: Option<render::GpuTimer>,
}

impl View {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> View {
        View {
            renderer: render::Renderer::new(device, queue),
            target: None,
            timer: render::GpuTimer::new(device, queue),
        }
    }
}

/// Where the frame goes: the resolved texture, its view, and the
/// multisampled view the UI draws into, if any.
pub struct Output<'a> {
    pub texture: &'a wgpu::Texture,
    pub view: &'a wgpu::TextureView,
    pub msaa: Option<&'a wgpu::TextureView>,
}

/// What drawing a frame reports.
pub struct Drawn {
    /// Another frame is needed: the UI animates, or the map view changed
    /// size and shows its new target from the next one.
    pub again: bool,
    /// GPU times of earlier frames that finished since the last call,
    /// oldest first.
    pub timings: Vec<render::Timings>,
}

/// Draws a frame of `app`, with `scene` in its map view, at `viewport`
/// (logical pixels) × `scale`.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    ui: &mut Runner,
    app: &mut Shell,
    view: &mut View,
    scene: &render::Scene,
    out: Output,
    viewport: Rect,
    scale: f32,
    diagnostics: Option<&HostDiagnostics>,
) -> Drawn {
    let timings = match view.timer.as_mut() {
        Some(timer) => timer.collect(device),
        None => Vec::new(),
    };
    if let Some(&t) = timings.last() {
        app.stats.gpu = Some(t);
    }
    app.stats.timestamps = view.timer.is_some();
    // On the target as it is: a map view that changed size shows its new
    // one from the next frame.
    app.place_labels(scene, view.target.as_ref().map(|t| t.size), scale);
    app.before_build();
    let theme = app.theme();
    let palette = theme.palette().clone();
    let tree = {
        let mut cx = BuildCx::new(&theme)
            .with_ui_state(ui.ui_state())
            .with_viewport(viewport.w, viewport.h);
        if let Some(d) = diagnostics {
            cx = cx.with_diagnostics(d);
        }
        app.build(&cx)
    };
    ui.set_theme(theme);
    ui.set_hotkeys(app.hotkeys());
    ui.set_selection(app.selection());
    ui.push_toasts(app.drain_toasts());
    ui.push_focus_requests(app.drain_focus_requests());
    ui.push_scroll_requests(app.drain_scroll_requests());
    let prepared = ui.prepare(device, queue, tree, viewport, scale);

    // The target follows the map view's size in pixels.
    let wanted = ui.rect_of_key(VIEWPORT_KEY).map(|r| {
        (
            ((r.w * scale).round() as u32).max(1),
            ((r.h * scale).round() as u32).max(1),
        )
    });
    let resized = wanted.is_some_and(|size| view.target.as_ref().is_none_or(|t| t.size != size));
    if resized && let Some((w, h)) = wanted {
        let target = render::Target::new(device, w, h);
        app.viewport = Some(damascene_wgpu::app_texture(target.color.clone()));
        view.target = Some(target);
    }

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("frame"),
    });
    // Measured only with a scene pass (none on the very first frame).
    let measuring = match (&view.target, view.timer.as_mut()) {
        (Some(_), Some(t)) => t.begin(&mut encoder),
        _ => None,
    };
    if let Some(target) = &view.target {
        let writes = view.timer.as_ref().and_then(|t| t.scene_pass(measuring));
        view.renderer
            .render(device, queue, &mut encoder, target, scene, writes);
    }
    if let Some(t) = &view.timer {
        t.after_scene(&mut encoder, measuring);
    }
    let [r, g, b, a] = rgba_f32_in(palette.background, ui.working_color_space());
    ui.render(
        device,
        &mut encoder,
        out.texture,
        out.view,
        out.msaa,
        wgpu::LoadOp::Clear(wgpu::Color {
            r: r.into(),
            g: g.into(),
            b: b.into(),
            a: a.into(),
        }),
    );
    if let Some(t) = &view.timer {
        t.end(&mut encoder, measuring);
    }
    queue.submit(Some(encoder.finish()));
    if let Some(t) = &view.timer {
        t.submitted(measuring);
    }
    Drawn {
        again: prepared.needs_redraw || resized,
        timings,
    }
}

/// The format of offscreen frames: what a PNG stores.
pub const OFFSCREEN_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// A whole frame drawn offscreen, after the UI settles (at most a few
/// frames): its RGBA8 pixels, the map view's rect in logical pixels, and
/// the app's final state.
pub struct Shot {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub viewport: Option<Rect>,
    /// The scene target's size, pixels.
    pub target: Option<(u32, u32)>,
}

/// Draws `app` offscreen at `width` × `height` pixels, `scale` of them
/// to a logical one, with `scene` in its map view (its own if `None`).
pub fn shot(
    gpu: &render::offscreen::Offscreen,
    app: &mut Shell,
    scene: Option<render::Scene>,
    width: u32,
    height: u32,
    scale: f32,
) -> Shot {
    let (device, queue) = (&gpu.device, &gpu.queue);
    let mut ui = Runner::new(device, queue, OFFSCREEN_FORMAT);
    // Animations at their ends: a screenshot never catches one midway.
    ui.set_animation_mode(damascene_core::AnimationMode::Settled);
    ui.set_theme(app.theme());
    ui.set_surface_size(width, height);
    let mut view = View::new(device, queue);
    let scene = scene.unwrap_or_else(|| {
        app.poll();
        app.update_paths();
        app.scene(scale)
    });
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("offscreen frame"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: OFFSCREEN_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let texture_view = texture.create_view(&Default::default());
    let viewport = Rect::new(0.0, 0.0, width as f32 / scale, height as f32 / scale);
    for _ in 0..8 {
        let out = Output {
            texture: &texture,
            view: &texture_view,
            msaa: None,
        };
        if !draw(
            device, queue, &mut ui, app, &mut view, &scene, out, viewport, scale, None,
        )
        .again
        {
            break;
        }
    }
    Shot {
        width,
        height,
        rgba: gpu.read(&texture),
        viewport: ui.rect_of_key(VIEWPORT_KEY),
        target: view.target.as_ref().map(|t| t.size),
    }
}
