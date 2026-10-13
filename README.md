# Interplanetary economy

A game about developing the solar system's economy under real orbital
mechanics. Design: [DESIGN.md](DESIGN.md).

## Layout

- `crates/orbit` — ephemerides, Lambert transfers, the Δv network, porkchop
  grids, transfer windows.
- `crates/orbit-cli` — the `orbit` command for inspecting all of the above.
- `crates/plan` — the M2 kill-test optimizer ([M2.md](M2.md)): a linear
  program that plans the whole economy for one draw of the hidden
  parameters, and a sweep that classifies the plans' development paths.
- `data/` — game data: the Δv network, the candidate asteroids, and the
  economy (`economy.toml`, with sources).
- `tools/neas/` — regenerates `data/neas.toml` from JPL Horizons and SBDB
  (`fetch.sh DIR`, then `python3 build.py DIR > ../../data/neas.toml`;
  needs numpy and scipy).

## The `orbit` command

```sh
cargo run --release -p orbit-cli -- nodes                  # places ships can be
cargo run --release -p orbit-cli -- path leo moon-south-pole
cargo run --release -p orbit-cli -- windows leo lmo        # Mars windows, 2040–2076
cargo run --release -p orbit-cli -- calendar --from eml1 --aero --max-dv 4
cargo run --release -p orbit-cli -- porkchop leo lmo --start 2041-06-01 --end 2042-03-01 --png pc.png
```

Units are km/s for Δv and days for time; dates are `YYYY-MM-DD` (TDB).

## The `plan` command

```sh
cargo run --release -p plan -- solve                 # central values of every parameter
cargo run --release -p plan -- solve --seed 7        # one random draw
cargo run --release -p plan -- sweep --draws 100 --csv sweep.csv
```

A quarterly solve takes ~30 s; a sweep runs 12 solves at a time
(`--jobs`), several minutes each.
