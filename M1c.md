# M1c — Creative sandbox and the map view

Status, 2026-10-15: steps 1–6 done (6 done before 5, its shape models
left out); next, step 7 (editing).

- Step 5: labels on the map view, picking by click, and the plan's
  panel. As built, see the step 5 plan below; where it differs:
  - Of labels whose anchors lie within 6 px of each other, only the
    first placed is kept: they read as naming one spot (from 2 au,
    Phobos's label beside Mars's read as a second point). A body drawn
    under 6 px names the spot it is drawn at, to 6 px past its radius:
    a ship or marker there is not labeled.
  - Only bodies drawn as disks (1 px in radius or more) hide what is
    behind them, as in the renderer, where a body under a pixel is a
    point of light. A label avoids every anchor's point where it can,
    and else covers only points ranked below it.
  - A click tries lines before bodies (amending item 3), so that a line
    in front of a body's disk can be picked; a line counts only where
    no body hides it.
  - Small bodies are labeled by their proper names, else their
    provisional designations ("Itokawa", "1996 FG3"); station loops by
    their places' ids ("NRHO").
  - The panel is a damascene list that builds only the rows shown. A
    leg's row lists its burns where it has more than one, or a flyby or
    a pass; Δv under 0.5 m/s (a stay's station-keeping) is shown as
    none.
  - Checks: an anchor is its body's f64 projection (0.0000 px), and its
    label stands its radius plus 3 px off it, drawn where placed; the
    Moon behind Earth, behind the eye or off the view has no anchor;
    clicks pick the Moon at its center, the ship beside its marker, a
    marker's item at its label, the leg on its line, nothing on empty
    space. Through damascene's pointer events, a click on a label
    reaches the map view (labels take no input) and one on a row
    reaches the row; at 1 and 2 physical pixels to a logical one, the
    Moon's anchor is within its phase's lean of where it is drawn (0.03
    and 0.28 px). Line picking follows the drawn Hermite curve (0.000 px
    on it, 79 px off its chord), halving each segment until a bound on
    its curve's image says it is straight to a quarter pixel or too far:
    over random segments, within 0.15 px of the curve sampled densely.
    Removing each of 10 checked features in turn fails its test.
  - Cost, at 1920×1080 with the published plan (18 ships), measured
    offscreen: placing labels 0.03–0.06 ms a frame (11–60 anchors); the
    frame's CPU work 0.6–0.8 ms, 1.05–1.26 ms with the panel shown; a
    pick ~3 ms (every line's segments bounded, those near the click
    halved). In a window, frames came at the display's refresh with and
    without step 5 (4.38 and 4.40 ms with the published plan designing;
    4.2–4.3 ms without a plan).
  - A review (a sub-agent, with runtime probes) found: Earth's label
    giving way to a ship's every half orbit seen from afar (the ship
    hidden by a body drawn as a point, then at its spot); markers ranked
    below the focus keeping its label out; a ship resting at a body
    hidden by it, so unlabeled and not pickable; lines over a disk not
    pickable; line picks missing curves that bow toward the eye or lie
    off the view's axis; a ship refused mid-design showing no reason;
    labels a frame behind the map view's size when it changes. All
    fixed, each with a test (the last: labels stretch with the stale
    image in that frame). HiDPI was checked at scales 1, 1.5 and 2.
  - Found in damascene 0.8: `rect_of_key` does not find keyed nodes in
    a virtual list's rows (its key index is rebuilt before layout
    realizes them). Hit-testing and scrolling to a row key work.

