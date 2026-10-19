# M1b — The sim engine, for sandbox and career

Status, 2026-10-19: steps 1 and 2 done; step 3's parts 1–3 done (the
extended station orbits pass their kill-test, the planner designs on
them, and a leg meets a vessel on its orbit), part 4 next (the plans
below). Decided: one engine for both modes, with economy components
landing in the sandbox first (Modes, below); the four decisions below,
each as recommended; and stations as vessels, flown to where they
stay, with docking a step of its own (Design, Vessels, stations and
stocks).

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
- Step 2, part 1: `data/economy.toml`'s reader and the world's draw
  are the engine's (`sim::params`: `EconomyFile::draw` gives an
  `Economy`, in the parameters' own units); `crates/plan` builds its
  periods from one (`plan::scenario::Scenario::new`). M2's scenarios
  for the central draw and seeds 1–3, at 3- and 6-month periods, on the
  full route graph and with the NRHO as the hub, read the same to the
  bit before and after (their `Debug` text, 16 files). The route
  graph's loader moved to `orbit` (`RouteGraph::standard`), beside
  `Places::standard`.
- Part 2: a plan's ship names its class and its start's load (`class`;
  `propellant_t`, `cargo` by `lox`, `ch4`, `lh2`, `equipment`,
  `payload`, `metal`, as `economy.toml` writes outputs); `sim::ships`
  reads them and checks the ship against its class. A game draws its
  economy from its seed. Settled in building it:
  - Landing is checked by body: a class lands on a body if it lists one
    of its surfaces, so a surface written out as a table is checked too.
  - A class that is not `interplanetary` leaves no system (a planet's,
    or a small body alone), and starts in Earth's, where it is bought.
  - Loads are held to tanks and holds to the gram: amounts written in
    decimal add up only to within rounding.
  - The sandbox warns on the step's first item past its stay (a step to
    a surface designs a landing, not a leg: found in a screenshot), and
    on the ship's row for its start and steps not yet designed.
  - Career: every ship starts at LEO, with a class or without. A log
    that breaks a class, or whose class or load does not read, is not a
    game (`Game::new` says why): a career log's actions were checked
    when made, so reading one back checks them again. A re-plan is
    checked on the steps as each re-plan leaves them in the log (as
    `Log::check` counts them), not as they stand after designs pass one
    over, so that the check made when it was made is the one made on
    reading it back.
  - A new name, class or load designs nothing (tests count designs),
    and keeps the ship's re-plans in the sandbox.
  - Review (a sub-agent): a new class folded a career log's re-plans in
    the sandbox, and the ship flew from its start without them; amounts
    in decimal at a hold's capacity overfilled it; a re-plan's breach
    was numbered by the ship's steps, not the re-plan's legs; messages
    rounded an overage away and called a small body a planet. Each
    fixed, with a test.
  - Found by the same review, from step 1: a career game took a re-plan
    from past the steps its log has, counting them as they stand (a
    re-plan passed over left out), and the log it wrote did not read
    back. Every action's log is now checked as reading it back checks
    it (`Log::check`), so such a re-plan is refused, as the log counts.
