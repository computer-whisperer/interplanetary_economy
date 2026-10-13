# Interplanetary Economy — MVP Design Sketch

Status: draft, 2026-10-12. Working title TBD.

## Vision (full game, for context)

The player runs a chartered public institution, "the Authority", whose
statutory mission is to lower the cost of access to the solar system. It starts
as a coherent, well-funded NASA-like agency living on appropriations. Over the
game it can accrete powers until it resembles the East India Company:
collecting tolls, granting concessions, issuing debt, governing settlements.
**Institutional powers replace the tech tree as the progression spine.**
Technology mostly arrives from Earth on its own schedule.

The world is real orbital mechanics plus the rocket equation plus a fictional
but plausible future economy. Decisions are mostly:

- **where**: which sites to develop, which hubs to put depots at
- **when**: which window, how much lead time to buy
- **how**: build it yourself or contract it out

and not "which tech next".

Beyond the MVP, the full arc includes:

- the Authority's later phase as a chartered company
- solar-electric cargo tugs and cyclers
- the outer system
- settlements with their own politics, up to independence movements
- Mars terraforming as a huge demand for imported nitrogen and water, with
  Titan's nitrogen as the natural source
- Earth pressure to regulate or nationalize an Authority that has grown too
  powerful

## What the MVP must answer

1. **Lead time.** Do windows and transit times make decisions feel like
   planning rather than waiting?
2. **Option space.** Do several development paths (Moon-first, NEO-first,
   Mars-first) compete under plausible parameters, or does one dominate?