- Step 6: the HDR chain, the sky, the bodies' orbits, rotation models and
  maps, atmospheres, the Sun's limb, eclipses and Saturn's rings. As
  built, see the step 6 plan below; where it differs:
  - Points of light (stars, bodies under a few pixels) are as bright
    to the viewer's eye as their sources would be, at the scene's one
    exposure, lifted with the sky over it (Realism; decided 2026-10-15). Until then they and the
    Milky Way showed at a fixed sky exposure where it exceeded the
    frame's (7.5e5), capped at a saturated star's brightness, and drawn
    as a camera records them. A body growing into a disk fades from its
    point over 1–3 px in radius (the point's share falling, the disk's
    opacity rising). Orbit lines leave a gap where their body is, which
    they otherwise hid.
  - The scene's pass writes radiance already exposed, so that f16 holds
    the range shown (unexposed, the Milky Way's median, 4e-10, lies
    under f16's smallest step, 6e-8); bloom's levels hold means, not
    sums, for the same reason. What
    lies past f16 once exposed (the Sun's disk, at exposures over ~1.3:
    every focus from Earth outward) loses its excess before bloom, so
    its glare stops growing there (1.5× under its energy at Earth's
    exposure, 120× at Saturn's). Kept (decided 2026-10-15): the glare
    is bright enough as it is; an f32 target was the alternative.
  - The Milky Way: SVS's composite is 0.50 × its background + 1.00 ×
    its bright-star layer (fitted, residual 0.0005), and that layer's
    stars integrate to 2.14e-4 × 10^(−0.4 V) (124 unsaturated stars, V
    4.5–6.5): one stored unit is 1.48e-7 radiance units. Over the sky's
    area its median is 24.6 mag/arcsec², its brightest percent brighter
    than 21.3, and an eighth of it under the 8-bit map's first step (26.9;
    corrected 2026-10-15 from 22.8). Tycho-2's stars fainter than
    Hipparcos are left out (a few percent of the background's light).
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
  - Mars's dust at optical depth 0.35 (the clear season); Mars's and
    Venus's air are estimates, stated in `sandbox::atmospheres`. (0.5
    had been dropped for hiding the surface toward the terminator, a
    fault of the shader's: part 8.)
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
  - Part 8: Mars was washed out toward its terminator. Against a Monte
    Carlo of the same dust (`tools/atmosphere/`), the air's multiple
    scattering is now spread by direction, the ground lit by its sky and
    seen by reciprocity; see the part 8 plan below.

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
Each departure from realism is a setting, in stops, whose 0 is true to
life (decided 2026-10-15): the background lift, the body lift and the
night lights, below.

**Light.**
- The Sun is the only light. Radiometry is physical, so Mars is lit at
  43% of Earth's sunlight and Jupiter at 4%.
- Exposure adapts to what is in focus.
- The scene is rendered in linear HDR. Exposure, bloom and a tonemap fit
  it to the output's headroom: up to the panel's peak on an HDR output (a
  16-bit float scRGB swapchain), and a filmic curve to 1.0 on SDR. The
  curve takes a color's largest channel, the others scaled with it, so
  that a color keeps its hue however bright (decided 2026-10-15: on each
  channel alone it showed every bright planet's point white, Mars's
  too).
- One exposure for the whole scene (decided 2026-10-15: as accurate as
  we can first; departures where play shows the need). Exposed for a
  sunlit body, as a camera would be, the true sky shows nothing; over
  Earth's exposure, +19 stops shows the Milky Way's band faintly, +22
  clearly. Rejected: a fixed exposure for the stars and the Milky Way
  where it exceeded the frame's, which showed them beside a sunlit body
  but which the exposure keys did not move.
- The sky (the stars and the Milky Way) is lifted over that exposure by
  a setting, the background lift: 17 stops by default, 0 (true) to 25
  (decided 2026-10-15: the true sky left the map black beside any
  sunlit body). The exposure keys move it with the rest of the scene.
  Over Earth's exposure, +17 shows stars to about magnitude 11 and not
  yet the Milky Way. (Earth's exposure has moved with its measured
  brightness: 1.1 stops down when it took Mallama's curve, 1.0 up when
  Robinson's; the defaults moved with it, 18 between, the look kept.)
- Bodies drawn as points of light are lifted over the scene's exposure
  by their own setting, the body lift: 17 stops by default, as the
  sky's, so that the planets show where they are, at their brightness
  among the stars (decided 2026-10-15). As a body's disk fades in, from
  1 to 3 px in radius, its point's lift falls in stops with its share.
  Rejected: points at the scene's exposure, which left Jupiter's from
  Earth at 0.0008 of the SDR white, invisible beside thousands of
  lifted stars; and points lifted with the sky, by one setting.
- Points of light are as bright to the viewer's eye as their sources
  would be: a star's flux is spread over the sky a pixel subtends at the
  viewer, not the sky it shows of the scene. A logical pixel is taken as
  CSS's reference pixel (0.0213°), so a window 1,080 logical pixels tall
  fills ~23° of the viewer's sight with the view's 50°: drawn as a
  camera records them, stars would show ~5× too faint against the Milky
  Way, whose radiance shows the same either way. A body is such a point
  while the eye could not resolve it (under 1′ in radius: every planet
  seen from Earth); past that its point's gain falls as its peak would,
  to 1 (none) where its image could be resolved.
- Earth's night lights are lifted over their radiance by a setting, 14
  stops by default, so that they show at the day side's exposure. Their
  radiance, ~1e-5 of daylight's at a bright city, is within a factor of
  a few: Black Marble's image is stretched, not calibrated.

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
- Each body's brightness is held to its published phase curve (decided
  2026-10-15): Mallama and Hilton 2018 for the planets, as JPL Horizons
  computes them, but Earth's; Allen's for the Moon, as Horizons'; the H,
  G system for Phobos, Deimos and the small bodies. Earth's is Robinson
  2025's fit to Earthshine and spacecraft photometry (decided
  2026-10-15): Mallama's full phase, EPOXI's 57.7° extrapolated by a
  model that swapped forward and backward scattering, is 0.75 mag too
  bright (a geometric albedo of 0.434 against 0.23, which with its own
  phase integral made Earth reflect 0.57 of its light, against 0.29). A sphere's shading spreads its
  light over its disk, and a gain brings the total to the curve at the
  camera's phase angle (held at the curve's range's edge beyond it);
  Saturn's point carries its rings' light. A map's mean is weighed as
  its disk shows it from its equator (the Moon's, its near side's). The
  exposure shows the focus's sunlit side at full phase, its mean, at
  `KEY`.
  - A body's point follows its curve as far as Horizons takes it
    (Mercury's, Venus's and the Moon's to 180°), Earth's to 144°, where
    its fit's data end; its disk's gain is held past 170°, where a
    crescent's shading carries too little light to scale.
  - Checked against Horizons over 2025–2080, from Earth and from bodies
    farther out (to 176° of phase): the curves but Earth's within 0.002
    mag (Mars 0.12, the terms for its longitude seen and its season left
    out); the planets' points from Earth within 0.005, to 177°.
  - Disks, against their curves, seen from 100 radii: bodies without air
    or maps within 0.03 to 120° of phase, Jupiter 0.02 (nearer, a disk
    shows less than a hemisphere: from 12 radii, 0.2 under at 120°).
    Earth with its air within −0.14 to +0.02 to 144°, and its face
    lit moves it ±0.2. The Moon from Earth −0.25 to +0.34 to
    150°, its waxing and waning differing by up to 0.33 near 100°, where
    its curve is one for both.
  - Open: the Moon's thinner crescents show limb highlands brighter than
    its near side's mean, which its curve counts already (0.5 mag over at
    166°). Bodies with air add their air's forward scattering, which
    their curves count already, at large phase angles (seen from 500
    radii, Mars 1.3 mag over at 120°, Venus 1.1 at 160°). Saturn's
    rings as drawn show ~2.5× under its curve's with them at small phase
    angles, so resolved Saturn shows 0.35–0.6 mag under its point.
  - Earth's model (its maps, clouds, air and glint) is close to physical
    (2026-10-15): with no gain, 0.06–0.36 mag under its curve to 120° of
    phase, and ~10% under its spherical albedo (0.294). Its gain is
    1.2–1.7 to 90°: at full phase its Lambert clouds lack backscatter.
    Past 90° it climbs (2.7 at 120°, 9.5 at 144°): its Lambert clouds
    lack the forward scattering that brightens a crescent. Its air is
    drawn over its surface as calibrated, so its disk shows up to 0.14
    mag over its curve. Its cloud map's mean value, 0.24, is cover
    weighed by brightness: 60% of the area has some (over 0.1 of full).
    Its aerosols are the global mean's (optical depth 0.14 at 550 nm;
    Hillaire's, 0.005, a clear day's). Before, held to Mallama's curve,
    its gain was 3.4 at full phase.
  - Before: Lambert shading of the Bond albedo, which left the giants
    ~1 mag faint (a geometric albedo of 2/3 of it; theirs is 1.4–2.5×
    that), and Mercury, Mars and the Moon too bright at large phase
    angles, to 1.4–1.6 mag.
