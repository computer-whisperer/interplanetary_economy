# M1b — The sim engine, for sandbox and career

Status, 2026-10-16: plan. Decided: one engine for both modes, with
economy components landing in the sandbox first (Modes, below), and
the four decisions below, each as recommended. Nothing built.

M1b builds the engine that runs the economy forward in time. The
Authority's ships are flown by the autopilot and carry mass and
propellant; depots and plants hold stocks. The engine also models
money, Standing and mandates, and third-party traffic that flies when
the Authority's infrastructure makes it affordable. One engine serves
both of the game's modes: the sandbox (M1c's creative mode, grown) and
career (the game M3 makes playable). It is deterministic and seeded,
and runs headless for scenario scripts as tests (DESIGN.md,
Milestones).

## Modes

Decided 2026-10-16, after the classic career/sandbox split:

- **Sandbox.** It explores the orbital engine. You run whatever
  missions and space programs you want, and change anything at any
  time, including what is behind the clock. It is where core UX and
  engine features are iterated without game-balance concerns.
- **Career.** It runs the locked-down economy engine. It is
  temporally consistent: plans and states before now can't be changed.
- **One engine.** This is a requirement. The modes differ in policy,
  not in code:
  - A game is its setup (a seed or the central draw, a start date, a
    mode) and a log of actions, each taking effect at a time. M1c's
    plan files are a sandbox log of one kind of action: a ship and its
    steps.
  - The sandbox may add, change or remove any action at any time. The
    engine re-runs from the earliest time the change affects.
  - Career adds actions only at now, taking effect at now or later.
    For a ship, that is its first checkpoint after now: the end of the
    leg in flight, or now if it is staying.
  - Career reveals hidden information with time and surveys: grades,
    deposit sizes, the launch price ahead. The sandbox knows
    everything.
- **Economy components land in the sandbox first.** Each is watched
  and edited there before career relies on it. Which ones the sandbox
  runs, and which as settings (losses, say), is decided component by
  component as each lands. A rule of thumb: a component that is a
  function of the log runs coherently in the sandbox. What depends on
  what the player knew when (an unsurveyed grade, Standing's political
  feedback) is career's.
- **Mid-flight redirects are possible eventually**, not in M1b. A
  redirect would start from the ship's coast, the way M1c's departures
  from a low orbit start from the ship's coasted path. So the engine
  keeps a ship's items after now replaceable. A redirect is then a new
  kind of checkpoint, not a new engine.

## Why

M2 answered Q2 with a linear program that sees every hidden draw and
plans 36 years at once, in six-month periods, on M0's Δv map (M2.md);
M1a step 11 reran its central world on legs the autopilot flew. The
game is played forward instead. Grades stay hidden until surveyed, the
launch price is learned as it moves, demand is sampled, and ships are
lost. M1b builds that game's engine, so that its rules can be run,
watched in the sandbox, scripted and measured before M3 puts a player
in front of them in career. Q1 (lead time), Q3 (demand) and Q5
(progression) are questions about play, and M1b is what M3 plays.

## Decisions

Decided 2026-10-16 by the user, each as recommended.

