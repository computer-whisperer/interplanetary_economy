# M1b — The sim engine, for sandbox and career

Status, 2026-10-16: step 1 under way. Decided: one engine for both
modes, with economy components landing in the sandbox first (Modes,
below), and the four decisions below, each as recommended.

- Step 1, part 1: designs are bit-identical on 1 and 24 threads, and
  going on from a checkpoint gives a fresh design's bits
  (`crates/orbit/tests/determinism.rs`, ~60 s). Ships: the crew
  rotation (stations, the surface, an aerocapture), a Mars window, and
  a ship that leaves low lunar orbit when the planes align and then
  leaves LEO for Mars from its own orbit. Edits: three to the crew
  rotation, two of them stepping back past a station visit. Nothing
  needed fixing.
- Part 2: plan files give ships ids and pass `[game]` and `[[replan]]`
  to the engine (`sim::log`). Career's actions are made at whole
  minutes (`log::minute`), so a log reads back as the game it was.
- Part 3: the planner's career rule (`Checkpoint::staying`, a
  not-before time, `Planner::outstays`). The low lunar orbit round
  trip, re-planned halfway through its wait in LEO, keeps its orbit to
  the bit until now and leaves from it, 222.5 m/s dearer than with its
  plane free: the plane it flew is the one it has. The visit check is
  made where the planner stands, before the waits (found by its test).
- Parts 4–5: designs keyed by their inputs, chained, kept and walked in
  the background (`sim::designs`); the game and its modes' policies,
  and a hash of each day's state (`sim::game`). A step's key covers the
  steps `mission::reads` names, checked: changing the next step leaves
  each of the crew rotation's steps the same to the bit. Changes from
  the plan:
  - Monthly snapshots and the event loop move to step 2. Step 1's
    state is its designs, which a re-run takes from the cache.
  - Keys replace `resume_from`: an edit walks the ship again through
    the cache, and stepping back at a station follows from what each
    step reads.
  - A sandbox edit of a ship with re-plans folds them into its steps.
  - A re-plan whose step is not yet designed when it is made is
    accepted, and passed over once designed if it cannot take effect
    (below, the review).
  - Tests: the game designs what the planner does; 1 and 24 threads
    give the same hashes; an edit designs from the step it changes on
    (two designs for the sortie's wait: the step and the end after
    it), an undo none; career keeps every day before now, refuses a
    re-plan of a leg begun and any sandbox edit, and its log reads back
    with the same hashes in career and in the sandbox; a longer NRHO
    visit is refused.
- Part 6 moves to step 2 (2026-10-16). Neither the economy's reader
  nor keyed draws has a consumer in step 1, and the move is not
  mechanical: M2's draw is interleaved with quantizing into its
  periods (`EconomyFile::scenario`), so moving it means splitting a
  raw draw from the periods. Step 2's ship classes are its first
  consumer, and shape that split.
- Part 7: the sandbox runs on the engine. `sandbox::plan::Loaded` is a
  sandbox game with what the sandbox adds (views with their leg colors,
  window lanes, duration sweeps); its design management, the app's
  undo stack, `editor::first_change` and `mission::resume_from` are
  deleted. A sweep designs from `mission::resume_point`, the first
  checkpoint whose step reads the leg, and is kept while the ship's
  inputs are unchanged. Every sandbox test passes, unchanged in what it
  checks; the crew rotation draws as before (9,343 m/s over 51.5
  days). Designs are now held twice, in the engine's cache and in the
  views (M1c held them once), until step 2's lighter stays.
- Part 8: `sim run LOG` designs a log to its end, prints each ship's
  items, the state hashed on the first of each month, and a scenario's
  checks (`[[check]]`: a ship staying at a place at a date, a step's
  leg's Δv within bounds), exiting nonzero when one fails; `--alone`
  designs each ship alone, one after another. `orbit mission` is
  deleted. The first scenario, `data/scenarios/llo-round-trip-replanned.toml`,
  re-plans the low lunar orbit round trip in LEO in career; a test runs
  every scenario. The published plan run `--alone` gives M1c's Δv on
  every leg; its design times were taken on a loaded machine (load
  average ~20 from other work) and are not comparable to M1c's (LEO to
  the DRO 22.5 s against 16.5).
