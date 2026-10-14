# M1c — Creative sandbox and the map view

Status, 2026-10-13: planned. Starts when M1a closes (its steps 10 and
11), before M1b.

## Why

The map view is the game's center interface: the real solar system in
3D, at true scale, as realistic as we can afford, on HDR displays. The
model is KSP's map view rather than its flight view. It replaces the 2D
heliocentric map with a cislunar inset that DESIGN.md first planned
(decided 2026-10-13; DESIGN.md, UI). The 2D plan was chosen before the
n-body sandbox existed, when the map only had to show patched conics. In
the sandbox the interesting things are 3D: halo orbits, the
near-polar NRHO, lunar flybys, the planes of departures. And the map is
where the player spends the game.

M1c builds that view first, as a creative mode with no economy: load a
mission plan, let the autopilot design and fly it, and watch it travel.
This:

- stress-tests the renderer on real trajectories before the
  progression mechanics are built on top of it;
- builds "a mission is a chain of legs" for real, which M1b needs anyway;
- looks at the shapes of trajectories. Every check in M1a compares a total
  (Δv, time) with a published number; nothing has yet looked at a path.

## Scope

In:

- mission plans: a file format, and the engine pieces that plan and fly
  them;
- the 3D map view: playback with a time scrubber and time warp, reference
  frames, bodies, trajectories, labels, picking;
- panels: the plan's legs, their Δv and durations, failures and why;
- editing a leg's window or duration and re-planning it in place.

Out:

