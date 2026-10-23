# M1b — The sim engine, for sandbox and career

Status, 2026-10-23: steps 1 to 4 done (step 4: a ship starts docked to
a vessel, or docks by a leg or a dock step, and rides it, leaving by a
leg of its own; a host's burns and upkeep move its assembly's mass, and
a transfer reaches the whole assembly; career buys a docked start at
LEO and checks docks; the sandbox draws, lists and edits docking; `sim
run` prints dock lines with the assembled station's checks holding; and
the tests and measurements are in); step 5, plants, programs and
surveys, in progress: its plan's seven decisions ruled 2026-10-23, each
as drafted, and part 1 built (keyed draws and the economy's
additions). Decided: one engine for both modes, with economy
components landing in the sandbox first (Modes, below); the four
decisions below, each as recommended; stations as vessels, flown to
where they stay, with docking a step of its own (Design, Vessels,
stations and stocks); and sites holding stocks, with the rest of step
5's seven (Design, Plants, programs and surveys).

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
- Step 3, part 1: extended station orbits (`orbit::reference`).
  `Reference::extend` adds revolutions to a catalog orbit a year at a
  time; the kill-test runs each orbit 34 years with its anchors and
  seams within the shooting's tolerance. The NRHO failed it first (its
  extension converging only by one or two revolutions), was diagnosed
  as the solver's extension from a short fresh base, and fixed the same
  day (the plan, below).
- Part 2: the planner on extended orbits. A ship arriving at a station
  gets an orbit of its own, extended on demand by its checkpoints and
  stays, so `reads` stops at the arriving step; stays follow the orbit
  and pay the member's rate, the flown controller not being robust to
  its phase (open). `visit_until`, `outstays` and career's refusal of a
  longer visit are deleted.
- Part 3: a leg meets a vessel where it is after one of its legs (`to =
  { ship = 3, leg = 2 }`): the planner designs the leg onto the vessel's
  orbit at its phase, the engine keys the step by the vessel's design
  and walks the visitor again when the vessel changes (`Fleet`), and
  career refuses a re-plan that replaces a leg another ship meets its
  ship after.
- Part 4: manifests on the arriving leg (`propellant_t`, `cargo`), kept
  as written and left out of design keys, so a manifest edit designs
  nothing; a re-plan's `here`; the class and partner checks.
- Part 5: transfers and purchases in the loop: gives at arrival, takes
  at leaving, each by commodity against the manifest's targets, short
  with why; purchases at LEO at the day's launch price; the state hash
  adds them.
- Part 6: career against the projection. An action is pending until its
  ship is designed, then refused if a vessel runs dry or a transfer
  falls short where none did before; a log read back is replayed, and
  `sim run` fails on a refusal.
- Part 7: the sandbox's editor sets meetings (the plan's vessels in the
  destination menu) and manifests; its panel shows each stop's
  transfers and a ship's purchases.
- Part 8: `sim run` prints transfers and purchases; the cargo and
  transfer checks; the depot scenarios, in career and, the tanker late,
  in the sandbox. A wait that ends a plan is now a stay to its end
  (found by the depot's `at` check).
- Part 9: tests and measurements (2026-10-21). Every test of the plan's
  list was written with its part: transfers and purchases by hand, the
  event order at one time and each short reason (`sim::state`'s unit
  tests, part 5), the meeting cycle refused (part 3), the hashes with
  transfers on 1 and 24 threads and a career log in the sandbox (the
  depot test, part 5), a manifest edit designing nothing (part 4), an
  edit of a vessel met designing its visitors from their meetings and
  one after the leg met designing nothing of theirs (part 3), career's
  refusals (part 6); where each lives is in the parts' Built bullets
  below. Measured (release, load ~9 from other work): the loop's re-run
  on the published plan, every ship a hauler, 2.6–3.1 ms alone and
  2.7–3.3 with a new load, 50 ships of 2,000 items 57–67 ms, the plan
  designed in 36.7 s; part 5's regression (5.5 ms, in `Running::pay`'s
  generated code, not its source) is gone with later changes, as a
  codegen effect would be, and was not chased. The full workspace suite
  at the end of step 3: 344 passed, 13 ignored (measurements), none
  failed, in 10.5 min.
- Step 4, part 1: docking's language: a start docked to a vessel, a leg
  that docks, a dock step (`orbit::mission`), the plan's checks over
  them, and the `module` class, without tanks or engines.
- Part 2: a docked ship rides its vessel: its items are the dock and its
  own legs, which leave from the vessel's stop before the vessel does; a
  leg that docks arrives while the vessel is there, by its design. The
  walk reads the fleet through `designs::Designs`, each stop keyed by
  the design that put the vessel there and when it leaves.
- Part 3: the dock step, made when the ship's chain of vessels holds the
  vessel, or one of them met it at that stop, tried at the chain's stops
  in turn.
- Part 4: the loop: a vessel's burns and upkeep move its riders' mass
  from its own tanks; a transfer reaches the vessel's whole assembly; a
  vessel that runs dry strands its riders.
- Part 5: career buys a docked start at LEO and refuses one elsewhere;
  the docked check and the place check through hosts; `sim run`'s dock
  lines; the assembled station and the carried lander as scenarios; the
  assembly is the same on 1 and 24 threads.
- Part 6: the sandbox draws a docked ship where its vessel is, with one
  label for the vessels at a point; the panel's dock rows; the editor's
  start menu of vessels, dock switch and dock step; sweeps of docked
  legs.
- Part 7: tests and measurements (2026-10-22). Each part's tests were
  written with it (the parts' Built bullets below): the language's
  round trip and the plan's checks (part 1); a docked ship riding and
  leaving from its vessel, and a dock step where the ship is (parts 2
  and 3, `crates/orbit/tests/meeting.rs`); the walk's keys and waits
  through a chain of vessels, an edit of the carrier designing its
  rider again and one after the rider left designing nothing (parts 2
  and 3, `crates/sim/tests/game.rs`); the loop's riders, station
  keeping for them, assembly transfers and stranding (part 4); career's
  docked start and the docked check (part 5); the editor's docking
  (part 6). Measured (release, load 10–30 from other work): the
  assembled station's 20 designs in 18.0 s on 24 threads and 98.9 s on
  1; its loop alone 0.036–0.050 ms, and 0.08–0.14 ms with a new load of
  the depot, which designs nothing; `sim run` on station-assembled
  24.4 s wall (136 s user), hauler-carries-lander 9.2 s (33 s),
  depot-tug-lander 20.4 s (91 s), every check holding (21, 9 and 17).
  The full workspace suite at the end of step 4: 361 passed, 13 ignored
  (measurements), none failed, in 12.8 min of tests.
- Step 5, part 1: keyed draws (`Draw::keyed`: the fixed hash of the
  seed and a key seeds a stream, its first values pinned) and the
  economy's additions (`Facility::{plant, ore, present, prior}`,
  `Program::noise`; absent options kept in every world, their surveys
  sampled last; the LP leaves them out). M2's 16 scenario texts are
  the same to the bit.

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
- **Stocks are what vessels carry,** and what sites hold (step 5).
  Loading, unloading and refuelling happen between two vessels that
  meet, as the arriving one's manifest says, or between a vessel and
  the site it is at. At LEO, anything can be bought from Earth at its
  price plus launch. Earth's surface is not a place, as in M2.
- **Docking** (decided 2026-10-21, step 4): a vessel docks to a host
  and rides it, through the host's stays and legs; the host's burns
  count the assembly's mass and draw from the host's tanks; a transfer
  with a vessel reaches its whole assembly, the vessel named first. A
  docked vessel leaves by a leg of its own, or docks to another host it
  is with; there is no undocking to stay alone. Whether a dock is made
  is decided in the design (the host is at the stop by its design), and
  only a host that runs dry strands its members. A ship may start
  docked to a vessel. A class without tanks (a station module) has no
  engines and is carried so.

### Plants, programs and surveys

Decided 2026-10-23 by the user (step 5 plan, below, each as drafted):

