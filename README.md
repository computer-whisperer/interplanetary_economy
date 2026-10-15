# Interplanetary economy

A game about developing the solar system's economy under real orbital
mechanics. Design: [DESIGN.md](DESIGN.md). Current milestone: the creative sandbox
and the 3D map view ([M1c.md](M1c.md)); done: the n-body sandbox and
autopilot ([M1a.md](M1a.md)) and the M2 kill-test ([M2.md](M2.md)).

## Layout

- `crates/orbit` — the n-body sandbox ([M1a.md](M1a.md)): ephemerides
  (DE440, integrated asteroids), the integrator and gravity field, the
  Earth–Moon three-body model and its periodic-orbit families, reference
  orbits carried into the full model, station-keeping and upkeep,
  transfer design in the full field (multiple shooting and Δv
  minimization, `shooting`), cislunar legs between station orbits and low
  orbits seeded by fans (`legs::plan`), legs between planetary systems
  seeded by patched conics (`interplanetary`), landing, ascent and
  atmospheric passes (`surface`), Lambert transfers between bodies with
  their porkchop grids and windows, the places ships go (`places`), and
  mission plans: chains of legs, each designed from where and when the
  last arrived, with paths sampled for drawing (`mission`, `track`).
- `crates/orbit-cli` — the `orbit` command for inspecting all of the above.
- `crates/render` — the map view's renderer ([M1c.md](M1c.md)): a scene
  drawn in linear HDR (the sky's stars and Milky Way; bodies with their
  maps, atmospheres, rings and eclipses; points; markers; paths' lines
  placed by reference frames' poses), then given the viewer's eye's glare
  and tonemapped to the output's headroom; GPU timers, and a windowless
  device for tests.
- `crates/sandbox` — the creative sandbox: the map view in a window, its
  UI on [damascene](https://github.com/computer-whisperer/damascene)
  0.8, HDR output where the compositor offers it; the bodies turned by
  their rotation models, with their orbits drawn; mission plans designed
  in the background and played back, in fixed or rotating frames; their
  brightness held to published phase curves; labels, picking by click,
  the plan's panel, and settings.
- `crates/plan` — the M2 kill-test optimizer ([M2.md](M2.md)): a linear
  program that plans the whole economy for one draw of the hidden
  parameters, and a sweep that classifies the plans' development paths.
  It prices transport from the route graph (`data/routes.toml`, M1a step
  11).
- `data/` — game data: JPL's DE440 ephemeris for 2025–2080 (`de440.bin`),
  the candidate asteroids, Phobos and Deimos (`moons.toml`), the Earth–Moon
  periodic-orbit families (`orbits.toml`), the places ships go
  (`places.toml`), mission plans (`missions/`), the route graph the
  autopilot measured (`routes.toml`, written by `orbit routes`), the
  economy (`economy.toml`, with sources), the bodies' rotation models
  (`rotation.toml`, IAU 2015) and maps (`bodies/`: color, relief,
  Earth's clouds, lights and oceans, and Saturn's rings, from NASA, USGS
  and PDS, with their terms), and the sky (`sky/`: the Hipparcos stars and NASA SVS's Milky
  Way).
- `tools/ephem/` — regenerates `data/de440.bin` from JPL (`fetch.sh DIR`,
  then `python3 build.py DIR ../../data/de440.bin`; needs numpy), and the
  Horizons fixtures the tests check it against (`fixtures.sh`).
- `tools/orbits/` — regenerates `data/orbits.toml` from JPL's periodic-orbit
  catalog (`fetch.sh DIR`, then `python3 build.py DIR > ../../data/orbits.toml`;
  needs numpy and scipy).
- `tools/moons/` — regenerates `data/moons.toml` (Phobos and Deimos as
  precessing ellipses fitted to JPL Horizons; `fetch.sh DIR`, then
  `python3 build.py DIR > ../../data/moons.toml`; needs numpy and scipy),
  and the Horizons fixtures the tests check them against (`fixtures.sh`).
- `tools/rotation/` — regenerates `data/rotation.toml` from NAIF's
  pck00011 (`fetch.sh DIR`, then `python3 build.py DIR >
  ../../data/rotation.toml`), and the Horizons sub-observer points the
  tests check it against (`fixtures.sh`).
- `tools/textures/` — regenerates `data/bodies/` (`fetch.sh DIR`, ~260
  MB, then `python3 build.py DIR ../../data/bodies`; needs numpy, Pillow
  and vips), checking each map's orientation against landmarks.
- `tools/rings/` — regenerates `data/bodies/saturn/rings.toml` from
  Cassini UVIS's Gamma Crucis occultation (`fetch.sh DIR`, then `python3
  build.py DIR > ../../data/bodies/saturn/rings.toml`; needs numpy).
- `tools/shapes/` — regenerates the moons' and small bodies' shapes,
  `data/bodies/ID/shape.bin` and `data/bodies/shapes.toml`, from their
  published models, two made up (`fetch.sh DIR`, ~25 MB, then `python3
  -I build.py DIR ../../data/bodies`, ~7 minutes; needs numpy).
- `tools/atmosphere/` — a Monte Carlo of the renderer's atmospheres, and
  with it the reference radiances `sandbox`'s test `atmospheres` holds
  Mars's air to (`mc.py`, its usage in `crates/sandbox/tests/atmospheres.rs`;
  `mc.py --check` runs its own two checks; needs numpy).
- `tools/photometry/` — downloads the Horizons magnitudes the bodies'
  brightness is checked against (`fixtures.sh`), and computes their
  colors from published spectra (`colors.py`, printing the arms of
  `photometry::color`; needs numpy). `tools/color.py` holds the color
  science it shares with `tools/sky/`.
- `tools/sky/` — regenerates `data/sky/` (`fetch.sh DIR`, ~250 MB, then
  `python3 build.py DIR ../../data/sky`; needs numpy, scipy, Pillow and
  OpenEXR): the star list, the Milky Way map, and its calibration against
  the stars' fluxes.
- `tools/neas/` — regenerates `data/neas.toml` (each asteroid's physical
  data and Horizons state at 2040-01-01) from JPL Horizons and SBDB
  (`fetch.sh DIR`, then `python3 build.py DIR > ../../data/neas.toml`), and
  the Horizons fixtures the tests check the integrated asteroids against
  (`fixtures.sh`).

## The `orbit` command

```sh
cargo run --release -p orbit-cli -- bodies                 # planets and candidate asteroids
cargo run --release -p orbit-cli -- windows earth mars     # Mars windows, 2040–2076
cargo run --release -p orbit-cli -- calendar --from earth --max-v-inf 4
cargo run --release -p orbit-cli -- porkchop earth mars --start 2041-06-01 --end 2042-03-01 --png pc.png
cargo run --release -p orbit-cli -- gate OUT --every 12      # M1a's gate sweep (hours)
cargo run --release -p orbit-cli -- routes OUT...            # data/routes.toml from sweeps
cargo run --release -p orbit-cli -- mission data/missions/gateway-rotation.toml --tracks OUT
```

`mission` designs each ship's legs in turn and prints each as it is
designed (`--tracks` writes the sampled paths as CSV). The format is in
[M1c.md](M1c.md), step 1 plan; places are in `data/places.toml`.

Transfers are ranked by total excess speed (v∞ at departure plus v∞ at
arrival). Units are km/s for speeds and days for time; dates are
`YYYY-MM-DD` (TDB).

## The `sandbox` command

```sh
cargo run --release -p sandbox                              # the map view
cargo run --release -p sandbox -- --plan data/missions/gateway-rotation.toml --frame earth-moon
cargo run --release -p sandbox -- --frames 600               # close after 600 frames, print timings
cargo run --release -p sandbox -- --screenshot shot.png      # one frame offscreen
```

`--plan` designs a plan's ships in the background and draws each item as
it is ready, colored by leg, the past dimmer; the clock starts at the
first ship's start. The top bar's Plan menu opens the plans in
`data/missions/` or starts a new one, and Ctrl+S saves the plan to its
file. The top bar picks the body the camera follows and
the frame paths are drawn in (centered on the focus, or rotating with
Earth and the Moon, the Sun and Earth, or the Sun and Mars; `--frame`).
Right drag orbits, the wheel zooms (down to a tenth of the focus's
size above it, at most 10 km), space pauses, `,` and `.` step the time warp from 1 s to 1
year per second, and `[` and `]` jump between burns and events. The
timeline under the map has a lane per ship, its legs and stays as bars
and its burns and events as ticks, and above them a lane per kind of
leg between planets in the plan, brightest where departures are
cheapest (each day's least Δv by patched conics), with the clock's time
as a cursor:
the wheel over it zooms, a drag pans, a press on its axis seeks the
clock, and a click on a bar selects that item. Bodies, ships, burns and events
and station orbits are labeled where there is room. A click on a body
focuses it; on a ship, a marker, a label or a line, it selects that
item in the plan's panel (`p` shows or hides it), where a click on an
item jumps to its start. Exposure follows the focus (its sunlit side
at a fixed level), one for the whole scene; `-` and `=` shift it a stop
at a time. The settings (the gear) hold the departures from realism,
in stops, each 0 for true to life: the sky's lift over the exposure and
that of bodies seen as points, 17 by default (`--background-lift
STOPS`, `--body-lift STOPS`), and Earth's night lights' over their
radiance, 14 (`--night-lights STOPS`). At 0, as in a photograph, no
stars or planets show beside a sunlit body; with the sky's exposure +19
stops over Earth's, the Milky Way's band shows faintly; at +22,
clearly. The settings also show or hide the labels (what is drawn can
still be clicked), and each class of line: the ships' paths and events,
the station orbits, and the planets', moons' and small bodies' orbits. On an HDR output, highlights use the display's headroom.
`--focus ID --altitude KM --azimuth DEG --elevation DEG --date
YYYY-MM-DD[THH:MM] --exposure STOPS` set the starting view (the
direction about the frame's axes), for screenshots too; with a plan, a
screenshot waits for it to be designed.
`DAMASCENE_COLOR_DEBUG=1` prints the color negotiation with the
compositor.

## The `plan` command

```sh
cargo run --release -p plan -- solve --hub nrho              # central values, the NRHO as the hub
cargo run --release -p plan -- solve --hub l1 --seed 7       # one random draw
cargo run --release -p plan -- solve --hub dro --free        # no program costs or lead times
cargo run --release -p plan -- sweep --hub nrho --draws 100 --csv sweep.csv
```

`--hub` keeps one station orbit (l1, l2, nrho, dro) and leaves the others
out: the LP over the whole route graph does not solve (M1a.md, step 11).
Choosing which programs to fund takes ~6 LP solves of ~30 s each at the
default six-month steps (`--months`); a sweep runs 12 draws at a time
(`--jobs`).

## Checks

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

CI runs these on each push to main and each pull request
(`.github/workflows/ci.yml`), on Rust 1.96.0. The render and sandbox tests draw on a
GPU; CI's runners have none and draw on lavapipe, Mesa's Vulkan on the
CPU. Locally, `VK_DRIVER_FILES` set to its ICD file and
`LIBGL_ALWAYS_SOFTWARE=1` (else wgpu takes a GPU through OpenGL) do the
same; the suite takes ~3 minutes on it.

## License

The code is licensed under either of the [Apache License, Version
2.0](LICENSE-APACHE) or the [MIT license](LICENSE-MIT), at your option.
Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in this work by you, as defined in the Apache-2.0
license, shall be dual licensed as above, without any additional terms or
conditions.

The data in `data/` keep their sources' terms, named beside them (in
`data/bodies/bodies.toml`, `data/bodies/shapes.toml`, `data/sky/sky.toml`
and the other files' headers) and in the `tools/` that build them:
NASA's, JPL's, USGS's and the PDS's imagery and data (public domain, or
free to use with credit requested), the GEBCO Grid (public domain,
credit required), ESA's Hipparcos catalog and Gaia DR2 (free to use with
credit: ESA, and ESA/Gaia/DPAC), and JAXA's Ryugu model (free to use
with credit: ISAS/JAXA; resampled here). None restricts commercial use or requires derived works
to share its terms.
