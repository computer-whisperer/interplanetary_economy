# M1a — N-body sandbox and autopilot

Status, 2026-10-14: steps 1–8 done; step 9 started, its open part
deferred; step 10 done: the gate passes for most legs and fails for legs
between stations and two kinds of departures to other planets (below);
step 11 next, then M1c (M1c.md).

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

- Step 6: `shooting` designs a path in the full field: nodes joined by
  integrated arcs, burns at some of them, ends on a station's reference
  orbit or at the apsis of a low circular orbit (LEO, plane free; low
  lunar orbit, plane free or at a set inclination). Minimum-norm Newton
  steps make it continuous; the generalized reduced gradient then
  minimizes total Δv over the few free unknowns, re-solving continuity at
  each trial point so every iterate is flyable. What mattered, measured:
  each node scaled by the body pulling hardest there (one Earth–Moon scale
  left LEO problems unsolvable); J2 in the state transition matrix
  (without it LEO sensitivities were ~20% off); a feasible path rather
  than SQP (9 iterations against 51 on a Hohmann rendezvous); times and
  inner-burn velocities as the free unknowns (stiff LEO velocities stalled
  the search); a Hessian from differences, measured again when a step
  shows the curvature has moved (it grows ~700× along a flyby burn); and
  lunar flybys as periapsis burns no lower than 100 km, held there when
  the search goes lower. Total Δv falls slowly with duration (a few m/s
  per day), so a free-duration search crawls: legs are designed at set
  durations, reached by continuation (steps of 0.25 days, halved when one
  fails). A departure window is a cost penalty (1 km/s per day squared
  outside it), then the departure is held at the nearer edge: as a hard
  wall it trapped searches, and with no penalty a low-orbit-to-low-orbit
  search drifted a week out, too far to pull back. A flyby held at its
  floor is also the periapsis of the arc leaving it (a burn with an
  inward part took that arc up to 0.17 km lower, found in review); seeds
  get nodes 1 and 4 hours from each end of each link, without which the
  DRO's flyby designs failed.
  `legs::plan` designs a leg between any two ends at given durations,
  seeded by fans of burns flown in parallel in the Sun–Earth–Moon field:
  from a station to a periapsis near a low orbit, directly or via a
  powered lunar flyby (a second fan from the flyby, burns within 60° of
  the velocity there); to a station, the closest approach to it; between
  two low orbits, from periapses of the far one. Seeds are kept within a
  few days of the durations wanted, designed at their own durations, and
  the nearest carried by continuation. Against published values, at their
  durations (m/s; `tests/transfers.rs`):

  | Leg | Here | Published |
  |---|---|---|
  | NRHO to polar 100 km lunar orbit, 0.5 d | 675 | 730 |
  | L2 halo to it, 3 d / DRO to it, 4 d | 786 / 814 | 800 / 830 |
  | LEO to NRHO after leaving LEO, 5.1 d / 5.4 d | 395 / 388 | 428.5 / 409.1 |
  | NRHO to LEO before capture, 5 d | 415 | 411.5 |
  | L2 halo round trip, 6.5 d each way | 754 | 811 |
  | LEO (200 km) to L1 halo, 4.1 d / 7.81 d | 3,632 / 3,523 | 3,660 / 3,513 |
  | Leaving a 185 km LEO for the Moon, 4.4 d | 3,141 | 3,142 |
  | Low lunar orbit round trip, 3.5 d each way | 1,678 | 1,800+ |
  | LEO to DRO after leaving LEO, 9 d | 304 | 344 |

  The first eight are Whitley & Martinez (2016), the L1 halo arXiv
  2502.11694's table 5, the departure Lunar Prospector's, the DRO ESA's
  Artemis I plan. The DRO is the outlier: 15% below Artemis I's plan and
  26% below Whitley & Martinez's round trip (957 m/s, a 21-day mission),
  for a DRO of the same size (65,000–92,000 km from the Moon); both plans
  carry constraints this search does not. Between stations there are no
  published fast transfers (the literature's impulsive NRHO–DRO designs
  take 40–97 days through other families); here NRHO to DRO costs 552 m/s
  and L1 halo to L2 halo 497, in 10 days. A plan takes 0.1–3 s on 24
  cores, LEO to the DRO 12 s. Field cost: evaluating the Earth–Moon series once per time took
  an evaluation from 254 to 166 ns; with the planets left out (their tides
  in cislunar space are ~1e-12 km/s², meters per leg) the transfer tests
  went from 120 s to 23 s with the same results. Earth's pole had been
  mirrored through the ecliptic (Earth's J2 was 47° off: harmless at lunar
  distances, not in LEO); the Moon's pole, by Cassini's laws, is within
  0.07° of the IAU's. Left for later steps: re-planning a standing route
  from its previous departure, and the reliability and timing sweep (step
  10).