- **Sites hold stocks.** A surface or a body (the south pole, the mare,
  Mars's surface, Phobos, each asteroid) is a site, with a stock by
  commodity and the plants installed there. A plant's output goes into
  its site's stock, and a ship there transfers with the site as a ship
  at LEO does with Earth. Storage is unbounded, as in M2; hydrogen
  boils off there as in a tank.
  - Rejected: a plant as a vessel. It sits on its body's rails, and two
    plants at a site would be two stocks for a lander to choose from.
- **Installing is a give of equipment** with `install = "option"` on
  the manifest of a leg to a site, adding 1/α t/yr per tonne
  `build_months` later. The technology must be finished at arrival; a
  survey is not needed (building blind, DESIGN.md, Prospecting). Not
  finished, the give falls short and career refuses the plan.
- **Programs are actions in the log,** `[[program]]`, technology and
  survey alike, each once, finishing `years` after its start and
  spending its cost evenly; career starts one at now. A program's cost
  and years are known; the deposits are what is hidden.
- **A survey's estimate is the first keyed draw.** At its finish, each
  deposit it covers gets its grade and reserve times exp(σz), σ the
  survey row's `noise` (0.3 for lunar ice and the asteroid prospectors,
  0.4 for Mars and Phobos, mapped from orbit [guess]), and its presence
  exactly. The stream is a `ChaCha8Rng` seeded by the fixed hash of the
  game's seed and the key `survey:<id>`, drawn in the sandbox too.
- **Absent deposits stay in the economy** (`Facility::present`), so
  that every world has every option and survey by name and a plan
  naming one reads; their surveys are sampled last, so no seed's other
  parameters move. A plant on one yields nothing; M2's LP leaves them
  out.
- **Career's knowledge is a policy, and the judge uses it.** An option
  is exact once a plant on it has built, estimated once its survey has
  finished, and before that the file's prior, scaled by the presence
  probability. The state runs on the truth; the judge's projection on
  what is known, so a refusal tells the player nothing a survey would.
  - Rejected: judging on the truth, a free survey read off refusals.