- Part 3: the state and the loop (`sim::state`), run again from the
  start whenever designs or the log change; the day's hash adds each
  ship's tanks and hold and whether it ran dry. Settled in building it:
  - What burns: a leg's, landing's or ascent's markers, and a station
    stay's Δv. A capture's circularizing burn does not (its leg's pass
    counts the rule's Δv, as its note says), nor a jump. Each leg's,
    landing's and ascent's markers add up to its Δv (checked over the
    published plan).
  - A burn the tanks cannot make is not made: the tanks keep what they
    had, and it falls short by its Δv less what they could give at the
    mixture ratio (the scarcer of LOX and fuel, once hydrogen has
    boiled off).
  - Upkeep and boil-off are paid at each event, before it: a burn, a
    midnight's tick, and where the loop stops. A stay's upkeep is its
    Δv spread evenly over it; boil-off is each fuel's at its
    propellant's rate (hydrogen's, methane's none), part-days and all.
    With a tick each midnight, upkeep that runs a ship dry strands it
    within the day. Between events a load is its last boiled off since, and with
    every ship designed, the loop runs to the last item's end and
    hydrogen boils off past it.
  - An unfinished ship's next step may burn at the time its design has
    reached, so the state is pending from it, not after it.
  - Measured (release, load ~24): the published plan, every ship a
    hauler (18 ships, 13,379 days from 2031 to 2067), the loop alone
    2.8 ms, a new load 2.9 ms with the game's own work; 50 ships of
    2,000 items each over as long, 59–78 ms. Against the gate's 1 s:
    no snapshots. Designing the published plan took 27.4 s
    (`tests/state.rs`, ignored: a measurement).
  - Review (a sub-agent): each tick read every item (2 s for 50 ships
    of 2,000 items); a stay's last part-day of upkeep was paid at the
    next midnight, after the leg's burns, and not at all at the game's
    end, so a ship could run dry in a stay it had left; hydrogen stopped
    boiling at the last item; the state at the time an unfinished
    design reached counted as known. The loop was rebuilt to pay at
    each event, each fixed with a test. A second review, mutating the
    rebuilt loop, found five of its rules no test held (the payment
    where the loop stops, its boil-off before a burn, a burn at the
    game's end, no upkeep once dry, the earlier of two unfinished
    ships), and a burn a moment before a ship's start recorded out of
    order. Each now has a test that its mutant fails.
  - Part 7's tests for burns, upkeep, boil-off, running dry and hashes
    are written with it: `crates/sim/src/state.rs` and
    `crates/sim/tests/state.rs` (1 and 24 threads; a new load designs
    nothing and gives a fresh game's), and `crates/sim/tests/game.rs`
    (a career log read back, and in the sandbox, tanks and all).
- Part 4: the sandbox.
  - The editor: a class menu ("none": massless, carrying nothing; each
    class with its propellant, tanks and hold), and, with a class, the
    start's propellant (empty: a full tank) and cargo by commodity,
    typed as the engine reads them. A new class or load designs
    nothing (the ship keeps its revision); the loop runs again. An edit
    the game refuses is said in the editor (`Loaded::edit` returns why).
  - The panel: the ship's class and its tanks and hold at the clock;
    each item's propellant (a stay's upkeep), and what a burn left;
    "tanks pending" where the loop has not run.
  - A ship that ran dry: said on its row and on the item (how far
    short, in mm/s for a day's upkeep), the items after it "not flown".
    On the map, the burn marked with a red block, the dry item's path
    after then dimmed (split there, but for a stay, which is drawn a
    revolution about the clock), and its later items and markers dimmed
    on the map and the timeline. A dry found where the state is pending
    is not shown: it may move as designs arrive. A ship's lines are
    built again when where it runs dry changes, its design the same (a
    test fails without it).
  - Seen in screenshots: a lander loaded short in the panel and on the
    map (a scratch plan, `sandbox --plan … --screenshot`); the class
    menu and the load fields (`OPTIONS_SHOT=dir cargo test -p sandbox
    --test editor`).
  - Review (a sub-agent): undo kept the editor's drafts, so a load typed
    and then its class undone was committed to a classless ship and
    panicked (and a wait's draft outliving its step, since M1c); a
    stay that ran dry was split into two halves each drawing its
    revolution; a pass's or jump's marker after it was not dimmed; the
    ship row said "at at" and an upkeep's shortfall "0.0 m/s"; the
    dry burn's mark looked like a jump's. Undo drops the drafts, a load
    needs a class and a wait a wait, and the rest is fixed, each with a
    test. Cleanup: the `bool` `Loaded::poll` returned, unread since step
    1 part 7, is gone with those under it.
- Part 5: headless.
  - `sim run` prints, under each item of a ship with a class, the
    propellant it burned (a stay's upkeep) and what that left, to the
    kilogram; where it ran dry, how short, and "not flown" after; and
    the ship's tanks and hold at its start, on the first of each month,
    and at its end. Per item, as the panel shows it, rather than per
    burn: a leg's burns are listed by Δv beside it.
  - Checks: `propellant_t = [lo, hi]` at a date; `step = n, dry = true`,
    or `dry = false`. Steps count from 0, as the Δv check's do; a ship
    that runs dry keeping station does so at the wait it was in, else at
    the leg that leaves the stay.
  - A ship that ran dry is, for `At`, stranded where its failed burn
    would have left from, if that was its item's first (a departure, a
    descent, an ascent): at the stay before it, or at the place the
    item before it arrived at. Run dry later in a leg, it is in flight;
    run dry keeping station, it drifts. Either way it is at no place.
  - Scenarios, each in career from LEO, their propellant computed by
    hand from the burns' Δv as designed, within what 20 m/s more or
    less in all would burn: `tug-llo-round-trip.toml` (a hydrolox tug
    to low lunar orbit and home by aerocapture; its hydrogen's boil-off
    checked to the kilogram in LEO, before any burn a design could
    move), `lander-south-pole.toml` (a methalox lander down and up;
    400 t after a day and a half, nothing boiled),
    `lander-loaded-short.toml` (200 t: it lands with 6.7 t, runs dry at
    the ascent, 1,317.2 m/s short, and is still at the south pole
    after).
  - Review (a sub-agent, with probes of each item shape `stranded`
    meets: after an aerocapture and a jump, an aero entry as the first
    item, career re-plans, a refused one): a ship that ran dry keeping
    station through a wait was said to run dry at the step after it; the
    scenarios' bounds were wider than their comments said, and the
    comments left out some boil-off. Fixed, the first with a test.
    Cleanup: the month walk `sim run`'s hashes and tanks share
    (`report::months`), and where a ship ran dry, said by the engine
    and the panel alike (`Dry::place`).
  - Left as they are: a stranded ship stays at its place to the end of
    the game, while one that finished its plan is at no place after its
    last item (as before). `At` and the propellant check are pending
    until the loop has run past their time over every ship, though a
    ship's tanks depend only on its own design (ships share nothing
    until step 3); `sim run` waits for every design.
- Part 6: lighter stays (the design below was written before building
  it, and held).
  - Before (release, load ~8): the published plan through `sim run`,
    93 MB at its peak and 26.3 s; a ship parked a year in LEO and one a
    year in the NRHO (`sandbox/tests/playback.rs`, `parked_a_year`),
    212 MB through `sim run`, and in the sandbox 163 MB designed, 1.3 GB
    once drawn about Earth, the first frame 332 ms, frames at an hour a
    second 1.3 ms median and 57 ms the longest.
  - A track holds one of three motions: samples (legs, captures, a ship
    resting); a coast (a stay in a low orbit): its field, and states
    kept every 6 h (at 00, 06, 12 and 18) from the planner's one
    integration, its first and last among them; or a reference (a
    station stay): its visit's, shared. Its samples are made when asked
    (`Track::samples`, every 2° over a span), and `Track::at` makes the
    state at a time: a coast's integrated from the kept state before it
    over that state's 6 h, read at the time, so that both agree.
  - The planner steps through a stay's integration without keeping its
    interpolants; the state a leg leaves from is as before, to the bit.
  - The sandbox builds a stay drawn a revolution about the clock from
    samples made about it, two revolutions each way (clamped to the
    stay; `paths::window`), and builds it again when the clock leaves
    their middle; the ship's other lines are kept (`paths::rebuilt`).
    The ship's marker rides the stay's line, as now; with the ships'
    lines hidden, a track of samples made about the clock for the stay
    it is in.
  - `sim run --tracks` makes each stay's samples for its CSV.
  - Checked: a stay's kept states are its one integration's to the bit,
    and its states between are within 0.4 mm of it in LEO and 89 mm in
    low lunar orbit, where integrating again in Earth's frame is that
    noisy (the plan asked for a meter); the career rule's past is kept
    to the bit; designs are unchanged (`sim run` prints the same items
    for the published plan, the crew rotation and the step 1 scenario).
    Hashes differ where a stay spans the time hashed: a station stay is
    read from its reference, and a coast from its kept state or
    integrated from it, rather than between samples. A day's hash reads
    a kept state: 365 midnights over the two parked ships take 0.5 ms.
  - After (load ~15–40): the published plan, 90 MB at its peak (it has
    few long stays; the review's runs, 86 MB before and 76 after); the
    parked ships, 47 MB through `sim run` (42), and in the sandbox 49 MB
    designed. Drawn, the sandbox holds 1.0 GB, as it does with the ships'
    lines hidden: the rest is the renderer's (the stays' lines took
    ~290 MB before). Frames as before: 1.1–1.6 ms median at an hour, a
    day and a week a second, 3.9 ms the longest at a day a second (where
    the stay is built again ten times a second); the longest frames,
    55–63 ms at an hour a second and 30 ms at a week, are there with the
    ships' lines hidden too, so not the stays'.
  - Review (a sub-agent, with probes of `at` and `samples` at every
    edge, the departure state against the old code's to the bit, and
    the sandbox's revolution drawn about the clock at every rate): a
    coast of a minute or two could keep its end twice, its last step
    ending a rounding past it, and panic; a window ending at a kept
    state had it twice; the sandbox built a ship's every line again
    when a stay's samples were made again (the ledger said they were
    kept); with the ships' lines hidden, a coasting ship's marker
    integrated its coast twice a frame (0.59 ms a frame, now 0.016); a
    window clamped to a stay's end could round short of it and be built
    again every frame after. Each fixed: the first three with a test,
    the fourth timed (its test checks the marker's place), the last by
    deciding from the clamp rather than from the end it rounds to.
- Part 7: tests. Most were written with the parts they test: burns,
  upkeep, boil-off, running dry and the hashes with part 3
  (`crates/sim/tests/state.rs`), the class checks with part 2, a new
  load designing nothing with parts 2 and 4. Added where the plan's
  list was not yet met:
  - Every item of the lander's sortie burns what a hand computation
    from its Δv gives, from the ship as it begins; only the first leg
    was checked. The hand computation takes 1 − e^(−x) as −expm1(−x),
    as the engine does: the sortie's NRHO stay burns 5 g of upkeep, and
    1 − e^(−x) lost eight digits of it.
  - Each class check (tanks, hold, landing, aerocapture, leaving
    Earth's system), broken alone by a ship from LEO: refused in career,
    a warning in the sandbox. Career's refusal was tested for landing
    and aerocapture, the sandbox's warning for landing and the hold.
  - The sortie of `crates/sim/tests/game.rs` is a hydrolox lander, so
    its hashes cover tanks and boil-off: at 1 and 24 threads, and an
    edit of its steps, re-run, against a fresh game's and undone. A new
    load's re-run was covered; an edit of the steps was tested only on
    a ship without a class.
- Part 8: measurements (2026-10-17, release).
  - Design times, owed from step 1: the published plan `--alone`, three
    runs at load 3.4–6.8 (most of it the runs' own threads): LEO to the
    DRO 16.7–16.9 s, every other leg 0.1–3.7 s, as M1c's (16.5–16.9 s;
    0.1–3.7). Step 1's 22.5 s was the load. Designed as one game
    (`sim run`), 25.0 s (26.3 before part 6).
  - The loop's re-run on the published plan, every ship a hauler (18
    ships, 13,379 days; `tests/state.rs`, ignored): the loop alone
    2.7–3.3 ms, a new load run again 2.9–3.4 ms; 50 ships of 2,000
    items 55–76 ms. Against gate item 4's 1 s: no snapshots.
  - Checked against part 3, alternately under the same load (another
    program using about 2 of the 24 cores): its loop alone 3.1–3.7 ms,
    and every leg's design time the same as now to 0.1–0.2 s (LEO to the
    DRO 17.5–18.8 s for both under that load). A first run during a
    build (load 27) read 4.9 ms for the loop and 50.7 s for the
    designs.
  - The full workspace suite passes at the end of step 2: 313 tests, 12
    ignored (measurements), in 5 min 54 s.

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
     sustained deficit. Step 6 measures it on standing routes'
     departures (moved from step 2, which has none), and gate item 4
     holds it.
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
- stations and depots as vessels, and docking; plants, technology
  programs and surveys (M2's data);
- third-party traffic and the Access Cost Index;
- money, Standing, mandates, crew and Supplies, and ship losses;
- pricing authority and revenue bonds (decision 2);
- inventory projections, the lead-time instrument M3 draws;
- in the sandbox, the least UI that shows each component as it lands
  (rows in the plan's panel);
- scenario scripts and a headless CLI.

Out:

- career's UI: inventory projection panels, the ledger, the economy's
  actions (M3); contracts, firms and concessions (M4);
- mid-flight redirects (eventually; Modes, above);
- legs that fail M1a's gate: between stations, slow and near-180°
  departures, and LEO to the DRO's design time. That is playbook work,
  taken up when the economy shows it needs those legs;
- meeting a vessel in a low orbit, whose plane and phase the target
  fixes: no transfer type in the playbook arrives so. Playbook work
  too;
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

### Vessels, stations and stocks

Decided 2026-10-17 by the user. It replaces a station placed by an
action, its orbit built from then:

- **Ships and stations are one kind of object, a vessel:** anything not
  on the ephemeris's rails. A station is a vessel that stays, flown
  there by a mission like any ship. How it is built depends on the
  station: a big stage, like a parked Starship, is one vessel; an
  assembly, like the ISS, is modules flown there and docked (step 4).
  - Rejected: placing a station by an action. It left open what a
    station is made of and how it got there.
  - Rejected: one orbit for each station place for the whole game
    (step 3's first draft). A fly-through, or a second station
    arriving, would be put on the first one's orbit. Both rejected
    forms kept each ship's design independent of the others'; a ship
    that meets a vessel now reads that vessel's design.
- **Going to an orbit** (`to = "nrho"`) gives the ship an orbit of its
  own in that family, as in M1c. **Going to a vessel** meets it: the
  leg arrives on the vessel's orbit, at its phase.
- **A vessel staying in a station orbit keeps one continuous orbit**
  from its arrival on, built from the catalog orbit as now and extended
  a year at a time (`Reference::extend`, anchored on the solve before;
  step 3 plan, part 1) as far as its stay and its visitors need, with
  upkeep simulated as it goes. Extending is new: `continue_orbit`
  starts from the catalog orbit, so M1a's yearly pieces and M1c's
  visits are each a separate orbit. M1a left the continuous reference
  to M1b.
- **Stocks are what vessels carry.** Loading, unloading and refuelling
  happen between two vessels that meet, as the arriving one's manifest
  says. At LEO, anything can be bought from Earth at its price plus
  launch. Earth's surface is not a place, as in M2.

### Plants, programs and surveys

- As in M2 (`data/economy.toml`). Equipment delivered to a site and
  installed adds capacity after the build time. Output goes daily into
  the site's stock (with stocks in vessels, step 5 decides which), and
  deposits run out. A plant can be installed only once its technology
  program is finished.
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

- What each vessel holds over time, from what is scheduled: planned
  arrivals and departures, plants at capacity, crew consumption,
  boil-off, and third-party demand at current prices (its mean, not a
  draw). Missions and standing routes are checked against it, and M3
  draws it.

### Scenario scripts and the CLI

- **A scenario** is a career log written by hand: a seed (or the
  central draw), a start, actions at dates, and checks.
  - Actions: buy a ship (a station is one), deliver and install
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
     NRHO-to-Mars payload goes through LEO, and its hand-off in low
     lunar orbit through a station or the surface (Out, above); the
     comparison counts both.
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
2. **Ships with mass:** the parameters moved from `crates/plan`; the
   state and the loop, with its daily tick (both from step 1); classes,
   purchase at LEO, missions with a class and a manifest, the rocket
   equation at burns, upkeep, landings and ascents, and boil-off. Stays
   in their lighter form. In the sandbox: each burn's propellant, and
   where a ship runs dry.
3. **Stations and stocks:** a station is a vessel that stays; legs
   that meet a vessel; a staying vessel's orbit, continued ahead;
   transfers between vessels, purchases at LEO, and projections.
4. **Docking:** vessels that move as one, so that a station can be an
   assembly.
5. **Plants, programs and surveys,** with keyed draws (survey noise is
   the first); in career, grades hidden until surveyed.
6. **Standing routes.**
7. **The cheapest cost, the Access Cost Index and the traffic
   generator.**
8. **Money, Standing, mandates, crew, losses and powers.**
9. **The gate.**

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

## Step 2 plan (2026-10-16): ships with mass

Ships get a class from `data/economy.toml`, a load at their start, and
tanks that their burns empty. The engine gets its state and the loop
that advances it. Ships share nothing yet (depots are step 3), so a
ship's tanks depend only on its own design, class and load. Each part
is committed on its own.

1. **The parameters move to the engine** (`sim::params`; step 1's part
   6).
   - `data/economy.toml`'s reader and the world's draw (`EconomyFile`,
     `Dist`, `Draw`) move from `crates/plan`. The draw gives one game's
     parameters in their own units (`Economy`): launch $/kg at the start
     and end of its decline, build months and program years, survey
     costs as quoted, demand's reference quantities.
   - `crates/plan` builds its periods from that (`Scenario`): the
     calendar, launch price by period, build and lead periods, survey
     costs divided by the deposit's prior, demand's price segments.
   - The draw keeps M2's order, so M2's seeds give M2's worlds. Before
     the move, the scenarios of the central draw and seeds 1–3 are
     written out (their `Debug` text); after it, they read the same to
     the bit. M2's tests pass unchanged.
   - Keyed draws wait for their first consumer, survey noise in step 5.
     Nothing in step 2 is random after setup.
2. **Classes and loads in the log** (`sim::ships`).
   - A ship names its class. Its start says what it carries: propellant
     (t; a full tank by default, at the class's mixture ratio) and cargo
     by commodity (LOX, methane, hydrogen, equipment, payload, metal):

     ```toml
     [[ship]]
     id = 0
     name = "Tug"
     class = "tug-h"
     start = { at = "leo", epoch = "2045-03-01", propellant_t = 120.0, cargo = { equipment = 20.0 } }
     ```

     `orbit`'s `Ship` keeps these as written, as `Plan` keeps `[game]`,
     and `sim` reads them. They are not design inputs.
   - A ship without a class is massless, as in M1c, so `data/missions/`
     reads unchanged.
   - What a class allows, checked against the ship's steps: aerocapture
     and entry (`aero`), landing at a surface (`lands`), legs between
     planets (`interplanetary`), and a load within its tank and hold.
     - Sandbox: a warning on the step, as a jump has now.
     - Career: an action that breaks one is refused when made. A career
       ship starts at LEO, where it is bought.
   - Prices wait. A purchase's cost needs the launch price in time,
     which comes with step 3's purchases at LEO; the account is step
     8's.
3. **The state and the loop** (`sim::state`).
   - The state: each ship's tanks (LOX and fuel) and hold, and whether
     and where it ran dry.
   - Events in time order: each burn of a flown leg (its markers:
     burns, flybys, passes, landings, ascents), ties broken by kind,
     then by ship id; and a daily tick at the start of each day.
   - **A burn** of Δv uses m(1 − e^(−Δv / g₀Isp)) of the mass before
     it, LOX and fuel at the mixture ratio. A jump is not burned: it is
     a junction not flown, flagged as now.
   - **The daily tick.** A station stay's upkeep (its item's Δv, spread
     evenly over the stay) for the part of the day stayed, by the same
     equation. Hydrogen boils off at `fuel_boiloff_per_day`, in tanks
     and holds. LOX and methane don't (`economy.toml` takes them as
     storable).
   - **Running dry.** A burn the tanks can't make strands the ship there.
     The loop records the burn and how short it fell (m/s), and the
     ship's design after it is not flown. Both modes do this in step 2;
     career's refusal of a plan that would run dry, when it is made,
     comes with step 3's projections.
   - The loop runs to the earliest time a ship's design has not reached;
     beyond it the state is pending.
   - Any change re-runs it from the game's start. Snapshots wait until a
     measurement says a re-run is too slow (gate item 4: 1 s over the
     whole game); the re-run is timed here on the published plan.
   - The state hash adds each ship's tanks and hold, by bits.
4. **The sandbox.**
   - The ship's section: a class menu ("none": massless) and the
     start's propellant and cargo.
   - The panel: each burn's propellant (t) on its row, and the tanks
     after each leg; the ship's row, its tanks at the clock.
   - A ship that runs dry: the burn marked, and its path after it drawn
     as not flown (dimmed).
   - An edit of the class or the load re-runs the loop and designs
     nothing.
5. **Headless.**
   - `sim run` prints each burn's propellant, and each ship's tanks
     monthly.
   - Checks: a ship's propellant at a date, within bounds; a ship runs
     dry at a step, or not at all.
   - Scenarios: a hydrolox tug from LEO to low lunar orbit and back,
     boiling off between; a lander landing at the south pole and
     ascending again; a ship loaded short, which runs dry.
6. **Lighter stays.**
   - A stay keeps a sample every 2° of its orbit: ~2,900 a day in LEO,
     ~160 kB a day, held twice (the engine's cache and the sandbox's
     views, step 1 part 7).
   - A stay in a low orbit keeps its first state, and states taken from
     its one integration [about a day apart; set by the sandbox's draw
     time]. Samples are made when asked: `Track::at` integrates from the
     kept state before the time asked, and the sandbox draws the
     revolution about the clock from samples made then, again as the
     clock leaves it. A station stay reads its visit's reference rather
     than copying it. Departures still leave from the state the planner
     integrated.
   - Checked: positions agree with today's samples [within 1 m, to
     confirm]; the published plan's memory before and after; the
     sandbox's frame time with the clock running through a long stay.
7. **Tests.**
   - Burns: each burn's propellant matches a hand computation from its
     marker's Δv; a station stay burns its upkeep; a hydrolox ship
     parked n days keeps (1 − f)ⁿ of its hydrogen, and a methalox one
     all its methane.
   - A ship loaded short runs dry at the right burn, short by the right
     Δv.
   - Each class check is refused in career and warned in the sandbox.
   - Hashes, now with tanks: the same at 1 and 24 threads; a career log
     in the sandbox gives the same; an edit, re-run, gives a fresh run's.
   - An edit of the load designs nothing.
8. **Measurements.**
   - Design times on a quiet machine, owed from step 1: the published
     plan `--alone` against M1c's.
   - The loop's re-run time on the published plan.
   - Decision 1 had step 2 measure design throughput at a month a
     second. That needs standing routes' departures, so it moves to
     step 6.

Left out of step 2: refuelling and loading at stops (step 3, with
stocks and purchases at LEO), turnaround days (step 6, with trips
counted), and trip costs (step 8).

## Step 3 plan (2026-10-17): stations and stocks

A station is a vessel that stays, flown there like any ship (Design,
Vessels, stations and stocks: decided with the user, with docking a
step of its own). Ships meet vessels, take and give what they carry,
and buy from Earth at LEO. Each part is committed on its own.

Ruled with the user (2026-10-17): manifests on the leg that arrives
(part 4); career refuses a plan that runs a vessel dry or short (part
6); a meeting names the vessel's leg (part 3); a station keeps no
reserve (part 5).

What is hard, and where (found in discussing the first draft):

- A ship's design reads another's: a leg that meets a vessel arrives on
  its orbit. Today each ship is designed alone, its keys from its own
  steps (part 3).
- A staying vessel's orbit has to reach as far ahead as its stay and
  its visitors need. Today a station orbit is built for one planned
  stay, and a station has no one orbit for its life (parts 1–2).
- Meeting a vessel in a low orbit needs a leg the planner does not
  have: the target fixes the plane and the phase. Out of M1b (Scope).

Found in part 1's first kill-test (2026-10-17), and the design that
follows from it:

- A solve from the catalog orbit lands on the member of the family
  nearest its guess, not on "the" orbit. Year-long pieces solved a year
  apart differ by 240–900 km (the halos), 700–38,000 km (the NRHO) and
  1,400–35,000 km (the DRO, stable: any nearby torus is a solution).
  The station-keeping loop switched from one piece to the next paid
  0.04–5.5, 3.5–9.7, 4.7–111 and 17–63 m/s a switch, against yearly
  upkeep of 3.1–3.7, 4.5–5.0, 0.8–5.4 and 0.6–0.8 m/s. A station's
  orbit must therefore be extended from the one it is on, never solved
  afresh.
- A solve's end is free, so its last revolutions leave the orbit by the
  guess's error divided by the unstable direction's growth per
  revolution: ×68 (L1 halo), ×353 (L2 halo), ×1.9 (NRHO), ×1 (DRO),
  from the catalog's stability index. Every join tried was made at a
  piece's end, where it is already leaving. Rejected: a join fixed in
  position (it is a maneuver, m/s); fixed in state (forward flight, an
  unstable orbit leaves); pieces solved apart with the loop paying the
  switch (the costs above); a controller with no reference (the
  prototype failed within days at hundreds of m/s a year, cause not
  found; it would also make a station's future path a simulation
  output every visitor's design must run).
- Chosen: anchored extension. A reference is solved `settle` revolutions
  past its usable end and the first of them, the anchor revolution, is
  kept. Extending it solves the next revolutions together with the
  anchor revolution, whose patches are weighted to stay
  (`ANCHOR_SCALE`: moving one 1 m costs the minimum-norm step as much
  as moving a free one 10 km), and `settle` more beyond, the first of
  which is the next anchor. `settle` = 1 + ceil(ln 10⁷ / ln λ) for the
  growth λ per revolution (L1 5, L2 4, NRHO 26); an orbit that would
  take more than 40 settles with 2, the anchor and the free end (the
  DRO: ν = 1 + 2·10⁻¹⁰ in the catalog; solves that long do not converge
  either). A catalog guess's arcs break by up to 10⁻¹ scaled (an NRHO's
  8·10⁻², 30,000 km), and that has decayed to the shooting's tolerance
  (10⁻⁸) at the anchor's end. The usable path never changes once
  extended over. The station-keeping loop is unchanged, and its burns
  across a seam are the test of the whole.
- Found building it (2026-10-17):
  - The minimum-norm step minimizes itself, not the distance from the
    guess: with the guess's whole residual to fix, the first iterations
    moved the anchor 150 m and nothing moved it back. The step now
    counts the anchor's displacement from where the solve before put it
    as its own, so each iteration draws the anchor back. Measured
    anchor moves: halos 10⁻¹⁰–10⁻¹¹ scaled, NRHO 2·10⁻⁹ (at
    `ANCHOR_SCALE` 10⁻³ it was 3·10⁻⁸, its share of the free patches'
    moves), DRO 0.
  - The guess past the anchor: the anchor's last patch flown on for
    `settle` − 1 revolutions, as long as the departure it inherited
    stays below the guess error (for ever on the DRO, whose settle is
    the least), then the catalog orbit phased on from the anchor.
    Rejected: flying on while within 10⁻¹ of the catalog orbit (a DRO's
    solved member sits that far from the catalog's, so its guess fell
    back to the catalog and the solve wandered off); the anchor
    revolution carried on in the rotating frame (its patch times, shifted
    by the solve to its own lunar distance, mismatch later revolutions';
    an NRHO's arcs broke by 30,000 km and the line search failed);
    fixing the anchor's first patch (an L1 extension then does not
    converge).
  - An anchored solve must connect to one state, and a long one of the
    NRHO does not converge from the catalog guess (83 revolutions stall
    at 10⁻⁴ residual; 54 converge slowly; 34 in two seconds), where a
    halo's year (36) does. `extend` solves in steps of at most 40
    revolutions with the settle, halves a step that does not converge,
    and keeps the size that did for the steps after: past its first
    years the NRHO's steps of 14, 7 and 4 revolutions fail nearly every
    time and 2 converge, each failure costing seconds (a year took 118 s
    with every step tried from 14). Its tail guess is the catalog orbit
    moved onto the anchor's member, by the anchor's offset from the
    catalog at the same phase (the solved member sits up to 40,000 km
    from the catalog's; from the bare catalog the NRHO's fifth year did
    not converge at any step size). Rejected: anchoring loosely first, then
    tightening from the converged solution (the loose pass displaces the
    anchor in directions the stiff pass can only undo by changing the
    whole free part, prohibitive on an unstable orbit: 3% back an
    iteration).
  - A first solve that does not converge is retried a day or two later,
    as the gate and the planner already retry.

1. **Extended orbits, a kill-test first** (`orbit::reference`).
   - `continue_orbit` takes `settle` (`settle_revolutions` from the
     catalog's stability index; 1 for a reference left as built) and
     keeps the anchor revolution; `Reference::extend` adds revolutions
     as above. Patches per revolution become a constant (16, as every
     caller passed). The first draft's chained form (a chunk's first
     patch fixed to the end of the one before) is dropped.
   - Each station orbit from 2045-03-01, extended a year at a time over
     34 years (to 2079; the ephemeris ends in 2080), each orbit on its
     own thread
     (`station_orbits_extended_for_years`, ignored; the short form,
     `extended_orbits_run_on`, in the suite). Checked:
     - the anchor's move and the seam's discontinuity, both within the
       shooting's tolerance (10⁻⁸ scaled: ~4 m, 0.01 mm/s);
     - the shape held every year (distance from the Moon as M1a step
       5b);
     - the time per year built, and the memory per year;
     - the loop across seams: a ship held to the extended reference
       (`stationkeeping::simulate`'s loop, Δv bucketed before, in the
       four revolutions after, and after each seam), the seam's extra
       within a tenth of a year's upkeep; `upkeep` of two years against
       the first year's.
   - If an orbit fails, stop and report before part 2.
   - Measured (2026-10-18; release, each orbit on its own thread, its
     arcs on all 24 cores, other work on the machine):

     | Orbit | Years | Build a year | Path a year | Anchor moved | Seam | Shape held | A seam's four revolutions over the rate after (upkeep a year) |
     |---|---|---|---|---|---|---|---|
     | L1 halo | 34 | 0.4 s | 1.9 MB | ≤ 3·10⁻⁹ | ≤ 2·10⁻⁹ | 34 of 34 | +0.04 to +0.40 m/s (3.4–3.6) |
     | L2 halo | 34 | 0.2 s | 1.6 MB | ≤ 2·10⁻⁹ | ≤ 2·10⁻⁹ | 34 of 34 | +0.01 to +0.15 (4.7–4.8) |
     | NRHO | 34 | 5.8 s | 4.2 MB | ≤ 6·10⁻⁹ | ≤ 4·10⁻⁹ | 34 of 34 | −0.09 to +0.01 (1.8–3.2) |
     | DRO | 34 | 0.1 s | 1.5 MB | 0 | ≤ 4·10⁻¹² | 34 of 34 | −0.03 to +0.09 (0.56–0.64) |

     Anchors and seams are scaled (10⁻⁸ is ~4 m). Upkeep over the first
     two years matched the first year's within the Monte Carlo's noise
     (L1 3.40 against 3.61, L2 4.77 against 4.67, NRHO 2.07 against
     1.61, DRO 0.56 against 0.64 m/s a year), and a seam's four
     revolutions cost what any four do, over three seeded runs: the
     loop does not see a seam. The rows are the rerun after the fixes
     and the speed-up below. The NRHO's first run built 24 years at 44 s
     a year and failed in its 25th at every step size; fixed, it built
     34 at 50 s a year.
   - **The NRHO failed the kill-test** (2026-10-18): its extension
     converged only in steps of one or two revolutions and its 25th year
     did not converge at any step size. Stopped, as planned, before part
     2; diagnosed and fixed the same day, below. (The perilune drift
     reported then, 2,733 to 2,322 km, was not real: see the diagnosis.)
   - **Diagnosed** (2026-10-18, a scratch test since deleted; its
     outputs stayed in the session's scratchpad):
     - No drift. The 2,322 km was the path's 24-year minimum sampled
       300 times a year (one sample per 29 h, where a perilune pass
       lasts hours) against the CR3BP's 2,733 km; the same sampling is
       why the shape check failed 3 years (on the first year it reads
       2,399 km where the refined perilune is 2,134 km). Refined, the
       perilune swings 2,090–3,560 km from one revolution to the next in
       an ~8-revolution cycle, the same band in year 0 and year 23; the
       yearly minimum is 2,089–2,319 km with no trend, the period
       6.349–6.368 d every year (the catalog's 6.48 d in its own time
       unit is 6.358 real days), the mean Jacobi constant
       3.0485–3.0496. Chains started in 2050, 2055, 2060 and 2065 and
       fresh catalog solves at years 5–23 sit in the same band. The
       chain is ballistic: 22,000 arcs each continuous to 1e-8 scaled
       (1e-5 m/s) are at most 0.2 m/s in all, 17 km of perilune at the
       NRHO's 79 km per m/s at apolune. The swing is the Sun's phase
       beating against a non-resonant period: this member makes 4.65
       revolutions a synodic month, where the 9:2 of M1a (6.56 d) lies
       between catalog samples and `nearest` picks the 6.48-day one (the
       published 15-year Gateway reference holds 3,195–3,557 km). Over
       34 years the yearly perilune cycles 2,179–3,556 km with the
       Sun–Earth–Moon geometry (lowest in years 13 and 32, highest in
       years 1 and 21), no trend. The tests' range checks now refine
       every local extreme of their samples (`extremes`).
     - The large steps' failures are the Newton's. At every failing line search the
       sum of squares falls as the linear model predicts (the linear
       solve is exact to 1e-7), but two adjacent patch epochs have been
       driven together (gaps of 0.00–0.04 h, from 9.5 h) and the trial
       that would cross them is rejected down to 1/64. Converged solves
       show the same squeeze: the shortest arc is under 3.8 h in a tenth
       of them, 1.0 h at the least, where the guess's is 8.2 h. Epoch
       unknowns are scaled by the CR3BP time unit (4.4 days), so moving
       a perilune patch's epoch an hour (9e-3 scaled) is the cheapest
       way to close a 7,000-km break (2e-2) at 2 km/s, and the
       minimum-norm step takes it every iteration. More catalog
       revolutions in a step are more perilune arcs and more iterations,
       so more chances to close a gap: steps of 4 and more fail nearly
       always, 2 usually converge, 1 rarely fails (a chain from 2065
       failed in its second year the same way). Ruled out: the anchor's
       stiffness (at `ANCHOR_SCALE` 1 and 1e-2 the solves fail the same
       way at 1e-7 residual); the flown part (without it, or 5 or 10
       revolutions shorter, the first extension fails at every step
       size); the member offset (without it 14-revolution steps converge
       in year 1, 15 s, and year 3 fails at every step size). The
       guess's own breaks: the catalog tail's perilune arcs 1e-1–2e-1
       scaled (the catalog cannot know the swing's phase; the member
       offset at perilune patches is 5,000–14,000 km, mostly phase), the
       junction with the flown part 1e-2–3e-2 and 2.8 h of phase. The
       25th year's failure was another's: see below.
   - **Fixed** (2026-10-18): two faults of the solver, neither the
     epoch scaling.
     - The 25th year's failure was the linear solve's. With the Newton
       converging quadratically (2·10⁻², 4·10⁻³, 6·10⁻⁵) the banded
       Cholesky of J Jᵀ met a non-positive pivot: the anchored rows are
       10⁻⁸ of the free ones, and where rows are nearly dependent
       cancellation swamps their pivots. `min_norm_sparse` retries with
       the diagonal raised by a growing share (10⁻¹⁰, ×100 a retry, up
       to 10²). With that the chain reached its 33rd year and crawled:
       at 3·10⁻⁴ residual the full step raised the sum of squares
       4,000-fold at one arc of the tail's first revolution, each 1/32
       step gained 3%, and 40 iterations ran out. The step is now damped
       (Levenberg–Marquardt, the same diagonal share: 10⁻⁶ and ×100
       while no size down to 1/64 reduces the sum of squares, ÷10 after
       a step that does). All 34 years build; the halos and the DRO are
       unchanged.
     - Measured and rejected first, each on the suite, a six-year chain
       and where it got that far the 34 years: epochs scaled by the
       shorter of their two arcs (too stiff: an NRHO's fresh solves
       fail, since a long solve's far end must shift its epochs hours
       for the real period); by the length unit over the speed (the
       chain failed in its sixth year); by √(length unit /
       acceleration), the time an epoch's move takes to turn nonlinear
       (no faster, failed in its eighth year); the step capped so no arc
       shortens by more than half (slower, no better); the epochs of any
       arc a step would close below a quarter of the spacing pinned and
       the step solved again (steps of 7 and 14 revolutions converge at
       times, 50 s a year on balance, and the chain still failed in its
       25th year: the Cholesky); the tail guessed as the first flown
       revolution replayed by Earth–Moon angle (the real NRHO's
       revolutions turn the line 1.29–1.74 rad and take 6.1–6.7 d one
       to the next, so no constant period predicts its phase: the tail
       broke by up to 5.8·10⁻¹); the catalog tail phased by clock time
       from the flown part's last perilune (up to 1.4: the CR3BP's time
       is the angle, so a tail on any other clock is non-ballistic at
       every arc); the same on the angle grid (breaks 8.5·10⁻²–9.3·10⁻²,
       a fresh guess's, but the L1 halo's chain then failed in its
       seventh year and the NRHO's in its 13th); the junction moved to
       the first patch after the flown part's last perilune (the NRHO's
       chain failed in its third year: a seam between two trajectories
       is most sensitive where the orbit is fastest). The guess and the
       scaling stay as they were; the squeeze above is real but was not
       what ended the chain.
     - **Sped up** (2026-10-18), 50 s a year to 5.8 for the NRHO: the
       step size that converged is kept on the reference
       (`Reference::step`) for the calls after, so a size that failed is
       not tried again (each of the NRHO's years had tried 14, 7 and 4
       revolutions first, each failing, for most of its time); and each
       iteration's arcs, independent, are integrated in parallel and
       collected in order (the serial result to the bit: the suite's
       anchors and seams are unchanged to the digit, and the halos' and
       the DRO's 34 years too). The halos' years went from 1.2 and 0.6 s
       to 0.4 and 0.2. The NRHO's path differs from the fixed solver's
       only where a year had converged at a larger step before (seams
       ≤ 4·10⁻⁹, were ≤ 3·10⁻⁹; the third seam's loop 2.09 against 2.13
       m/s a year after it). At a month a second a game year passes in
       12 s of wall time, so an NRHO station's orbit now keeps ahead of
       the clock; 34 years built headless took 3.2 min with the cores to
       itself, against gate item 4's 10 min for a whole scenario.
     - Open: the 9:2 member (M1a's choice, eclipse phasing).
2. **The planner on extended orbits.**
   - A ship arriving at a station orbit place gets its own orbit, built
     from the catalog orbit at the arriving leg's window opening, as
     now, and extended a year at a time on demand (shared behind a
     lock, appended only) by the ship's checkpoints and stays. It reads
     only the arriving step, so `reads` no longer reaches through the
     leg after a station.
   - A design gets the extent its times need: from when the ship is
     ready, through its window's close and longest duration, with a
     margin of the loop's horizon. What it reads, the span included,
     then does not depend on how far the orbit has been built.
   - A stay's track is the loop's flight (`stationkeeping::simulate`
     with a seed from the ship and the stay), as the user ruled: the
     station flies the closed loop, its burns are the stay's Δv, kept
     with their times so the loop pays them as made (part 5). The
     Monte Carlo rate stays for projections and the gate. Every stay
     pays; today a visit shorter than four revolutions pays none.
   - A `Reference` carries the catalog orbit and the settle it was built
     with, so that extending it cannot be handed another member (found
     in part 1's review).
   - Memory: from part 1's numbers. If a station's life does not fit
     as dense steps, the past is kept as patches and re-integrated on
     demand.
   - Deleted: `visit_until`, `Planner::outstays`, and career's refusal
     of a longer visit (its tests become tests that the visit is
     designed). `resume_point` no longer steps back to a station's
     arrival.
   - Checked:
     - designs bit-identical across thread counts
       (`crates/orbit/tests/determinism.rs`);
     - the published plan and the crew rotation, each leg's Δv and
       design time before and after. Station orbits start as before,
       so legs should move little; stays gain upkeep;
     - memory.
   - Settled before building it (2026-10-18):
     - `orbit::station::Orbit`: a reference behind a read-write lock,
       built from the catalog member at the arriving leg's window
       opening less a margin, over what the arriving step needs (its
       window, longest duration and margin) plus the settle.
       `cover(until)` extends it by whole years until its path reaches
       `until` plus the margin. The years are the quantum so that the
       sequence of solves, and so the path, is the same whichever step
       or walk asks first (a solve in steps of two revolutions grouped
       57 then 57 differs from one grouped 114 where a year's
       revolutions are odd). A leg leaving or reaching the orbit is
       designed against a window of it (`Orbit::window`, a reference of
       its own over the window with the margin each way), and a stay
       follows a window, so no lock is held while anything is designed.
       The orbit's period is its first build's mean: the mean over the
       whole path moves as the path grows, and a departure window sized
       by it designed differently resumed than fresh (found by the
       determinism tests, 0.1 ms of departure).
     - The station-keeping policy is the catalog member's, judged once
       per process on a year of the member's orbit from 2045-03-01,
       solved with its settle (`upkeep`, four runs): the cheapest that
       holds it. A stay pays its rate over its span, as before. Judged
       per visit, over a visit's few maneuvers, a policy can hold them
       and lose the ship within a year (measured: an NRHO visit's
       "every revolution aiming half" did); judged on a year solved
       with a settle of one, against the free end's departing
       revolutions, the NRHO's came out "aiming one" where a settled
       year gives the same (below).
     - `reads` is the step's own leg: a station start reads nothing, so
       `resume_point` always finds a checkpoint and gives one, not an
       option of one. `Reference` carries what it was built to extend
       from (`Extension`: the member, the settle, the anchor revolution
       and the remembered step) in place of `ahead` and `step`.
   - **Built and taken out again** (2026-10-18): the loop flown over a
     stay, as the user ruled (the station flies the closed loop, its
     burns the stay's Δv with their times). It was built whole: the
     loop made resumable (`stationkeeping::Loop`, flown on in pieces,
     keeping the ship's states every six hours and after each burn; it
     stays, `simulate` is built on it), the vessel's flight kept in its
     orbit from its arrival as far as the reference reached (so flown
     in the same pieces whoever asked), a stay's track a coast of the
     flight's states cut at the departure, the leg after designed
     against the flight, and burns markers of their own kind. Two
     findings took it out, both measured on the NRHO:
     - Inserted on the reference to the bit (the leg put it there), the
       ship never leaves it: truth and reference are one integration,
       navigation noise alone predicts a miss under the deadband, and
       without a burn there is no execution error. A 43-day stay made
       no burn. Insertion with the loop's navigation error (as the
       Monte Carlo inserts) was restored.
     - The loop's success depends on where in the orbit its maneuvers
       fall. A flight's phase is its arrival's, and with a maneuver
       interval equal to the period it stays there; the kill-test's
       loop ran at an interval 0.1–0.2% off its reference's period (the
       34-year mean), so its phase drifted, and it held. On the chain's
       first year, "every revolution aiming two" at the exact period
       loses the ship in every seed (worst 510–760 km, 14–25 m/s a year
       before it is lost) and at 1.001–1.005 periods holds at 0.6–1.7
       m/s a year; from a start 2.3 days later the same policy holds
       at the exact period (0.9–1.0) and is lost at 1.002 and more;
       from 4.1 days later it holds from 0.998 to 1.01. "Aiming one"
       holds at the exact period at every phase tried (2–10 m/s a
       year) and was lost from a flight's arrival phase after 290 days
       with an 8 m/s burn. The controller (a 3-D position match over
       the horizon through Φ_rv⁻¹, every so many seconds from an
       arbitrary start) is not robust to its phase, so a flown stay
       would be a coin flip per visit. Stays follow the orbit and pay
       the member's rate until it is: maneuvers at a geometric phase
       (the apolune, as Gateway's x-axis-crossing control) targeting
       crossings, or a damped solve, to discuss; the rate judged as
       above is the controller's at the reference's own phase (the
       NRHO's "every revolution aiming one", ~6 m/s a year against
       the chain's 1.6–2.0 and Gateway's ~1). The flown version is in
       the session's scratchpad (`station_flown_loop.rs`).
   - Measured (2026-10-18, release, load 5–8 from other work on the
     machine; step 2 part 8 measured at 3.4–6.8):
     - The published plan's legs are M1c's within a few m/s (LEO to the
       DRO 3,380, NRHO to low lunar orbit 687, DRO to low lunar orbit
       812, LEO to the NRHO 3,483). A leg reaching a station now builds
       the orbit's first year in its design time (NRHO 1.3 s, DRO 0.4)
       and the first to each member judges its policy once per process
       (NRHO ~3 s): LEO to the NRHO 5.5 s, LEO to the DRO 25.1 s
       (16.7–16.9 before, at the lower load), the NRHO to LEO 1.2 s, the
       other legs 0.1–3.6 s as before.
     - The crew rotation: 9,354 m/s over 51.4 days (M1c: 9,343 over
       51.5); its legs 3,485, 680, 695 and 508 m/s; its NRHO stays pay
       0.4 and 0.1 m/s at 6.57 m/s a year ("every revolution aiming
       one", the member policy at the reference's own phase: above);
       designed in 12.3 s (10 before: the first NRHO orbit's year and
       the member's policy).
     - Memory: the two ships parked a year (one in the NRHO), designed
       in the sandbox: 57 MB resident, 49 before (the NRHO's orbit is
       built a year long and its stay follows a window copied from it;
       a game of hundreds of NRHO visits at ~4 MB each wants the orbit
       kept as patches and re-integrated by window: open).
     - The full workspace suite after the last fix: 271 passed, one
       failed, a sandbox test of a dry stay's drawing whose leg now left
       at its window's opening, the moment the ship started (no stay to
       draw); given a wait before the leg, its suite passes.
     - Open: the solver's extension from a short fresh base (above);
       the station-keeping controller's phase robustness (above);
       memory as patches.
3. **Meeting a vessel.**
   - A leg's destination names a vessel, by id (names change and
     repeat), and the leg of its plan that brings it there:
     `to = { ship = 3, leg = 2 }`; without `leg`, where it starts. The
     editor writes the vessel's last leg.
   - The leg arrives on the vessel's orbit at its phase; the ship then
     stays on that orbit and leaves from it. The planner designs legs
     to and between station orbits already (between them, 88.5%
     converge: M1a's known failure). A vessel on a surface or at a
     small body is met by going there.
   - Why the leg is named: the stop met is fixed by the log, not by
     when the ship gets there. Which designs wait on which is then seen
     in the log, before any is made, and a re-plan of the vessel after
     the leg met changes nothing of its visitors' designs.
   - Refused when the log is read, in both modes: meeting a vessel in a
     low orbit; meetings that wait on each other (a cycle).
   - The engine: the meeting step's key adds the vessel's key after
     the leg met. The ship's walk waits for the vessel's walk to get
     there. An edit of the vessel at or before that leg designs its
     visitors again from their meetings.
   - Career: a re-plan of a vessel is refused if it replaces a leg
     that another ship's plan meets it after.
   - Whether the vessel is there when the ship arrives, and while it
     transfers, is the loop's (part 5), not the design's.
   - Settled before building it (2026-10-19):
     - A meeting is a `Spot` with `meeting: Some(Meeting { ship, leg })`
       (`leg` a step index, from 0, waits counted; `None`: the vessel's
       start) and `kind` the vessel's place after that leg as the plan
       stands: resolved by `mission::Meetings` when a plan is read and
       whenever a log changes (`Log::check`, against each ship's steps
       with its re-plans), and taken from the vessel's checkpoint when
       the leg is designed. Rejected: a destination enum, which every
       static reader of a leg's place (the class checks, the planner's
       rules, the window lanes) would have had to resolve through the
       plan. A design-refused re-plan is applied by the static
       resolution and not by the design; where they disagree, the
       static checks may warn wrongly, the design is right.
     - `Meetings::new` refuses: a ship not in the plan, the ship itself,
       a wait or a step the ship has not, a vessel in a low orbit, and
       meetings that wait on each other (a depth-first search over
       (ship, through-step) nodes, where a step's leg waits on the
       vessel's design through the leg met). A start is a place.
     - The planner: `Planner::meets` names the next leg's vessel;
       `Planner::meet(Result<&Checkpoint, String>)` designs that step
       (`step` asserts there is none). The vessel's `Here::Station`
       orbit is the arrival (the same `Arc`, so both ships extend one
       orbit) and the ship's own after it; its `Here::Body`/`Surface`
       the leg's place; `Here::Low` fails the step. The window's
       opening moves so the leg arrives no earlier than the vessel's
       orbit begins (the orbit is built from the vessel's own window
       opening less the margin; its leg's duration does not move it);
       the ship then carries the vessel's place as its label.
     - The engine: `designs::Meet` resolves a meeting to the vessel's
       design key after the leg met and its checkpoint, or why it is
       not reached (a failed step, a plan that ends before it), or
       `None` while not designed that far. The step's key hashes the
       vessel's key (or the failure text). The game keeps a `Fleet`
       (each ship's standing steps, design steps and whether its walk
       ended) behind a mutex, published on every change; a background
       walk that meets a vessel not there yet ends with what it saw of
       the vessel, and the game walks the ship again when the vessel's
       design changes (at once, if it changed in between). A ship
       whose inputs did not change is still walked again when a ship
       it meets is (its meeting's key is the vessel's); the walk finds
       the rest in the cache.
     - Career refuses a re-plan from a step at or before a leg another
       ship's standing plan meets its ship after. The log refuses the
       same where the meeting was made by an earlier re-plan; one in a
       ship's first steps the log does not date (ships are not ordered
       among re-plans), so a log reads every meeting against the plan
       as it stands at its end, which career's refusal keeps equal to
       the plan as it stood when the meeting was made.
   - Built (2026-10-19): `Meeting`, `Meetings`, `Planner::meets`/`meet`
     (`orbit::mission`); `Met`/`Meet`, the keys and the walk's waits
     (`sim::designs`); the fleet, the waits' wake-ups, `reshaped`'s
     transitive re-walks and the re-plan refusal (`sim::game`); the
     log's resolution and ordering rule (`sim::log`); the sandbox's
     duration sweep given the vessel met (`sandbox::plan`). Tests:
     `crates/orbit/tests/meeting.rs` (a tug meets a depot at the NRHO:
     its stay is on the depot's orbit to the bit, it leaves from it;
     a vessel not there, or in a low orbit, fails the step) and six in
     `crates/sim/tests/game.rs` (the walk waits and arrives on the
     orbit, on 1 and 24 threads alike, and the log reads back; an edit
     of the depot after the met leg designs nothing of the tug, one
     before it designs the tug from its meeting, an undo nothing; a
     depot that never gets there fails the tug's step with why; a
     depot met where it starts; the log's refusals, each; career's
     refusal).
   - Measured (2026-10-19): the tug's leg to the depot costs 3,512.5
     m/s against 3,508.3 to the NRHO on an orbit of its own, and its
     leg home 618.7 against 509.7, leaving at the end of its
     revolution-long window where the tug alone left 1.6 days in: the
     meeting fixes the phase, and the phase picks the departures the
     window offers. The tug's stay is on the depot's orbit to the bit,
     and the same on 1 thread as on 24. A meeting's key: an edit of the
     depot's leg home designs two steps of the depot and none of the
     tug; the depot a day later designs its four and the tug's three
     from the meeting. The planner's meeting test designs in 22 s; the
     game's three slow ones (the two-ship plan designed four times,
     and the start-meeting plan once) in 89 s. The full workspace
     suite: 313 passed and two failed, both class-warning tests, a
     freshly designed ship having lost its breaches to `reshaped`'s
     new order (fixed: it stands again after its design); the game's
     21 tests then pass in 118 s.
   - Open: the sandbox's editor lists no vessels yet (part 7); whether
     the vessel is there is the loop's (part 5); a meeting's static
     place and its design can disagree where a design-refused re-plan
     changed the vessel's leg (above).
4. **Manifests in the log** (`orbit::mission::Go`, read by
   `sim::ships`).
   - Written on the leg that arrives at a stop, as the start writes its
     load: the amounts to have after the stop.

     ```toml
     [[ship.leg]]
     to = { ship = 3, leg = 1 }
     days = [4.5]
     propellant_t = 40.0                     # the tanks' excess given on arrival
     cargo = { lox = 0.0, equipment = 20.0 } # LOX given on arrival, equipment taken on leaving
     ```

     - What is above them is given on arrival: at the end of the step's
       last item, after its burns.
     - What is below them is taken as the ship leaves: at the start of
       its next leg, landing or ascent.
     - A commodity not named is left as it is. A ship that never leaves
       a stop takes nothing there.
   - With whom: the vessel met. At LEO, with none, a take is bought
     from Earth. A take elsewhere with none, or a give with none, has no
     one: a warning in the sandbox, refused in career.
   - Kept as written. Design keys leave them out, as they leave out a
     ship's class and load, so a manifest edit designs nothing.
   - Class checks: tank and hold amounts within capacity. A transfer
     needs a class on both sides.
   - A career re-plan of a staying ship can also set the manifest of
     the stop it is at (`here`), whose takes are still to come.
   - Rejected: on the leg that leaves (a last stop could deliver
     nothing); as steps of their own, like waits (they would shift the
     step numbers that keys, re-plans, breaches and checks count).
5. **Transfers and purchases in the loop** (`sim::state`).
   - What a ship gives goes into the vessel met: the propellant that
     vessel burns into its tanks, the rest into its hold, within their
     capacity. What it takes comes from the vessel's hold first, then
     its tanks. A station keeps no reserve: drained, it runs dry
     keeping station, which the loop shows (and career refuses).
   - Events: each vessel's in its own order (an arrival's burns, then
     its gives; its takes, then the departure's burns). At one time:
     the tick, then gives, then takes, by ship id. A transfer first
     pays both vessels' upkeep and boil-off to its time.
   - A transfer that cannot be made in full is made as far as it can
     be, and recorded as short: asked for, got, and why (the stock, the
     capacity, or the vessel not there: not yet arrived, gone, or run
     dry).
   - Purchases at LEO (takes, and a career ship's price and start
     load) are recorded with their cost: the launch price at the time
     (geometric between the drawn 2040 and 2076 prices, held outside
     them), plus the goods' price (propellant's, equipment's). Payload
     pays launch only; metal cannot be bought. There is no account yet
     (step 8).
   - The state hash adds purchases and shortfalls.
   - Measured: the loop's re-run on the published plan.
6. **Career against the projection.**
   - An action is pending until its ship is designed to its end. It is
     refused then if, with it, a vessel runs dry or a transfer falls
     short, where none did without it. The clock does not pass a
     pending action's first effect, so nothing of a refused action
     happens.
   - Reading a career log back checks each action so again, in the
     order made. `sim run` fails on one refused.
   - In step 3 the projection is the loop's state: nothing is random
     yet. A projection of means, apart from the state, comes with the
     first draws in the loop (demand, step 7; losses, step 8).
   - The alternative: career reports a plan that runs dry or short and
     does not refuse it, until M3's UI designs a plan before it is
     made.
7. **The sandbox.**
   - The editor: a leg's destination lists the plan's vessels, by name
     and leg; a manifest per leg, as the start's load.
   - The panel: each stop's transfers (given, taken, with whom; what
     fell short and why); purchases and their cost.
8. **Headless.**
   - `sim run` prints each stop's transfers, and each purchase with its
     cost.
   - Checks: a ship's cargo by commodity at a date, within bounds; a
     transfer short or not.
   - Scenarios, in career: a hauler bought at LEO flies to the NRHO and
     stays there as a depot; a tug buys propellant at LEO, meets it and
     gives; a lander meets it, refuels, lands at the south pole and
     comes back up. In the sandbox, the same with the tug late: the
     lander's take falls short, and it runs dry.
9. **Tests and measurements.**
   - Transfers and purchases by hand; the event order at one time; a
     short transfer for each reason; a meeting cycle refused.
   - Hashes with transfers at 1 and 24 threads; a career log in the
     sandbox gives the same; a manifest edit designs nothing; an edit of
     a vessel met designs its visitors again from their meetings, and
     an edit after the leg met designs nothing of theirs.
   - Career: an action that runs a vessel dry or short is refused, and
     so is one reading back that way.
   - The full workspace suite at the end.

Left out of step 3: docking and assemblies (step 4); meeting a vessel
in a low orbit (Scope, Out); a station's reserve, kept from its
visitors; where plants' output goes (step 5); turnaround days (step
6); selling (steps 7 and 8).
