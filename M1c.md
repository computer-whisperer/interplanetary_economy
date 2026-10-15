# M1c — Creative sandbox and the map view

Status, 2026-10-14: steps 1–4 and 6 done (6 moved ahead of 5, its
shape models left out); next, step 5 (panels and labels).

- Step 6: the HDR chain, the sky, the bodies' orbits, rotation models and
  maps, atmospheres, the Sun's limb, eclipses and Saturn's rings. As
  built, see the step 6 plan below; where it differs:
  - Points of light (stars, the Milky Way, bodies under a few pixels)
    show at a fixed sky exposure where it exceeds the frame's, capped at
    a saturated star's brightness (part 2, not 1): uncapped, the Moon
    as a few pixels flared. A body growing into a disk fades from its
    point over 1–3 px in radius (the point's share falling, the disk's
    opacity rising). Orbit lines leave a gap where their body is, which
    they otherwise hid.
  - The scene's pass writes radiance already exposed, so that f16 holds
    the range shown (unexposed, a star at the Sun's exposure needed
    3e5); bloom's levels hold means, not sums, for the same reason. What
    lies past f16 once exposed (the Sun's disk, at exposures over ~1.3:
    every focus from Earth outward) loses its excess before bloom, so
    its glare stops growing there (1.5× under its energy at Earth's
    exposure, 120× at Saturn's).
  - The Milky Way: SVS's composite is 0.50 × its background + 1.00 ×
    its bright-star layer (fitted, residual 0.0005), and that layer's
    stars integrate to 2.14e-4 × 10^(−0.4 V) (124 unsaturated stars, V
    4.5–6.5): one stored unit is 1.48e-7 radiance units, its median
    ~22.8 mag/arcsec². Tycho-2's stars fainter than Hipparcos are left
    out (a few percent of the background's light).
  - Orbits are sampled at most 10 days apart (the Sun's reflex motion:
    Neptune's line missed it by 290 km at a 384th of its period), 6
    hours for small bodies (1996 FG3's by 460 m at a day).
  - Earth turns by the IERS rotation angle (UT1 = TT − 69 s), not the
    IAU's linear W (0.05° off at J2000). The Moon's pole in `ephem`
    comes from the IAU model, not Cassini's laws; Earth's stays the
    fixed J2000 axis its J2 acts about.
  - Earth's map is Blue Marble taken as reflectance (its ice 0.8–0.96,
    oceans near black: their blue is the air's); thick clouds 0.8;
    elevation clipped at 6,400 m (GEBCO's scaling). Saturn, Venus,
    Uranus and Neptune have no maps (no Cassini global map of Saturn was
    found; the others are near featureless in visible light).
  - Mars's dust at optical depth 0.35 (0.5 hid the surface toward the
    terminator); Mars's and Venus's air are estimates, stated in
    `sandbox::atmospheres`.
  - Not done: shape models (Phobos, Deimos, Bennu, Ryugu: a mesh
    pipeline, and sources each needing care).
  - Checks: Sirius, Betelgeuse and Polaris within 0.003 px of their
    SIMBAD places at 10″ pixels, Barnard's star 0.2 px (62 px from its
    J2000 place); orbit lines through their bodies within 0.02 px seen
    from 10 km over them; sub-observer points against Horizons within
    0.0005° (the Moon and Earth 0.003°, Jupiter and Saturn 0.002°); a
    map's mark within 0.06 px of its place; relief within 0.3%; the
    atmosphere's zenith transmittance within 1e-5 of analytic; an
    umbra within 0.17 px of the Sun–Moon axis, the penumbra within
    0.0002 of the covered share; rings within 0.04% of the slab's
    formulas. The 2045-08-12 eclipse falls off Florida at 17:43 TDB,
    the axis 1,346 km from Earth's center (NASA: 17:42:39, γ 0.2116).
  - Cost, GPU at 1920×1080: the post chain 0.19 ms; the sky 0.08 ms;
    Earth filling the view with its maps 0.05 ms more, with its air
    0.55 ms more (0.80 ms in all).
  - Data: `data/sky` 15 MB, `data/bodies` 65 MB (the repository's
    `.git` 93 MB). Every map seen stays on the GPU while the app holds
    it (all of them: ~860 MB), and decoded in memory too (~785 MB with
    the sky's).
  - Saturn's rings' optical depth is one occultation's, noisy where it is
    low (σ ≈ 0.085 a 10 km bin, and spans off by up to −0.1): kept as
    measured, negatives included, so that gaps average empty; the C
    ring's faint parts are uncertain either way.
  - Found in review and fixed: the spot over a growing disk was hidden
    by the disk; bloom saturated f16 near the Sun; bright points were
    clipped before exposure; points left out clouds (Earth's 4× too
    faint) and lunar-Lambert shading (the Moon's up to 1.5×); maps were
    uploaded again each time a body came back into view (70–100 ms for
    Earth's); one unreadable map left every body plain; the rings'
    profile wrapped at its edges, and clipping its noise at 0 made every
    gap a faint ring.
  - Open: translucent air and rings write depth, so lines behind them
    are hidden (Saturn's orbit line at the rings' edge), and from inside
    an atmosphere the sky would hide lines and points; a ringed body as
    a point carries no ring light; a body as a point before the Sun
    adds light rather than hiding it.
  - Open: Mars's air against a Monte Carlo of the same dust (plane-
    parallel, checked against the analytic single scattering and by
    binning escapes): single scattering agrees within 5%, but Hillaire's
    isotropic multiple scattering is 1.2–4× too bright for this
    forward-scattering dust, and the ground gets no skylight (near the
    terminator 2.5× its direct light). Near the terminator the surface
    is 23% of the light shown where it should be 62%: washed out.

- Step 4: plans designed in the background and drawn as lines in fixed
  or rotating frames, with markers for ships and events, and playback.
  As built, see the step 4 plan below (amended in three places, each
  saying why).
  - Gate item 2's check for lines: a low lunar orbit in a frame scaled
    to 1 au and centered far from it (as the Sun–Earth frame is), seen
    from 100 km to 5 au, draws within a pixel of its f64 projection (lit
    pixels within 0.70–1.06 px of the curve, for a line 1.5 px wide; the
    curve within 0.71 px of a lit pixel). With the high parts alone it
    is ~9 km off.
  - Between samples the line follows the Hermite curve: from 30 km over
    LEO within 0.81 px where the chords bow 22 px; from 300 km, cut on
    the GPU into 8–16 pieces, within 0.81 px; a 650,000 km cruise
    segment from 10 km and a 13,000 km NRHO segment bowing ~80 km from
    18 km, cut and placed by the CPU, within 1.0 and 1.2 px (on the GPU
    alone, 14.5 px off). A segment passing behind the eye draws its
    front part only. Past and future meet within a pixel of where the
    curve is at the moment: on curving segments, on one receding from
    100 to 10,000 km, and 100 days into a low orbit seen from 30 km.
  - Frames: in all five, samples' velocities match their positions'
    change (5e-8), and lines stay within 0.2 m of the tracks between
    samples (a ship's marker is drawn on its line, within 1 m of
    `Track::at`). The station loops (L1, L2, NRHO, DRO) close; the
    NRHO's perilune is 2,733 km from the Moon's center.
  - Each of these tests fails with the feature it checks removed
    (checked by breaking each in turn).
  - Cost, GPU timestamps: the published plan (104,670 samples) 0.37–0.84
    ms at 1920×1080. Two million segments (20 low orbits of 100,000
    samples): seen whole, 2.4–2.5 ms GPU and 0.8 ms CPU a frame; from 10
    km inside their tangle, 3.7–4.5 ms GPU and 3.0 ms CPU. Building a
    plan's paths in another frame: 4–13 ms for the published plan, and at
    most 82 ms while every core is designing it. In a
    window, the plan added nothing measurable (0.85 ms of GPU a frame
    against 0.89 without); frames came every 18.5 ms both ways, because
    the compositor put the window on another output (headroom 1.23×).
  - Shapes, in the crew rotation: in the Earth–Moon frame the outbound
    and return legs swing either side of the Earth–Moon line and the
    NRHO stays retrace their loop; in the unscaled frame, the low lunar
    orbits a week apart draw ~16,000 km apart, as the Moon's distance
    changes (the scaled frame keeps them at the Moon).
  - A review (a sub-agent, with runtime probes) found: a crash focusing
    a small body with any time before 2040 in view or in the plan; the
    window stalling up to 2.7 s while a plan designed (the UI's rebuilds
    queued behind the designer on rayon's pool); past and future split
    across the screen rather than along the curve (100+ px on receding
    segments), and drifting with f32 times (200 px after 100 days); f32
    not precise enough near long segments (20 px from 10 km); the bow
    estimate missing S-shaped segments; ascent markers off their jump
    times; a ship whose design panicked shown as designing forever (and
    the planner panicking on a small body before 2040). All fixed, each
    with a test; the line fixes are the plan's three amendments.
  - Left as they are: a landing straight from a plan's start, or an
    ascent as its last step, has no track to place a marker on (they
    are still jump targets), and a ship's marker is not drawn during a
    landing or an ascent, which have no path.

- Step 3: the renderer draws the ephemeris's bodies as ray-cast spheres
  in 64-bit positions, with a camera that follows a body. As built, see
  the step 3 plan below.
  - Gate item 2's check passes: a ship's marker in low lunar orbit lands
    within 0.04–0.22 px of its 64-bit projection, the same to four
    decimals at every distance from 100 km to 5 au (the dot's own
    rasterization, no precision lost with scale).
  - The Moon's limb from 100 and 10 km up sits within 0.18 px of the
    horizon computed in 64 bits.
  - Radiance: a lit center reads 0.49976 for 0.5, the Sun 46,208 for
    46,239 (16-bit float rounding). Flux: a resolved sphere's pixels sum
    to the same image integrated in f64 within 0.05%; a point carries
    the distant sphere's flux within 0.2%, at the screen's center and
    near its corner. (The distant-sphere formula itself runs 2–7% low
    for spheres 40–200 px across: up close, less than a hemisphere is
    visible.)
  - At true brightness and exposure 1.0, the planets seen from afar are
    far too faint to see, and only the Sun shows. Finding them is for
    labels (step 5); exposure is step 6.
  - Screenshots into 8-bit sRGB show steps one code apart across smooth
    gradients (the Moon's surface from low orbit). The float swapchain
    has none; dithering for SDR goes with step 6's tonemap.
  - In a window, frames at the display's refresh (4.3–4.4 ms).
  - A review (a sub-agent) checked the ray casting against an f64 one
    over 396 setups (0.001 px). It found points near the screen's
    corners up to 46% too faint (a pixel there subtends less sky);
    a panic focusing an asteroid before 2040; a jump when refocusing
    mid-flight; small bodies' first view inside the 10 km floor; and
    the screenshot flags bypassing the camera's limits. All fixed, the
    first three with tests. Also from it: spheres wholly off the screen
    are culled, and a frame's step is capped at 0.25 s.

- Step 2: `crates/render` (the scene target, the empty scene, GPU
  timers, a windowless device) and `crates/sandbox` (the window, the
  shell UI on damascene 0.8, and `sandbox --screenshot`/`--frames`).
  As built, see the step 2 plan below.
  - The host is ours, on damascene's exported host layers: they
    negotiated HDR with the compositor on the development machine (an
    `Rgba16Float` scRGB swapchain, 203-nit reference white, 400-nit
    peak: headroom 1.97×). The scene's linear values pass through to
    it; tonemapping to the headroom is step 6.
  - Three runs of 300 frames of the empty scene with the UI. Two got
    frames every 4.4–4.6 ms on average (the display's refresh, under
    Fifo), with 0.4–0.9 ms of GPU time each. The third, its window
    placed elsewhere by the compositor, got frames every 50 ms, 3.2 ms
    of GPU time, and a headroom of 1.23×. An idle GPU's clocks swing its
    times by several fold, so gate item 3 is measured under load.
  - Tests: the scene target reads back exactly, HDR values included
    (1.5 and 7.0); offscreen, the scene's color fills the map view's
    rect and the bars around it show the UI; the target follows the map
    view's size; timestamps time a frame.
  - Left out of our host, for now: AccessKit, the clipboard and primary
    selection, touch and pinch, the soft keyboard, file drops, and the
    app's announcement, link and plot requests, which the stock host
    wires and the sandbox does not need yet.
  - A review (a sub-agent) found the timer reporting an older frame as
    the latest when several finished at once, and a camera drag left
    running when the button came up outside the window; both fixed, the
    first with a test.

- Step 1: mission plans (`data/missions/`, `orbit::mission`), the places
  they name (`data/places.toml`, `orbit::places`, which `orbit gate` and
  `orbit routes` now read too: the regenerated `routes.toml` is
  byte-identical), paths sampled for drawing (`orbit::track`), and
  `orbit mission FILE`. As built, see the step 1 plan below.
  - Gate item 1, its first half (drawing waits for the view): the legs
    M1a checked against published values fly as
    `data/missions/published.toml`, 18 ships, all in 44 s on 24 cores.
    Their Δv matches M1a's designs to a few m/s (the Mars handbook
    departures 3,576 / 3,550 / 3,523 / 3,583 m/s, 2000 SG344 4,038,
    Phobos 1,205 each way, NRHO to low lunar orbit 684).
  - A sensitivity: a station's reference is rebuilt for each visit, from
    its start, and a leg's design depends on it. With a longer reference
    for the NRHO visit (an earlier build), the same NRHO leg found
    another local minimum: 795 m/s, with a 286 m/s departure burn
    against 31.
  - The crew rotation (LEO, 24 days in the NRHO, a week at the south
    pole, home by aerocapture): 9,343 m/s over 51.5 days, designed in
    10 s. In the earlier build its leg from low lunar orbit back to the
    NRHO found no design in its first window, which is why windows are
    retried. Probed, every start within a band of a few hours failed
    (all 8 seeds), and quarter-day starts around it all converged: the
    step 10 sweep's 2% for this leg (216 of 220 dates).
  - Shapes, plotted from `--tracks`: the outbound and return legs
    swing either side of the Earth–Moon line, the NRHO's stays retrace
    their loop in the rotating frame, with perilune ~3,000 km over the
    north pole and apolune ~70,000 km south. Consecutive drawn items
    meet exactly, except a stay in low lunar orbit coasted back and
    forth over 8 days (7 m).
  - Samples: ~31 s apart in LEO, 6 hours in cruise; between them,
    Hermite interpolation stays within 0.1 m of the integration in LEO
    (the test's bound; theory says 3 cm).
  - Known gap, as planned: arriving in a low orbit and leaving it again,
    the next leg starts from its own orbit. LEO to low lunar orbit and
    back: the return leaves 3,494 km from the ship, in a plane 6.9° off
    (197 m/s to change), reported and not flown. It can be much worse:
    the same trip from 2033-03-30 leaves low lunar orbit in a plane 166°
    off (3.2 km/s), and a departure for Mars on arrival in LEO is 85°
    off. Decided 2026-10-14: it stays reported through M1c and is fixed
    before M1b (DESIGN.md, Milestones).
  - Long stays in a low orbit are sampled ~2,900 times a day, so a wait
    through a synodic period in LEO would be ~2 million samples. Step 4's
    lines will need a lighter form for long coasts.
  - Stays in a station orbit cost the upkeep simulated on the visit's
    reference, from 4 runs: 0.5–1.9 m/s per year for the NRHO across
    visits (step 5d's 1.09 came from 20), and none on references under 4
    revolutions (an L2 halo visit of a revolution and a half).
  - A review (a sub-agent) found the jump at a junction lost when the
    leg left the moment the ship arrived; fixed, with a test.

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

The format above was the first sketch; step 1 settled it (Step 1 plan,
item 2: a plan holds ships). Plans live in `data/missions/`.

Missing from the engine today:

- **One entry point for every leg kind.** `legs::plan` designs legs
  within a planetary system at given durations, and
  `interplanetary::plan` legs between systems at given dates; nothing
  picks the dates, or chooses between the two.
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
- The bodies' orbits, each about its parent, through the body (added
  2026-10-14).
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
  - Damascene's stock host (`damascene-winit-wgpu`) creates its device
    with no optional features, so no GPU timestamps, and passes only
    primary-button drags to the app. (It does have hooks for app
    textures, `gpu_setup` and `before_paint`; corrected 2026-10-14.)
    Our host is built from the layers it exports for custom hosts
    (`host::gfx::WindowGfx`, `host::color`, `host::input`), which
    include its HDR negotiation.
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
| volumetric | Host glue: one device, the scene in a texture under a damascene widget, input routing (~300 lines, `volumetric_ui_v2/src/host.rs`, `session.rs`) | The pattern only, on damascene 0.8's host layers (volumetric's host is on 0.6 and hard-codes an sRGB swapchain); 16-bit float target |
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
   machine (Radeon RX 6900 XT, Mesa RADV), measured by GPU timestamps.
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
6. Realism (done before step 5, decided 2026-10-14: a visual pass
   first):
   - rotation models and textures;
   - the HDR chain;
   - stars and the Milky Way;
   - the bodies' orbits;
   - atmospheres, clouds and night lights;
   - relief, eclipses, rings and shape models.
7. Editing: a leg's window or duration, re-planned in place; loading and
   saving plans.
8. The gate.

### Step 1 plan (2026-10-14)

1. **Places** (`data/places.toml`, `orbit::places`): the places ships
   go, by the ids the route graph uses (`leo`, `l1`, `nrho`, `llo`,
   `moon-south-pole`, `lmo`, `phobos`, ...), each defined physically: a
   low circular orbit (body, altitude above the mean radius or radius,
   inclination or a free plane; LEO is 6,778 km, 400 km above the
   equator, as the gate flew it), a
   station orbit (a catalog family and the period wanted; the nearest
   member), a small body or moon to meet, or a body's surface. Every
   small body in the ephemeris is a place by its id. Today these
   definitions are spread over `orbit gate` and `orbit routes`; both
   read the file instead.
2. **The format** (`data/missions/*.toml`, `orbit::mission`). A plan
   holds ships, since the gate's plans do (a ship on every Mars window):

   ```toml
   name = "Gateway crew rotation"

   [[ship]]
   name = "Orion"
   start = { at = "leo", epoch = "2045-03-01" }

   [[ship.leg]]
   to = "nrho"
   days = [5.0]        # durations offered, days; the cheapest flies

   [[ship.leg]]
   wait = 30           # days

   [[ship.leg]]
   to = "llo"
   days = [0.5]

   [[ship.leg]]
   to = "moon-south-pole"
   ```

   - A place is an id or written out:
     `{ low = "earth", altitude = 200 }`,
     `{ station = "l2-halo-south", period = 6.57 }`,
     `{ body = "2000-sg344" }`, `{ surface = "moon" }`.
   - `aero = true`: arrive through the atmosphere, by aerocapture into
     a low orbit or entry to the surface.
   - `window = ["2045-03-01", "2045-03-31"]` bounds the departure.
     Without one, the window opens when the ship is ready (arrived,
     plus any waits) and lasts as gate's did: a revolution of the
     station left or reached, 5 days between low orbits about Earth and
     the Moon, a day in Mars's system; between planets, a synodic
     period (at most 3 years), so the leg takes the next window.
   - `days` is required within a system. Between planets it is
     optional; without it the leg takes the cheapest date and duration
     by patched conics in the window.
3. **One entry point** (`mission::Planner`) by the kinds of the two
   ends:
   - a low orbit and its body's surface: an event priced by `surface`
     (landing, ascent, entry);
   - within a system: `legs::plan`, in the field gate used for that
     system. A window where no design converges is tried again opened
     a quarter of its length later, up to three windows, and the leg
     says so: the gate sweep converged most legs on 98–100% of dates,
     and one bad window should not end a chain;
   - between systems: the cheapest dates by patched conics, each cell
     priced for the actual ends, then `interplanetary::plan`, trying
     the next-cheapest dates when one fails. Station orbits as ends
     between planets wait for step 9's open part: an error says to fly
     through LEO.
4. **Chaining.** Each leg leaves from where and when the last arrived.
   - A ship in a station orbit follows one reference per visit, built
     before the leg that arrives, long enough for the stay and the leg
     that leaves. A stay there costs that reference's upkeep
     (`stationkeeping::upkeep`) per day.
   - A ship in a low orbit coasts in the system's field.
   - After an aerocapture the ship coasts up to the low orbit's radius
     and circularizes there, so the path is drawn without a jump. The
     speed leaving the pass is aimed in the field so the apoapsis is at
     the low orbit's radius (half a Keplerian period missed it by 12 km
     at Mars, from its J2).
   - Known gap: a leg leaving a low orbit picks its own plane and phase
     (the node is free in `End::Low`), so after arriving in one the next
     departure does not start from the ship's own orbit. The planner
     reports the jump at each such junction (km, and the plane change
     it would take); fixing it is playbook work (departures from a set
     plane). Leaving after a launch or an ascent, the plane is free in
     fact: launch timing picks it.
5. **Paths for drawing** (`orbit::track`): a design records each arc's
   integration frame, and `Leg::track` integrates its arcs again with
   dense output. Samples go every 2° of the motion about the body
   pulling hardest, and at most 6 hours apart (~30 s in LEO, a few hours
   in cruise), with velocities, so that cubic Hermite interpolation
   between them stays well under a meter in LEO. Stays are sampled the
   same way. Burns, flyby periapses, passes and surface events are
   markers.
6. **`orbit mission FILE`** prints each item as it is designed: the leg,
   its dates, Δv per burn and in total, the options offered, the time
   taken, and why a leg failed. `--tracks DIR` writes the samples as
   CSV, for looking at shapes before the view exists.
7. **Missions** in `data/missions/`: the crew rotation above, and the
   legs M1a checked against published values (gate item 1).

### Step 2 plan (2026-10-14)

1. **Crates.**
   - `crates/render`, wgpu only: the scene target (`Rgba16Float`, linear,
     1.0 the SDR reference white, sized to the viewport), a reversed-Z
     `Depth32Float` buffer for step 3, the empty scene (cleared to
     space's black), GPU timers, and an offscreen device for tests.
   - `crates/sandbox`, the `sandbox` binary: the host, the damascene app,
     and the frame function that the window and the tests share.
2. **Host.** Our own winit loop, built from damascene-winit-wgpu 0.8's
   host layers: `WindowGfx` for the surface, damascene's renderer, MSAA
   and the color negotiation (including live renegotiation when the
   window moves between outputs); `host::input` for mapping events.
   - The device requests timestamp queries, inside encoders too, when
     the adapter has them.
   - The app asks for `ColorPreferences::hdr_extended()`, so an HDR
     output gets an scRGB float swapchain, and SDR an 8-bit sRGB one.
3. **Frame.** One encoder and one submit per frame:
   - build the UI, with the scene target shown by a `surface()` keyed
     `viewport`, and prepare it;
   - read the viewport's rect; if its size in pixels changed,
     reallocate the target (the tree shows the new one from the next
     frame, so a resize draws one more frame);
   - render the scene into the target, then the UI, which composites
     it.
   The same function draws into the swapchain or into an offscreen
   texture.
4. **Input.** A press over the viewport (damascene's hover target is
   within `viewport`) goes to the camera, and to damascene too (a click
   on the map still takes focus from a text field); the wheel over it
   goes to the camera alone; everything else goes to damascene. In step 2 the camera is a stub that accumulates
   orbit, pan and zoom and shows them; step 3 builds the real one. As
   in KSP's map view: right drag orbits, middle drag pans, the wheel
   zooms, a left click picks (step 5).
5. **Shell UI.** A top bar (title, whether HDR is active, the output's
   headroom), the viewport filling the rest, and a status line: frame
   time, GPU time for the scene and the whole frame, the camera stub.
6. **GPU timers.** Timestamps at the frame's start, after the scene and
   after the UI (inside the encoder where the device allows; otherwise
   around the scene pass only). They are read back through a ring of
   three buffers, so measuring never stalls the pipeline.
7. **Offscreen harness.** A windowless device runs the same frame
   function.
   - Tests render the scene alone (read back as linear floats) and the
     whole frame (UI and scene, 8-bit sRGB).
   - `sandbox --screenshot PATH` writes the whole frame as a PNG.
   - Checks: the target's clear value reads back exactly; in the whole
     frame the viewport's rect shows the scene's color and the rest
     shows the UI; the timers report positive times where supported.

### Step 3 plan (2026-10-14)

1. **Positions.** The scene is described in 64-bit km in the
   ephemeris's frame (solar-system barycentric, ecliptic J2000). The CPU
   subtracts the eye's position and rotates into view space in 64 bits,
   and only then hands 32-bit vectors to the GPU. Depth is reversed with
   an infinite far plane (depth = near / distance, near at 1 m).
2. **Spheres** are ray-cast in the fragment shader, not tessellated: the
   silhouette is exact from 100 km above the Moon to a dot. A sphere's
   quad covers the cone of rays that hit it (the whole screen when the
   camera is close). Spheres wholly off the screen are culled. The ray's hit distance uses the camera's
   altitude above the sphere, computed in 64 bits
   (`t = h(2r + h) / (b + √(b² − h(2r + h)))`), which stays exact where
   `b − √…` would cancel: looking straight down from low orbit. Depth is
   written per pixel.
3. **Light and units.** Linear radiance, 1.0 the radiance of a white
   Lambertian surface under the Sun at 1 au (SDR white until step 6's
   exposure). A body is Lambertian with its Bond albedo and a tint (NASA
   fact sheets), lit at (1 au / its distance from the Sun)². The Sun is
   a sphere of constant radiance (1 au / R☉)² ≈ 46,000. Limb darkening,
   bloom, exposure and tonemapping are step 6.
4. **Points.** A sphere under a pixel is drawn as a point carrying its
   true flux: the disk-average radiance of a Lambertian sphere at its
   phase angle, (2/3)·albedo·E·Φ(α), times its solid angle over the
   point's. The flux is continuous where a sphere shrinks to a point,
   anywhere on the screen: off the axis a pixel subtends pixel²·cos³θ.
5. **Markers** for ships (step 4 uses them): a fixed size in pixels,
   depth-tested against the bodies.
6. **Camera** (`sandbox`): focused on a body and following it; right
   drag orbits as a turntable about ecliptic north; the wheel zooms by
   distance, from 10 km above the body's surface to 1e10 km. A new focus
   is reached by a 0.6 s animation (position along the line between the
   two moving bodies; distance in log space). The focus is picked from a
   select in the top bar. Panning is dropped: the map view always looks
   at something, as KSP's does.
7. **Time.** A minimal clock so following can be seen: space pauses, `.`
   and `,` change the rate by tenfold; the date and rate in the status
   line. Step 4 builds playback.
8. **Tests** (offscreen):
   - Gate item 2's check: a marker in low lunar orbit lands within a
     pixel of its position projected in 64 bits, with the camera 100 km
     to 5 au away.
   - The Moon's limb, seen from 100 km up, within a pixel of the
     horizon computed in 64 bits.
   - A lit sphere's center pixel has the radiance the units say, and
     the Sun's disk its radiance.
   - A point's summed flux matches the sphere it stands in for.

### Step 4 plan (2026-10-14)

Sizes measured first: the missions in `data/missions/` hold ~116k
samples in all (the crew rotation's first stay in LEO, 2.6 days, is
7,685); an ephemeris frame at one moment takes ~240 ns.

1. **Lines** (`render::paths`). A path is samples (time, position,
   velocity) in some frame's coordinates, and a pose per frame at the
   moment drawn maps them into the world (origin, axes, scale).
   - Positions reach the GPU as two f32s per coordinate (high and low
     parts), and the eye, in the same frame's coordinates, likewise;
     the vertex shader subtracts the eye first. The error is then
     ~6e-8 of the distance from the eye, as for bodies. This replaces
     The view's "chunks with 64-bit anchors and 32-bit offsets": cruise
     samples are 650,000 km apart, so an anchored chunk of a few dozen
     of them is off by ~10 km; holding that under a pixel 10 km from a
     ship needs chunks under ~45,000 km, and thousands of anchors moved
     every frame.
   - Chunks of 64 segments stay, as the units of culling and of detail:
     each frame the CPU culls them against the view in f64. Where a
     straight segment's chord would bow visibly from the Hermite curve
     (over 0.25 px; 2° of LEO bows ~1 km), its segments are cut into
     pieces along the curve on the GPU, a power of two up to 32, the
     chunk's parts of 8 segments each cut as finely as it needs
     (amended after measuring: cutting every segment of a near chunk in
     32 took 18.8 ms of GPU time for two million segments seen from 10
     km).
     The bow is bounded as 4/27·(|h·v₀ − Δp| + |h·v₁ − Δp|), which also
     bounds the curve for culling (amended in review: the midpoint's
     offset alone missed an S-shaped segment).
   - Where f32 is not enough, the CPU places the pieces (amended in
     review): segments whose pieces would number over 32, or whose
     Hermite terms (up to a segment's length, 650,000 km in cruise)
     would round by more than 0.25 px seen from the eye, are cut and
     made relative to the eye in f64, a few dozen near the eye. In f32
     a cruise segment seen from 10 km is off by ~20 px between samples.
     So a ship's marker on its line stays on it at any zoom.
   - Segments are clipped at the near plane before projection, drawn as
     antialiased capsules a fixed width in pixels, depth-tested against
     the bodies. Two samples at one time (a burn, or two motions
     meeting) are not joined.
   - Each path has two colors, past and future. The CPU finds each
     path's segment and fraction at the clock's time in f64 each frame,
     and the fragment shader colors by that (amended in review: times
     as f32 seconds since a path's start drift by a second after 30
     days, 2 km of LEO; and the fraction along a piece is recovered
     perspective-correctly from the screen).
   - Lines and markers are drawn in a second pass over the scene's
     depth; step 6 moves that pass after the tonemap, so exposure does
     not touch them.
   - Paths are uploaded when they change (a new item, another frame),
     and stay on the GPU between frames.
2. **Frames** (`sandbox::frames`):
   - **Focus** (the default): fixed ecliptic axes centered on the
     camera's focus, as in KSP.
   - **Rotating with two bodies**: Earth–Moon, Sun–Earth, Sun–Mars;
     the origin at their barycenter, axes from `reference::EarthMoonFrame`
     (renamed `RotatingFrame`, since it serves any two bodies);
     optionally scaled by their distance ("pulsating").
   - A path's samples are mapped into the frame at their own times
     (positions and velocities, in parallel), and drawn by the frame's
     pose now.
   - The camera's turntable turns with the frame: its azimuth and
     elevation are about the frame's axes now, so in the Earth–Moon
     frame a halo stays still while the ship moves along it. Changing
     the frame keeps the view's direction.
   - A select in the top bar picks the frame; `--frame` on the command
     line.
3. **Station orbits as reference loops.** Each station place's catalog
   orbit, one revolution in the CR3BP, drawn in the Earth–Moon
   pulsating frame at its pose now: in that frame it is still, and in
   any other it turns with the Moon. Faint, in every view.
4. **Plans in the sandbox.** `sandbox --plan FILE` reads a plan and
   designs its ships on a background thread (ships in parallel), and
   each item appears when it is ready. Paths are colored by leg (a
   categorical palette in plan order); a stay takes the color of the
   leg that arrived, dimmer, and before the first leg it is gray. A
   stay resting on or at a body (its track is the body's own motion)
   draws no line. The status line counts ships designed and failures;
   the panels are step 5.
5. **Markers.** Shapes: a disc for each ship where it is now (none
   before its first item or after its last); a diamond for a burn; a
   ring for a flyby or an atmospheric pass; a square for a landing
   (where the ship leaves its orbit) or an ascent (where it reaches
   orbit); a cross for a junction's jump. Each sits on its path, mapped
   like the samples; past ones dimmer. Δv and altitude labels are step
   5's.
6. **Playback** (`clock.rs`):
   - time warp in named steps: 1 s, 10 s, 1 min, 10 min, 1 h, 6 h,
     1 day, 1 week, 1 month and 1 year per second (`,` and `.`), and
     space pauses;
   - a scrubber under the map spanning the plan (the ephemeris without
     one); dragging it sets the time;
   - `[` and `]` jump to the previous and next marker;
   - with a plan, the clock starts at its first ship's start.
7. **Tests** (offscreen unless said):
   - A line about the Moon, in a rotated and scaled frame, seen from
     100 km to 5 au: its pixels within a pixel of the path projected in
     f64 (gate item 2's check, for lines).
   - From 10 km off a LEO path, the drawn line passes within a pixel of
     the Hermite curve between samples.
   - A segment passing behind the eye draws only its part in front.
   - The past and future colors meet within a pixel of the clock's
     time.
   - Frames (unit): poses invert; the Moon on the rotating frame's x
     axis; samples' velocities in a rotating frame match their
     positions' change; a catalog loop closes.
   - Playback (unit): warp steps, jumps to markers, the scrubber's
     mapping.
   - GPU time for the published plan's paths, and for a synthetic two
     million segments (gate item 3 needs headroom).
8. **Not here:** the panels, labels and picking (step 5); a lighter
   form for long stays' tracks themselves (each sample is 56 bytes in
   `Track`, 48 on the GPU; at two million samples, worth revisiting when
   step 8's plan is written).

### Step 6 plan (2026-10-14)

Moved ahead of step 5 (decided 2026-10-14), with the bodies' orbits
added. Built assets (textures, the star catalog, the Milky Way) are
committed under `data/`, as `de440.bin` is, rebuilt by scripts in
`tools/` (decided 2026-10-14; ~80–100 MB in all). Each source's license
or terms are recorded beside it when imported.

The renderer as step 4 left it: one HDR pass (ray-cast spheres, then
points as 2 px squares of light, additive), then an overlay pass
(lines, markers) over its depth, straight into the texture damascene
shows. A clear color for the sky, Lambertian spheres of one albedo each,
no tonemap.

Order of work, each part committed with its tests:

1. **The HDR chain.**
   - The scene renders into its own HDR texture; a post pass writes the
     texture damascene shows: exposure, bloom, a tonemap to the output's
     headroom (from the host; 1.0 on SDR), and on SDR a dither of one
     8-bit step against banding. The overlay pass then draws over the
     tonemapped image, against the scene's depth, so exposure does not
     touch lines and markers.
   - Bloom: Hushfold's chain (13-tap downsample, tent upsample), with
     no threshold and a small weight (energy kept, as a lens's glare),
     and no clamp below f16's range: the Sun's 46,000 survives.
   - Tonemap: linear up to a knee, then a smooth shoulder that
     approaches the headroom; per channel, so the Sun goes white.
   - Exposure follows the focus: its sunlit side at normal incidence
     maps to a fixed level (key / (albedo × irradiance)), eased over
     0.6 s on refocus. `-` and `=` shift it by a stop.
   - Points of light (stars, and bodies under a pixel) are a Gaussian
     spot carrying the same flux, not a 2 px square, and take the
     stars' fixed exposure ("Realism": stars get their own), so planets
     seen from afar look like the bright stars they are. Where a body
     grows from a point to a disk (1–3 px), the two cross-fade.
2. **The sky.**
   - Stars: the Hipparcos catalog (118,000 stars, to V ≈ 9; CDS
     I/239): direction in ecliptic J2000, proper motion applied on the
     GPU for the scene's date, flux from V, color from B−V (blackbody
     temperature, Ballesteros 2012). Parallax from anywhere in the
     solar system is under 1″, and aberration ~20″: both left out.
     `tools/sky/` builds `data/sky/stars.bin`.
   - The Milky Way: NASA SVS's Deep Star Maps 2020 background (Gaia
     DR2's faint stars and the diffuse light, in celestial coordinates,
     with Hipparcos and Tycho stars removed, so none is drawn twice),
     resampled into a texture. Its scale against the stars' fluxes is
     calibrated from SVS's matching bright-star image, so both share
     units. Tycho-2's stars fainter than Hipparcos (V 9–11.5) are
     splatted into it at build time if the gap shows.
   - The sky is fixed in the ICRF. In a rotating frame the camera turns
     with the frame, so the stars turn across the view: once a month in
     the Earth–Moon frame.
3. **The bodies' orbits** (new, in `sandbox`). Each body's path about
   its parent (planets and small bodies about the Sun, the Moon about
   Earth, Phobos and Deimos about Mars) over one period around now, from
   the ephemeris, drawn as paths with a pose per parent placed at the
   parent's position now, as KSP does: the line passes through the body
   in every frame. Windows step by 1/16 of a period, so rebuilds are
   rare. They are a second set of paths, uploaded apart from the plan's,
   dim, with past and future showing the direction of motion.
4. **Rotation models and textures.**
   - `orbit::rotation`: each body's orientation from IAU WGCCRE 2015
     (Archinal et al. 2018): pole and prime meridian, with the Moon's
     and Neptune's periodic terms. Earth's rotation uses UT1 = TT −
     ΔT, ΔT taken as 69 s (its 2024 value; it cannot be predicted, and
     each 10 s is 0.04°). Checked against Horizons's sub-observer
     longitudes and latitudes, fetched as fixtures. `ephem::pole` is
     made to read it, if the mission tests hold; else reported.
   - Textures, equirectangular, east longitude, 0° at the center, with
     mipmaps; the level picked from the pixel's footprint on the
     sphere (no seam at the date line):
     - Earth: Blue Marble Next Generation (NASA Earth Observatory);
     - the Moon: SVS CGI Moon Kit (LROC WAC, LOLA);
     - Mars: Viking colorized mosaic (USGS), MOLA;
     - Mercury: MESSENGER MDIS (USGS);
     - Jupiter: Cassini (PIA07782); Saturn from Cassini if a global
       map is found, else bands;
     - Venus, Uranus and Neptune near uniform, as they look in
       visible light.
   - Each texture is scaled so its mean matches the body's albedo, so a
     resolved body and its point agree.
   - Shading: Lambert for clouded bodies; for the Moon, Mercury and
     small bodies, the lunar-Lambert law (McEwen), which keeps the full
     Moon flat to its limb.
   - Relief: normals from elevation (LOLA, MOLA, ETOPO), lit, not
     displaced.
5. **Atmospheres.** Earth, Mars and Venus, by Hillaire's 2020 method:
   transmittance and multiple-scattering tables computed on the CPU at
   start (in f64, so tests can check them), and a ray march per pixel
   through the shell, from the sphere's own quad grown to the
   atmosphere's top. The surface is seen through it (transmittance and
   in-scattering), and beyond the limb it blends over the sky. Lit from
   behind, the shell glows as a ring.
   - Earth: Rayleigh, Mie, ozone (Hillaire's values); clouds folded into
     the surface's albedo; night lights (Black Marble 2016) shown with a
     fixed boost, like the stars, since at the day side's exposure they
     would not show; a sun glint off the oceans.
   - Mars: dust (Mie) and CO₂, from published parameters; Venus: a haze
     over a bright cloud deck.
6. **Shadows, rings, the Sun, shape models.**
   - Eclipses: the Sun's visible fraction from each surface point, as
     the overlap of the Sun's disk with up to four occluding spheres the
     CPU picks (Earth on the Moon, the Moon on Earth, Phobos on Mars).
   - Saturn's rings: a radial profile of color and opacity (Cassini,
     PDS Ring-Moon Systems Node), lit, with the planet's shadow on them
     and theirs on the planet.
   - The Sun: limb darkening that keeps its total flux; bloom does the
     glare.
   - Shape models for Phobos, Deimos, Bennu and Ryugu (PDS; Ryugu's
     terms checked), drawn as meshes once over a few pixels.
7. **Tests**, beside each part:
   - The tonemap: monotonic, linear below its knee, under the headroom;
     SDR has no flat run of more than an 8-bit step in a smooth ramp.
   - Exposure leaves lines' and markers' colors alone.
   - Stars: Sirius, Betelgeuse and Polaris land within a pixel of their
     positions projected in f64; a star's spot sums to its flux.
   - Orbits: each body's line passes within a pixel of the body in
     every frame.
   - Rotation: sub-observer points against Horizons, per body.
   - Textures: a known feature (Tycho crater, Olympus Mons, the Strait
     of Gibraltar) lands within a pixel of its projected position.
   - Atmospheres: the tables against analytic optical depths; the limb
     brighter than the disk's center; a ring from the night side.
   - Eclipses: the Moon's shadow on Earth at 2045-08-12's total eclipse
     falls where the Besselian elements put it.
   - GPU time at 1920×1080 for each part, for gate item 3.
8. **Not here:** Earth's clouds moving, cities growing, Jupiter's
   features drifting (the textures are fixed in time); shadows on ships.