- Each body's color is held to its measured color at full phase the same
  way, the gain per channel, its luminance kept (decided 2026-10-15):
  what it reflects of the Sun's light, the Sun white, from spectra
  through the eye's color matching functions
  (`tools/photometry/colors.py`). The planets from their geometric
  albedos in nine bands (Mallama et al. 2017; the Johnson-Cousins and
  Sloan bands alone differ by up to 0.09 in a channel); the Moon's from
  Lane and Irvine 1973, its red as their 1964 data and other studies
  have it; Phobos's and Deimos's as D-type asteroids', which theirs are
  likened to; small bodies' as their class's mean (DeMeo et al. 2009),
  the Sun's where their class is unknown. A map keeps its parts' colors
  over its mean. Earth's color is its air's (Rayleigh scattering's over
  white clouds): its disk keeps its maps' colors, under its air, and
  its point, which has no air, takes its measured color.
  - Points show their colors within 0.001; disks without air within
    0.014 (as the face shown varies). Earth's disk is 0.10 over its
    measured blue (in a channel, of luminance 1), its air drawn over its
    calibrated surface (above). Rejected: Earth's gain per channel as
    the others', which tinted its clouds pale blue. Earth's color is
    Robinson 2025's, with its curve (Mallama et al.'s came from the same
    extrapolation as its full phase).
  - Before: the maps' means and rough tints. Mars's map reflected 1.7×
    the blue its measured color does, and Uranus and Neptune were paler
    than their measured cyan.
  - A saturated color past the tonemap's knee keeps its hue and loses a
    little luminance: Mars's sunlit side at `KEY` shows ~8% darker on
    SDR than under a shoulder per channel (calculated). The Sun's disk,
    far past it, shows its limb's reddening rather than white.

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
- Each class of line shown or hidden in the settings: the ships' paths
  with their events, the station orbits, and the planets', moons' and
  small bodies' orbits (added 2026-10-15); and the labels, all at once
  (what is drawn still picks).

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
  positions, decluttered greedily by priority (step 5). Measured first:
  ~2.6 µs a label a frame, so Hushfold's distance-field text, the
  planned fallback, was not needed. Polychora's pairwise relaxation, the
  planned declutter, was rejected (2026-10-15): it never drops a label,
  so in a dense cluster the labels spread far from what they name.
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
| Hushfold | HDR, bloom and tonemap chain (~330, `gfx.rs`, `post.wgsl`) | Drop the warm grade and vignette; raise the clamp at 60 so the Sun's energy survives; tonemap to the output's headroom |
| Hushfold | Growable instance buffers, mesh builders, GPU timestamps, screenshot and recording | UVs on vertices |