- flying low over terrain (KSP's flight view);
- ship models (later, as flavor; ships are markers);
- mass and propellant (M1b);
- the economy;
- hand-placed burns. A burn editor is open (DESIGN.md, Open questions);
  creative mode is where it would go.

## Mission plans

A plan is a ship's start and a chain of legs:

```toml
name = "Gateway crew rotation"
start = { orbit = "leo", epoch = "2045-03-01" }

[[leg]]
to = { orbit = "nrho" }         # a member of a family in data/orbits.toml
window = ["2045-03-01", "2045-03-31"]
durations = [5.0]               # days, or `longest` for a frontier

[[leg]]
wait = 30                       # days in the NRHO, kept by station-keeping

[[leg]]
to = { surface = "moon/south-pole" }
```

The format above is a sketch; step 1 settles it. Plans live in
`data/missions/`.

Missing from the engine today:

- **One entry point for every leg kind.** `legs::plan` designs legs
  within a planetary system. Legs between systems are designed only by a
  helper inside `tests/interplanetary.rs`, which tries a few hyperbola
  planes; that moves into the library behind the same entry point.
- **Chaining.** Each leg starts where and when the previous one ended. A
  wait coasts in the station orbit with station-keeping
  (`stationkeeping`).
- **Paths for drawing.** A designed path keeps only its nodes' states.
  The view needs samples spaced by the local dynamics: about a minute
  apart in LEO, a few a day in cruise. They come from re-integrating with
  dense output, and are stored with velocities so that playback can
  interpolate between them.
- **Events that are not flown.** Landing, ascent and aerocapture are Δv
  rules (`surface`), so they draw as markers with a jump, not as paths.

Designing a leg takes 0.1–12 s, so plans are designed in the background,
and each leg appears when it is ready.

## The view

### Scale

Distances run from a 100 km lunar orbit to the outer planets, ~1e9 km.
Two standard techniques cover that in one pass, with no second camera
for distant objects (KSP's "scaled space"):

- **Camera-relative positions.** Positions stay 64-bit on the CPU, as the
  ephemeris and the paths already are. Each frame subtracts the camera's
  position before handing 32-bit offsets to the GPU. Near the camera this
  is exact. Far away, the rounding is ~6e-8 of the distance, ~1e-4 of a
  pixel.
- **Reversed depth with no far plane**, in a 32-bit float buffer. Depth
  precision is then roughly constant relative to distance (~1e-7), from a
  metre to the outer planets.

Trajectories are stored in chunks, each with its own 64-bit anchor and
32-bit offsets, so precision depends on a chunk's size, not on its
distance from the Sun.

### Frames

What a trajectory looks like depends on the frame, and the sandbox has no
single right one. A halo orbit is a closed loop only in the frame turning
with the Earth and Moon. In a fixed frame it spirals along the Moon's
path.

- **Fixed, centered on any body.** The default follows the camera's
  focus, as in KSP: focusing on the Moon draws paths relative to the
  Moon.
- **Turning with two bodies:** Earth–Moon, or Sun–planet. The origin is
  their barycenter, x points from the first body to the second, and z lies
  along their orbit's angular momentum. Optionally the frame is scaled by
  their distance, as in the CR3BP, where catalog orbits close.

Each sample of a path is drawn in the frame at that sample's own time.
"Moon-centered" means the Moon's position when the ship was there, not
now. So a path's vertices depend on the frame, and are recomputed on the
CPU when the frame changes.

### Realism

As realistic as we can afford, in stages, while the map stays readable.

**Light.**
- The Sun is the only light. Radiometry is physical, so Mars is lit at
  43% of Earth's sunlight and Jupiter at 4%.
- Exposure adapts to what is in focus.
- The scene is rendered in linear HDR. Exposure, bloom and a tonemap fit
  it to the output's headroom: up to the panel's peak on an HDR output (a
  16-bit float scRGB swapchain), and a filmic curve to 1.0 on SDR.
- One deliberate departure from realism: stars get their own fixed
  exposure. A real camera exposed for the sunlit Earth sees none.

**Bodies.**
- Spheroids with the right flattening: Earth 1/298, Mars 1/170, Jupiter
  1/15, Saturn 1/10.
- Each is oriented by IAU rotation models: the pole and prime meridian
  from Archinal et al. 2018. `ephem` has poles today but no prime
  meridians.
- Each carries a surface texture and relief shading from elevation data
  (LOLA for the Moon, MOLA for Mars, ETOPO for Earth).
- Bodies smaller than a pixel are drawn as points with their true
  brightness, from distance, phase and albedo.

**Earth and Mars.**
- Atmospheres by precomputed scattering (Bruneton 2017 or Hillaire
  2020), seen from space: the limb glow and the color at the terminator.
- On Earth, clouds and night lights.

**Shadows and small bodies.**
- Eclipses: analytic sphere shadows, such as Earth's on the Moon and
  Phobos's on Mars.
- Saturn's rings, with their shadow.
- Shape models for Phobos, Deimos, Bennu and Ryugu.

**The Sun and the sky.**
- The Sun is a limb-darkened disc with bloom.
- Stars come from a catalog (Hipparcos, or a bright subset of Gaia), with
  positions, magnitudes and colors from B−V, and a Milky Way map behind
  them.

**What is drawn on the map.**
- Paths: antialiased lines colored by leg, with past and future
  distinguished.
- Station orbits as reference loops.
- Burns and flyby periapses as markers carrying their Δv and altitude.

Textures, elevation, star catalogs and shape models come from NASA, USGS,
ESA and PDS. Each source's license is checked when it is imported and
recorded in `data/`, as with the economy's sources.

### Engine

- **Graphics and shaders:** wgpu 30, with WGSL shaders loaded by
  `include_str!`.
  - wgpu is where every reusable piece below lives, and it is damascene's
    main backend.
  - It also opens the scRGB swapchain on Mesa (damascene's
    COLOR_MANAGEMENT.md), so HDR output needs no other API.
  - We give up ray tracing, mesh shaders and bindless resources, which
    this scene does not need.
  - ash is the fallback if wgpu blocks something. vulkano is not used:
    polychora's experience shows it restricts SPIR-V and leaves draws
    `unsafe` anyway.
- **UI:** damascene 0.8 for panels, tables, plots and text, in our own
  host on winit.
  - Damascene's stock runner (`damascene-winit-wgpu`) always clears the
    frame and offers no hook for our passes.
  - The scene renders into an `Rgba16Float` texture shown by a damascene
    `surface()` widget, so panels, menus and popovers layer over the map
    correctly. Damascene encodes the output.
  - Damascene passes only left-button drags to a widget, so raw input
    goes to the camera whenever the pointer is over the viewport
    (volumetric's pattern).
- **Labels:** damascene text nodes, rebuilt each frame at projected
  positions, decluttered by polychora's pairwise relaxation. Hundreds of
  per-frame labels in damascene have never been measured. Step 5
  measures first, and falls back to Hushfold's distance-field text if
  damascene is too slow.
- **Crates:**
  - `crates/render`, the renderer. It takes a scene described in 64-bit
    positions, and knows nothing of orbits or missions.
  - `crates/sandbox`, the app: host, UI, plans, playback.
  - `crates/orbit` gains the mission planner.

**Reuse.** Surveyed 2026-10-13. We take:

| From | Piece | Changes |
|---|---|---|
| volumetric | Host glue: one device, the scene in a texture under a damascene widget, input routing (~300 lines, `volumetric_ui_v2/src/host.rs`, `session.rs`) | 16-bit float target; drop the file, settings and job plumbing |
| volumetric | Line pipeline (~600, `volumetric_renderer/src/pipelines/line.rs`) | Clip at the near plane (it doesn't: segments passing behind the camera draw wrong); camera-relative positions; joins |
| volumetric | Offscreen render and pixel-test harness (~1,700, `offscreen.rs`, `frame_tests.rs`) | New test scenes |
| volumetric | Orbit camera with animated refocus (~1,460, `camera.rs`, `navigation.rs`) | 64-bit eye and target; follow a moving body; zoom from 100 km to 1e10 km; reversed depth |
| volumetric | Points and GPU picking (~730) | Make points pickable |
| Hushfold | HDR, bloom and tonemap chain (~330, `gfx.rs`, `post.wgsl`) | Drop the warm grade and vignette; raise the clamp at 60 so the Sun's energy survives; tonemap to the output's headroom |
| Hushfold | Distance-field text (~770, `text.rs`) | Labels' fallback; screen-aligned at a fixed pixel size |
| Hushfold | Growable instance buffers, mesh builders, GPU timestamps, screenshot and recording | UVs on vertices |
| polychora | Label declutter (~300, `overlay.rs`) | The algorithm only |

Skipped: volumetric's deferred renderer and SSAO (an 8-bit CAD look),
Hushfold's procedural sky, polychora's 4D renderers and its vulkano glue.

## Gate

1. Every leg M1a checked against a published value (steps 6–8) exists as
   a mission file, flies, and is drawn. The halos, the NRHO and the DRO
   close in the Earth–Moon frame.
2. Zooming from 5 au to 100 km above the Moon shows no jitter. An offscreen
   test checks that a ship in low lunar orbit is drawn within a pixel of
   its position projected in 64 bits.
3. A plan with a ship on every Mars window from 2040 to 2075 plays at a
   month per second at the display's refresh rate on the development
   machine (Radeon RX 6800 class), measured by GPU timestamps.
4. On an HDR output the Sun and sunlit limbs use the headroom. On SDR the
   same scene tonemaps without clipping bands.
5. Re-planning an edited leg meets M1a's times (≤ 0.1 s re-planning,
   ≤ 10 s from the playbook) without stalling the view.

## Steps

1. Mission plans in `crates/orbit`:
   - the format;
   - one entry point for every leg kind;
   - chaining and waits;
   - paths sampled for drawing;
   - `orbit mission FILE` printing each leg's Δv and duration.
2. Host and an empty scene:
   - winit, wgpu and damascene 0.8;
   - the scene texture in a surface widget;
   - input routing;
   - the offscreen test harness and GPU timers.
3. Scale:
   - camera-relative positions and reversed depth;
   - the orbit camera following a body across the full zoom range;
   - lit spheres, the Sun, points for bodies under a pixel.
4. Paths:
   - chunked lines for trajectories and station orbits;
   - frames;
   - the playback clock and time warp;
   - markers for burns, passes and events.
5. Panels and labels:
   - measure damascene's label cost first;
   - the plan's legs, Δv and failures;
   - labels with declutter;
   - picking.
6. Realism:
   - rotation models and textures;
   - the HDR chain;
   - stars and the Milky Way;
   - atmospheres, clouds and night lights;
   - relief, eclipses, rings and shape models.
7. Editing: a leg's window or duration, re-planned in place; loading and
   saving plans.
8. The gate.