- Review (a sub-agent, 2026-10-16), each finding checked against the
  code and fixed, with a test:
  - A re-plan made later whose first step replaced comes earlier was
    passed over while an earlier one waited for its checkpoint. Now the
    last made that takes effect at a checkpoint is applied there, with
    those before it (one of which that does not take effect there only
    changed steps the last replaces).
  - A re-plan that cannot take effect (its leg had begun, or a station
    visit built shorter) is passed over: the ship flies on as before
    it (`ShipDesign::refused`), and the plan as it stands leaves it
    out. It had stopped the ship for good, and every later re-plan
    with it.
  - A career log opened in the sandbox app ran in career, and an edit
    panicked. The sandbox opens it as a sandbox log.
  - Undoing a rename left the new name on the kept ship.
  - `minute` added a minute to about one minute in eight read back
    past 2044 (f64 days hold a minute to ~4e-9 of one there), so career's
    now crept a minute on each save.
  - `sim run` did not print a failure no item says (a refused start, a
    panic), nor re-plans passed over.
  - A design kept could be let go between being made and its walk
    saying so. Each ship's keys taken are now shared with its walk,
    which adds a key before keeping a design. (A first fix polled the
    walks before letting designs go; the sandbox's tests caught it
    rebuilding a ship's lines on another's removal.)
  - A ship with no legs in a free-plane orbit, re-planned in career,
    had its plane chosen backward to its start: what it does now
    begins at now.
  - The key after a re-plan made while the ship stayed chained from
    the step before it; it chains from the replaced step, whose stay
    fixes a free plane (not reachable before, by how the cache is
    used).
  - Left as they are: undo keeps the designs of the last 64 ships
    replaced, so an undo further back designs again; two walks can
    design the same step at once (both count in `designs_made`).

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
    For a ship, that is now if it is staying (from where it is, its
    orbit as flown), or the end of the leg in flight.
  - Career reveals hidden information with time and surveys: grades,
    deposit sizes, the launch price ahead. The sandbox knows
    everything.
  - A career game's log opens in the sandbox as a sandbox log, its
    re-plans kept as made; anything in it may then change.
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
    designs run on the background pool (`sandbox::background`). In
    career the clock can't pass a departure whose design isn't done; in
    the sandbox it is free, as in M1c. Headless, the engine waits for
    designs. Either way, the result doesn't depend on how fast designs
    run.
- **Keyed draws.** Each random draw made during play is keyed by what
  it is for: demand by category and month, a loss by ship and step,
  survey noise by survey. Each comes from a ChaCha stream seeded by a
  fixed hash of the game's seed and its key. An edit then re-rolls
  nothing it doesn't touch, in either mode. The world's draw, made once
  at setup, stays M2's sequence, so M2's seeds give M2's worlds. A
  draw per parameter was considered first and rejected for that
  reason; no action can move a draw made at setup.
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
  sends the engine actions and draws its designs and state.
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

## Step 1 plan (2026-10-16): the engine

