# Interplanetary economy

A game about developing the solar system's economy under real orbital
mechanics. Design: [DESIGN.md](DESIGN.md).

## Layout

- `crates/orbit` — ephemerides, Lambert transfers, the Δv network, porkchop
  grids, transfer windows.
- `crates/orbit-cli` — the `orbit` command for inspecting all of the above.
- `data/` — game data: the Δv network and the candidate asteroids.
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
