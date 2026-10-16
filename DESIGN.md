# Interplanetary Economy — MVP Design Sketch

Status: draft, 2026-10-12; n-body sandbox and the 3D map view 2026-10-13;
the planning panels moved into M1c 2026-10-15.
Working title TBD.

## Vision (full game, for context)

The player runs a chartered public institution, "the Authority", whose
statutory mission is to lower the cost of access to the solar system. It starts
as a coherent, well-funded NASA-like agency living on appropriations. Over the
game it can accrete powers until it resembles the East India Company:
collecting tolls, granting concessions, issuing debt, governing settlements.
**Institutional powers replace the tech tree as the progression spine.**
Technology mostly arrives from Earth on its own schedule.

The world is real n-body gravity plus the rocket equation plus a fictional
but plausible future economy. Lagrange points and the orbits around them are
not scripted; they are where the physics puts them. Decisions are mostly:

- **where**: which sites to develop, which orbits to put depots on
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
- Daily economy tick, real time with pause, speeds from 1 day/s up to
  about 1 month/s. Ships fly arcs integrated when their legs are planned,
  so speed costs nothing extra. Departures, arrivals, window openings and
  projected stockouts can each auto-pause.
- Orbits are deterministic, so with perfect information the game would be a
  solved optimization problem. Uncertainty comes from:
  - hidden resource grades
  - the launch-price path
  - sampled third-party demand
  - mission failures
  - (M4) firm behavior

## Map