3. **Demand.** Does induced third-party traffic (missions that fly only when
   the Authority's infrastructure makes them affordable) work as the economy's
   engine?
4. **Indirection.** Does contracting add real decisions, or is it strictly
   better or strictly worse than building directly?
5. **Progression.** Does gaining institutional powers work as a replacement
   for a tech tree?

Q2 is answered headless by an optimizer before any UI exists (M2). M2's
answer: under any one set of rules one path dominates, but which one the
rules decide (methalox only: asteroids; both propellants: the Moon), and
later decisions differ by world; see M2.md, Verdict. Q4 is
answered by A/B comparison: the game is complete with direct building alone,
and contracts are a separable layer (M4).

## Scenario

- 2040-01-01 to 2075-12-31: 36 years, about 17 Mars windows.
- Real ephemerides and real Δv. Fictional economy: reusable heavy launch
  exists, and the Earth launch price per kg to LEO follows a falling, uncertain
  curve drawn per game. The median runs from $250/kg in 2040 to $100/kg in
  2075 (`data/economy.toml`, from Citi 2022, Terzi & Nicoli 2026 and Metzger
  2023; this sketch first guessed $500 → $150). This is the single most
  important tuning knob: cheap launch makes in-space production pointless, and
  expensive launch makes it mandatory.
- Single player. No combat.
- Daily simulation tick, real time with pause, speeds from 1 day/s up to
  about 1 month/s. Departures, arrivals, window openings and projected
  stockouts can each auto-pause.
- Orbits are deterministic, so with perfect information the game would be a
  solved optimization problem. Uncertainty comes from:
  - hidden resource grades
  - the launch-price path
  - sampled third-party demand
  - mission failures
  - (M4) firm behavior

## Map

The world uses patched conics. Heliocentric legs are Lambert solutions between
bodies on Keplerian orbits. Legs within one body's sphere of influence use a
fixed Δv table.

| Region | Nodes | Notes |
|---|---|---|
| Earth | Surface, LEO, EML1 | Surface→LEO is a purchased launch service, not a flown leg. EML1 is the high-energy hub: cheap from the Moon, cheap to depart interplanetary. |
| Moon | LLO, South Pole, Mare | South Pole: ice, near-continuous sunlight. Mare: regolith only, 14-day night. |
| Mars | Low Mars orbit, Surface, Phobos | Surface site is mid-latitude ground ice. Phobos composition is uncertain, which makes it a prospecting gamble. |
| NEAs | ~8 real asteroids from NASA's NHATS accessibility list | Composition class from spectral type where known (Ryugu and Bennu are carbonaceous). Grade stays hidden until surveyed. |

Approximate local Δv in km/s (from standard Δv maps; M0 calibrates them):

| Leg | Δv (km/s) |
|---|---|
| LEO → EML1 | ≈ 3.8 |
| EML1 → LLO | ≈ 0.6 |
| LLO ↔ lunar surface | ≈ 1.9 each way |
| EML1 or LLO → LEO with aerobraking | ≈ 0.8–0.9 |
| Mars surface → LMO | ≈ 4.1 |
| Mars landing | mostly aerodynamic, ~1 propulsive |

### Interplanetary legs

The Lambert solution gives a hyperbolic excess speed v∞ at each end. It is
converted into burns at the node like this:

- **Departure from a low orbit:** an Oberth burn,
  Δv = √(v∞² + v_esc²) − v_circ.
- **Departure from EML1:** drop to a low perigee first, then burn there.
- **Arrival:** either propulsive capture (same formula) or aerocapture at
  Earth or Mars, at ≈ 0 Δv for aero-capable ships.
- **NEA arrival:** a rendezvous, Δv = v∞, since there is no gravity well.

Every origin–destination pair gets a precomputed porkchop over departure date ×
time of flight. The game reads off a window calendar, and for each shipment a
choice along the Δv versus transit-time frontier.

## Physical economy

### Propellant: methalox and hydrolox

Two propellants share liquid oxygen; each ship design burns one of them.

- **Methalox** (LOX/methane): Isp ≈ 375 s, mixture ≈ 3.6:1. Dense and
  storable for months, so it is the propellant of interplanetary haulers.
- **Hydrolox** (LOX/hydrogen): Isp ≈ 450 s, mixture ≈ 5.5–6:1. More Δv per
  tonne, but liquid hydrogen boils off in storage and needs bulky tanks, so
  it suits short cislunar legs: landers, tankers and tugs.

Sources:

- **Moon:** polar ice gives oxygen and hydrogen (water is 8:1 by mass, so
  some oxygen is left over); regolith gives oxygen only. No carbon, so no
  methane.
- **Mars:** oxygen and methane, from atmospheric CO2 plus ground ice.
- **Carbonaceous NEAs:** oxygen and methane, from water plus organics.
  Grade is uncertain. (Their water could also give hydrogen, but it would
  boil off on the months-long trip home; MVP asteroid plants make methalox
  only.)
- **Earth:** everything, at launch price.

The Moon fuels cislunar traffic, the asteroids and Mars fuel deep-space
traffic, and the spare lunar oxygen and imported methane connect the two.

Decided 2026-10-13, replacing **methalox only**. That rule made the Moon
an oxygen-only source so that a Moon-first strategy was half a solution
and the network had cross-dependencies by construction. M2 showed it
instead decided the winner: asteroids led in ~70% of draws, and counting
lunar ice as complete propellant flipped that to the Moon in 84% (M2.md,
"Propellant chemistry test"). Two propellants keep cross-dependencies (the
lunar oxygen surplus, methane for deep space) without discarding the
Moon's hydrogen.

### Commodities

| Commodity | Source (MVP) | Role |
|---|---|---|
| LOX | Earth, Moon, Mars, C-type NEAs | Oxidizer for both propellants (78% of methalox, ~85% of hydrolox) |
| Methane | Earth, Mars, C-type NEAs | Methalox fuel |
| Hydrogen | Earth, Moon | Hydrolox fuel; boils off in storage |
| Equipment | Earth only | What facilities and ships are built from |
| Supplies | Earth only | Crew life support |
| Precious metals | Metal-rich NEAs | Export to Earth. Price falls with cumulative volume, so flooding the market crashes it. |

### Facilities

Facilities are built by delivering Equipment to the site, plus a build time.
Output scales with the Equipment delivered, so expansion is incremental.

| Facility | Sites | Output |
|---|---|---|
| Depot | any orbital node | stores propellant, sells it to traffic |
| Ice plant | Moon South Pole, Mars surface, C-type NEA, Phobos (if wet) | LOX + hydrogen (Moon); LOX + methane (elsewhere) |
| Regolith oxygen plant | any lunar site, Mars surface | LOX; more equipment per t/yr than ice |
| Metals mine | metal-rich NEA | Precious metals |
| Outpost | any surface or orbital node | houses crew, consumes Supplies |

- Lunar ice comes in finite deposits of falling grade (rich, typical,
  lean), each drawn per game; leaner ice needs more equipment per t/yr.
- Power is folded into equipment mass per unit of output, which varies by
  site: solar falls off as 1/r², lunar night costs extra mass, the polar sites
  get a bonus.
- MVP plants are robotic. Crew exists for mandates and outposts.

### Ships

The MVP has a few predefined designs, bought on Earth and delivered to LEO. A
design is defined by dry mass, propellant capacity, cargo capacity, crew
capacity, whether it can aerocapture, and which surfaces it can land on.

- **Heavy hauler** (Starship-class, methalox): ~120 t dry, ~1,200 t
  propellant, ~100 t cargo, aero-capable at Earth and Mars.
- **Lunar lander/tanker:** stays in cislunar space; methalox and hydrolox
  versions.
- **Cislunar tug** (hydrolox, with an aerobrake): moves propellant and
  cargo between LEO, L1 and lunar orbit.
- **Crew transport:** habitat plus shielding. Crew legs have a maximum transit
  time, a stand-in for radiation dose.

Every leg is costed with the rocket equation, and ships refuel at depots. A
route is feasible only if each depot along it holds enough propellant *when
the ship arrives*. That is where lead time bites.

### Prospecting and failure

- Each resource site has a hidden grade. Survey missions reveal it with noise.
  Building blind is allowed.
- Each leg and landing has a small chance of losing the ship. Losing crew
  costs a lot of political standing.

## The Authority

**Money:**
- **Appropriation:** annual, set by Earth politics as a function of Standing.
- **Revenue:** propellant sales to third-party traffic, precious-metals sales,
  and later fees and royalties.
- **Spending:** launch purchases, Equipment, ships, facility and crew running
  costs.

**Standing** (a single number in the MVP) rises with:
- mandates met
- Access Cost Index improvements
- traffic growth

It falls with missed deadlines, lost crew and visible waste. It drives the
appropriation and gates new powers.

**Mandates** are scripted political goals with deadlines, for example:
- 8 crew continuously at the lunar south pole by 2046
- a crewed Mars landing in the 2052 window
- an asteroid resource demonstration by 2050
- lunar-surface access cost below $X/kg by 2055

They set direction, not method.

**Access Cost Index:** published monthly. It is the cheapest $/kg to deliver
cargo to each destination, given current infrastructure, posted depot prices
and the launch price. It serves as the Authority's statutory scorecard, and the
third-party demand model uses exactly the same calculation. One number ties
the score, the feedback and the demand together.

**Powers** (the MVP progression, standing in for a tech tree):

1. **Start:** procurement and building. Propellant is sold at cost.
2. **Pricing authority:** set markups at depots. This funds expansion, but
   higher prices suppress traffic, and Earth offsets part of net revenue
   against the appropriation. That makes it double-edged on purpose.
3. **Revenue bonds:** borrow against projected depot revenue.
4. **Concessions:** grant exclusive resource rights at a site in exchange for
   royalties. Requires the contracts layer (M4).

The Authority petitions for each power once Standing passes a threshold.

## Demand: induced third-party traffic

Each month a generator samples candidate missions by category:
- lunar national bases
- Mars science and crewed programs
- NEA prospectors
- cislunar tourism

Each candidate has a payload profile, a destination, and a value V that grows
with the Earth economy. It flies if V exceeds its cheapest cost C. C is
computed exactly like the Access Cost Index: Earth launch plus propellant
bought at Authority depots.

Missions that fly buy propellant at depots and follow the same windows, so
demand is lumpy but forecastable. Each depot ends up with an emergent demand
curve whose elasticity comes from the cost structure, not from a formula.

## Player actions

- Buy launches and ships. Schedule missions: ship, cargo, legs, and a
  departure picked from the porkchop.
- **Standing routes**, modeled on the WOLF mod's logistics: "every Earth→Mars
  window, send a hauler with this manifest along this path, taking the
  cheapest departure with time of flight ≤ N days." These run automatically
  and are flagged when they become infeasible.
- Build and expand facilities.
- Survey sites.
- Set depot prices (once granted).
- Petition for powers.
- (M4) Post contracts.

## Contracts layer (M4, separable)

Contract types:
- **Fixed-price delivery:** a firm delivers X to node N by date D.
- **Offtake / anchor tenancy:** the Authority commits to buy Y t/yr at node N
  at price P for T years. The firm finances and builds the plant.
- **Concession:** exclusive rights at a site in exchange for royalties.

There are 3–5 firms, each with its own capital, efficiency (firms run
operations more cheaply than the Authority), cost of capital (higher than the
Authority's) and risk appetite. A firm:
- estimates a contract's value using the same cost model, with noisy private
  estimates, and bids
- builds real assets on the map
- sells its surplus to third-party traffic
- can default

Intended trade-off (the real public–private partnership trade):

- **For contracting:** private capital stretches a capped budget, prospecting
  and execution risk move to the firm, and commercial capacity that serves
  third parties gets seeded.
- **Against:** it costs margin and control, and firms cherry-pick the
  profitable work.

If playtests or the optimizer show contracting strictly dominates or is
strictly dominated, the layer gets cut or reworked.

## UI (M3)

Minimal and information-dense:

- **System map:** 2D heliocentric with real positions and a time scrubber,
  plus a cislunar inset.
- **Window calendar:** a Gantt chart of upcoming windows per route, shaded by
  Δv.
- **Porkchop panel** when scheduling.
- **Inventory projection** per node: stock over time with scheduled arrivals,
  departures and consumption, with projected stockouts highlighted. This is
  the main lead-time instrument.
- **Ledger:** appropriation, revenue, spending, debt, Standing, mandates, and
  Access Cost Index history.

## Milestones

- **M0, orbital engine.** Done (2026-10-12): `crates/orbit`, CLI `orbit`.
  - Planet ephemerides from JPL's approximate Keplerian elements. NEAs are
    piecewise two-body fits to JPL Horizons over 2040–2076 (`tools/neas/`),
    good to ~0.002 au except in segments containing a close Earth flyby
    (Bennu 0.016 au, 2000 SG344 0.010 au, 2017 SV19 0.006 au), where
    positions also jump by that much at the segment boundary. Chose this over osculating elements propagated
    two-body, which drifted ~0.1 au over the span because they carry the
    Sun's wobble around the barycenter. Close Earth flybys appear as orbit
    changes.
  - Izzo Lambert solver, porkchop grids, Δv network (`data/network.toml`),
    window detection, and a window-calendar CLI. Porkchops are computed on
    demand (a full 36-year calendar takes ~4 s on 24 cores); caching is
    deferred until M1 shows it is needed.
  - Check: the Mars minimum-C3 values in NASA/TM-2010-216764 are reproduced
    to ≤0.25% (≤0.5% for one row). The handbook's 2035/2037/2039 dates are
    inconsistent with JPL's own ephemeris; its C3 values for those years are
    reproduced at dates 63–81 days later.
  - Earth–Moon L1 uses a lunar-assist model (2026-10-13): 0.14 km/s plus
    lunar flybys reach C3 = 2 km²/s², with a perigee burn on top for more,
    at the cost of ~60 extra days per departure or capture. It is fitted to
    the published 0.14 km/s to Earth escape, flyby-only C3 ≈ 2 (McElrath et
    al. 2012), and EML2 → Mars under 1.0 km/s. It replaced a two-body model
    that overcharged every L1 departure by ~0.55 km/s and would have biased
    the M2 comparison against asteroid-first strategies. Consequence: from
    L1, Mars orbit (with aerocapture) costs ~0.6 km/s and the best small
    NEAs 0.4–1.5 km/s, so the cost of the economy concentrates in reaching
    L1 (3.77 km/s from LEO, 2.5 from the lunar surface).
  - Simplifications to revisit: zero-revolution Lambert only; the L1 flyby
    phase is a fixed duration rather than computed; no aerocapture speed
    limit.
- **M1, headless sim core.**
  - Nodes, commodities, facilities, ships, rocket-equation costing, missions
    and standing routes.
  - Budget, Standing, mandates, the traffic generator, the Access Cost Index.
  - Deterministic and seeded, with scenario scripts as tests.
- **M2, kill-test optimizer.** Done (2026-10-13), see M2.md. The gate
  fails as written and passes in intent. With methalox and hydrolox and
  finite lunar ice, the Moon leads 90% of draws; with methalox only,
  asteroids led ~75%. The rules, not the physics, decide the dominant
  path, and the decisions after the first move differ by world.
  - Model: a time-expanded network with six-month steps (quarterly and
    monthly steps gave the same plans at 3× and 20× the solve time). Facility modules and
    ship trips are decision variables (the LP relaxation is probably enough to
    classify strategies). For a fixed ship and leg, propellant per trip is
    linear in cargo. Interplanetary edges exist only at discretized
    departure/time-of-flight options.
  - Objective: maximize the value of third-party traffic served (piecewise-
    linear demand), subject to budget and mandates.
  - Sweep: launch-price curve, NEA and Phobos compositions, lunar ice grade,
    demand mix, budget. Classify each optimal plan by which development path
    it follows.
  - **Gate.** Pass: no single path wins a large majority of plausible draws,
    and first-five-year decisions differ across draws. Fail: retune or rethink
    before building any UI.
- **M3, playable UI** on the core.
- **M4, contracts and firms;** A/B against direct-build-only.

## Deliberately out of the MVP

- **Solar-electric tugs.** The first expansion. Spiraling out of LEO is slow,
  so they also make EML1 more important.
- **Cyclers.** They only matter once there are real crew flows.
- Tethers, mass drivers, ship design.
- Local production of Supplies or Equipment.
- Settlements and population growth.
- Venus, Ceres and the outer system.
- Terraforming, multiple Earth factions.

## Open questions and tuning knobs

- **Launch-price curve.** The default above is a guess; M2 sweeps it.
- **Opening variety.** Under the current settings the Moon is the best
  first move in ~90% of worlds. Settings that move it (M2.md, Verdict):
  the probability that minable lunar ice exists (now certain), the
  methalox/hydrolox demand mix and deep-space mandates, hydrogen boil-off,
  and asteroid water as a source of hydrolox.
- **Gap between firm efficiency and the Authority's cost of capital.** This
  decides whether contracting is a real choice.
- **Clawback.** What fraction of net revenue Earth offsets against the
  appropriation.
- **Stack.** Assumed: a headless Rust core for M0–M2. The UI technology gets
  decided at M3.