Skipped: volumetric's deferred renderer and SSAO (an 8-bit CAD look),
Hushfold's procedural sky, polychora's 4D renderers and its vulkano glue.

Planned and not taken (step 5, 2026-10-15): volumetric's points and GPU
picking (points are the scene's own, step 3 plan, item 4; picking runs
on the CPU in f64, since the bodies are spheres and the rest points and
lines); Hushfold's distance-field text and polychora's label declutter
(see Engine, Labels).

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

### Step 5 plan (2026-10-15)

Measured first, with a probe over the sandbox's UI at 1920×1080 (release
build, labels moving every frame): damascene's text nodes cost ~2.6 µs
a label a frame on the CPU, mostly in its paint pass. That is 0.6 ms for
200 labels, 1.4 ms for 500 and 2.6 ms for 1,000; text that changes every
frame costs no more. So labels are damascene text, and Hushfold's
distance-field text is not needed. Only the labels left after declutter
are built.

1. **Labels** (`sandbox::labels`):
   - What is labeled:
     - bodies, by name;
     - ships where they are, by name;
     - markers, by what they are: a burn's Δv; a flyby's body,
       altitude and Δv; a pass's body and altitude; a landing or an
       ascent; a jump's gap and plane change;
     - station loops, by their place, at the loop's first sample.
   - Each is projected in f64 through the scene's camera onto the map
     view. None is labeled behind the eye, off the view, or behind a
     body (where the ray from the eye enters a sphere before it).
   - A body's label sits beside its disk. A body whose disk is taller
     than a quarter of the view has none: it is what the view is about,
     and the status line names it.
   - Declutter is greedy by priority. Each label is tried right of,
     left of, above and below its anchor, and kept at the first place
     that overlaps no label already placed and no anchor shown; else it
     is dropped. Priorities, highest first: ships; the focus; the Sun and
     planets; moons; markers ahead of the clock; small bodies; markers
     behind it; station loops. Within one, nearer the eye first.
   - This replaces polychora's pairwise relaxation (the reuse table).
     It pushes overlapping labels apart and never drops one, so with
     the inner planets seen from Saturn their labels would spread
     across the screen, far from what they name.
   - Drawn as damascene text over the map view, at the caption's size,
     on a dark translucent backing; markers' labels in their marker's
     color.
