//! The window (M1c.md, step 2 plan, items 2 and 4): a winit loop on
//! damascene-winit-wgpu's host layers, which bring the surface, the UI
//! renderer and HDR negotiation; input over the map view goes to the
//! camera, the rest to the UI.

use crate::app::{Shell, VIEWPORT_KEY};
use crate::frame::{self, Output, View};
use damascene_core::color::ColorPreferences;
use damascene_core::prelude::*;
use damascene_core::{Cursor, EventCx, HostDiagnostics, KeyModifiers};
use damascene_winit_wgpu::HostConfig;
use damascene_winit_wgpu::host::WindowGfx;
use damascene_winit_wgpu::host::input::{
    key_modifiers, map_key, map_physical, pointer_button, winit_cursor,
};
use std::sync::Arc;
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::{ElementState, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

/// Logical pixels a wheel line scrolls a UI list.
const WHEEL_LINE: f32 = 50.0;

/// Opens the window and runs until it closes, or for `frames` frames;
/// then prints what the window negotiated and its frame times.
pub fn run(app: Shell, frames: Option<u64>) -> Result<(), Box<dyn std::error::Error>> {
    let event_loop = EventLoop::new()?;
    let mut host = Host {
        app,
        config: HostConfig::default().with_color_preferences(ColorPreferences::hdr_extended()),
        instance: None,
        gfx: None,
        view: None,
        backend: "",
        pointer: None,
        modifiers: KeyModifiers::default(),
        cursor: Cursor::Default,
        pending_resize: None,
        last_frame: None,
        frame_index: 0,
        failed: None,
        frames,
        log: Log::default(),
    };
    event_loop.run_app(&mut host)?;
    if let Some(why) = host.failed {
        return Err(why.into());
    }
    print!("{}", host.log.summary());
    Ok(())
}

/// What the window negotiated, and its frames' times, for the summary.
#[derive(Default)]
struct Log {
    adapter: String,
    format: String,
    present: String,
    hdr: bool,
    output: Option<(f32, f32)>,
    /// Wall time between frames, and the GPU's per frame, ms.
    wall: Vec<f32>,
    gpu: Vec<f32>,
    /// Whether `gpu` times whole frames, or (without timestamps between
    /// passes) the scene's pass alone.
    whole_frame: bool,
}

impl Log {
    /// Frames before this are left out of the times: start-up.
    const SETTLE: usize = 10;

    fn summary(&self) -> String {
        let stats = |v: &[f32]| {
            let mut v: Vec<f32> = v.iter().skip(Self::SETTLE).copied().collect();
            if v.is_empty() {
                return "–".to_string();
            }
            v.sort_by(f32::total_cmp);
            let mean = v.iter().sum::<f32>() / v.len() as f32;
            let p95 = v[((v.len() - 1) as f32 * 0.95) as usize];
            format!("mean {mean:.2} ms, 95% {p95:.2} ms over {} frames", v.len())
        };
        let output = match self.output {
            Some((headroom, nits)) => {
                format!(", output headroom {headroom:.2}× over {nits:.0} nits")
            }
            None => String::new(),
        };
        format!(
            "{}\nsurface {} ({}), HDR {}{output}\nframe interval: {}\nGPU per {}: {}\n",
            self.adapter,
            self.format,
            self.present,
            if self.hdr { "active" } else { "not active" },
            stats(&self.wall),
            if self.whole_frame {
                "frame"
            } else {
                "scene pass"
            },
            stats(&self.gpu),
        )
    }
}

struct Host {
    app: Shell,
    config: HostConfig,
    instance: Option<wgpu::Instance>,
    /// Dropped before `gfx` on close, explicitly.
    view: Option<View>,
    gfx: Option<WindowGfx>,
    backend: &'static str,
    /// Logical pixels.
    pointer: Option<(f32, f32)>,
    modifiers: KeyModifiers,
    cursor: Cursor,
    pending_resize: Option<PhysicalSize<u32>>,
    last_frame: Option<Instant>,
    frame_index: u64,
    failed: Option<String>,
    /// Frames to draw before closing, if not until the window is closed.
    frames: Option<u64>,
    log: Log,
}

impl ApplicationHandler for Host {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gfx.is_some() {
            return;
        }
        match self.bring_up(event_loop) {
            Ok(()) => {}
            Err(why) => {
                self.failed = Some(why);
                event_loop.exit();
            }
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.frames.is_some_and(|n| self.frame_index >= n) {
            self.view = None;
            self.gfx = None;
            event_loop.exit();
            return;
        }
        // The compositor may have moved the window to another output, or
        // toggled HDR.
        if let Some(gfx) = self.gfx.as_mut()
            && let Some(plan) = gfx.color.poll()
        {
            gfx.apply_renegotiation(&plan);
            gfx.window.request_redraw();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if matches!(event, WindowEvent::CloseRequested) {
            self.view = None;
            self.gfx = None;
            event_loop.exit();
            return;
        }
        let Some(gfx) = self.gfx.as_mut() else {
            return;
        };
        let scale = gfx.window.scale_factor() as f32;
        let over_map = |gfx: &WindowGfx| gfx.renderer.ui_state().is_hovering_within(VIEWPORT_KEY);
        let app = &mut self.app;
        let dispatch = |app: &mut Shell, gfx: &WindowGfx, events: Vec<UiEvent>| {
            let cx = EventCx::new().with_ui_state(gfx.renderer.ui_state());
            for e in events {
                app.on_event(e, &cx);
            }
        };
        match event {
            WindowEvent::Resized(size) => {
                let current = PhysicalSize::new(gfx.config.width, gfx.config.height);
                if self.pending_resize.is_some() || size != current {
                    self.pending_resize = Some(size);
                    gfx.window.request_redraw();
                }
            }
            WindowEvent::Focused(false) => app.camera.cancel(),
            WindowEvent::CursorMoved { position, .. } => {
                let at = (position.x as f32 / scale, position.y as f32 / scale);
                self.pointer = Some(at);
                app.camera.moved(at);
                let moved = gfx.renderer.pointer_moved(Pointer::moving(at.0, at.1));
                dispatch(app, gfx, moved.events);
            }
            WindowEvent::CursorLeft { .. } => {
                self.pointer = None;
                let events = gfx.renderer.pointer_left();
                dispatch(app, gfx, events);
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let Some(button) = pointer_button(button) else {
                    return;
                };
                // A release ends a drag even where the pointer is unknown
                // (it left the window mid-drag).
                if state == ElementState::Released {
                    app.camera.release(button);
                }
                let Some((x, y)) = self.pointer else { return };
                match state {
                    ElementState::Pressed => {
                        // Only when the map is the hover target: a menu
                        // over it must not start a drag underneath.
                        if over_map(gfx) {
                            app.camera.press(button, (x, y));
                        }
                        let events = gfx.renderer.pointer_down(Pointer::mouse(x, y, button));
                        dispatch(app, gfx, events);
                    }
                    ElementState::Released => {
                        let events = gfx.renderer.pointer_up(Pointer::mouse(x, y, button));
                        dispatch(app, gfx, events);
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let Some((x, y)) = self.pointer else { return };
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / scale / WHEEL_LINE,
                };
                if over_map(gfx) {
                    let eph = app.eph;
                    app.camera.wheel(eph, lines);
                } else {
                    gfx.renderer.pointer_wheel(x, y, -lines * WHEEL_LINE);
                }
            }
            WindowEvent::ModifiersChanged(m) => {
                self.modifiers = key_modifiers(m.state());
                gfx.renderer.set_modifiers(self.modifiers);
            }
            WindowEvent::KeyboardInput {
                event:
                    key @ winit::event::KeyEvent {
                        state: ElementState::Pressed,
                        ..
                    },
                is_synthetic: false,
                ..
            } => {
                let (logical, physical) =
                    (map_key(&key.logical_key), map_physical(key.physical_key));
                if logical != LogicalKey::Unidentified || physical != PhysicalKey::Unidentified {
                    let events =
                        gfx.renderer
                            .key_down(logical, physical, self.modifiers, key.repeat);
                    dispatch(app, gfx, events);
                }
                if let Some(text) = &key.text
                    && let Some(e) = gfx.renderer.text_input(text.to_string())
                {
                    dispatch(app, gfx, vec![e]);
                }
            }
            WindowEvent::Ime(winit::event::Ime::Commit(text)) => {
                if let Some(e) = gfx.renderer.text_input(text) {
                    dispatch(app, gfx, vec![e]);
                }
            }
            WindowEvent::RedrawRequested => self.redraw(),
            _ => {}
        }
    }
}

impl Host {
    fn bring_up(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
        let attrs = Window::default_attributes()
            .with_title("Interplanetary sandbox")
            .with_inner_size(LogicalSize::new(1280.0, 800.0));
        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .map_err(|e| format!("window: {e}"))?,
        );
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| format!("surface: {e}"))?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))
        .map_err(|e| format!("no compatible GPU adapter: {e}"))?;
        let info = adapter.get_info();
        self.log.adapter = format!("{} ({:?}, {})", info.name, info.backend, info.driver_info);
        self.backend = match info.backend {
            wgpu::Backend::Vulkan => "Vulkan",
            wgpu::Backend::Metal => "Metal",
            wgpu::Backend::Dx12 => "DX12",
            wgpu::Backend::Gl => "GL",
            _ => "other",
        };
        let (device, queue) =
            render::request_device(&adapter, "sandbox").map_err(|e| format!("GPU device: {e}"))?;
        let mut gfx =
            WindowGfx::with_surface(&adapter, &device, &queue, window, surface, &self.config);
        gfx.renderer.set_theme(self.app.theme());
        self.log.format = format!("{:?}", gfx.config.format);
        self.log.present = format!("{:?}", gfx.config.present_mode);
        self.view = Some(View::new(&device, &queue));
        gfx.window.request_redraw();
        self.gfx = Some(gfx);
        self.instance = Some(instance);
        Ok(())
    }

    fn redraw(&mut self) {
        let (Some(gfx), Some(view)) = (self.gfx.as_mut(), self.view.as_mut()) else {
            return;
        };
        if let Some(size) = self.pending_resize.take() {
            gfx.resize(size.width, size.height);
        }
        let texture = match gfx.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t)
            | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            wgpu::CurrentSurfaceTexture::Outdated => {
                gfx.surface.configure(&gfx.device, &gfx.config);
                gfx.window.request_redraw();
                return;
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                if let Some(instance) = &self.instance
                    && gfx.recreate_surface(instance).is_err()
                {
                    gfx.surface.configure(&gfx.device, &gfx.config);
                }
                gfx.window.request_redraw();
                return;
            }
            // Minimized or hidden: the next event redraws (retrying
            // spins where the platform returns at once, as on Metal).
            wgpu::CurrentSurfaceTexture::Occluded => return,
            wgpu::CurrentSurfaceTexture::Timeout => {
                gfx.window.request_redraw();
                return;
            }
            other => {
                eprintln!("sandbox: no surface texture: {other:?}");
                return;
            }
        };
        let now = Instant::now();
        let dt = self.last_frame.map(|t| now - t).unwrap_or_default();
        self.last_frame = Some(now);
        self.frame_index += 1;
        self.app.stats.frame_ms = Some(dt.as_secs_f32() * 1e3);
        self.app.tick(dt.as_secs_f64());
        let scale = gfx.window.scale_factor() as f32;
        self.app.stats.output = gfx.color.output_luminance();
        let diagnostics = HostDiagnostics {
            backend: self.backend,
            surface_size: (gfx.config.width, gfx.config.height),
            scale_factor: scale,
            msaa_samples: gfx.msaa.as_ref().map_or(1, |m| m.sample_count),
            frame_index: self.frame_index,
            surface_reconfigures: gfx.reconfigures,
            last_frame_dt: dt,
            working_color_space: gfx.renderer.working_color_space(),
            color_management: gfx.color.status().clone(),
            surface_color: Some(gfx.surface_color.clone()),
            ..Default::default()
        };
        self.log.hdr = diagnostics.hdr_active();
        self.app.stats.hdr = self.log.hdr;
        let scene = self.app.scene(scale);
        self.log.output = self.app.stats.output;
        if self.frame_index > 1 {
            self.log.wall.push(dt.as_secs_f32() * 1e3);
        }
        let surface_view = texture.texture.create_view(&Default::default());
        let viewport = Rect::new(
            0.0,
            0.0,
            gfx.config.width as f32 / scale,
            gfx.config.height as f32 / scale,
        );
        let drawn = frame::draw(
            &gfx.device,
            &gfx.queue,
            &mut gfx.renderer,
            &mut self.app,
            view,
            &scene,
            Output {
                texture: &texture.texture,
                view: &surface_view,
                msaa: gfx.msaa.as_ref().map(|m| &m.view),
            },
            viewport,
            scale,
            Some(&diagnostics),
        );
        for t in drawn.timings {
            self.log.gpu.push(t.frame.unwrap_or(t.scene));
            self.log.whole_frame = t.frame.is_some();
        }
        gfx.queue.present(texture);
        let cursor = gfx.renderer.snapshot_cursor();
        if cursor != self.cursor {
            gfx.window.set_cursor(winit_cursor(cursor));
            self.cursor = cursor;
        }
        // Continuously, paced by the display: the status line shows live
        // timings, and playback (step 4) animates every frame.
        gfx.window.request_redraw();
    }
}
