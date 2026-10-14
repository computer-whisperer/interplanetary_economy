# Interplanetary economy

A game about developing the solar system's economy under real orbital
mechanics. Design: [DESIGN.md](DESIGN.md). Current milestone: the n-body
sandbox and autopilot ([M1a.md](M1a.md)); next, the creative sandbox and
the 3D map view ([M1c.md](M1c.md)); done: the M2 kill-test
([M2.md](M2.md)).

## Layout

- `crates/orbit` — the n-body sandbox ([M1a.md](M1a.md)): ephemerides
  (DE440, integrated asteroids), the integrator and gravity field, the
  Earth–Moon three-body model and its periodic-orbit families, reference
  orbits carried into the full model, station-keeping and upkeep,
  transfer design in the full field (multiple shooting and Δv
  minimization, `shooting`), cislunar legs between station orbits and low
  orbits seeded by fans (`legs::plan`), legs between planetary systems
  seeded by patched conics (`interplanetary`), landing, ascent and
  atmospheric passes (`surface`), and Lambert transfers between bodies
  with their porkchop grids and windows.
- `crates/orbit-cli` — the `orbit` command for inspecting all of the above.
- `crates/plan` — the M2 kill-test optimizer ([M2.md](M2.md)): a linear
  program that plans the whole economy for one draw of the hidden
  parameters, and a sweep that classifies the plans' development paths.
  It prices transport from the route graph (`data/routes.toml`, M1a step
  11).
- `data/` — game data: JPL's DE440 ephemeris for 2025–2080 (`de440.bin`),
  the candidate asteroids, Phobos and Deimos (`moons.toml`), the Earth–Moon
  periodic-orbit families (`orbits.toml`), the route graph the autopilot
  measured (`routes.toml`, written by `orbit routes`), and the economy
  (`economy.toml`, with sources).
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
```

Transfers are ranked by total excess speed (v∞ at departure plus v∞ at
arrival). Units are km/s for speeds and days for time; dates are
`YYYY-MM-DD` (TDB).

## The `plan` command

```sh
cargo run --release -p plan -- solve                 # central values of every parameter
cargo run --release -p plan -- solve --seed 7        # one random draw
cargo run --release -p plan -- solve --free          # no program costs or lead times
cargo run --release -p plan -- sweep --draws 100 --csv sweep.csv
```

Choosing which programs to fund takes ~10 LP solves of ~22 s each at the
default six-month steps (`--months`); a sweep runs 12 draws at a time
(`--jobs`).