- Step 7: arcs integrate in the frame of the body whose sphere of
  influence they start in (Earth, Mars, else the barycenter), nodes in one
  frame; references record theirs. `interplanetary` seeds legs between
  planetary systems by patched conics: the heliocentric Lambert arc, and a
  hyperbola at each planet with the arc's excess velocity there, lasting
  until the ship nears the planet's sphere of influence and patched to the
  arc where it ends by Newton's method on the excess velocities. (Patched
  at the planets' centers, a 0.56 km/s departure's arc missed by 1.2
  million km after the 19 days the ship spends leaving; a plain update of
  the excess velocities diverged.) New ends: a small body to meet, and an
  escape or capture with a given excess velocity (osculating, days out).
  The NASA handbook's Mars departures flown LEO (400 km) to a 400 km Mars
  orbit on its dates: leaving LEO 3,576, 3,550, 3,523 and 3,583 m/s (2031
  and 2033 Type I, 2033 and 2043 Type II) against 3,584, 3,558, 3,529 and
  3,582 from the handbook's C3, within 0.25%; into Mars orbit within 0.2%
  of the same arc's patched conics (Mars's J2 included since step 8). (In C3 itself, more sensitive, 0.03–2%
  below the handbook's: Earth swings ~12 m/s about the barycenter the
  handbook departs from.) Escape to the 2033 arc's excess velocity, 3,552.6
  m/s against a hyperbola's 3,557.5; LEO to 2000 SG344 (2066-10-26, 324
  days), 4,038 m/s against patched conics' 4,021. A Mars design takes
  0.2–4 s. Left for later steps: leaving station orbits for other planets
  (lunar flyby escapes, step 9), Phobos and Deimos (their ephemerides and
  Mars's figure, with step 8's landings), and searching dates (step 10).