1. **Which trips fly.** M1a left this to here ("whether M1b flies ships
   per trip for the legs that pass is decided before M1b"). Every
   trip of the Authority's ships is designed and
   flown. Third-party traffic and the Access Cost Index are priced from
   the route graph (`data/routes.toml`: legs within a system at the
   medians of flown designs, between systems by patched conics) and are
   not flown.
   - **Scale.** An estimate from M2's central world, with the NRHO as
     the hub (`plan solve --hub nrho`, run 2026-10-16). In the 2050s it
     lifts ~660 kt a decade of lunar propellant to low lunar orbit and
     moves ~240 kt on to LEO and ~130 kt to the NRHO, and ~115 kt of
     payload from LEO to low lunar orbit. In its fleet's tanks and
     holds, that is ~750 ascents a year and as many landings (rules,
     no design; a lander-h lifts at most 87 t in its tanks) and
     ~300–1,000 designed legs a year, counting returns.
   - **Cost.** Re-planned from the route's recent designs
     (`legs::replan`), a leg takes 0.01–0.3 s on one core (M1a, step
     10; LEO to the DRO 0.93). The sweep's median, all legs together,
     was 0.05–0.06 s. Re-plans alone come to ~1–25 cores' worth at a
     month a second in a mature economy, and up to ~3 core-hours for a
     36-year game headless. That is a lower bound:
     - a leg leaving a low orbit the ship arrived in is designed twice,
       and up to ~10 more times when it waits for the planes to align
       (M1c step 7, part 8). That is most departures from LEO and low
       lunar orbit, the bulk of the designed legs;
     - re-plans fall back to cold designs (0.07–5 s, on all cores) on
       5.5% of departures on most legs and 58% between stations, and
       for each route's first departure and each guard check;
     - departures to other planets are designed cold (0.2–4 s).

     So a mature economy at a month a second probably outruns 24
     cores. Designs running ahead of departures (a route designs its
     next as soon as the last is designed) absorb bursts, not a
     sustained deficit. Step 2 measures it, and gate item 4 holds it.
   - **What fails.** Some departures fail as they did in M1a's sweep:
     legs between stations (88.5% converge, 42% of re-plans succeed)
     and slow or near-180° departures to other planets. A mission's
     failed leg is reported as in M1c, and the player picks another
     option. A standing route's failed departure takes its next, and the
     route is flagged when nothing in its window designs.
   - **Rejected: tables for everything** (M1a's fallback). Ships would
     move on paths that were never flown, and the map exists to show
     flown paths. Costs would also miss phasing, which M1a step 11
     named among what decides the hub.
   - **Rejected: flying third-party traffic too.** In the 2050s M2's
     central world carries ~12 kt of payload a year for third parties,
     120–1,200 missions at 10–100 t each [guess], and sells ~14 kt of
     propellant at the hub to departing traffic. Flying them would add
     as many designs again, for ships nobody plans.
2. **Powers.** DESIGN.md's M1b list left the powers out (DESIGN.md,
   The Authority). Pricing authority and revenue bonds are in M1b, as
   rules a scenario can exercise; concessions come with M4's
   contracts. Q5 is about the powers, and M3 plays what M1b has.
3. **Crew** is in (M2 had none): crew carried by crew transports with
   a longest transit, outposts consuming Supplies, and crew lost with
   a ship. The mandates need them (8 crew continuously at the south
   pole by 2046, which takes rotations).
4. **The gate** is below, with its thresholds as proposed (20%, 10
   minutes, 1 s); DESIGN.md gave M1b none.

## Scope

In:

- the engine: the action log, both modes' policies, re-running from
  an edit, keyed draws, and the sandbox running on it;
- ships with mass: classes from `data/economy.toml`, bought at LEO,
  propellant and cargo, the rocket equation at each burn of a flown leg,
  upkeep in station orbits, landings and ascents by `surface`'s rules,
  and boil-off;
- missions: M1c's plans with a ship of a class and a manifest;
- standing routes (DESIGN.md, Player actions);
- stations and depots, plants, technology programs and surveys (M2's
  data);
- third-party traffic and the Access Cost Index;
- money, Standing, mandates, crew and Supplies, and ship losses;
- pricing authority and revenue bonds (decision 2);
- inventory projections, the lead-time instrument M3 draws;
- in the sandbox, the least UI that shows each component as it lands
  (rows in the plan's panel, a stock at a place);
- scenario scripts and a headless CLI.

Out:

- career's UI: inventory projection panels, the ledger, the economy's
  actions (M3); contracts, firms and concessions (M4);
- mid-flight redirects (eventually; Modes, above);
- legs that fail M1a's gate: between stations, slow and near-180°
  departures, and LEO to the DRO's design time. That is playbook work,
  taken up when the economy shows it needs those legs;
- upkeep in low orbits (the lunar gravity field and drag; M1a, step
  5d): still none;
- legs between Mars and asteroids or between asteroids (as in M2);
- departures to other planets from station orbits, and arrivals there
  (M1a step 9, deferred): the planner sends them through LEO. M2's
  central world flies payload from the NRHO to Mars.

## Design

### The engine

- **The log** (Modes, above): a game's setup and its actions. The
  sandbox edits it anywhere; career appends at now.
- **Re-running.**
  - Designs are pure functions of their inputs and are kept by them,
    so a re-run reuses every design its change doesn't touch. This
    generalizes what the sandbox does now (`sandbox::plan`: a ship's
    design generations, re-planned from the checkpoint an edit
    reaches).
  - The economy's state is snapshotted [monthly, to set]. A change
    re-runs from the last snapshot before the earliest time it
    affects. In career that is now, so a re-run is the game going on.
- **Time.**
  - The engine runs by events in time order. Burns, departures and
    arrivals fall where the flown designs put them. A daily tick runs
    production, boil-off and crew consumption. Demand and the Access
    Cost Index come monthly, the appropriation yearly. Ties at one
    time break in a fixed order: by kind, then by id.
  - The engine runs as far ahead as its designs allow. In a window,
    designs run on the background pool (`sandbox::background`) and the
    clock can't pass a departure whose design isn't done. Headless, the
    engine waits for them. Either way, the result doesn't depend on how
    fast designs run.
- **Keyed draws.** Each random draw is keyed by what it is for: the
  world's draw by parameter, demand by category and month, a loss by
  ship and step, survey noise by survey. Each comes from a ChaCha
  stream seeded by the game's seed and its key. An edit then re-rolls
  nothing it doesn't touch, in either mode.
- **Determinism.** The state is hashed each day, and tests compare the
  hashes. Checked first: that designs are bit-identical across thread
  counts. The fans collect in order and re-planning is serial, but
  this has not been tested.

### Ships, mass and propellant

- A ship has a class, a plan (its items), propellant by component (LOX,
  and methane or hydrogen), cargo by commodity, and crew.
- At each burn of a flown leg (its markers), the propellant burned is
  m(1 − e^(−Δv / g₀Isp)) of the mass before the burn, split as LOX and
  fuel at the class's mixture ratio. Stays in a station orbit burn the
  orbit's upkeep; landings and ascents burn their rules' Δv. Hydrogen
  boils off daily, in tanks and in depots (`fuel_boiloff_per_day`).
- Designs don't depend on mass (burns are impulsive), so a leg is priced
  after it is designed. In the sandbox, a leg the ship's propellant
  can't fly is shown as where it runs dry; in career, it is refused
  when planned, against the projection.
- Stays get a lighter form. M1c samples a low orbit ~2,900 times a day
  (its known gap), which a fleet parked in LEO for years can't afford.
  A stay keeps what it needs to resume (the ship's state where it
  arrived, a station's reference, a site), and positions are computed
  when they are drawn or when a leg leaves. Which form is chosen in
  step 2.

### Places, stocks and stations

- Each place holds a stock of each commodity. At LEO, anything can be
  bought from Earth at its price plus launch. Earth's surface is not a
  place, as in M2.
- **A station is one continuous orbit.** Placing one builds its
  reference from that moment (`reference::continue_orbit`; a year of
  the NRHO took 3.6 s in M1a step 5c) and extends it ahead as plans
  reach further, with upkeep simulated as it goes. Extending is new:
  `continue_orbit` starts from the catalog orbit, so M1a's yearly
  pieces and M1c's visits are each a separate orbit. Every leg to or
  from the station meets that reference, so ships meet the depot
  itself rather than another orbit of its family at another phase.
  M1a left the continuous reference to M1b.
- A depot stores propellant at a station or a surface site. Loading,
  unloading and refuelling happen while a ship stays at the place, as
  its manifest says.

### Plants, programs and surveys

- As in M2 (`data/economy.toml`). Equipment delivered to a site and
  installed adds capacity after the build time. Output goes daily into
  the site's stock, and deposits run out. A plant can be installed only
  once its technology program is finished.
- In career, grades stay hidden until surveyed. A survey costs and
  takes `data/economy.toml`'s numbers, and reveals the grade with noise
  [noise to set]. (M2 divided a survey's cost by the chance the deposit
  has the resource, to offset its foresight; the engine doesn't.)
  Building blind is allowed (DESIGN.md, Prospecting).

### Missions and standing routes

- A mission is M1c's plan for one ship, plus its class and a manifest:
  what to load, unload, buy or sell at each stop, and how much
  propellant to take on. It is designed when planned, then priced and
  checked against the projection.
- A standing route (DESIGN.md, Player actions) is a ship or ships with
  a manifest along a chain of places, repeating.
  - Within a system, each departure is re-planned from the route's
    recent designs (`legs::replan`). A failed re-plan falls back to a
    cold design, and M1a's guard applies: a re-plan above 1.25× the
    recent median is checked against a cold design. The history, the
    fallback and the guard live in the gate sweep (`orbit-cli`'s
    `gate.rs`) and move into the library.
  - Between planets, one departure per window, designed cold: the
    cheapest within the route's longest duration. DESIGN.md has every
    departure re-planned from the previous one; between planets,
    patched conics already seed each design, and windows ~26 months
    apart share little.
  - A departure that the projection says can't refuel waits. The route
    is flagged infeasible when its window closes.

### Third-party traffic and the Access Cost Index

- **The cheapest cost** to each destination, $/kg delivered there from
  Earth. It counts launch to LEO plus propellant bought at depots at
  their posted prices along the route graph's legs, by the rocket
  equation, for a reference ship class [class to choose]. The same
  calculation gives the published monthly index and drives the
  generator, as DESIGN.md asks.
- **The generator.** Each month, for each category in `[[demand]]`, it
  draws candidate missions, each with a payload and a value per kg V.
  V is drawn so that the expected quantity flying at a cost C is M2's
  curve, Q(C) = Q_ref (C / C_ref)^−ε. That makes V Pareto with index ε
  between the category's bounds. The number of candidates grows 3% a
  year (Q_ref, as in M2), not V.
- **A candidate flies if V ≥ C.** It pays launch and buys its
  propellant at the depots on its route, on the route graph's durations
  and, between planets, in the windows (`orbit::windows`). Demand is
  then lumpy, comes at windows, and drains depots. A stockout loses the
  sale, and lost sales are counted.
- **Propellant at the hub** (`what = "propellant"`): a candidate is a
  mission leaving Earth that refuels at a station. It buys there if V is
  at least the depot's price; otherwise it brings its own from Earth,
  and the sale is lost.

### Money and the Authority

- **The account.**
  - In: the appropriation, yearly (a draw of `budget`, scaled by
    Standing); revenue from propellant and metal sales.
  - Out: equipment, ships, programs and surveys (capital, all that
    M2's `budget` covers), and the Authority's own launches,
    operations per trip and crew upkeep.
  - Propellant is sold at cost and third-party payload pays its own
    launch, as in M2, so those pass through the account.
  - `budget` was calibrated for capital alone; how much more the
    appropriation covers is [to set].
  - With pricing authority, Earth offsets part of net revenue against
    the appropriation (the clawback; DESIGN.md, Open questions).
  - Spending can't take the account below zero; bonds borrow against
    projected depot revenue.
- **Standing** is one number. It rises with mandates met, Access Cost
  Index improvements and traffic growth. It falls with missed deadlines,
  crew lost and visible waste. Its formula is [guess] until M3's
  playtests.
- **Mandates** are scripted, with deadlines, and checked daily.
  DESIGN.md's four are the first set.
- **Crew** ride in crew transports, with a longest transit. At an
  outpost they consume Supplies.
- **Losses.** Each leg and landing loses its ship with a small
  probability [to set], from its keyed draw.
- **Powers** (decision 2): a markup on a depot's price, and bonds.

### Projections

- For each place, stock over time from what is scheduled: planned
  arrivals and departures, plants at capacity, crew consumption,
  boil-off, and third-party demand at current prices (its mean, not a
  draw). Missions and standing routes are checked against it, and M3
  draws it.

### Scenario scripts and the CLI

- **A scenario** is a career log written by hand: a seed (or the
  central draw), a start, actions at dates, and checks.
  - Actions: buy a ship, place a station, deliver and install
    equipment, start a program or a survey, plan a mission, set up a
    standing route, set a price, petition for a power.
  - Checks: a stock, the account, Standing, a mandate, or the Access
    Cost Index at a date, within bounds.
- `sim run SCENARIO` runs one headless, prints a monthly ledger, and
  exits nonzero when a check fails. Scenarios live in
  `data/scenarios/`, and the tests run them.
- A log is the whole game, so a saved game is its log. The sandbox
  opens any log, a career game's included.

### Crates

- `crates/sim`: the engine, built on `orbit`.
- `sandbox::plan`'s design management (background designing,
  generations, re-planning an edit from its checkpoint) moves into
  the engine. It is deleted first and rebuilt there; the sandbox then
  draws the engine's state and sends it actions.
- The reader of `data/economy.toml` and its draws move into the engine
  from `crates/plan` (`params`: `EconomyFile`, `Dist`, `Draw`), and
  `crates/plan` builds its periods from them.

## Gate

1. **Deterministic.** A scenario run twice, and at 1 and 24 threads,
   gives the same state hash every day.
2. **One engine.** A career game's log opened in the sandbox gives the
   same hash every day. An edit in the sandbox, re-run from a snapshot,
   gives the hashes a fresh run of the edited log gives.
3. **Agrees with M2 where the models overlap.**
   - M2's central world, with the NRHO as the hub, written as a
     scenario: its program and survey, its plant, its fleet, and its
     largest flows as standing routes.
   - Over 2045–2060, its supply by source and its Earth purchases land
     within 20% of the LP's.
   - Each difference is explained by what the LP leaves out: phasing,
     departures one at a time, lumpy demand, boil-off by the day. Its
     NRHO-to-Mars payload goes through LEO (Out, above), and the
     comparison counts that.
4. **Fast enough.**
   - That scenario runs 2040–2075 headless in 10 minutes on the
     development machine.
   - At a month a second, no departure waits on its design.
   - In the sandbox, an edit's economy re-runs, its designs reused, in
     1 s over the whole game.

Fail:

- (1) and (2) are bugs, fixed before going on.
- (3) means the engine and the LP disagree on a rule; one of them
  changes.
- (4) means repeat departures of a standing route reuse a design moved
  by whole synodic months, and are re-designed only every so often;
  or snapshots come closer together.

## Steps

Each step gets its own plan before it starts, and is committed on its
own. Each economy component lands in the sandbox first (Modes, above).

1. **The engine.**
   - `crates/sim`: the log, both modes' policies, keyed draws,
     snapshots and re-running, the event clock and daily tick, and the
     state hash.
   - The sandbox runs on it: `sandbox::plan`'s design management is
     deleted and rebuilt in the engine, and M1c's behavior holds.
   - The parameters moved from `crates/plan`. Scenario scripts and
     `sim run`.
   - First of all: designs bit-identical across thread counts.
2. **Ships with mass:** classes, purchase at LEO, missions with a
   class and a manifest, the rocket equation at burns, upkeep, landings
   and ascents, and boil-off. Stays in their lighter form. In the
   sandbox: each burn's propellant, and where a ship runs dry.
3. **Stations and depots:** placing a station (one reference, extended
   ahead), stocks, loading and refuelling, purchases at LEO, and
   projections.
4. **Plants, programs and surveys;** in career, grades hidden until
   surveyed.
5. **Standing routes.**
6. **The cheapest cost, the Access Cost Index and the traffic
   generator.**
7. **Money, Standing, mandates, crew, losses and powers.**
8. **The gate.**