The world is an n-body sandbox (M1a.md). Ships are integrated in the
gravity of the real Sun, planets and Moon (JPL's DE440), and fly by flight
rules that an autopilot sets.

Decided 2026-10-13, replacing **patched conics with a fixed Δv table**
inside each planet's sphere of influence (M0). There, the Earth–Moon L1
hub existed because the table listed it, and its cheap escape was a fit to
two published numbers; that fit alone moved M2's results, and most of the
economy's cost is the climb to L1. In the sandbox, Lagrange-point orbits,
the natural pathways between them, cheap escapes and each orbit's
station-keeping cost come from the physics, everywhere in the solar
system, and where hubs go is for the economy and the player to find.
Patched conics were cheaper and fully predictable; measured costs show
integration is cheap enough (M1a.md, Measured costs), and the autopilot
keeps the chaos away from the player.

**Places:**

| Region | Places | Notes |
|---|---|---|
| Earth | Surface; low and high Earth orbits; orbits about Sun–Earth L1 and L2 | Surface→LEO is a purchased launch service, not a flown leg. |
| Cislunar | Lyapunov and halo orbits about Earth–Moon L1 and L2; distant retrograde orbits (DROs) about the Moon | The near-rectilinear halo orbit (NRHO) is an L2 halo that spends most of its week high over the lunar south pole. |
| Moon | Low lunar orbits, South Pole, Mare | South Pole: ice, near-continuous sunlight. Mare: regolith only, 14-day night. |
| Mars | Low Mars orbits, Surface, Phobos | Surface site is mid-latitude ground ice. Phobos composition is uncertain, which makes it a prospecting gamble. |
| NEAs | ~8 real asteroids from NASA's NHATS accessibility list | Composition class from spectral type where known (Ryugu and Bennu are carbonaceous). Grade stays hidden until surveyed. |

Stations and depots sit on a member of an orbit family, chosen freely
within the family. Each has an upkeep, the station-keeping Δv per year:
an L1 halo needs regular corrections, a DRO almost none.

### Trajectories

- **Legs:** a leg goes from a start (a ship, or an orbit) to a goal (an
  orbit, a rendezvous, a surface site, or an escape) within constraints
  (departure window, longest time, most Δv). The autopilot offers options
  along the Δv-versus-time frontier, each a reference trajectory plus the
  flight rules to fly it. For an interplanetary leg the frontier is the
  porkchop: departure date × time of flight.
- **Autopilot:** a playbook of known transfer types gives starting
  guesses (Lambert arcs, two-burn transfers between orbits, the natural
  pathways into and out of Lagrange-point orbits, ballistic lunar
  transfers, lunar flybys), and correction makes them fly in the sandbox.
- **Flight rules** run at discrete events: burns, course corrections,
  station-keeping, aerocapture and entry (aero-capable ships, at Earth and
  Mars), and landings and ascents (Δv from a gravity-loss model per body).
- **Costs:** the economy sees the legs that were flown: Δv, time, and
  upkeep where ships wait.

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
| Depot | any station orbit | stores propellant, sells it to traffic |
| Ice plant | Moon South Pole, Mars surface, C-type NEA, Phobos (if wet) | LOX + hydrogen (Moon); LOX + methane (elsewhere) |
| Regolith oxygen plant | any lunar site, Mars surface | LOX; more equipment per t/yr than ice |
| Metals mine | metal-rich NEA | Precious metals |
| Outpost | any surface site or station orbit | houses crew, consumes Supplies |

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
  cargo between LEO, high cislunar orbits and lunar orbit.
- **Crew transport:** habitat plus shielding. Crew legs have a maximum transit
  time, a stand-in for radiation dose.

Every burn is costed with the rocket equation, and ships refuel at depots.
A route is feasible only if each depot along it holds enough propellant
*when the ship arrives*. That is where lead time bites.

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

- Buy launches and ships. Plan missions: ship, cargo, and a chain of
  legs (reach this orbit, meet that depot, land there, by this date); the
  autopilot offers options along each leg's Δv-versus-time frontier and the
  player picks.
- **Standing routes**, modeled on the WOLF mod's logistics: "every Earth→Mars
  window, send a hauler with this manifest along this path, taking the
  cheapest departure with time of flight ≤ N days." These run automatically,
  the autopilot re-plans each departure from the previous one, and they are
  flagged when they become infeasible.
- Place stations and depots on orbits.
- Build and expand facilities.
- Survey sites.
- Set depot prices (once granted).
- Petition for powers.
- (M4) Post contracts.

## Contracts layer (M4, separable)

Contract types:
- **Fixed-price delivery:** a firm delivers X to place N by date D.
- **Offtake / anchor tenancy:** the Authority commits to buy Y t/yr at place N
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

The **map view** is the center interface: the real solar system in 3D at
true scale, after KSP's map view, as realistic as we can afford, on HDR
displays. It has a timeline of the plan's ships, and frames that follow the camera's
focus or turn with two bodies (Earth–Moon, where Lagrange-point orbits are
closed loops, or Sun–planet). M1c builds it, with its own wgpu renderer
and damascene for the panels (M1c.md).

Decided 2026-10-13, replacing a 2D heliocentric map with a cislunar inset
in the rotating frame. The 2D plan was chosen before the n-body sandbox
existed, when the map only had to show patched conics, and it was
cheaper. In the sandbox the interesting things are 3D (halos, the
near-polar NRHO, lunar flybys, departure planes), and the map is where
the player spends the game. Flying low over terrain (KSP's flight view)
is not planned.

Around the map, minimal and information-dense:
- **Window calendar:** a Gantt chart of upcoming windows per route, shaded by
  Δv. Built in M1c (step 7) as lanes of its timeline.
- **Leg options panel** when planning: the Δv-versus-time frontier (a
  porkchop for interplanetary legs). Built in M1c (step 7).
- **Inventory projection** per place: stock over time with scheduled arrivals,
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
    deferred until M1b shows it is needed.
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
  - Simplifications to revisit: zero-revolution Lambert only; no
    aerocapture speed limit.
  - Superseded by M1a except for Lambert, porkchops, window detection and
    time handling, which become the autopilot's first playbook entry. The
    Δv network and its lunar-assist fit were deleted in M1a step 1 and
    the Kepler-element planets replaced by DE440 in step 2, and the NEA
    fits by integration in step 4. The handbook check above departed from the Earth–Moon
    barycenter all along (the Kepler-element "Earth" was the barycenter);
    with DE440 it holds to 0.08%.
- **M1a, n-body sandbox and autopilot.** See M1a.md. DE440 ephemerides,
  ships integrated as particles, flight rules, and an autopilot that turns
  legs into flown trajectories from a playbook of transfer types.
  - **Gate.** Over departures every 5 days across 2040–2075, ≥ 99% of the
    core cislunar and Mars-system legs design successfully, Δv lands within
    10% of published values, and re-planning a standing route's departure
    takes ≤ 0.1 s. Fail: fall back to tables computed once from the
    physics, without per-trip flights in the sandbox.
- **M1c, creative sandbox and the map view.** Done (2026-10-16), see
  M1c.md; the gate passes for now, with LEO to the DRO designing in
  16.5 s (over 10 s) and the Sun's disk washed out on SDR open. Mission plans
  loaded into the sandbox, flown by the autopilot and played back in the
  3D map view, with no economy. Before M1b, so that the view the game
  centers on is built and stress-tested before the progression mechanics.
  Its step 7 is the planning interface (decided 2026-10-15): a timeline
  of the plan's ships, the window calendar and the leg options panel
  (from M3), editing and re-planning in place.
  - **Gate.** The legs M1a checked against published values fly and
    draw, and halo orbits close in the Earth–Moon frame. Zooming from 5 au
    to a low lunar orbit shows no jitter. A ship on every Mars window
    2040–2075 plays at a month per second at the display's refresh rate.
    HDR outputs use their headroom.
- **M1b, headless sim core** on the sandbox.
  - Places, commodities, facilities, ships, rocket-equation costing,
    missions and standing routes flown by the autopilot.
  - Budget, Standing, mandates, the traffic generator, the Access Cost Index.
  - Deterministic and seeded, with scenario scripts as tests.
- **M2, kill-test optimizer.** Done (2026-10-13), see M2.md. The gate
  fails as written and passes in intent. With methalox and hydrolox and
  finite lunar ice, the Moon leads 90% of draws; with methalox only,
  asteroids led ~75%. The rules, not the physics, decide the dominant
  path, and the decisions after the first move differ by world. Its
  planner read M0's Δv network and is out of the workspace until M1a
  rewires it to a graph of flown legs.
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
- **M3, playable UI** on the core, around M1c's map view and planning
  panels: inventory projections, the ledger, and the economy's actions.
- **M4, contracts and firms;** A/B against direct-build-only.

## Deliberately out of the MVP

- **Solar-electric tugs.** The first expansion. Spiraling out of LEO is slow,
  so they also make high cislunar orbits more important. They need
  continuous guidance, which the flight rules can take.
- **Cyclers.** They only matter once there are real crew flows.
- Tethers, mass drivers, ship design.
- Flying low over terrain (KSP's flight view). Ship models are later
  flavor; on the map, ships are markers.
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
- **Where stations go.** With the hub at any one of the four Earth–Moon
  station orbits, M2's central world is worth the same within 0.5%, and
  the LP over all four at once does not solve (M1a.md, step 11). Where
  to build stations and how to route traffic stay the player's
  decisions: the game should show the trade (transfer costs, upkeep,
  phasing) rather than compute one answer.
- **Trip costs.** Wear (a ship's price over 50 flights) and $0.5M of
  operations per trip [guess] cut the central world's lunar plant to a
  third and its value by 17% (M1a.md, step 11): the largest lever
  measured since M2.
- **Presenting the option space.** The sandbox can fly anything its
  physics allows; the MVP offers the autopilot's playbook options. How to
  show more (natural pathways, frontiers, a burn editor) is open; M1c's
  creative mode is where a burn editor would go.
- **Stack.** Decided 2026-10-13 (M1c.md, Engine): a Rust core; the map
  view on our own wgpu renderer (wgpu over ash and vulkano: the reusable
  code from earlier projects is wgpu, and it opens HDR swapchains);
  damascene for panels and text.