2. **Panel** (`sandbox::panel`). With a plan loaded, a panel right of
   the map shows it (`p` toggles):
   - each ship: its name, whether it is designing, designed or failed,
     its total Δv and its span;
   - each item: its kind, label, start, duration and Δv, and a leg's
     burns; a failure in red, with why; a junction's jump, with its
     gap and plane change; the item's note;
   - the item each ship is in at the clock's time, marked;
   - clicking an item seeks the clock to its start and selects it.
3. **Picking** (`sandbox::pick`), by a left click on the map, takes the
   first of:
   - a label, for what it names;
   - the nearest ship or marker within 8 px;
   - the nearest body whose disk is under the pointer, or whose point
     is within 8 px;
   - the nearest path within 4 px, for its item.

   A body picked is focused (the camera flies to it). A ship, marker or
   path selects its item in the panel, scrolled into view; for a ship,
   the item it is in. A click on empty space clears the selection. A
   selected item's markers' labels come first in the declutter.

   Picking runs on the CPU in f64, from the projections the labels
   already make. GPU picking (volumetric's, in the reuse table) is not
   needed: the bodies are spheres, and the rest are points and lines.
4. **Tests**:
   - Declutter (unit): placed labels overlap neither each other nor
     anchors shown; of two in conflict, the higher priority stays; the
     same input places the same labels.
   - Labels (unit): a body's label lands beside its projected point
     within 1 px; none is placed behind a body, behind the eye or off
     the view.
   - Picking (unit): a click on a body's center, on a marker beside a
     body, on a path, on a label and on empty space each picks what it
     should.
   - Panel (unit): a designed plan's rows carry each item's Δv and
     duration, and a failure's reason.
   - Offscreen: labels are drawn where they were placed.
   - CPU time for labels, the panel and a pick with the published plan,
     in a window.
5. **Not here:** the camera following a ship; a selected leg's line
   highlighted; hover tooltips; editing (step 7).

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
     approaches the headroom; per channel, so the Sun goes white. (Since
     2026-10-15, on a color's largest channel, its hue kept: Realism.)
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

## Step 6, part 8 plan (2026-10-15): air against a Monte Carlo

Mars looked washed out toward its terminator. A Monte Carlo of the same
dust (plane-parallel, its first order checked against the analytic slab
and its local estimates against binned escapes) found three faults in
the shader, all of them in how light reaches and leaves the ground or
scatters more than once (single scattering agreed within 5%):
Hillaire's isotropic multiple scattering is 1.2–4× too bright for
forward-scattering dust; the ground has no skylight (near the
terminator 2.5× its direct light); and ground light the dust scatters
forward toward the eye is counted lost.

The fix, kept by the user (2026-10-15):

1. **Delta-scaling** (δ-Eddington, with Nakajima and Tanaka's
   single-scattering correction): the share f = g² of Mie scattering in
   the phase function's forward peak is taken as unscattered (δ-
   Eddington's share for Cornette–Shanks's Henyey–Greenstein factor; its
   second Legendre moment χ₂ would also take the factor 1 + cos²θ's
   symmetric 0.1, which is no peak: f 0.1 at g = 0). The scaled medium
   (Mie scattering × (1 − f), its residual phase function
   Cornette–Shanks with the residual's asymmetry) carries everything but
   single scattering:
   - multiple scattering: the table is computed in the scaled medium,
     and added along the view ray with its σ′ₛ and the scaled
     transmittance;
   - the ground: lit by the scaled direct beam and the scaled skylight,
     and seen through the scaled transmittance (forward-scattered ground
     light is ground light, blurred by less than a pixel from orbit).
   Single scattering keeps the true medium (its forward aureole, the
   night-side halo), as does what the air lets through of whatever is
   behind it (stars, other bodies, the Sun's disk). The transmittance
   table's spare channel holds the Mie column along each ray, so the
   shader gets the scaled transmittance as T · e^(f σₘ column).
2. **Skylight**: a table of the scaled sky's irradiance on the ground by
   the Sun's cosine (twilight included), per channel, integrated on the
   CPU from the same model; the ground adds albedo × it (× (1 + n·up)/2
   on relief), dimmed by eclipses.
3. **Test**: the Monte Carlo becomes `tools/atmosphere/` (with its two
   checks) and writes a fixture of reference radiances for pixels of a
   uniform Mars (albedo 0.25) at phase angles 40°, 90° and 130°; a
   sandbox test renders the same pixels and holds the air alone (black
   ground) and air with ground to it.

As built (2026-10-15), where it differs, each found against the fixture:

- Multiple scattering in the scaled medium was still isotropic, and the
  true one is not: the Sun at a cosine of 0.3 and the eye at 0.6, it
  falls ~5× from 54° to 160° from the Sun's direction (Henyey–Greenstein
  of asymmetry 0.4–0.5 fits it, as two scatterings by the dust give:
  χ₁² = 0.51). So it stays Hillaire's, in
  the true medium, spread by Henyey–Greenstein with the square of the
  local mean cosine (blended from isotropic by Mie's share, evaluated
  once a ray). Isotropic in the scaled medium, the haze was 0.57–1.34×
  the Monte Carlo's; spread, 0.78–1.10× (the Sun over 0.2 in cosine).
- The ground seen through the scaled beam alone lost what the air
  scatters out of the view and back in from beside it (diffuse ground
  light: the Monte Carlo's ground was seen by 0.89 where the beam gave
  0.70). It is seen through T(μ) + E(μ)/μ instead: by reciprocity, the
  skylight a beam from the eye would give it (for an eye inside the
  air, in proportion to the column crossed). Hillaire's ground bounce
  is dropped from the multiple scattering (it counted that light again),
  and with it the atmosphere's `ground_albedo`.
- The skylight's multiple scattering is spread the same way.
- Against the fixture, where the Sun is over 0.2 in cosine: the ground's
  part within 8%, the haze within 22%, the whole within 5% but at 130°
  phase (−19%, the haze). Nearer the terminator the Monte Carlo's slab
  dims the Sun more than the shell does (1.14× at 0.19, 1.7× at 0.11):
  there the render runs up to 1.2× it. The skylight table: within 0.4%
  of τ/2 under a thin Rayleigh sky on a flat planet.
- Cost: Earth filling a 1080p view with its maps and air, 1.05 ms (0.80
  before).
- Earth's air shares all of it, unchecked against a Monte Carlo (its
  Mie is thin, its multiple scattering near isotropic).