- **Production accrues continuously** at the capacity in force, paid
  up at transfers at the site and at midnight, as upkeep and boil-off
  are, hydrogen's boil-off and the reserve's exhaustion in closed form.
  (The draft's daily production tick is replaced.)
- As in M2 otherwise (`data/economy.toml`): deposits run out, and a
  plant stays on its deposit. M2's survey cost divided by the presence
  chance offsets the LP's foresight; the engine pays the row's cost.

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
   - Settled before building it (2026-10-20):
     - The manifest is a field of the leg (`mission::Go::manifest`, a
       `Manifest` as written: `propellant_t`, `cargo`). Design keys and
       the game's inputs hash steps through `designs::flight`, which
       leaves it out, so a manifest edit keeps the ship's design, its
       revision and the day's hashes. The game's edit treats one as a
       refit, like a new class or load: re-plans are kept, and each
       manifest is written to the step it stands on (the plan's, the
       re-plan's leg that made it, or the re-plan's `here` that set it
       last). Rejected: manifests beside the steps on the ship (every
       edit of the steps would have to move them, and re-plans carry
       their own).
     - With whom, checked where the plan is read: a manifest at a stop
       with no vessel met, away from LEO, has no one ("no vessel to
       transfer with at nrho"); a vessel met without a class has no
       tanks or hold ("ship \"Depot\" has no class to transfer with"):
       breaches, a warning in the sandbox and refused in career. At LEO
       with none nothing is said here: a take buys from Earth, and a
       give with none is the loop's to record as short (part 5). A
       manifest on a ship without a class is a read error, as a load's
       is. Tank and hold capacity are checked per leg with the start's
       wording ("leg 1: a tug-h's tanks hold 150 t, not 160").
     - `here` (`Replan::here`) sets the manifest of the last leg before
       `from`, applied with the re-plan (`designs::apply`); refused, by
       the log and by career, when no leg before `from` arrives at a
       stop; a breach of it reads "replan 1: here: ...". What was given
       on arrival was by the manifest standing then: the loop reads
       re-plans by their dates when it comes to transfers (part 5).
   - Built (2026-10-20): `Manifest` on `Go`, its keys and the wait's
     refusal (`orbit::mission`); `Replan::here`, parsed, written and
     checked (`sim::log`); `flight` and `apply` (`sim::designs`);
     `ships::Manifest`, `manifest`, `read`'s checks and `breaches`'s,
     given the plan's ships (`sim::ships`); `Game::edit`'s refit and
     `remanifest`, `Game::replan`'s `here`, `allowed`'s label
     (`sim::game`); the editor's new leg (`sandbox::editor`). Tests:
     three in `sim::ships` (a manifest by commodity; what one is not;
     no room or no one), the log's round trip and refusal, the orbit
     plan's round trip, and two in `crates/sim/tests/game.rs` (a
     manifest edit designs nothing, in a plain plan and in a re-planned
     career log in the sandbox, and reads back; each manifest check
     refused in career and warned in the sandbox, and `here` set, refused
     at the start and beyond the tanks, read back).
   - Measured (2026-10-20): the manifest edit keeps the design count,
     the revision and the day's hashes, in the sortie and in the
     re-planned round trip; the two game tests run in 7 s. The full
     workspace suite: 328 passed, 13 ignored (as before), none failed,
     in 9.4 min; clippy and the formatter clean.
   - Open: the sandbox's editor edits no manifests and its panel shows
     none (part 7); the loop reads none (part 5).
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
   - Settled before building it (2026-10-20):
     - A manifest's `propellant_t` is two targets, LOX and the ship's
       fuel at its class's mixture ratio; each cargo entry is the hold's
       target for that commodity. Above a target the ship gives, below
       it takes, by commodity: the tanks drift off the ratio as hydrogen
       boils off, and a transfer brings them back to it. Rejected: giving
       the surplus of the total at the ratio (a ship short of hydrogen
       would give LOX it should keep, and stay off the ratio).
     - Events at one time: the tick; burns at the end of their item (an
       arrival's: a leg's insertion, a capture's circularizing); gives;
       takes; the other burns (a departure's: a landing's and an ascent's
       single marker is at its item's start); each by ship id. Gives are
       at the end of the step's last item, takes at the start of the
       next leg, landing or ascent (or its first burn, a moment earlier
       by the planner's slack); a ship with no later leg takes nothing.
     - The vessel is there when its leg named has arrived (its start,
       without one), its next leg, landing or ascent has not begun, and
       it has not run dry; else the transfer is short with why (`NotYet`,
       `Gone`, `Dry`). A give puts LOX and the fuel the vessel burns into
       its tanks and the rest into its hold, to their capacity
       (`Capacity`); a take drains its hold first, then its tanks, into
       the taker's tanks or hold to their capacity (`Stock`, `Capacity`).
       No vessel met away from LEO, a vessel without a class, or a give
       at LEO: `NoOne`. Metal from Earth: `NotSold`. A ship that ran dry
       is stranded and transfers nothing.
     - At LEO a take is bought: launch at the day's price (geometric
       between the economy's start and end prices, held outside them,
       `Economy::launch_usd_per_kg_at`) plus the goods (propellant's and
       equipment's prices; payload pays launch only). A ship starting at
       LEO is bought there, the class's price and its load the same way
       (`ShipState::purchases`), in either mode, so that a career log
       opened in the sandbox hashes the same (found by that test: bought
       in career alone, it did not); a ship starting elsewhere is simply
       there, and career refuses a start load with metal. Transfers are
       recorded on the
       ship whose manifest asked (`ShipState::transfers`: asked, got,
       why, cost), and the vessel's load at the time.
     - A re-plan's `here` is dated: what the ship gives on arrival is by
       the manifest standing then (the leg's, with the `here`s made by
       that time), what it takes on leaving by the manifest as it stands
       (`Game::stops`, from `owners`, the replay `remanifest` shares).
     - The day's hash adds each ship's transfers and purchases by then.
   - Built (2026-10-20): `Stop`, `With`, `Transfer`, `Kind`, `Short`,
     `Purchase`, `Bought`, `Course::{stops, bought}`,
     `ShipState::{transfers, purchases}`, the event phases and
     `Loop::transfer` (`sim::state`); `launch_usd_per_kg_at`
     (`sim::params`); `Game::stops`, `owners`, the hash, the metal
     refusal (`sim::game`). Tests: five in `sim::state` by hand (a give
     fills the tanks then the hold and a take drains them back; the
     event order at one time; each short reason; a take at LEO at the
     day's launch price and a career ship bought at its start; a drained
     depot runs dry keeping station), and
     `a_tug_refuels_a_depot_and_buys_at_leo` in `crates/sim/tests/state.rs`
     (a hauler depot at the NRHO and a tug that gives it all but 20 t,
     tops up what three days of station-keeping burned, buys a full tank
     at LEO, both bought at LEO; the same hashes on 1 and 24 threads
     from the log written, and in career).
   - Measured (2026-10-20, release): the loop's re-run on the published
     plan, every ship a hauler, 5.5 ms alone and 5.6–5.7 with a new load
     (against 3.3–3.5 and 3.3–3.4 at the commit before, run alternately
     on the same machine; part 2 measured 2.7–3.3); 50 ships of 2,000
     items 92–115 ms (65–83 before). A plan with no manifests makes no
     transfers, so the difference is not the transfers' work: `perf`
     puts it in `Running::pay`, whose source did not change (its
     generated code did, and `ShipState` and `Running` grew); not
     chased further against the gate's 1 s. The depot test designs its
     plan three times (24 threads, 1 from the log, 24 in career) in
     127 s; the five by-hand tests run in 0.1 s. The full workspace
     suite: 334 passed, 13 ignored (as before), none failed, in 10.7
     min. Two earlier runs found: the manifest-edit test's hashes now
     change (the stop's shortfall is state; the test says so), and a
     career log opened in the sandbox hashed differently while only
     career's ships were bought (fixed above). One run had
     `plan::model::tests::hydrogen_boils_off_in_storage` fail inside
     HiGHS ("HiGHS error: invalid problem", `Highs_run` returning an
     error) under the suite's load; it passed in the two runs after,
     and nothing of the plan crate changed.
   - Open: the sandbox's panel shows no transfers or purchases and `sim
     run` prints none (parts 7 and 8); career does not yet refuse a plan
     that runs a vessel dry or short (part 6); the loop's 2 ms.
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
   - Settled before building it (2026-10-20):
     - Career acts one action at a time. A re-plan or a ship added
       passes its static checks (class, meetings, `here`) as before,
       then is refused while an action is pending ("an action is
       pending: ship \"Tug\" re-planned from step 1"); the log changes
       and the action is pending (`Game::pending`), with the log before
       it and what ran dry or short before it kept. Once every ship is
       designed (`poll`, `wait`), the action is judged: a ship that runs
       dry, or a transfer that falls short, that did not before (dry by
       ship; short by ship, step, give or take, commodity) refuses it:
       `Game::refused` says why, the earliest finding first (a short
       take before the dry departure it causes), and the log goes back
       to before it, its designs from the cache. The clock does not
       move while an action is pending. Rejected: queuing actions made
       while one is pending (each would need the log as the ones before
       it would leave it, before they are judged).
     - A career log read back is the game it was: its ships first,
       judged together as one action (ships are not dated among the
       re-plans), then each re-plan made again at its time through
       `replan` and judged in turn; one refused, statically or by the
       projection, refuses the log ("replan 1: ..."), and `sim run`
       fails on it. The clock comes to the log's now after the last.
       The sandbox opens a career log as before, its re-plans applied at
       once, one that cannot hold passed over; in career such a re-plan
       is refused when made again, so the passed-over case is the
       sandbox's.
     - `data/scenarios/lander-loaded-short.toml`, a ship that runs dry
       by design, is a sandbox scenario now: career refuses it.
   - Built (2026-10-20): `Pending`, `Found`, `Game::{pending, refused,
     may_act, act, settle, judged, found, designed}`, the replay in
     `in_mode`, `done`/`wait`/`poll`/`advance` (`sim::game`);
     `run::run` fails on a refusal; `Display` for `Short`
     (`sim::state`). Tests: `career_refuses_what_runs_dry_or_short` in
     `crates/sim/tests/game.rs` (a re-plan that takes hydrogen a depot
     does not have is refused once designed, the clock held meanwhile
     and the log restored; headless, a log with it is refused; one
     within the stock is accepted and reads back as the same game, its
     clock at the log's now; a lander added loaded short is refused and
     gone; a log whose ship runs dry is refused), and the career tests
     adapted (an action waits for the plan's ships; a re-plan that
     cannot hold refuses the log read back, and is passed over in the
     sandbox).
   - Measured (2026-10-20): the projection test designs the depot and
     the tug, two re-plans of the tug, the log read back, a headless
     game of the refused log and the lander, in 95 s on 24 threads; the
     `here` test with its log read back in 52 s. Found building it: a
     re-plan's `here` made before the ship arrives governs what it
     gives on arrival (part 5's dating), so a test's `here` asking for
     more than the ship had was rightly refused; and a propulsive leg
     home with hydrogen-short tanks ran the tug dry before its short
     take came up, which is why the judge reports the earliest finding.
     The full workspace suite: 335 passed, 13 ignored (as before), none
     failed, in 12.4 min.
   - Open: the sandbox has no career mode yet, so pending and refused
     actions reach it only through `sim run` and the tests (M3); a
     ship added in career is judged alone when made, but judged with
     the plan's ships when its log is read back (ships are not dated).
7. **The sandbox.**
   - The editor: a leg's destination lists the plan's vessels, by name
     and leg; a manifest per leg, as the start's load.
   - The panel: each stop's transfers (given, taken, with whom; what
     fell short and why); purchases and their cost.
   - Settled before building it (2026-10-20):
     - The destination menu lists every other ship of the plan where it
       starts and after each of its legs ("Depot, start — leo", "Depot,
       leg 2 — nrho"), then the places (the vessels are few and the
       plan's own; the 22 places run past the menu's height, which the
       GPU test found when its vessel, listed last, sat under the
       scrim), legs counted from 0 as the file's
       `leg` and the engine's messages count them (the editor's own step
       rows count from 1; left as it was). Choosing one makes the meeting
       (`Spot::meeting`, new in `orbit::mission`, which the file's reader
       shares) at the place it resolves to as the plan stands
       (`Meetings::place`); a vessel the plan has not, or a wait of its,
       is refused there, and a ship meeting itself or meetings waiting on
       each other by the game's check, shown in the editor. The trigger
       names the vessel ("Depot, leg 2"; `editor::destination`), the
       engine's labels elsewhere stay "ship 0, leg 2". A start is a
       place, never a meeting.
     - A manifest is edited under its leg, as the start's load is edited
       under the class: propellant, t, and cargo by commodity
       (`Field::LegPropellant`, `Field::LegCargo`), shown only with a
       class. Empty leaves a commodity as it is; a cargo at 0 is kept (it
       gives the commodity away), unlike the start's cargo, where 0 is
       none. The game treats the edit as a refit (part 4): nothing is
       designed again.
     - The panel shows a stop's transfers on the stay after the arriving
       leg, or on the arrival itself where the ship does not stay (its
       plan ends there, or its next leg leaves at once): those made in
       full as one line each way ("gave LOX 60.0 t, LH2 8.60 t to Depot";
       "took LOX 23.1 kg from Depot"; at LEO, "bought LOX 85.7 t, LH2
       12.2 t from Earth for $12.3M"), and each that fell short as a
       warning with why ("take of 308.0 kg of LH2 fell short: 0 g (the
       vessel has less)"). Purchases on the ship's row ("Bought at LEO
       for $1,150M: the ship $900M, LOX 85.7 t $200M, …"). The hold is
       shown by commodity. Rejected: transfers on the leg that asked (a
       give and the take it pairs with would sit two rows apart, with
       the stay between).
   - Built (2026-10-20): `Spot::meeting` (`orbit::mission`); the fields,
     `destination`, the vessel options of `menu`, `choose`/`place` given
     the plan's ships (`sandbox::editor`); the stop rows, `bought`,
     `musd`, the hold by commodity (`sandbox::panel`); the app passes the
     plan's ships. Tests: the editor's unit tests (keys; a vessel chosen,
     refused; a manifest set and read), the panel's (money and mass;
     transfer and purchase lines), `crates/sandbox/tests/stops.rs` (the
     loop's depot plan in the sandbox: both ships bought at LEO, the
     tug's stop row gives and takes with the depot, its LEO stay buys from
     Earth with the cost, the depot's hold shows the hydrogen; a tug with
     no one to give to warns on the leg and says what fell short at the
     stop) and `a_leg_meets_a_vessel_with_a_manifest` in
     `crates/sandbox/tests/editor.rs` (the menu's vessel option clicked,
     the trigger's name, a manifest typed, the panel's lines once
     designed).
   - Measured (2026-10-20): the stops tests design the depot plan and the
     lone tug in 51 s; the editor's three GPU tests, the new one
     designing the tug's leg to the depot met at its start, in 35–38 s.
     Found building it: the depot is a methalox hauler, so the tug's
     hydrogen goes into its hold (the stops test first asked for
     hydrogen on the depot's purchase line); a depot whose start names
     no load has full tanks, so the LOX given has no room (the GPU test
     asserts the warning); the vessel option, listed after the 22
     places, lay under the menu's scrim (vessels first now); the
     manifest rows lie under the editor's 320 px fold, so the test
     scrolls the editor a wheel line at a time as the host does; and
     `frame::shot` draws with a fresh runner, which loses the scroll, so
     the harness reads its own texture back. The full workspace suite:
     342 passed, 13 ignored (as before), none failed, in 16.3 min run
     beside the screenshot runs and clippy (12.4 min alone at part 6);
     the harness's last change, test-only, was run again on its own.
   - Open: the editor's step rows count from 1 while a meeting's leg
     counts from 0 as the file does; the destination menu scrolls its
     22 places with no search; the options section and the panel's
     items label a meeting "ship 0, leg 0" as the engine does, the
     editor's trigger alone naming the vessel; `sim run` prints no
     transfers or purchases (part 8).
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
   - Settled before building it (2026-10-20):
     - `sim run` prints, after a ship's tanks, what it was bought for
       ("bought at LEO: the ship $80.0M, LOX 939.130 t $206M, CH4
       260.870 t $57.3M ($344M in all)") and its transfers in time
       order, each with the step that asked ("gave 17.468 t of LOX to
       Depot (ship 0, leg 0) 2045-03-15 06:06  (step 0)"; "bought … from
       Earth, $12.3M"; a short one marked SHORT with what got through
       and why). The vessel is named with the engine's label. The hold
       reads by commodity. `report::musd` is the one money format (the
       panel's moved here).
     - Two checks (`Check::Cargo`, `Check::Transfer`): `at` and `cargo =
       { lh2 = [8.0, 9.0] }`, each commodity within its bounds ("Depot on
       2045-03-12: LH2 8.600 t", failing "…, LH2 not 8.000–9.000"); and
       `step`, `give = "lox"` or `take = "lox"`, and `short = true` or
       `false` ("Tug, step 0: gave 17.468 t of LOX to Depot (ship 0, leg
       0) 2045-03-15 06:06", failing "; expected: short"; none found,
       "Tug, step 0: no take of LH2", pending while the state is).
     - The tug of the plan is a second hauler, the tanker: a hydrolox
       tug's LOX surplus (17 t, the rest flies it home) and 50 t of hold
       could not make the difference between the lander refuelled and
       the lander stranded, and its hydrogen into a methalox depot's hold
       served nothing (the tests cover that case). The depot is flown
       with 215 t to arrive with 10 t (the rocket equation on the
       hauler's 120 t dry and the leg's 3,485 m/s), the tanker keeps 25
       t to aerocapture home and carries 50 t of equipment, of which the
       lander takes 10 t down to the surface, so the cargo check has
       something to check. Late, the tanker starts 2045-03-20, after the
       lander has left the depot: its takes fall short (10 t, no
       equipment) and it runs dry at its ascent.
     - Found by the depot's `at` check: a wait that ends a plan made no
       item, so a depot "staying 60 days" was nowhere, kept no station
       and paid no upkeep. Now a wait as a ship's last step is a stay to
       its end, the wait's own step (`Planner::advance`; a wait of no
       days is still nothing): the depot keeps station for 60 days at
       1.1 m/s. Found by the late scenario: the lander drains the depot
       to the gram, and at the next tick the depot runs dry keeping
       station (2.6 mm/s short) as part 5 settled a drained station does,
       so the tanker's gives then fall short against it. The scenario
       says so and checks it, rather than reserve the depot something
       (a station's reserve is left out of step 3).
   - Built (2026-10-20): `Check::{Cargo, Transfer}`, parsed, written and
     checked (`sim::log`, `sim::game`); `report::{with, transfer,
     transfers, purchases, musd}`, the hold by commodity, and `sim run`
     printing them (`sim::report`, `sim::main`); the trailing stay
     (`orbit::mission`); `ships::commodity` public; the two scenarios.
     Tests: the log's round trip and refusals with both checks; the
     depot test's checks held and failed with their messages
     (`crates/sim/tests/state.rs`); `a_wait_at_the_end_is_a_stay`
     (`crates/orbit/tests/missions.rs`: a depot's 30 days, a stay after
     a leg, a wait of no days); `every_scenario_holds` over the two
     scenarios (17 and 15 checks).
   - Measured (2026-10-20, release): `sim run` designs each depot
     scenario in 22 s; the career scenario's depot arrives with 9.859 t
     of its 215, the tanker gives 259.927 t of LOX, 72.202 t of methane
     and the 50 t of equipment, the lander takes 252.315, 70.088 and 10
     and comes up with 75.697 t; late, the lander takes 7.711 and 2.142,
     runs dry at its ascent 428.5 m/s short, the depot runs dry keeping
     station 2.6 mm/s short, and the tanker's gives fall short against
     it. The depot's 60-day stay keeps station at 1.1 m/s. The scenario
     test runs both in 40 s; the depot test with its checks in 114 s; the
     orbit's mission tests, the trailing stay among them, in 76 s. Found
     running the tests: the depot's station-keeping, paid first at a
     transfer and since its arrival, takes a few kilograms off what its
     tanks gain and makes a few kilograms of room in full tanks, which
     two tests had asserted to the gram (allowed for now). The full
     workspace suite: 344 passed, 13 ignored (as before), none failed,
     in 10.5 min.
   - Open: the equipment the tanker carries costs $511M of its $855M at
     the economy's central draw ($10,000/kg plus launch), which dwarfs
     the ships; no scenario yet sells anything (step 7); `sim run`
     prints transfers by ship, not by stop.
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
   - Done (2026-10-21): each item above has its test, written with the
     part it tests (the summary at the top, and the parts' Built
     bullets); the short reasons are all seven (`a_transfer_short_says_why`:
     not yet there, gone, run dry, the stock, the capacity twice, no one
     three ways, and metal from Earth). Measured: the loop's re-run as
     at the top (`the_published_plan_runs_again_quickly`, ignored,
     release); the full workspace suite: 344 passed, 13 ignored, none
     failed, in 10.5 min.

Left out of step 3: docking and assemblies (step 4); meeting a vessel
in a low orbit (Scope, Out); a station's reserve, kept from its
visitors; where plants' output goes (step 5); turnaround days (step
6); selling (steps 7 and 8).

## Step 4 plan (2026-10-21): docking

Vessels that move as one, so that a station can be an assembly (Design,
Vessels, stations and stocks: an assembly, like the ISS, is modules
flown there and docked). Drafted and ruled with the user (2026-10-21),
each decision as drafted; each part is committed on its own.

What docking adds to step 3's vessels: a ship that meets a vessel today
arrives on its orbit and stays there on its own, keeping station and
paying for it, and is left behind when the vessel leaves. A docked
vessel goes where its host goes, through the host's stays and legs; the
host's burns move the assembly's mass; and a visitor's transfer with
any vessel of an assembly reaches the whole. Modules without engines
become possible, carried by ships and left docked.

Decided (ruled 2026-10-21):

- **Docking is directed: a vessel docks to a host and rides it.** The
  host's legs move the assembly, the host's stays keep station for it,
  and the host's design knows nothing of its members. A docked vessel
  leaves by flying a leg of its own (undocking at its departure), or
  moves to another host it is with by a `dock` step. There is no
  undocking to stay alone: a vessel with engines leaves by a leg, and a
  module without them, alone on a station orbit, would run dry at its
  first upkeep and drift (step 3 plan, part 5's rule for a drained
  station).
  - Rejected: an assembly as a set with no direction, the mover chosen
    when a leg is flown. Two members' legs would contend for the whole,
    and leaving would need its own say.
- **Burns draw from the burner's own tanks; transfers reach the
  assembly.** The rocket equation at a host's burn counts every
  member's dry mass and load; the propellant comes from the host's
  tanks; boil-off is each vessel's. A give to, or take from, a vessel
  reaches the vessel named first and then the rest of its assembly, by
  id, until the amount is met: a depot of several stages is one stock
  to its visitors (DESIGN.md, Facilities: a depot stores propellant and
  sells it).
  - Rejected: tanks pooled for burns too (drop tanks). The manifests
    already say what is piped between vessels, and pooling would need a
    policy for who keeps what when they part.
- **Whether a dock is made is decided in the design, not the loop.** A
  leg that docks arrives while its host is at the stop named, by the
  host's design: the leg waits for the host's arrival (as a meeting
  waits for the orbit now) and fails if the host has left by its
  earliest arrival ("Depot leaves nrho 2045-04-01, before the leg can
  arrive"). The loop has one thing left to find: a host that ran dry
  drifts, and its members are stranded with it.
  - Rejected: a dock the loop finds not made (as a transfer with a
    vessel not there falls short). The member's position in the design
    (riding the host) would then disagree with the loop's (left on the
    orbit), and nothing after it could be trusted.
- **A ship may start docked to a vessel**: `start = { at = { ship = 1
  }, epoch = ... }`, wherever the vessel is at the epoch (it must be
  stopped there: a start on a vessel in flight fails, as does one
  before the vessel's own start). This reverses step 3's "a ship starts
  at a place, not at a vessel": a module gets onto its carrier at LEO
  only so, since a leg meeting a vessel in a low orbit is out of M1b
  (Scope). In career a ship starts at LEO, so its vessel must be there
  then; it is bought with its load as a ship starting at LEO is.
- **A class may have no tanks**: `module`, a station module (dry 20 t,
  no propellant, a 50 t hold, $200M [guess]). It cannot burn, so a leg
  of its own, or a start of its own away from a vessel, is a breach ("a
  module has no engines: it starts docked to a vessel and rides it");
  the loop would in any case run it dry at its first burn or tick.

The language (`orbit::mission`):

```toml
[[ship]]
id = 3
name = "Hab"
class = "module"
start = { at = { ship = 1 }, epoch = "2045-03-06" }   # launched docked to the Tug, at LEO

[[ship.leg]]
dock = { ship = 0, leg = 0 }   # when the Tug has brought it to the Depot: move over

[[ship]]
id = 1
name = "Tug"
class = "tug-h"
start = { at = "leo", epoch = "2045-03-06" }

[[ship.leg]]
to = { ship = 0, leg = 0 }   # meet the Depot after its leg to the NRHO
days = [5.0]
dock = true                  # and dock

[[ship.leg]]
wait = 2

[[ship.leg]]
to = "leo"                   # leaves alone: the Hab has moved to the Depot
days = [5.0]
aero = true
```

- `dock = true` on a leg that meets a vessel: the ship docks on arrival
  and stays docked until it leaves. On a leg to a place, an error. A
  vessel at a body or on a surface is docked to by going there, as it
  is met.
- A `dock` step, `dock = { ship, leg }` (`{ ship }` at the vessel's
  start), with a manifest like a leg's: the ship docks to the vessel
  named where they both are, at the first time at or after it is ready
  when its assembly holds the vessel or has met it at that stop (its
  carrier's leg met it, docked or not). If its assembly never gets
  there, the step fails ("Hab, docked to Tug, does not reach Depot
  (ship 0, leg 0)"). Its manifest's gives are made at the dock, its
  takes when the ship leaves.
- A docked vessel's waits count as now, from its arrival (its start, or
  its dock). Its next leg leaves from where its host is when it is
  ready, if the host is stopped then, else at the host's next arrival;
  the leg is designed from the host's checkpoint (its orbit at its
  phase, its low orbit, its body or surface), as a meeting is designed
  to one now.
- The plan's checks (`Meetings`, which grows to cover docks and starts
  at vessels): a vessel docks to no one through itself; a dock names a
  step the vessel has, not a wait; a start at a vessel is at or after
  the vessel's start; and the designs wait on no cycle, a start at a
  vessel waiting on the vessel's whole plan, conservatively, since the
  step it depends on is known only when designed.

Where the work is (found drafting):

- The planner designs each ship alone, and a meeting reads one
  checkpoint of another ship (step 3 plan, part 3). Riding reads a span
  of the host's design: its checkpoints and items from the dock until
  the member leaves, and for a `dock = true` leg one step further (the
  host's departure, for the dock window). The walk waits on the host as
  it waits on a vessel met, and the step's key takes the host's key at
  the last step read, so an edit of the host before the member's
  departure designs the member again from its dock, and one after it
  designs nothing of the member's. A member that never leaves is docked
  to the end of its host's design, an item re-made as the host's design
  grows (nothing of it is designed, so that is cheap).
- Chains: a vessel docked to a vessel docked to a third. The design
  follows the chain of hosts to the one with an orbit (`Here::Docked`,
  resolved through the fleet); the loop sums an assembly's masses
  through it; the plan check forbids cycles.
- A docked vessel has no stay: its item is `ItemKind::Docked` (label
  "docked to Tug"), from its dock to its departure, with no track: the
  sandbox, `sim run`, the hashes and `Check::At` read its position from
  its host at the time. The loop's membership comes from the dock
  events (a start at a vessel, a dock leg's arrival, a dock step) and
  the departures (the member's next burned item's first marker), not
  from the items' ends.

1. **The language and its checks** (`orbit::mission`): `Step::Dock`,
   `Go::dock`, a start at a vessel; the file read and written back (a
   round trip); `Meetings` over docks and starts; the errors above. The
   `module` class in `data/economy.toml` and its breach (`sim::ships`,
   `sim::params`). Nothing designed differently yet: a dock is read,
   and refused where it cannot be designed.
   - Built (2026-10-21): `Step::Dock`, `Go::dock`, a start at a vessel,
     and `Step::{manifest, manifest_mut, to, meeting, is_stop}` for the
     readers (`orbit::mission`); the file's `dock` key read and written
     (a start's and a dock's vessel as sub-tables, as a meeting's `to`
     is); `Meetings` over docks and starts, its cycle check's nodes a
     ship's start and each step (a start docked to a vessel waits on the
     vessel's whole plan); `check` and `reads` over dock steps; the
     planner refuses the three forms with "not designed yet". The engine
     reads manifests and meetings through the step methods (`designs`,
     `game`, `log`, `ships`); the `module` class ($200M fixed, so that
     the seeds' other draws stay as they were) and its breach; the
     kill-test's LP leaves classes without tanks out (`plan::scenario`).
     The sandbox shows a dock step's row with its manifest; its start
     menu still offers places only (part 6). Tests:
     `docking_reads_and_writes_back` (`crates/orbit/tests/missions.rs`:
     the round trip, the resolved kinds, eleven refusals, a dock step
     where a vessel is in a low orbit allowed, two cycles),
     `docking_is_not_designed_yet` (`meeting.rs`), `a_module_rides`
     (`sim::ships`), `a_replan_may_dock` (`sim::log`), `a_dock_is_keyed`
     (`sim::designs`). Run: orbit's missions, meeting and career tests;
     sim's unit, game and state tests; the sandbox's unit, editor, plan,
     stops, options and windows tests; the kill-test's tests.
2. **Riding** (`orbit::mission`, `sim::designs`, `sim::game`):
   `Here::Docked`, `ItemKind::Docked`; a start at a vessel; a dock
   leg's arrival (the wait for the host's arrival, and the failure when
   it has left) and the member's departure from its host's checkpoint;
   `Fleet::at(vessel, t)`, the walk's wait, the keys; a member docked
   for life. A chain of two. Tests: a lander started on a hauler at LEO
   rides to the NRHO and leaves for low lunar orbit from the hauler's
   orbit, its items as expected; a dock leg failing when the host has
   left; an edit of the host before and after the member's departure.
   - Built (2026-10-21): `ItemKind::Docked(host)`, an event at the dock
     (the member's position is then its host's: no stay of its own, and
     the host keeps station for both), `START` for a start's item;
     `Here::Docked`, `Checkpoint::{ready, docked_to, departs}`, `Others`
     (what a step reads of other ships: the vessel met, when it leaves
     for a leg that docks, and where the host is for a docked ship),
     `Planner::{next, docked, start_items, docked_to, docks}`; a docked
     ship's leg leaves from its host's stop before the host does (the
     window closed at its departure), a dock leg arrives after the host
     and before it leaves (`orbit::mission`). `designs::Designs` (the
     walk's view of the fleet: `met`, `at`), `Host`, `Designed::Start`
     with its items, keys over what is read; `Fleet::at` finds a vessel's
     stop at a time by its design, through the vessels it is docked to,
     keyed by the design that put it there and when it leaves (so an
     edit of the host after the member's departure, leaving that as it
     was, re-keys nothing); `Fleet::met` through a docked vessel;
     `Game::at`; a start docked to a vessel counts as reading it
     (`meets_any`: found by the test, the lander was not re-walked after
     the hauler's edit). The sandbox's sweep refuses a leg from a docked
     vessel or that docks (part 6). Tests:
     `a_docked_ship_rides_and_leaves_from_its_vessel`
     (`crates/orbit/tests/meeting.rs`, 15 s) and
     `a_docked_ship_rides_its_vessel_in_a_game` (`crates/sim/tests/
     game.rs`, 55 s: a chain of two, `Game::at` before and in the
     lander's flight, the two edits). Found: the tests' position helper
     missed a track's first state by a tenth of a microsecond; it now
     allows a millisecond, as the sandbox's does. Known approximation: a
     member leaving its host at LEO before the host flies takes a free
     plane, as the host does. Run: orbit's missions, meeting and career;
     sim's unit, game and state; the sandbox's unit, editor, plan,
     stops, options, windows, tanks, labels and timeline; the kill-test.
3. **The dock step**: its time by the assembly's co-location, its
   failure, its manifest; a module carried by a tug and moved to a
   depot. Tests on that plan.
   - Built (2026-10-21): `Checkpoint::with` (the vessel the leg that
     arrived there met), `Others::dock`, `Planner::dock_step`, the dock
     step's items (a stay to the dock for a ship at a stop of its own,
     keeping station; the dock event; a docked ship rides) in
     `orbit::mission`; `Designs::dock` and the key part "docks" in
     `sim::designs`; `Fleet::trace` (the chain of vessels a ship is
     docked through, each at its own checkpoint, and when the chain
     leaves) and `Fleet::dock` in `sim::game`: from the later of ready
     and the vessel's arrival, the ship is with the vessel when its
     chain holds it, or a vessel of the chain (or the ship itself) met
     it at that stop, or the vessel met one of them where it is; not
     with it, a docked ship is tried at its chain's next stop (when the
     whole chain has stopped: found by the test, the first draft docked
     at the carrier's departure, the chain in flight giving its next
     stop), a ship at its own stop fails at once; the vessel leaving
     first, or the chain's plan ending, fails. The dock's manifest is
     part 4's (the loop). Tests: `a_dock_step_docks_where_the_ship_is`
     (`crates/orbit/tests/meeting.rs`, by hand) and
     `a_dock_step_joins_the_vessel_the_ship_is_with`
     (`crates/sim/tests/game.rs`, 55 s: the plan's module on a tug to a
     depot, the tug leaving alone; a visitor that met the depot docking
     a day later, its stay kept; a ship on its own orbit, a carrier that
     never meets, a depot that leaves first, each in its words).
4. **The loop** (`sim::state`): membership in time; a burn against the
   assembly's mass from the burner's tanks; no upkeep for a member;
   members stranded with a dry host (`Dry` says so: "stranded: Depot,
   which it is docked to, ran dry"); transfers reaching the assembly,
   the vessel named first; `there` through the host. Tests by hand: the
   rocket equation with a rider; a take across two stages; a stranded
   member.
   - Built (2026-10-21): a ship's docks from its items (the vessel it is
     docked to from each dock until its next flight's burns begin, or
     the next dock, or for ever), the chain of vessels at a time, the
     riders of a vessel and their mass, a vessel's assembly; a burn's
     inert mass is the burner's dry mass and its riders' (legs and
     upkeep alike), from its own tanks; a vessel that runs dry strands
     its riders (`Dry::with`, "with ship 0, which ran dry": the report,
     the panel and the game's check say "stranded"); dock events in the
     loop (after arrivals, before gives: both pay up, and a ship docking
     to a vessel that ran dry is stranded at once); a transfer with a
     vessel reaches its assembly, fitted members but the ship itself,
     the vessel first and the others by id, each paid up first (a take
     drains each one's hold then tanks; `Short::Stock` against the
     assembly's stock). A dock at `t` counts from the moment after, a
     flight leaving at `t` until it (`host_of`). `sim::state`;
     `sim::game` hashes the stranding. Tests (`sim::state`):
     `a_vessels_burn_moves_its_riders` (the rocket equation to 1 ng),
     `a_vessel_keeps_station_for_its_riders`,
     `a_transfer_reaches_the_assembly` (a take of 390 t: 100 from the
     depot, 290 from the stage; a give: 50 and 340),
     `a_rider_is_stranded_with_a_vessel_that_runs_dry` (and a take from
     it falls short, dry).
5. **Career and headless**: a start at a vessel bought at LEO (and
   refused elsewhere); `Check::Docked { ship, at, to }` (`docked_to =
   0`, or `"no one"`), `Check::At` through hosts; `sim run` lines
   ("docked to Tug 2045-03-06 — 2045-03-11 04:12"; a burn's "with Hab,
   20 t"); the scenarios: `data/scenarios/station-assembled.toml`
   (career: a depot stage at the NRHO; a tug carrying a module docks
   and leaves it; a lander docks, takes propellant and the module's
   equipment, and lands) and the carried lander (the hauler's
   propellant on arrival against the depot scenario's 9.859 t). The
   hashes at 1 and 24 threads.
   - Built (2026-10-21): career buys a ship at LEO or docked to a vessel
     that starts there (`allowed`, through chains of starts), and refuses
     one whose vessel is elsewhere at its epoch once designed
     (`Found::away`: "starts docked to ship 0 at nrho, not at LEO, where
     it is bought"); a start docked at LEO is bought with its load.
     `Check::Docked` (`docked_to = 0`, or `"no one"`: the dock in force,
     from the dock until the ship's next flight's burns or its next
     dock, `state::docked_to`); `Check::At` through hosts ("docked to
     Depot, at nrho"; in flight with the host, not held). `sim run`
     prints a dock as "docked   to Depot (ship 0)  2045-03-15 06:06,
     until 2045-03-19 21:46" (or "docking anew", or "for the rest of the
     plan") and a burn's ", with 30.000 t docked" (`ShipState::riders`).
     Scenarios: `data/scenarios/station-assembled.toml` (career: the
     depot stage and tanker of the depot scenario; a hydrolox tug
     carrying a module with 10 t of equipment docks and leaves it; a
     lander docks, fills its tanks from the depot and takes 5 t of
     equipment from the module through the depot, lands and comes back
     up; 21 checks) and `data/scenarios/hauler-carries-lander.toml`
     (career: a hauler with 800 t carries a 340 t lander to the NRHO; 9
     checks). Measured (2026-10-21, release): the tug burns 108.644 t
     carrying the module against 97.4 t alone; the hauler 771.573 t with
     the lander against 205.141 alone, arriving with 28.427 t; the depot
     keeps station for the module at 0.110 t over 90 days. Tests:
     `an_assembly_is_the_same_on_any_threads` (1 and 24 threads: the
     same items and hashes), `career_buys_a_docked_start_at_leo_only`,
     `a_docked_check_reads_back` (`sim::log`); `every_scenario_holds`
     over four scenarios. Found: the lander's take lines name the depot
     while the equipment came from the module (the assembly is one
     stock; the report says whom the manifest named), and a module's
     hold line at its own last item predates a later take from it.
6. **The sandbox**: positions through hosts (`app::ship_position`;
   labels: an assembly's vessels at one point share a label); the
   panel's "docked to Tug" rows and an assembly's mass on the host's
   burns; the editor: a start's menu offers vessels, a dock toggle on a
   leg to a vessel, a "dock to" step. GPU tests as step 3's.
   - Built (2026-10-21): a ship's position through the vessel it is
     docked to where none of its own tracks spans the time
     (`app::ship_position`, to the depth of the fleet); the vessels at
     one point share a label, the root's name first ("Depot + Hab");
     the panel's dock rows by name ("docked · to Tug") and a burn's
     " · with 20.000 t docked"; the editor's `Field::{Dock, DockTo,
     AddDock}` (a "dock" switch beside "aero" on a leg to a vessel, a
     "dock to" menu of the vessels on a dock step, "+ Dock" adding one
     to the first other vessel), the start's menu offering each other
     vessel ("Tug — docked, where it is then") before the places and
     showing "Tug, docked"; the sandbox's sweep reads what a leg from a
     docked vessel, or one that docks, reads (`Others` from `Game::met`
     and `Game::at`), so those legs sweep too. Tests: `docking_is_edited`
     (`sandbox::editor`), the start test's expectation turned around,
     and `a_module_is_docked_in_the_editor` (`crates/sandbox/tests/
     editor.rs`, GPU: the tug's switch, the module's start and dock step
     from the menus, the designed docks in the panel by name, the tug's
     leg with 20 t docked; shots dock-switch, start-menu, dock-step,
     assembly-panel).
7. **Tests and measurements**: the loop's time with assemblies; the
   design waits with a chain; the full workspace suite.
   - Done (2026-10-22): each item has its test, written with the part it
     tests (the parts' Built bullets above; the chain's waits in
     `a_docked_ship_rides_its_vessel_in_a_game` and
     `a_dock_step_joins_the_vessel_the_ship_is_with`, `crates/sim/tests/
     game.rs`). Measured (`the_assembled_station_runs_again_quickly`,
     ignored, release, load 10–30 from other work): the assembled
     station's 20 designs in 18.0 s on 24 threads and 98.9 s on 1; the
     loop alone 0.036–0.050 ms, and 0.075–0.136 ms with a new load of
     the depot (215, 214, 213, 212, 215 t), which designs nothing; `sim
     run`: station-assembled 24.4 s wall, 136 s user;
     hauler-carries-lander 9.2 s, 33 s; depot-tug-lander 20.4 s, 91 s;
     every check held (21, 9 and 17). The full workspace suite: 361
     passed, 13 ignored (measurements), none failed, in 12.8 min of
     tests (the binaries' times summed; compiling not counted).

Left out of step 4: undocking to stay alone (above); tanks pooled for
burns; docking's duration (instantaneous at the arrival; a setting if
wanted); crew moving between vessels (step 8); meeting or docking in a
low orbit (Scope).

## Step 5 plan (2026-10-22): plants, programs and surveys

Plants that turn equipment delivered to a site into propellant and
metal, the programs that must be finished before they are installed,
the surveys that reveal what a deposit holds, and the first keyed draws
(Design, Plants, programs and surveys; Steps, 5). Drafted 2026-10-22;
ruled 2026-10-23, each decision as drafted; each part is committed on
its own.

What step 5 adds to step 4's vessels: today equipment is a commodity a
ship carries and gives into a vessel's hold, and nothing is made
anywhere; every tonne of propellant comes from Earth at LEO. After step
5 a lander lands at the south pole with equipment and installs it into
a plant option, and from a year later the site's stock grows by what
the plant yields; a lander landing there fills its tanks from it.
Technology programs gate what can be installed, and in career a
deposit's grade is known only by a survey's estimate, or once a plant on
it runs.

Scale, at the central draw (`data/economy.toml`): a tonne of equipment
on the rich lunar ice yields 28.6 t a year of water, 25.5 t of LOX and
3.2 t of LH2 (the typical deposit 23.5, the lean 13.2); regolith oxygen
2.9 t a year per tonne, Mars ice 2.8, Phobos 4.6, asteroid volatiles
9.1, PGM 3.3 kg. M2's central world's 66 kt a year of lunar propellant
is ~2,300 t of equipment, $23B at $10,000/kg before launch. The lunar
ice program is $3.3B over 5 years and its survey $300M over 3; a plant's
build is 12 months.

Decided (2026-10-23, each as drafted):

- **Sites hold stocks, and a plant's output goes into its site's.** A
  surface or a body (the south pole, the mare, Mars's surface, Phobos,
  each asteroid) is a site: a place with a stock by commodity and the
  plants installed there. A ship there transfers with the site
  (`With::Site`), as a ship at LEO transfers with Earth: what its
  manifest gives is stored, what it takes comes from the stock. Storage
  is unbounded, as in M2 (a tank farm, if wanted, is a later facility);
  hydrogen boils off in a site's stock as it does in a tank. This
  settles step 3's "where plants' output goes": not into a vessel.
  - Rejected: a plant as a vessel. A plant sits on its body, on the
    ephemeris's rails, so it is not a vessel by the design's definition;
    it would need an id, a class and a manifest's naming, and two plants
    at one site would be two stocks a lander must choose between.
- **Installing is a give of equipment with `install = "option"`** on
  the manifest of a leg to a site: the equipment the manifest gives at
  that stop goes into the plant option named (`lunar-ice:rich`,
  `regolith-oxygen-polar`, `mars-ice`, `volatiles@ryugu`) instead of
  the stock, and `build_months` later each tonne adds 1/α t/yr of
  capacity. The technology must be finished when the ship arrives; the
  survey is not needed (building blind; DESIGN.md, Prospecting). Not
  finished, the give falls short (`Short::NotReady`: "lunar-ice is not
  finished until 2046-01-01") and the equipment stays aboard; career
  refuses such a plan as it refuses one short of stock. The option must
  be one of the site's (a static error otherwise); which deposit a
  plant works is the player's choice, as in M2, and a plant stays on its
  deposit when the deposit runs out.
  - Rejected: an install step without a ship (nothing on the surface
    would make it); installing later from the site's stock (a rule for
    when, and by whom).
- **Programs are actions in the log,** `[[program]] id = "lunar-ice",
  start = "2040-01-01"`, technology and survey alike (both are
  `Program`s). A program finishes `years` after its start and spends
  its cost evenly over them, recorded as a ship's purchases are (the
  account is step 8's). Each starts once. Career starts one at now; the
  sandbox anywhere. A program's cost and years are known when it
  starts: the hidden information is the deposits', not the programs'.
- **A survey's estimate is the first keyed draw.** When a survey
  finishes, each deposit it covers (the lunar ice's three share one)
  gets an estimate of its grade and of its reserve, the truth times
  exp(σz), z standard normal from the stream keyed by the survey's id,
  σ the survey row's `noise` (new, fixed; [guess]: 0.3 for lunar ice and
  the asteroid prospectors, 0.4 for Mars and Phobos, which are mapped
  from orbit: LCROSS put Cabeus at 5.6 ± 2.9 wt%, and a rover does
  better), and learns whether the resource is there at all, exactly.
  The stream: `ChaCha8Rng` seeded by the fixed hash (`sim::hash::Fixed`)
  of the game's seed (0 for the central draw) and the key
  `survey:lunar-ice` (step 1 plan, part 6). The estimate is drawn in the
  sandbox too: it is state, and a career log opened in the sandbox
  hashes the same.
- **Absent deposits stay in the economy.** Today a world where Phobos
  is dry, or an asteroid barren, has no option and no survey for it, so
  a plan naming one would fail to read and give the draw away. Every
  option and survey the file describes exists in every world,
  `Facility::present` false where the draw left it out, its grade as
  drawn (the samples are made today before the `continue`); the absent
  option's survey has its cost and years sampled last, after everything
  drawn today, and an asteroid's absent kinds get no grade and no draw,
  so no seed's other parameters move. A plant on an absent deposit is
  built and yields nothing. The kill-test's LP skips absent options
  (`plan::scenario`).
- **Career's knowledge is a policy, and the judge uses it.** For each
  option, `Known::{Exact, Estimate, Prior}`: the sandbox knows
  everything; in career an option is exact once a plant on it has
  finished building (its output is there to see), estimated once its
  survey has finished, and before that the prior: the file's central
  grade and reserve, the output scaled by the probability the resource
  is present. The game's state runs on the truth. Career's judge (step
  3 plan, part 6: a plan is refused when it runs a vessel dry or a
  transfer short) runs its projection with the knowledge of now instead
  of the truth, so that a refusal tells the player nothing a survey
  would: a blind build is accepted on its prior and plays out on the
  truth. The loop takes the grades to use as an input (`state::run(…,
  &Knowledge)`): the truth for the state, the known for the judge. M3
  draws `Game::known`.
  - Rejected: judging on the truth. The grade would be hidden on the
    screen and read off the refusals, a free survey.
- **Production accrues continuously** at the capacity in force, paid up
  when a transfer is made at the site and at each midnight for the
  hash, as upkeep and boil-off are, with hydrogen's boil-off and the
  deposit's reserve in closed form (the stock's hydrogen tends to P/λ;
  the reserve's exhaustion is found at its moment). The Design's "a
  daily tick runs production" was written before the loop paid lazily
  (step 2 plan, part 3); this keeps the loop's time independent of how
  many sites sit idle, and a take made during a day exact. The Design
  is rewritten with the ruling.

The language (`orbit::mission`, `sim::log`):

```toml
[[program]]
id = "lunar-ice"            # a technology, from data/economy.toml
start = "2040-01-01"

[[program]]
id = "survey:lunar-ice"     # a survey, likewise
start = "2041-06-01"

[[ship]]
id = 0
name = "Lander"
class = "lander"
start = { at = "leo", epoch = "2045-03-01", cargo = { equipment = 10.0 } }

[[ship.leg]]
to = "llo"
days = [3.5]

[[ship.leg]]
to = "moon-south-pole"
cargo = { equipment = 0.0 }   # give all of it ...
install = "lunar-ice:rich"    # ... into the rich deposit's plant

[[ship.leg]]
wait = 3

[[ship.leg]]
to = "llo"

[[check]]
program = "lunar-ice"
at = "2045-01-02"
finished = true               # or spent_musd = [lo, hi]

[[check]]
site = "moon-south-pole"
at = "2047-03-08"
stock = { lox = [250.0, 260.0], lh2 = [26.0, 32.0] }
# or: capacity_tpy = { "lunar-ice:rich" = [286.0, 287.0] }

[[check]]
survey = "survey:lunar-ice"
at = "2044-06-01"
grade = { "lunar-ice:rich" = [0.04, 0.07] }   # the estimate; or present = true
```

- `install` goes with a leg to a site, naming one of its options; on a
  leg elsewhere, or to a vessel, an error. The stop's equipment given is
  installed; the other commodities given are stored.
- A `[[program]]` names a program of the economy, once; `start` to the
  minute, as a re-plan's `at` is. In career `start` must be now when
  the action is made. A program runs nothing dry and needs no judging:
  a log read back applies its programs first, each checked statically,
  then its ships and re-plans as now (step 3 plan, part 6), so that an
  install sees its technology's finish.
- Checks: `program` (finished, or spent so far), `site` (the stock by
  commodity, the capacity by option, within bounds), `survey` (an
  estimate within bounds, or present). `Check::Transfer`'s `give =
  "equipment"` covers an install, short or made.

Where the work is (found drafting):

- The loop keeps sites beside ships: `SiteState { place, stock, plants
  }`, a plant per option with its installments (when, tonnes, ready),
  what it has produced and whether its deposit is exhausted;
  `State::sites`. `With::Site(PlaceId)` where `With::NoOne` is today at
  a surface or a body (`Game::stops`, by the route graph's id);
  `Stop::install`. The hash adds each site's stock, capacity and
  installments by day, and each program's spending.
- α follows the grade: `Facility` keeps `plant` and `ore` apart (today
  only their sum, `alpha`), so that a known grade gives a known α for
  the judge's projection, and grows `present`, the prior grade and
  reserve (`Dist::central`); the survey's `noise` rides on its
  `Program`. The lunar ice's three deposits share one survey (one
  estimate each, from one stream); an asteroid's kinds have a survey
  each (`survey:volatiles@ryugu`, `survey:pgm@ryugu`): learning one kind
  absent leaves the other open, one survey more than a visit would take
  (an approximation kept, for the ids M2's LP uses).
- Knowledge threads through `Game`: the state's loop runs with the
  truth, the judge's with `Game::known(now)`. The prior's capacity
  scales by the presence probability, a mean.
- Programs are kept in `Plan` as written (`Plan::programs`, as
  `replans` and `checks` are) and read by `sim::log` into
  `Log::programs`; the sandbox's editor adds and dates them.
- The sandbox's panel gets rows apart from ships: each site with plants
  or stock (the stock by commodity, each option's capacity and what is
  building, the deposit's reserve left or exhausted; the truth, since
  the sandbox knows everything, with the estimates beside it where a
  survey has finished), and each program (started, finishes, spent).
  The editor's manifest gets an `install` menu of the site's options;
  "+ Program" adds one at the clock's date.

1. **Keyed draws, and the economy's additions** (`sim::draw`,
   `sim::params`): `Draw::keyed(seed, key)`, the fixed hash of the seed
   and the key seeding a `ChaCha8Rng`, a normal and a chance from it;
   `Facility::{plant, ore, present, prior_grade, prior_reserve}`,
   `Program::noise`, `noise` in `data/economy.toml`'s survey rows;
   absent deposits kept, their surveys sampled last; the LP skips them
   (`plan::scenario`). Tests: the same key gives the same stream and
   different keys different ones; a stream's first value is fixed (as
   `hash::it_is_fixed` is); the facilities' and programs' numbers of the
   central draw and seeds 1–3 are unchanged by the additions (captured
   before and compared after, once, as step 2's part 1 did); M2's tests
   pass unchanged.
   - Built (2026-10-23): `Draw::keyed(seed, key)` on `Draw` in
     `sim::params` (no module of its own: the fixed hash of the seed
     and the key seeds a `ChaCha8Rng`, always random); `Dist::sample`
     and `Draw::chance` public. `Facility::{plant, ore}` with
     `alpha()` and `alpha_at(grade)`, `present`, and `prior: Prior {
     grade, reserve, present }` (the file's centrals and the presence
     probability); `Program::noise` from the survey rows' `noise`. The
     draw keeps absent options (an absent deposit's reserve the
     central, not drawn), makes an option of each `asteroid_plant` kind
     for every asteroid (the absent kinds at the centrals, nothing
     drawn), and makes the absent options' surveys after the ops
     samples. `plan::scenario` keeps the present options and the
     programs that are technologies or a kept option's, indices
     remapped: the 16 scenario texts of step 2's part 1 (the central
     draw and seeds 1–3, 3 and 6 months, the full graph and the NRHO
     hub) are the same to the bit. Tests: `keyed_draws_are_fixed` (the
     first two values pinned from this build, not computed apart as the
     hash's were: a guard on the rand crates), `absent_options_are_kept`
     (the central world's dry Phobos: the option, its survey after the
     present options' surveys, both kinds on every asteroid), the
     planner's `absent_options_and_their_surveys_are_left_out`; the sim
     suite passed with its hashes unchanged.
2. **Programs in the log** (`orbit::mission`, `sim::log`, `sim::game`):
   `[[program]]` read and written back; `Log::programs`; a program once,
   known to the economy; career's start at now, and a log read back
   applying its programs first; `ProgramState` (started, finishes,
   spent by a time); `Check::Program`; the hash; `sim run`'s lines
   ("program  lunar-ice  2040-01-01 — 2045-01-01, $3,300M; spent
   $1,980M by 2043-01-01"). Tests: the round trip and refusals; career
   refuses a start not at now and a second start; the hash with a
   program.
3. **Sites and plants in the loop** (`orbit::mission`, `sim::ships`,
   `sim::state`, `sim::game`): `install` on a manifest, `With::Site`,
   `Stop::install`; `SiteState`, installments and capacity after the
   build, production accrued at each event and midnight, the reserve's
   exhaustion, hydrogen's boil-off in the stock; a give stores, a take
   draws; `Short::NotReady`; the hash; `Check::Site`; `sim run`'s site
   lines ("site  moon-south-pole: lunar-ice:rich 286.3 t/yr from
   2046-03-08 (10.000 t); LOX 254.483 t, LH2 29.112 t"). Tests by hand:
   an installment yields from its ready date at 1/α; a take draws what
   accrued; the reserve caps it at its moment; hydrogen in the stock
   tends to P/λ; an install before the technology falls short and the
   equipment stays aboard; an absent deposit yields nothing.
4. **Surveys and knowledge** (`sim::state`, `sim::game`): the estimate
   at a survey's finish from its keyed draw (`Survey { estimates by
   option, present }` in the state); `Known`, `Game::known(t)`; the
   judge on knowledge (`state::run` with a `Knowledge`); career refuses
   an install whose technology is not finished at the arrival
   ("installs lunar-ice:rich 2045-03-08, before lunar-ice is finished
   2046-01-01") and accepts a blind build on its prior; `Check::Survey`.
   Tests: the estimate is the same on 1 and 24 threads and in a career
   log opened in the sandbox; the judge accepts a blind build that the
   truth would refuse, and the state then falls short; an install
   before the technology is refused.
5. **The sandbox** (`sandbox::panel`, `sandbox::editor`): site and
   program rows; the editor's `install` menu and "+ Program"; the map
   unchanged (a site is a place already drawn). GPU tests as step 4's:
   the rows by name after a plan with an install; the editor's menu.
6. **Headless and the scenario** (`sim::main`, `data/scenarios/`): `sim
   run` prints programs and sites after the ships, and the hash covers
   them; `data/scenarios/lunar-ice-plant.toml` (career: the lunar ice
   program from 2040 and its survey from 2041, the estimate checked
   against the truth's bounds; a methalox lander lands in 2045 with 10 t
   of equipment and installs it on the rich deposit; a hydrolox lander
   lands two years later and fills its tanks from the stock; checks on
   the programs, the survey, the capacity, the stock and the takes).
   The step 3 and 4 scenarios unchanged.
7. **Tests and measurements**: the loop's time with sites on the plant
   scenario (the published plan's 5.5 ms is the mark); the design times
   unchanged (nothing of step 5 is read by the planner); the hashes at
   1 and 24 threads; the full workspace suite.

Left out of step 5: storage capacity at a site (a tank farm); plant
wear, operating costs and power (M2 has none); a program's schedule risk
(its years as a keyed draw); a survey mission flown (a survey is a
program paid for, as in M2; a flown prospector is a later realism
setting); installing from a site's stock; selling the output (steps 7
and 8); crew at a site (step 8).