- Step 8: `surface` gives landing and ascent as the ideal two-burn Δv
  between a low orbit and the surface plus a loss per body and direction,
  fitted to flown values (Moon: Apollo 11's 23 + 2,081 m/s down from 111
  km and 1,846 m/s up, so 0.374 and 0.159 km/s of loss; Mars: 4,200 m/s up
  to 100 km, Palaszewski, so 0.594), and an atmosphere's passes for
  aero-capable ships: aerocapture at a pass altitude then a periapsis
  raise, entry then a terminal descent (674 m/s at Mars, Palaszewski),
  each failing above a speed limit (Earth 12.5, Mars 8 km/s: this game's
  assumptions). `End::Atmosphere` ends a leg there. LEO to Mars by
  aerocapture on the 2033 arc: 3,550 + 82 m/s (2,820 m/s to capture
  propulsively); NRHO to LEO by aerocapture in 5 days: 229 + 188 + 97 m/s
  (M0's fit had 0.77 km/s from L1). Phobos and Deimos are precessing
  ellipses about Mars (node, periapsis and a Laplace pole fitted to
  Horizons over 2025–2080, `tools/moons/`): within 105 and 124 km of
  Horizons off the fit's grid. A fit was chosen over integrating them like
  the NEAs: Phobos's 7.65-hour orbit would cost seconds of integration at
  every load, and Mars's unmodeled J4 would turn its node ~6° in 40 years.
  (Daily samples alias the moons' mean motions; the build unwraps them day
  by day.) Mars has a figure (J2 of MRO120D, the IAU 2000 pole; the 2015
  values as found quoted gave an obliquity of 23.9°, not 25.19°). Low Mars
  orbit (300 km) to Phobos and back: 1,205 m/s each way in 2.2 hours,
  Hohmann's 1,206, seeded by Lambert arcs about Mars.

- Step 9, so far: escapes from a station's orbit at a given excess
  velocity, and captures into one (`legs::escape_seeds`, through `plan`):
  fans followed until 1.5 million km from Earth, directly or dropping to a
  low perigee (directly or by a powered lunar flyby, the chains the LEO
  returns use) and burning there along the velocity to the excess speed
  wanted. Measured from the L2 halo (14.5 days): burns up to 0.1 km/s
  escape at 0.03–0.34 km/s; 0.3 km/s along Earth's heliocentric velocity
  costs 255 m/s in 40 days; 1 km/s only directly, ~760–790 m/s (falling
  from L2 to a perigee takes ~0.8 km/s without a flyby). Open: with the
  lunar flyby to perigee, the asymptotes reached point 45–57° from a given
  direction even over a month of departures (the perigee's place follows
  the Moon's phase), so larger excess velocities in a chosen direction
  need more: a second lunar flyby to turn the asymptote, multi-flyby
  sequences as in McElrath et al., or a leg defined by excess speed alone.
  Ballistic lunar transfers not started.
  Deferred (2026-10-13): the rest of step 9 waits until the economy shows
  it cares. Until then, steps 10 and 11 define escapes from station
  orbits, and captures into them, by excess speed alone, in any
  direction. That is a stand-in: it assumes a departure can be phased to
  suit the planetary window, which overstates what is reachable. Step
  11's M2 rerun shows how much those legs move the economy.

- Step 10: the gate sweep, `orbit gate` (`crates/orbit-cli/src/gate.rs`),
  with three library pieces: `End::Hyperbola` (escape or capture by
  excess speed alone, step 9's stand-in), `legs::replan` (a route's
  earlier design moved to a later departure) and `interplanetary::plan`.
  What ran, on 24 cores (2026-10-14):
  - cold plans of the 50 cislunar and Mars-system legs at departures
    every 60 days over 2040–2075: 11,440 plans in 2.6 h. Every 5 days,
    as the gate asks, would take ~31 h;
  - standing routes of the same legs through 2045 and 2065, a departure
    each station revolution (or every 5 days): ~4,300 departures;
  - upkeep of each station in 37 yearly pieces;
  - LEO to low Mars orbit (burning, and by aerocapture) and to the 20
    NEAs, every 5 days through each window: 8,616 plans in 67 min.

  Against the gate:
  1. **Convergence (99%).**
     - The 38 legs other than between stations: 99.83% (15 failures in
       8,800). Pass.
     - Between stations: 88.5%, from 63% (L2 halo to L1 halo) to 100%
       (DRO to NRHO). Fail. Two orbits' relative phase repeats every 12
       days (the NRHO and a halo) to ~400 (the L2 halo and the DRO,
       whose periods nearly match), and transfers of 6–14 days leaving
       within one revolution cannot reach every phase. The published
       transfers between these orbits take 40–97 days, for this reason.
       Fixing it is playbook work: wider windows, or transfers over
       several revolutions.
     - Between planets: 96.9%, counting as failures six designs that
       converged at ~110 km/s (~52 km/s burns at each end). Fail, in two
       kinds of departure. Below 1 km/s of excess speed, 22% fail (53%
       when also near 180°): the hyperbola lingers for weeks where the
       Moon pulls hardest, and patched conics seed it badly. Within 30°
       of a 180° transfer, 8% fail: the arc's plane is ill-defined, the
       case broken-plane transfers exist for. The other 7,388 departures
       failed 3 times. Converged designs sit at 0.999× patched conics
       (median); all 8 above 1.1× are in these two kinds.
  2. **Δv within 10% of published values**, share of dates: LEO to the
     L1 halo 100%, to the NRHO 96%, NRHO to LEO 98%, LEO to low lunar
     orbit 100%, the NRHO, L2 halo and DRO to low lunar orbit 95–100%.
     LEO to the DRO 36%: median 308 m/s against Artemis I's 344, step
     6's mismatch.
  3. **Time.** Plans run in parallel share the cores, which inflates
     their times.
     - Re-planning: the median over all re-plans is 0.05–0.06 s,
       measured in the sweep, where 50 routes shared the cores (six legs
       run alone re-planned 1.1–3.8× faster). Pass overall. Per leg,
       alone: 0.013 s for NRHO to low lunar orbit; 0.23–0.32 s for LEO to
       the NRHO, the L2 halo to LEO, the NRHO to the L2 halo and DRO
       escapes; 0.93 s for LEO to the DRO. Legs along long cislunar arcs
       fail per leg.
     - Re-plans succeed on 94.5% of departures for most legs, at the
       cold plans' Δv (median ratio 1.001); the rest, including each
       route's first, plan cold. Between stations 42%, and 6% of
       departures fail outright; escapes from the DRO and the L2 halo
       60%.
     - Cold designs (timing passes from earlier runs): 0.07–5 s for most
       legs; 5–32 s between stations, a fail. Between planets the sweep
       ran plans in parallel, so their times are unmeasured; step 7 had
       0.2–4 s for Mars.
  4. **Upkeep.** Every station held its orbit through all 37 years. L1
     halo 3.1–3.7 m/s per year, L2 halo 4.5–5.0, DRO 0.60–0.78: within
     their ranges. NRHO median 2.3, range 0.8–5.4: 4 of 37 years above
     the published 0.5–4.

  Also measured: the cold planner sometimes lands in a costlier family
  of paths. 1.5% of designs on most legs, 3.6% between stations, cost
  more than 1.25× their leg's median, up to 3× (NRHO to low lunar orbit:
  2,016 m/s against 697). Standing routes guard against it: a re-plan
  above 1.25× the route's recent median is checked against a cold plan,
  and the cheaper flies. Fixed on the way: a year of station-keeping
  took hours once a policy lost the orbit (the ship flew on through the
  Moon's point mass); a run now ends 500 km off, which already counted
  as lost.

  The ~110 km/s designs came from their seeds: patching the hyperbolas
  to the heliocentric arc by Newton's method diverged on departures below
  ~1 km/s, to excess speeds up to 115 km/s against the arc's 0.4–0.8.
  Halving steps until the mismatch shrinks drove those excess speeds
  toward zero instead, and lost the test's 2000 SG344 design, so the
  seeds stay as they are. `interplanetary::plan` now rejects a design
  costing over 3× the leg's patched conics (`interplanetary::patched`;
  designs that keep their family cost 0.85–1.9×), which removes the six.

  Left open: legs between stations; slow and near-180° departures to
  other planets, including how to seed slow ones; per-leg re-planning
  time on long arcs; LEO to the DRO. Step 11 prices legs
  from these sweeps, as tables; whether M1b flies ships per trip for the
  legs that pass is decided before M1b.

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

### Step 10 plan (2026-10-13)

The sweep designs every core leg at departures every 5 days from
2040-01-01 to 2075-12-31, and reports for each leg: the share of
designs that converge, Δv against published values, and the time to
re-plan and to design cold. It also reports each station orbit's upkeep
across the span.

1. **Legs.** The cislunar field is the tests' (the Sun, Earth with J2,
   the Moon). Its places:
   - station orbits: the L1 halo (12 days), L2 halo (14.5), NRHO (9:2)
     and DRO (14);
   - LEO at 400 km, free plane, or the publication's altitude where a
     leg checks one;
   - polar low lunar orbit at 100 km.

   The legs:
   - LEO to and from each station and low lunar orbit (10);
   - each station to and from low lunar orbit (8);
   - station to station (12);
   - each station and low lunar orbit to and from escape at 1 and 3 km/s
     (20; by excess speed alone, step 9's stand-in).

   The south pole is the low lunar orbit legs plus `surface`'s rules.
   Durations are the published legs' (steps 6–8). Between stations a leg
   takes the cheapest of 6, 8, 10, 12 and 14 days: at 10 days alone, too
   many dates were badly phased.

   In the Mars system, low Mars orbit to and from Phobos (2).

   Between systems, LEO to low Mars orbit (burning, and by aerocapture)
   and LEO to each candidate NEA. Departures are every 5 days within each
   window (`windows`), each at the porkchop's best time of flight for
   that day.
2. **A departure date d means leaving within [d, d + W]**, where W is one
   revolution of the station the leg leaves or reaches (5 days between
   low orbits). Every date can then reach every phase of the station.
3. **References** span 2040–2075 in yearly pieces overlapping by 60 days,
   each carried from the catalog by `continue_orbit`. A leg uses the
   piece that covers its window and duration. One continuous reference
   per station, which the game needs, is M1b's.
4. **Re-planning** (`legs::replan`, step 10b):
   - An earlier departure's design moves later by whole revolutions of
     the station end. Each node is carried with the Earth–Moon line:
     relative to the Moon near the Moon, to Earth near Earth, and in the
     pulsating frame between, blended across the zones (or with Phobos in
     the Mars system).
   - It is then designed at the same duration. Of a route's recent
     designs, the one moved nearest a whole number of synodic months
     (cheapest breaking ties) is tried first, then the next.
   - Measured:
     - NRHO to low lunar orbit: 10–20 ms per departure, at the cold
       plan's Δv.
     - L2 halo to LEO a synodic month on: 0.28 s, within 0.01% of the
       cold plan.
     - Shifted by half a month, legs that reach past the Moon may not
       converge. There the seed's family of paths ends: closing the
       seed's gaps in stages folds at λ ≈ 0.6.
   - A route plans cold when re-planning fails.
5. **Two measurements.**
   - **Cold plans**, at departures every 5 days, each leaving within one
     revolution. They give the playbook's reliability, its design time,
     and Δv against published values. (Run every 60 days: every 5 would
     take ~31 h. Design times come from a pass of plans run alone.)
   - **Standing routes**, one per leg, with a departure each revolution
     of the station (every 5 days between low orbits), through sample
     years (2045 and 2065). The windows tile, so each departure is a new
     ship. Each is re-planned from the route's recent designs, and
     planned cold when that fails. They give the re-planning time, how
     often it succeeds, and its Δv against the cold plans.
6. **Escape by excess speed** (`End::Hyperbola`, step 10a): a periapsis at 200 km
   where a burn along the velocity reaches the hyperbola's speed for the
   v∞ wanted. It is seeded as a low orbit there, so the lunar-flyby
   chains to perigee apply.
7. **Plans between systems in the library**: the tests' helper, which
   tries planes of the hyperbolas, becomes `interplanetary::plan`.
8. **Upkeep**: the station-keeping Monte Carlo at the start of each year,
   for each station.
9. **Output**: `orbit gate` writes one CSV row per leg and date
   (`cold.csv`, `routes.csv`, `upkeep.csv`, `between.csv`), and a
   summary per leg (`summary.txt`):
   - the share converged;
   - Δv median and range, against the published value;
   - the median re-plan time;
   - the 95th-percentile and longest cold design.

   The pass criteria are the gate's; results go into the status above.

### Step 9 plan (2026-10-13)

1. Escapes from station orbits at a given excess velocity, and captures
   into them: fans from the orbit (backward for captures) followed until
   the ship is ~1.5 million km from Earth, keeping those whose osculating
   excess velocity there is near the one wanted (unpowered lunar flybys
   come with the field); designed to `End::Excess`. Check: McElrath et
   al. (2012), ~0.14 km/s from Earth–Moon L1/L2 to escape, lunar flybys
   alone reaching C3 ≈ 2 km²/s² in ~2 months.
2. Ballistic lunar transfers: LEO to lunar orbits in 3–4 months through
   the Sun's tide (CAPSTONE flew one to the NRHO): the backward fans from
   the station already reach LEO when let run that long; arcs far from
   Earth and the Moon lengthen with the local dynamical time. Check: the
   arrival burns against direct transfers', and published values.

### Step 8 plan (2026-10-13)

1. Landing and ascent (`surface`): between a low circular orbit and rest
   on the surface (its rotation included), Δv is the ideal two-burn
   transfer plus a loss per body and direction, fitted to flown values:
   the Moon from Apollo 11 (descent 23 + 2,081 m/s from 111 km, ascent
   1,846 m/s to 17 × 83 km; NASA TM X-58040), Mars from published ascent
   vehicles; small bodies and Phobos none. The gate's south-pole legs are a
   station orbit to polar low lunar orbit (step 6) then down.
2. Aerocapture and entry, for aero-capable ships: an arrival whose
   hyperbola's periapsis is at the atmosphere's interface, which the pass
   brakes into an orbit up to the low orbit (then a periapsis raise, Δv
   from the conic), or (entry) to the surface with a terminal propulsive
   Δv; each with an entry-speed limit at the interface.
3. Phobos and Deimos as particles about Mars from Horizons states (like
   the NEAs, with Mars's J2, IAU pole), checked against Horizons; low Mars
   orbit to and from Phobos.

### Step 7 plan (2026-10-13)

Interplanetary legs fly in a barycentric field, but integrating near LEO
in barycentric coordinates (~1.5e8 km) leaves ~15 cm of noise per step,
far above the 0.07 mm continuity wanted there. So:

1. Frames. Each arc integrates in the frame of the body whose sphere of
   influence its first node is in (Earth, Mars, else the solar-system
   barycenter); nodes keep one global frame. An arc's end and state
   transition matrix carry over unchanged if its equations of motion are
   written globally: f_global(t, x) = f_local(t, x - B(t)) + B'(t), B the
   local center's state. References record their frame, so a station's
   orbit is an end in any field.
2. Mars gets a figure (J2, radius, IAU pole) for low Mars orbit; small
   bodies become ends (rendezvous with the ephemeris state).
3. Seeds from patched conics: a heliocentric Lambert arc between the
   planetary systems (`transfer`), a departure hyperbola from the low orbit
   with that excess velocity (its periapsis on the cone the asymptote
   allows, the plane free), and an arrival hyperbola likewise; nodes along
   each, multiple shooting closes the patches.
4. Escape at a given excess velocity: an end where the ship's osculating
   two-body excess velocity about Earth equals it, some days out.
5. Checks: the NASA handbook's Mars departures flown LEO to low Mars orbit
   (departure C3 from the LEO burn against the handbook's), and a NEA
   rendezvous against NHATS.
