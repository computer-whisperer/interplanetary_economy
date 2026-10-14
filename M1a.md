# M1a — N-body sandbox and autopilot

Status, 2026-10-13: steps 1–5 done.

- Step 1: the Δv network and its gateways are deleted; `crates/orbit` keeps
  Lambert arcs between bodies, porkchops and windows, ranked by total excess
  speed (v∞ at both ends), and the `orbit` command works on bodies.
- Step 2: the Sun, planets and Moon come from DE440 (`data/de440.bin`, 4.0
  MB, 2025–2080, built by `tools/ephem/`). Against Horizons (DE441): Earth
  within 0.1 m, the Moon from Earth 8 m, the Sun, Mars and Jupiter under 1
  mm. Heliocentric transfers connect planetary systems: for Earth, the
  Earth–Moon barycenter. The NASA handbook's Mars departures turned out to
  be from the barycenter (M0 assumed Earth's center): from it every row
  matches to 0.08%; from Earth's center, which swings ~12 m/s around it,
  up to 0.9% off.
- Step 3: `integrate` (DOP853, generated from scipy's coefficients; it takes
  exactly scipy's steps), `gravity` (point masses at DE440 positions from a
  chosen center, with indirect terms, plus Earth's J2) and `cr3bp`. Checks:
  JPL's catalog NRHO, L1 halo and DRO close within 0.7 mm after a period;
  the Moon flown as a particle stays within 1.0 km of DE440 after 30 days;
  Earth's J2 regresses a LEO node within 0.5% of secular theory. Geocentric
  and barycentric integrations differ by ~100 m over 10 days near Earth,
  because DE440's Earth also feels relativity (~1.9e-13 km/s² beyond point
  masses); the geocentric frame is the better model there.
- Step 4: asteroids start from Horizons states at 2040-01-01 and are
  integrated to 2080 when the ephemeris is built (~40 ms wall time for all
  20, in parallel; `Ephemeris::standard()` builds once per process), with
  DOP853's dense output for lookups. The Sun's relativistic term was needed:
  without it, errors reached hundreds of km in the first decade. Against
  Horizons, worst errors over 2040–2080 run from 6 km (2001 FR85) to 8,700
  km (Ryugu, after close approaches in the 2070s; Bennu 3,900 km after its
  2060 flyby), against 0.002 au (~300,000 km) for the old fits. Porkchop
  grids now sit on whole multiples of their steps, so overlapping searches
  share points (osculating elements set the default time-of-flight range,
  and they wander).
- Step 5, so far: state transition matrices for the CR3BP and the full
  field (matching finite differences; the catalog's stability indices to
  four decimals), and the orbit families (`data/orbits.toml`, `families`):
  seven Earth–Moon families from JPL's catalog, ~60–90 samples each, with a
  symmetric corrector that makes any position between two samples a
  periodic orbit (continuation where a family bends). JPL lists families by
  Jacobi constant, which folds back on itself (the L2 halos interleave the
  NRHO and large-halo branches), so the build orders members by period or
  Jacobi constant, whichever is smoother, and splits families into runs at
  gaps (members that hit the Moon are left out). 1 of 490 midpoints has no
  orbit: the smallest DROs branch there.
- Step 5c: catalog orbits carried into the full model (`reference`):
  patch points from the CR3BP orbit, each in the real Earth–Moon geometry
  of its moment, made continuous by variable-time multiple shooting (patch
  states and epochs both free; minimum-norm Newton steps halved while they
  make things worse). The NRHO, an L1 halo and a DRO hold their shapes for
  six revolutions (distances from the Moon within ~10% of the CR3BP
  orbit's). Three things mattered, found by measurement: the guess must
  come from one period of the CR3BP orbit taken modulo the period
  (integrated straight on, an unstable orbit leaves itself within ~40
  days); CR3BP time must follow the real Earth–Moon line's turning (JPL's
  fixed time unit is ~2% off the Moon's real rate); and patch epochs must
  be free (with fixed epochs the NRHO's perilune timing stalls Newton).
  The shooting's normal equations are banded, so a banded Cholesky solve
  makes long references cheap: a year of NRHO (56 revolutions) in 3.6 s.
- Step 5d: station-keeping (`stationkeeping`): navigation errors of 1 km
  and 1 cm/s (3σ, as in the NRHO studies) and 1% burn errors; at each
  maneuver the ship targets the reference's position a horizon ahead
  through its state transition matrix. The policy (every half or whole
  revolution, aiming half, one or two ahead) is chosen per orbit as the
  cheapest that holds within 500 km over 20 Monte Carlo runs: an NRHO
  must not burn at perilune, a very unstable halo cannot aim two
  revolutions ahead. Upkeep: NRHO 1.09 m/s per year (published 0.5–4),
  L1 and L2 halos 3.2 and 4.3 (published 5.3–7.4, for ARTEMIS's Lissajous
  orbits and navigation), DRO 0.64 (stable; the cost of correcting
  navigation errors). Low lunar orbits and LEO show no upkeep yet: the
  lunar gravity field and atmospheric drag are not modeled (a 100 km polar
  lunar orbit costs ~75 m/s per year in practice).

## Why

M0 gives the game patched conics: heliocentric Lambert legs, and a fixed
Δv table inside each planet's sphere of influence. The Earth–Moon L1 hub
exists only because the table lists it, and its cheap escape is a fit to
two published numbers (DESIGN.md, M0). That fit alone moved M2's results,
and most of the economy's cost is the climb to L1, so the numbers that
matter most are the ones typed in by hand.

M1a replaces that with an n-body sandbox: ships are integrated in the
gravity of the real Sun, planets and Moon, and fly by flight rules that
an autopilot sets. Lagrange-point orbits, the natural pathways between
them, cheap escapes and each orbit's station-keeping cost come from the
physics; where hubs go is for the economy and the player to find.

The physics is general from the start. The autopilot grows by playbook
entries, starting with the transfers the MVP needs; how to present the
rest of the option space to the player is open (DESIGN.md, Open
questions).

## Physics

**Massive bodies.** JPL's DE440 ephemeris (public domain), trimmed to
2025–2080 (from 2025 so the NASA handbook check still runs): the Sun, the
planetary system barycenters, the Earth–Moon barycenter and the Moon, as
DE440's own Chebyshev series (4.0 MB). Earth follows from the barycenter
and the Moon by the Earth/Moon mass ratio. `tools/ephem/` extracts them. Phobos and
Deimos come from JPL as well (Horizons, or its Mars satellite
ephemeris). A fictional system would integrate its bodies once to make
the same tables.

**Field.** Point masses, plus J2 for Earth and Mars. Each ship includes
only the bodies whose pull matters where it is (a threshold on their
tidal acceleration); near Earth that is the Sun, Earth and Moon. A lunar
gravity field (mascons make most low lunar orbits decay) and solar
radiation pressure are later additions.

**Ships** are massless particles, each integrated on its own by an
adaptive 8th-order Runge–Kutta (DOP853). A ship's arc is integrated when
it is planned and replayed by the simulation, so the path drawn on the
map is the path flown, and speeding up game time costs nothing. Ships
are independent, so they integrate in parallel.

**Determinism.** Chaotic orbits amplify any difference, so runs must be
bit-identical across machines. The inner loop uses only add, multiply,
divide and square root (Chebyshev evaluation needs no trigonometry), and
the step-size controller's power function comes from a pure-Rust libm,
not the platform's.

**NEAs** become particles in the same field, started from Horizons states
at 2040-01-01, with the Sun's relativistic term (it moves an asteroid by
hundreds of km per decade). This replaces M0's piecewise two-body fits and
their errors near close Earth flybys (Bennu 0.016 au, 2000 SG344 0.010 au,
2017 SV19 0.006 au).

**Earth–Moon CR3BP mode** (the Moon on a circular orbit, ship massless):
the model JPL's orbit catalog uses, for importing orbits and for checks.

## Flight rules

Closed-loop guidance runs at discrete events, not continuously:

- **Burn:** impulsive in M1a; finite burns, and their gravity losses,
  later.
- **Course correction:** re-target from the current state at scheduled
  times.
- **Station-keeping:** hold a ship near a reference orbit. The Δv per
  year is that orbit's upkeep, which the economy pays for any station
  there.
- **Aerocapture and entry:** a pass through an atmosphere changes
  velocity by a simple model, for aero-capable ships, as now.
- **Landing and ascent:** Δv from a gravity-loss model per body, fitted
  to published values, not an integrated powered descent.
- Later: navigation and burn errors, so that course corrections cost
  something; continuous guidance for low thrust.

Station-keeping is required, not optional. Measured in the CR3BP
(2026-10-13), a 1 m offset, uncontrolled, grows:

| Orbit | 1 m becomes 1 km after | 1,000 km after |
|---|---|---|
| L1 halo (12-day period, JPL stability index 36) | 19 days | 40 days |
| Near-rectilinear halo orbit (NRHO; 6.6-day period, index 1.27) | 63 days | — |
| Distant retrograde orbit (DRO; 14-day period, index 1.0) | stays under 90 m after 140 days | — |

Even the L1 reference orbit, integrated alone at full precision, leaves
the halo within a few months.

## Autopilot

A **leg** is a start (a ship's state, or an orbit), a goal (an orbit, a
rendezvous, a surface site, or an escape at a given v∞) and constraints
(departure window, longest time, most Δv). The autopilot returns options
along the Δv-versus-time frontier; each is a reference trajectory, a
burn schedule and the flight rules to fly it. A mission plan is a chain
of legs.

**Orbits** are members of known families: low orbits about any body,
Lyapunov and halo orbits about the Lagrange points (the NRHO is an L2
halo), DROs about the Moon. Members come from JPL's Three-Body Periodic Orbits catalog
(`ssd-api.jpl.nasa.gov/periodic_orbits.api`; the Earth–Moon families
include the 6.6-day NRHO) and are carried into the full model by
multiple shooting over several revolutions, where they become
quasi-periodic and station-keeping holds them.

**Playbook** (starting guesses):

1. Lambert arcs and porkchops (M0) for heliocentric legs.
2. Two-burn transfers between orbits in one system: a porkchop-style grid
   over departure point × arrival point × time of flight.
3. The natural pathways into and out of Lagrange-point orbits (their
   stable and unstable manifolds), computed numerically from each orbit's
   monodromy matrix.
4. Ballistic lunar transfers: LEO to lunar orbit in ~3–4 months for less
   Δv, using the Sun (CAPSTONE flew one).
5. Lunar flybys for Earth escape and capture.

**Correction.** Multiple shooting with the state transition matrix
(Newton's method) makes a guess fly in the sandbox; Δv is then minimized
under the leg's constraints. A standing route re-plans each departure
starting from the previous one.

Entries 1–3 are textbook. Entries 4 and 5 are research-grade, though well
studied; until they work, Earth escape uses entries 1–3.

## Measured costs (2026-10-13)

Evaluations per simulated day, from scipy's DOP853 at relative tolerance
1e-11 (under a millimeter of drift per day); cost per evaluation from a
Rust benchmark with DE-style Chebyshev tables: ~90 ns for the Sun, Earth
and Moon, ~310 ns for 11 bodies; with the state transition matrix ~160
and ~470 ns. Both were throwaway measurements.

On the real engine (step 3, `examples/field_cost.rs`): 239 ns per
evaluation for the Sun, Earth and Moon, 537 ns for all ten bodies; LEO
takes 6,960 evaluations per day, 3.8 ms per simulated day, and a cislunar
coast 63, 0.04 ms. The evaluation is ~2.5× the benchmark's because Earth
and the Moon each re-evaluate the Earth–Moon barycenter and the Moon's
offset, and the center's position is looked up again; evaluating each
series once per time would recover most of it.

| Orbit | Evaluations per day | CPU per ship-day |
|---|---|---|
| LEO, 400 km | 7,000 | 0.6 ms |
| Low lunar orbit, 100 km | 5,600 | 0.5 ms |
| NRHO | 250 | 22 µs |
| L1 halo | 56 | 5 µs |
| DRO | 47 | 4 µs |
| Interplanetary cruise | 6 | 2 µs |

A 250-day Earth departure took ~1,500 evaluations, half of them in the
first three days, and drifted 16 m. A cislunar trip should take under a
millisecond to integrate and, by estimate, ~10 ms to design from a good
guess (about ten Newton iterations). M2's central world lifts ~150 kt/yr
off the lunar surface in the 2050s, ~3,000 lander ascents a year:
designing every trip from scratch would cost on the order of a minute of
CPU per game year.

## Verification

Physics:

- Catalog orbits close in CR3BP mode (measured: within millimeters after
  one period).
- Body positions match Horizons (they are the same data; this checks the
  extraction).
- Real spacecraft in Horizons are reproduced over coasts without
  maneuvers: candidates are CAPSTONE in the NRHO and JWST at Sun–Earth L2
  (solar radiation pressure is large for JWST, so it is a check only once
  that is modeled).
- NEAs integrated from 2040 match Horizons through 2076, including the
  close Earth flybys.

Autopilot, against published values:

- staging orbits compared on transfer cost: Whitley & Martinez, "Options
  for Staging Orbits in Cis-Lunar Space" (IEEE Aerospace, 2016)
- NRHO insertion and station-keeping (Artemis/Gateway studies)
- CAPSTONE's ballistic lunar transfer
- lunar flyby escapes (McElrath et al. 2012)
- Mars departure C3 (NASA/TM-2010-216764, M0's check, now through the
  sandbox)

## Gate

The core set of legs:

- LEO to and from EML1 and EML2 halos, the NRHO, a DRO and low lunar
  orbit
- among those orbits
- those orbits to and from the lunar south pole
- those orbits and LEO to Earth escape at a given v∞, and capture back
- low Mars orbit to and from Phobos; Earth-system orbits to Mars orbit
  and to NEA rendezvous

Pass if, over departure dates every 5 days across 2040–2075:

1. at least 99% of designs converge and meet their constraints;
2. Δv is within 10% of published values where they exist;
3. re-planning a standing route's departure takes ≤ 0.1 s (median), and
   a design from the playbook ≤ 10 s;
4. every station orbit has a computed upkeep, within published ranges
   where they exist.

Fail: fall back to tables computed once from the physics (the Earth–Moon
CR3BP, then with the Sun): places and legs priced by physics, without
per-trip flights in the sandbox. M1b then reads tables.

## What M1a deletes

M0's patched-conic layer: `data/network.toml`, `network.rs` and the
gateways in `transfer.rs` (the lunar-assist fit, Oberth burns at nodes),
the Kepler-element planets, and the NEA fits (`tools/neas/`, the elements
in `data/neas.toml`). Each piece is deleted at the start of the step that
replaces it. Lambert, porkchop grids, window detection and time handling
stay, as playbook entry 1.

`crates/plan` (M2's planner) reads the old network, so it leaves the
workspace when the network goes, and comes back reading a route graph
that the autopilot exports (step 11).

## Steps

1. Delete the network and its gateways; take `crates/plan` out of the
   workspace.
2. Replace the Kepler-element planets: DE440 extraction (`tools/`), the
   table reader, the check against Horizons.
3. Integrator, field and CR3BP mode; catalog orbits close.
4. Replace the NEA fits: NEAs as particles, checked against Horizons.
5. Orbit catalog import, continuation into the full model,
   station-keeping and upkeep.
6. Correction: multiple shooting, Δv minimization; two-burn transfers
   among orbits and to LEO.
7. Heliocentric legs from Lambert guesses; the Mars C3 check through the
   sandbox.
8. Landing, ascent and aerocapture rules.
9. Lunar flyby escapes and ballistic lunar transfers.
10. The gate sweep.
11. Route graph export; `crates/plan` reads it; rerun M2's central world
    to see where the hub goes.