The engine without an economy: the log, both modes' policies, designs
reused by their inputs, the time-ordered loop with snapshots and the
state hash, keyed draws, and the sandbox and a headless runner on it.
Each part is committed on its own. Revised the same day after a review
against the code (a sub-agent's); what changed is noted where it
matters.

1. **Designs bit-identical,** before anything is built on them.
   - The published plan and the Mars plan are designed on a one-thread
     pool and on 24 threads (`pool.install`). Their items (Δv, times,
     markers, track samples) are compared bit for bit.
   - A ship re-planned from a checkpoint is compared bit for bit with
     a fresh design of the edited plan. M1c's test allows 1e-9; gate
     item 2 needs equality.
   - If either differs, the cause is fixed first. The parallel
     reductions keep their order (rayon's `min_by`, ordered collects),
     so differences are not expected.
2. **The log** (`sim::log`).
   - Setup: a seed (else the central draw), the mode, a start, and, in
     career, now.
   - Actions:
     - a ship: its id, name, start, epoch and steps, as M1c's plans
       have them;
     - a re-plan: a ship's steps from one of them on, taking effect at
       a time.
   - Ships get stable ids, since names change and can repeat. A ship
     without one gets one when the file is read, and it is written
     out.
   - The file is M1c's plan format with a `[game]` table and
     `[[replan]]` entries. The engine reads those two, and `Plan`'s
     parser reads the rest, from a table: `PlanFile` denies unknown
     fields. A plan without them is a sandbox log, so `data/missions/`
     reads unchanged.
   - In the sandbox, editing a ship that has re-plans edits the steps
     in effect where the edit lands. Removing a re-plan is an edit too.
3. **Career's rule.**
   - **A ship staying at now** is re-planned from now. Its last
     checkpoint, the arrival, is before now: the stay before a leg
     belongs to the leg's step.
     - It resumes from a checkpoint made at now on its stay: a low
       orbit as flown (its own orbit, `Here::Low` with its coast at
       now), or a body or surface as it is.
     - So a free plane is fixed by what was flown. A stay in one is
       coasted back from the departure, so re-designing it from its
       arrival would rewrite positions before now. Its departures are
       then those from the ship's own orbit (M1c step 7, part 8).
     - Waits count from the arrival, and departures are not before
       now. That is a new constraint in the planner, recorded in the
       re-plan and part of its key.
     - The stay's item is cut at now, and what is before now is
       unchanged.
   - **A ship in flight** is re-planned from its leg's arrival.
   - **Stations.** `resume_from` steps back from any station
     checkpoint, because the visit's reference was built long enough
     for the steps after it (`visit_until`). This covers a ship staying
     in a station orbit, one flying to one, and one that started at
     one.
     - A re-plan whose visit fits in the reference already built
       resumes there as it is.
     - One that needs a longer visit is refused until step 3's
       continuous references. (After the review: refused when made;
       passed over, the ship flying on as before, if found only once
       designed.)
   - `orbit::mission` gains the checkpoint at a time within a stay,
     the not-before constraint, and public times on `Checkpoint`.
4. **Designs by their inputs** (`sim::designs`).
   - **Keys are chained.** Step k's design is keyed by a hash of step
     k−1's key, k, and what step k reads.
     - What it reads: the steps from its checkpoint through its leg,
       through the next leg's for a leg to a station; labels and notes;
       career's not-before; later steps add economic inputs, such as
       when a depot can refuel the ship.
     - The start's key covers its place, epoch, and the steps
       `Planner::new` reads.
     - An edit at step 10 keeps steps 0–9's keys, as M1c keeps their
       items. Keying on all later steps, as first written, would have
       designed whole ships again.
     - A chain needs no hashing of checkpoints or references, and
       keys for a log exist before its designs do.
   - **A fixed hash** of our own over f64 bits, not std's, so that
     keys, draws and state hashes replay across builds.
   - `Item::seconds` (wall time) is left out of comparisons and hashes.
   - **Requests run on the host's pool:** the sandbox's background
     pool, or the default one headless.
     - In the sandbox every ship is requested to its end, as M1c does.
     - A request whose key the log no longer wants stops after the
       step it is on, as generations stop now (an edit, a closed game).
     - A panic in a design is caught and becomes the ship's failure,
       as now.
     - The loop uses a design when it reaches the step with the same
       key, and designs again when keys differ. With no economy yet,
       they never differ.
   - **The cache** keeps the designs reachable from the current log
     and the undo history, and a bounded number of others. (As built:
     the designs of the last 64 ships replaced.) The game
     owns undo now: a history of logs. An undo, a re-added ship or an
     unchanged ship in a re-run designs nothing.
5. **The loop** (`sim::game`).
   - It advances in time order through each ship's steps and a daily
     tick (empty until step 2), and snapshots its state monthly. (Moved
     to step 2: step 1's state is its designs, which a re-run takes
     from the cache.)
   - A change re-runs from the last snapshot before the earliest time
     it affects:
     - a sandbox edit: the time of the checkpoint it resumes from
       (`resume_from`'s, stepping back at a station);
     - a start changed: the earlier of the two epochs;
     - a ship added or removed: its epoch;
     - career: now.
   - The clock: free in the sandbox, as in M1c. In career it can't
     pass a departure whose design isn't done.
   - The state hash covers each ship's state at the start of each day:
     its place, position (by bits) and Δv so far. It hashes states,
     not item records, so a stay cut at now hashes as before.
   - Per ship: how far it is designed, whether it is done, and why it
     failed (a refused start, a failed leg, a panic). Also the
     status line's progress.
   - The sandbox draws designs as they arrive, as M1c does; the
     loop's state is what it has reached. The two can differ only
     once ships share depots (step 3).
6. **Keyed draws** (`sim::draw`).
   - The world's draw stays M2's sequence (`ChaCha8Rng` seeded with
     the seed, as `params::Draw::seeded`), drawn once at setup.
   - Every later draw comes from a ChaCha stream seeded by the fixed
     hash of the seed and its key; the central draw uses seed 0.
   - `data/economy.toml`'s reader and draw (`params`: `EconomyFile`,
     `Dist`, `Draw`) move here from `crates/plan`, which builds its
     periods from them. Its conditional and lazy draws keep their
     order.
   - M2's tests use only the central draw, so a few seeds' world
     parameters are captured before the move and checked after.
7. **The sandbox on the engine.**
   - Deleted: `Loaded`'s design management (generations, checkpoints,
     messages, the restart on undo), and `editor::first_change`, which
     only told `Loaded::edit` where an edit lands.
   - Into the engine: the log's files (open, empty, save), `world()`,
     each ship's status, `progress` and `wait`.
   - The sandbox keeps:
     - the view's ship (items, revision, first leg) and its helpers
       (`event_time`, `position`, `on_track`, the span, the events);
     - the plan files' list (`plans_in`, `new_plan_path`);
     - the window lanes, which its own wait includes;
     - duration sweeps. A sweep is requested from the engine, from the
       checkpoint an edit at its step would resume from. It is shown
       while the ship's key at that step is unchanged.
   - The sandbox's tests move to the new API and pass, unchanged in
     what they check.
8. **Headless.**
   - `sim run LOG` runs a log to its end and prints each ship's items
     and the state hash monthly. It replaces `orbit mission FILE`,
     which is deleted.
   - A scenario is a log with checks: where a ship is at a date, a
     leg's Δv within bounds. A failed check exits nonzero.
   - `data/scenarios/` holds the first ones: the published plan in
     career, with a re-plan at a date of a ship in LEO or low lunar
     orbit. Most of that plan's ships touch a station, which refuses
     longer visits until step 3.
9. **Tests.**
   - The same log gives the same hashes at 1 and 24 threads.
   - A sandbox edit, re-run from a snapshot, gives the hashes of a
     fresh run of the edited log.
   - Career:
     - an edit before now is refused;
     - an edit at now leaves every state before now as it was;
     - a ship parked in LEO, re-planned, keeps its positions before
       now and leaves from its own orbit.
   - A career log opened in the sandbox gives the same hashes.
   - An undo designs nothing.
