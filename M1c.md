# M1c — Creative sandbox and the map view

Status, 2026-10-16: steps 1–7 done (6 done before 5, its shape models
left out; of step 6, part 9's spins and small bodies' maps open); now
step 8, the gate (step 8 plan, at the end).

- Step 8, so far:
  - Item 1 holds at the end of step 7: the published plan flies with no
    jumps (the low lunar orbit round trip's return now from its own
    orbit, +62.0 m/s), its Δv as at step 1 (Mars handbook departures
    3,576 / 3,550 / 3,523 / 3,583 m/s, 2000 SG344 4,038, Phobos 1,205
    each way, NRHO to low lunar orbit 684). `orbit mission` designs ships
    one after another, so its times are each leg's alone: 0.1–3.7 s,
    and LEO to the DRO 16.5 s, over item 5's 10 s.
  - Item 3's plan, `data/missions/mars-windows.toml`: the 17 ships fly
    (5,611–6,284 m/s, 202–348 days), designed in 50 s one after another.
  - Item 3 measured in a window on DP-1 (3840×2160 at 240 Hz, HDR), a
    month a second around the Sun: frames every 7.28 ms (95%: 8.79), GPU
    0.98 ms. Not at the refresh (4.17 ms), and CPU-bound. Without a plan,
    6.81 ms: the plan was not the cause.
    - The bodies' orbits were one set of paths, prepared again whenever
      any orbit's window moved: every frame at a month a second
      (Phobos's step is half an hour), ~62,000 small bodies' samples
      with it. Now a set each (from `Shell::orbit_sets`): 5.87 ms
      without a plan, 5.67 with it.
    - Of the rest, `queue.submit` seemed to take ~2.2 ms a frame in the
      kernel (`DRM_IOCTL_AMDGPU_CS`, ~1.1 ms a call). That was a second
      sandbox rendering at the same time (found 2026-10-16 with a preload
      probe timing each submission). Alone, a submission takes
      0.06–0.08 ms, two a frame (vkcube's 0.023). With two sandboxes at
      once, the later one's take 0.9–1.1 ms. The 6900 XT here has no
      resizable BAR, so 256 MiB of its VRAM is CPU-visible, and RADV puts
      each command buffer's chunks there (80 KiB of commands, 16 KiB of
      uploads). A frame is ~48 command buffers (wgpu makes two a pass,
      most of them the glare's chain), so a submission carries ~96, and
      two such processes move them in and out of that window at every
      submission. With `RADV_PERFTEST=nosam` (the chunks in system
      memory) or `localbos`, two at once stay at 0.03–0.06 ms. Smaller
      wgpu staging blocks (`MemoryHints::MemoryUsage`, the sandbox's
      share of the window from 83 MiB to 14) did not help, and vkcube
      beside the sandbox does not cause it.
    - The compositor caps it: alone, frames come every 4.6–5.9 ms by run,
      a frame's work ~1.9 ms; vkcube on the same GPU and output runs
      ~196 frames a second; prism's own frames on DP-1 take 2.3 ms
      (peak 3.5), 19% of them a vblank late. Known to the user, and being
      worked on in prism (2026-10-16).
    - Windows the compositor puts on the Radeon VII's outputs render on
      the 6900 XT and cross between GPUs: the 18.5 and 50 ms frames of
      steps 2 and 4.
  - Item 5, frames while the Mars plan designs (all cores): 6.8–7.7 ms
    on average, 95% 12.5–15 ms, the longest 79–107 ms (up to 203 in
    later runs, 13 frames over 25 ms of work a run); after it, the
    longest 65–72 ms.
    - Each time items arrived, every ship's lines were built again on
      the main thread (4–126 ms) and the renderer prepared and uploaded
      them all (14–68 ms): ~980,000 samples, 96% of them the 18–24 days
      each ship waits in LEO (2,900 samples a day, below), only a
      revolution of which is drawn at a time. Even with designing done,
      a rebuild took ~60 ms, so an edit stalled the view too.
    - Now each ship's lines are a set of their own (`paths::SHIP_SETS`),
      built again when its items change (`plan::Ship::revision`, unique
      in the process) or its colors do (its first leg's place); the
      frame, its center or the lines shown build all. Alone, a ship's
      set of ~60,000 samples builds in 1.7 ms and prepares in 2.0.
    - Beside designs at the same priority, a ship's build still took
      33–67 ms: the design pool has a thread per core, and the main
      thread's parallel work runs on rayon's global pool. The design
      pool, and reading the bodies' maps (on the global pool before, a
      task per body, which ran to its end ahead of the view's work at
      start), now run on pools 10 nice steps below the thread that starts
      them (`background`, Linux).
    - Measured again (9 runs, load average 8–12 from other work on the
      machine where recorded): frames while designing every 4.5–5.1 ms
      on average (95% 7.5–8.3), 0–3 over 25 ms of work a run (0 in four
      of seven), the longest 12–65 ms; after designing, the longest
      9.0–12.5 ms (five runs).
    - Left: a change of frame or focus builds every ship's set again,
      ~60 ms with this plan (the LEO waits' samples).
  - Item 5, each leg designed alone: LEO to the DRO takes 16.5–16.9 s
    (35 s of CPU), over the 10 s; M1a left it open (M1a.md, gate: cold
    designs 5–32 s between stations, "LEO to the DRO" open). Where the
    time goes (`legs::plan`): seeds 0.2 s, the 8 starts 1.5 s, then
    continuation (`Leg::frontier`) from the starts' 6–8 days to the 9
    asked for, 10.7 and 14.9 s for the two chains that reach the cheap
    family (both to 3,383.8 m/s). Steps of 0.125 days fail (0.15–0.25 s
    each), halved ones succeed (0.6–1.5 s, each a whole design), and the
    size doubles back to fail again: ~16 of each over the day. Asked for
    8.0 days, the leg takes 7.2 s; 8.5 days, 17.4 s.

- Step 7, part 8: leaving a low orbit from the ship's own orbit (from
  M1b; how decided 2026-10-15: small plane offsets paid at once, large
  ones waited out, else the jump reported).
  - A leg leaving a low orbit the ship reached by a flown leg (or an
    aerocapture) is designed as before, with its plane free, then again
    from the ship's own orbit: `End::Orbit` on a reference coasted from
    where the ship arrived (`own_reference`), the duration kept. The
    seed (`legs::own_orbit_seed`) is the free design turned about where
    it heads (`legs::heading`: its departure conic's asymptote, or its
    apoapsis if bound) into the plane nearest the ship's
    (`legs::turn_about`, near the departure body only, not near its
    moons; turned half a revolution, it leaves the other way), its
    departure moved to where the ship passes nearest it within half a
    revolution, with the burn's size along the ship's velocity.
  - Which flies (`Planner::own_orbit`): at once, if it costs at most a
    tenth of the free design's departure burn more (`PLANE_FAIR`).
    Else, within a system, the leg waits for the planes to align: over
    a synodic period of the moon its designs move with (or the window
    the plan sets), the times the free design, moved later with the
    moon (`carry`), heads along the ship's plane (`legs::aligned`);
    about each of the first two, five departures a few hours apart are
    designed. The cheapest about the first alignment with a fair one
    flies; else the cheapest of all, if at most a quarter of the
    departure burn more (`PLANE_MOST`); else the free design flies and
    the jump is reported, the leg's note saying what leaving from its
    own orbit would cost. Between planets the date is the date
    search's (or the pin's) and only the phase moves.
  - A station reached by a leg that may wait is followed through the
    wait (its reference continued a synodic month longer), which moves
    its design with the plane free a little: low lunar orbit to the
    NRHO in the labels tests' plan, 689.0 to 700.3 m/s.
  - Measured (each in 0.2–5 s):

    | Leg | Plane free | From its own orbit |
    |---|---|---|
    | LLO to LEO, published round trip (6.9° off) | 3,949.9 m/s | at once, +62.0 (the jump: 197) |
    | LLO to LEO on arrival, 2033-04-02 (166° off) | 3,949.9 | 13.3 days after the free design, +29.7 |
    | LLO to the NRHO | 696.1 | at once, +35.0 |
    | LEO after an aerocapture, to the NRHO | 3,508.6 | 0.8 days after, +69.2 |
    | LEO after an aerocapture, to LLO | 3,951.0 | 4.9 days after, +28.0 |
    | Low Mars orbit after an aerocapture, to Phobos (56° off) | 1,205.0 | at best +1,438: jumps |
    | LEO to Mars on arrival (91° off) | 6,149.7 | +2,773 at once: jumps |

    (After the review's fixes below; before them the middle rows were
    +11.0, +24.0, +58.3 and +245.) Passes next to each other about an
    alignment differ by tens of m/s (the 166° case: +29.7, +51.3,
    +128.5 two hours apart; +11.0 on the pass before), so which passes
    are tried moves these: the review's change to the longest wait
    moved the steps between tries. Phobos orbits in Mars's equator, so
    from a low orbit 56° off it takes a plane change: real, and left
    reported.
  - Tried and dropped: requiring the planes to match, rather than
    where the departure heads to lie in the ship's plane, found no
    alignment from LEO to LLO (43°) or to Phobos, and only 11° for the
    166° case (+101 m/s); heading to where the path leaves the body's
    reach picked worse times than the departure conic's (LEO to the
    NRHO: 10.4 days, +554); planning afresh at an aligned time found
    nothing in short windows, and left when it liked. Designs from the
    ship's orbit are sensitive to the departure time (LEO to LLO about
    one alignment: +245 to +921 m/s across half a day), hence five
    tries about each.
  - Not done: waiting between planets (a plane cost per date could
    join the date search); seeds keeping a station's phase when moved
    (moved with the moon, a design meets the NRHO elsewhere on it): low
    lunar orbit to the NRHO from 2045-03-06 costs +449 m/s at once and
    no design converges about an alignment, so it jumps.
  - Tests: the published round trip leaves at once, continuous to 1 m;
    the 166° case waits and the Mars leg after it jumps; a departure
    turned about its heading heads the same way, in the plane asked
    for (unit).
  - Waits drew as a dense shell over the planet (each revolution of a
    stay drawn): a stay is now drawn and picked a revolution about the
    clock (`render::Path::window`, from `Track::revolution`), its first
    or last when the clock is outside it.
  - A review of this part, the drawing and the planning change (a
    sub-agent, with runtime probes) found, fixed:
    - Options were not what pinning flies where a station's visit is
      sized by the longest duration designed: NRHO to low lunar orbit
      offering 0.5 and 8 days showed 794.7 m/s for 0.5 days, and pinned
      it flew 683.8. Visits are now sized by the durations offered and
      the pinned one together (`Go::longest`), and sweeps pin each
      duration; the test now offers 8 days and fails when undone.
    - A seed whose departure was held inside its window turned into the
      plane of the ship's orbit at the wrong time's Moon (at 70°:
      −72.1° and −82.9° turns against −65.4°); now the same.
    - Between planets a departure could leave up to a revolution before
      the plan's window or pin; now not before it.
    - About Phobos the tries about an alignment were minutes apart and
      landed on one pass; there, the alignment alone is tried.
    - The longest wait came from the Moon's rates at the moment: 26.8–
      32.9 days; now from the mean motions, 28.9–29.6.
    - A coast that failed while looking for alignments dropped the
      design at once; now it keeps it.
  - Costs: a leg offering five durations, leaving low lunar orbit for
    LEO, re-plans in 3.3 s (after a 60-day stay, 4.6 s) on this
    machine's 24 threads; on 4, 10.1 and 14.3 s (gate item 5 asks for
    10 s on the development machine). The designs dominate: coasting
    the stay once rather than for each design changed nothing
    measurable, and parted the stay from the leg by 1.3 m; not kept.
- Planning (decided 2026-10-15): each duration a leg within a system
  offers is designed alone, so an option is what pinning it flies (see
  part 5).

- Step 7, a review of parts 2–5 (a sub-agent, with runtime probes)
  found, all fixed:
  - Picking an option overwrote the leg's own durations and window, so
    Unpin lost them (the published Mars 2031 leg pinned and unpinned
    flew 291.66 days at 6.641 km/s, not its 190 at 7.512). Choices are
    now a `pin` beside `days` and `window` (replacing part 3's
    `depart`); Unpin takes the pin away. Within a system a pinned
    duration is designed alone and the offered ones stay in the file.
  - A pin in the porkchop's first column, rounded to the minute, could
    fall before the ship is ready and fail; a pin within half a minute
    of it now leaves when the ship is ready.
  - Designed again from its start, a ship kept its old start's
    checkpoint, and a later edit to its first leg could go on from it
    (a start moved from low lunar orbit to the NRHO, then the first leg
    edited: "llo → llo", no design converged). Fixed; the test fails
    when undone.
  - A failure was kept when it hung on the steps after it (a station
    reached, its visit's length from the next leg): `resume_from` no
    longer goes on from where a plan had stopped, and steps back once,
    not to the first non-station checkpoint, from a station.
  - A selected leg that failed showed "designing" forever and could
    not be selected again; its options now say why it failed.
  - A small body's leg whose ship is ready before 2040 panicked in the
    synodic period (and its porkchop would have had no rows): crossings
    take their periods and durations where the ephemeris knows both
    ends, and porkchops without rows are none.
  - Window lanes were keyed by their rounded labels; now by their ends
    exactly. Lanes and porkchops share one shading.
  - Lesser: a porkchop kept from before an edit was clickable while
    the leg was designed again (now none is offered until it is); a
    one-day window drew a one-column porkchop (now at least 120 days
    from its opening); sweeps of short legs folded to four durations
    (now to two significant figures); a second sweep of a leg could
    take the first's result (sweeps have ids); superseded designers
    finished a leg before stopping (they check before each step, and
    stop when the plan is closed); a plan file named `new.toml` clashed
    with "New plan"; the plan's start went stale after start edits; a
    file opening with a blank line lost its comments.
  - Not changed: porkchop rows are the date search's default durations,
    so a leg's own `days` outside them fall off the chart.

- Step 7, part 7: beyond picking (asked 2026-10-15). Tried and kept,
  for legs between planets:
  - A Δv budget: a slider under the porkchop, from its least Δv to 1.5
    times it, dims the cells over it. "Fly fastest" pins the shortest
    leg within it (the cheaper of equals), "Fly earliest arrival" the
    one that arrives first; both are marked on the chart (■, □) and
    named under it. Within 1.1 times the least for the 2033 Mars
    window: 157.7 days against the cheapest's 199.7, flown at 6.549
    km/s (patched conics: 6.57).
  - "Later window" moves the leg's window a synodic period of its
    planets on, unpinned (a pin's duration dropped with it); "Default
    window" takes the window away.
  - The selected leg's bar dragged on the timeline moves its departure:
    it lands on the cheapest leg of its porkchop leaving nearest then
    (dragged 60 days: 2033-06-30, 385.66 days, a slow transfer). A drag
    that starts anywhere else pans.
  - Pins and these moves are undoable like edits.
  Considered and not built:
  - Moving a ship's start moving its pinned legs: departures between
    planets are fixed by the planets, not by the ship, so shifting them
    with the start is wrong for the legs that can be pinned.
  - Goals within a system (fastest under a Δv, arrive by): its options
    are a handful of durations, and designs of several at once landed
    hundreds of m/s apart (part 5), so a goal over them would have
    picked noise; a sweep, then a click, does the job. (Since then each
    duration is designed alone, so this could be tried again.)

- Step 7, part 6: editing. As planned (step 7 plan, item 6); where it
  differs or adds:
  - A click on a ship's row in the panel opens its editor in place of
    the options: its name, its start (a menu of every place) and
    epoch, and per step a leg's destination (the same menu),
    durations (by commas), window (two dates; one alone makes it an
    instant) and aerocapture, or a wait's days; each step moved up or
    down or removed; "+ Leg" (to the NRHO in 5 days) and "+ Wait" (a
    day) in its header, which stays in view; "Remove ship". "+ Ship"
    in the panel's header adds one leaving LEO at the clock's day.
  - Typed fields keep what is typed until Enter, or a press elsewhere,
    commits it; Escape drops it. A value that does not read (a date
    that is not one, a duration not positive, a window that closes
    before it opens) is refused, saying why. While a field is typed
    into, the hotkeys without Ctrl are off: damascene matches hotkeys
    before a field sees its key, so typing "1.5" warped time.
  - Each change designs the ship again from the first step it changes
    (`editor::first_change`); a new name alone renames. Ctrl+Z undoes
    the last change (ships added or removed: the whole plan designed
    again).
  - Legs that cannot be flown say why at once, by the planner's own
    rules, now one function (`mission::leg_kind`; `mission::check` a
    ship): a surface left other than by ascent, reached other than by
    landing or entry, a leg within a system without durations or with
    a pin, a station left or reached from another planet.
  - Ships have ids of their own in the designer's messages, so adding
    and removing ships does not send one ship's items to another.
  - Plan files' waits and durations must be finite (TOML has `nan`
    and `inf`).
  - Checks: the editor opened from a ship's row; "1.5" typed over a
    leg's durations (no time warp while typing), committed by Enter,
    and the leg designed again at 1.5 days; a wait and legs added; a
    leg from the surface to the NRHO refused at once in the planner's
    words; two undos; a ship added (edited at once) and removed. The
    fields' reading, the steps' moves and `first_change` are
    unit-tested.

- Step 7, part 5: the options panel. As planned (step 7 plan, item
  5); where it differs or adds:
  - The panel shows the options of the leg of the item selected (a
    stay's, the leg after it), under the plan's list, and keeps that
    leg (by ship and step) while it is designed again.
  - Between planets: the porkchop is computed when the leg is selected
    (Δv by patched conics, a column a day up to 360 and the date
    search's durations up to 180 rows: ~65k cells, a few ms), not kept
    in the item; the item keeps its window as searched, without a pin
    (`Item::window`). Shaded as the lanes are, in ten bands (no drawn
    contours); departure dates under it at their places, the
    durations' range at its corners; the dates tried as rings, the
    flown departure filled in its leg's color; the cell under the
    pointer ringed, with its date, duration and Δv in a line below. A
    click pins the leg to the cell, its departure to the minute and its
    duration to a hundredth of a day; Unpin clears `depart` and `days`.
  - Within a system: the offered durations' designs as points, the
    flown one filled; a click pins that duration. "Sweep durations"
    designs twelve durations from half the shortest offered to twice
    the longest, evenly in ratio, in the background, each alone (as a
    pin would design it), from the checkpoint before the leg, the
    plan left as it is; they show as smaller rings, clickable alike.
  - Found: the autopilot's design of a duration depends on which
    others are designed with it. From the NRHO to low lunar orbit,
    designed one at a time, 1.65–4 days cost 675–692 m/s; designed
    together in one call, 983–1,259 m/s (and 0.5 days 682 against 962
    alone). So a leg's offered durations are not its frontier, and
    pinning one can fly it hundreds of m/s from where it was shown.
    Sweeps design each duration alone for that reason. Decided
    2026-10-15: the planner designs each offered duration alone too
    (`Planner::within`, a call of `legs::plan` per duration, in
    parallel), so an option is what pinning it flies (checked from the
    NRHO to low lunar orbit at 0.5, 2 and 4 days: 683.8, 681.1, 702.6
    m/s, the same pinned; offered together, 2 days had not been).
    `legs::plan` keeps its shared seeding for M1a's gate (stations'
    legs, the cheapest of 6–14 days), its doc saying so.
  - Checks: the Mars leg's porkchop drawn; a cell clicked three
    quarters across pins it and the leg flies at its date (to the
    minute) and duration; unpinned, it flies as before; a duration
    clicked within Earth's system pins it and flies it; a sweep leaves
    the plan as it was, converges 12 of 12, and its longest clicked
    flies at the swept Δv (to 1e-9 km/s).

- Step 7, part 4: window lanes. As planned (step 7 plan, item 4);
  where it differs or adds:
  - Measured first: `interplanetary::patched` takes 0.5 µs; from LEO
    to low Mars orbit, every day's departures over 2025–2080 at the
    date search's 350 durations (78–776 days, every 2) take 0.2 s on
    24 cores. So each lane is computed once over the ephemeris's
    whole span, not the timeline's, on the design pool.
  - Legs between planets are priced through one `World::crossing`,
    which the planner's date search now uses too; outside what the
    ephemeris knows of either end (a small body before 2040, arrivals
    past 2080) it prices none.
  - The lanes sit between the axis and the ships' lanes, gray: a
    pixel's days' least Δv brightest at the lane's least, dark from
    1.5 times it (a sequential ramp of one hue, the ships' legs keeping
    the colors). Each ship's departure is a tick in its leg's color.
    A lane's label's tooltip gives its range. The gutter is now 170 px.
  - Checks: the published plan's three lanes (LEO to low Mars orbit,
    by aerocapture, and to 2000 SG344) and each of its legs between
    planets in its lane; the Mars lane's cheapest day within 45 days
    of the handbook's dates the plan pins (−34, +11 and 0 days: the
    handbook's are the least C3, the lane counts the capture too), and
    there within 10% of it.
  - Cost: the timeline's elements 0.24 ms a frame to build with the
    published plan's three lanes (0.09 without), the UI's build and
    layout 1.05 ms median.
  - Found on the way: a panic in a task spawned on rayon's pool aborts
    the process; lanes catch theirs (a lane that panics stays dark).

- Step 7, part 3: plans written out. As planned (step 7 plan, item 3):
  - `Plan::to_toml` writes ships' starts as tables (`[ship.start]`),
    times as `YYYY-MM-DD` or `YYYY-MM-DDTHH:MM` (`Epoch::stamp`, to
    the minute), places as they were written. A file's opening
    comments are kept; its other comments are lost when the sandbox
    saves over it (notes on legs are kept).
  - `depart` pins legs between planets and events; within a system it
    is refused (set a window), as is a pin before the ship is ready. A
    pinned leg tries the cheapest durations at its date. (Replaced by
    `pin` after the review of parts 2–5.)
  - The top bar's Plan menu lists `data/missions/` and "New plan"
    (empty, saved as `new-plan.toml`, or `new-plan-2.toml`, … if
    taken); Ctrl+S saves to the plan's file, with a toast.
  - Checks: every plan in `data/missions/` written out reads back as
    itself and writes out the same; the published Mars 2031 leg pinned
    by `depart` flies as when pinned by a one-day window; the menu
    opens a plan; a plan saved reads back.

- Step 7, part 2: re-planning from the middle. As planned (step 7
  plan, item 2):
  - Each item names its step (`Item::step`). A planner's checkpoint
    (`Planner::checkpoint`) is taken before its first step and after
    each leg; `Planner::resume` goes on from one with the ship's steps
    edited; `mission::resume_from` picks it (the last before the edit,
    not one in a station's orbit; else the start).
  - In the sandbox, `Loaded::edit` replaces a ship and designs it again
    from there, keeping the items before. Each ship's design has a
    generation; a designer of an older one stops after the step it is
    on, and what it sent is dropped. The fields legs are designed in
    are built once a process, not once a plan.
  - Checks: the sortie (NRHO, low lunar orbit, the south pole, a wait,
    back up) with its wait on the surface lengthened, resumed after
    the landing, gives what a fresh design of the edited plan gives
    (times to 1e-9 days, Δv to 1e-9 km/s); in the sandbox, the same
    edited after designing and while still designing, the other ship
    untouched.

- Step 7, part 1: the timeline. As planned (step 7 plan, item 1);
  where it differs or adds:
  - A press on the axis seeks the clock, held to the span shown; on the
    lanes, a press that moves 3 px or more (either way) pans across,
    and one that does not seeks the clock to where it was released and
    selects the bar it pressed (none on empty space). A double click
    on empty lanes fits the plan again. The wheel over the axis or the
    lanes zooms by 1.25× a line; over the gutter of names, the lanes
    scroll.
  - The span pages when the clock crosses out of it (playing, or
    jumping), not when the span is zoomed or panned off the clock.
  - Eight lanes show before they scroll (the plan's "a third of the
    window" not built).
  - Ticks on a ladder from an hour to ten years, each labeled with its
    date (years `2045`, months `2045-03`, days `2045-03-05`, hours
    `03-05 12:00`); days restart at each month's first.
  - The plan is fitted to at least 10 days, with 2% spare either side.
  - A bar's tooltip is its row in the panel. Failed legs are red.
  - Damascene routes the wheel to keyed elements only when the host
    asks (`pointer_wheel_event`, then `App::on_wheel_event`); the host
    now does, before scrolling.
  - Checks: bars within 0.0000 px of their items' times, in their
    lanes, at 1 and 2 physical pixels to a logical one; a click on a
    bar selects it and seeks to the click; a press on the axis seeks,
    and dragged past its end holds the clock at the span's end; the
    wheel keeps the moment under the pointer, frames after; a press
    moved off a bar selects nothing; a double click fits the plan; the
    plan is fitted with the clock set elsewhere. The ladder's steps and
    labels, the fit, paging and panning are unit-tested.
  - A review (a sub-agent, with runtime probes) found: paging undoing
    zooms and pans whenever the clock was off the span, even paused;
    the axis dragged past its end racing the clock a span a frame; a
    press left over making a later click on the axis deselect; only
    horizontal motion breaking a click; the plan never fitted with the
    clock set before it; month and day labels a few pixels short of
    room. All fixed; the first, the second and the fifth fail their
    tests when undone.
  - Cost, published plan (18 ships, 38 items) at 1920×1080: the
    timeline's elements 0.09 ms a frame to build; the whole UI's build
    and layout 0.93 ms median, 1.35 ms at the 95th percentile.

- Step 5: labels on the map view, picking by click, and the plan's
  panel. As built, see the step 5 plan below; where it differs:
  - Of labels whose anchors lie within 6 px of each other, only the
    first placed is kept: they read as naming one spot (from 2 au,
    Phobos's label beside Mars's read as a second point). A body drawn
    under 6 px names the spot it is drawn at, to 6 px past its radius:
    a ship or marker there is not labeled.
  - Only bodies drawn as disks (1 px in radius or more) hide what is
    behind them, as in the renderer, where a body under a pixel is a
    point of light. A label avoids every anchor's point where it can,
    and else covers only points ranked below it.
  - A click tries lines before bodies (amending item 3), so that a line
    in front of a body's disk can be picked; a line counts only where
    no body hides it.
  - Small bodies are labeled by their proper names, else their
    provisional designations ("Itokawa", "1996 FG3"); station loops by
    their places' ids ("NRHO").
  - The panel is a damascene list that builds only the rows shown. A
    leg's row lists its burns where it has more than one, or a flyby or
    a pass; Δv under 0.5 m/s (a stay's station-keeping) is shown as
    none.
  - Checks: an anchor is its body's f64 projection (0.0000 px), and its
    label stands its radius plus 3 px off it, drawn where placed; the
    Moon behind Earth, behind the eye or off the view has no anchor;
    clicks pick the Moon at its center, the ship beside its marker, a
    marker's item at its label, the leg on its line, nothing on empty
    space. Through damascene's pointer events, a click on a label
    reaches the map view (labels take no input) and one on a row
    reaches the row; at 1 and 2 physical pixels to a logical one, the
    Moon's anchor is within its phase's lean of where it is drawn (0.03
    and 0.28 px). Line picking follows the drawn Hermite curve (0.000 px
    on it, 79 px off its chord), halving each segment until a bound on
    its curve's image says it is straight to a quarter pixel or too far:
    over random segments, within 0.15 px of the curve sampled densely.
    Removing each of 10 checked features in turn fails its test.
  - Cost, at 1920×1080 with the published plan (18 ships), measured
    offscreen: placing labels 0.03–0.06 ms a frame (11–60 anchors); the
    frame's CPU work 0.6–0.8 ms, 1.05–1.26 ms with the panel shown; a
    pick ~3 ms (every line's segments bounded, those near the click
    halved). In a window, frames came at the display's refresh with and
    without step 5 (4.38 and 4.40 ms with the published plan designing;
    4.2–4.3 ms without a plan).
  - A review (a sub-agent, with runtime probes) found: Earth's label
    giving way to a ship's every half orbit seen from afar (the ship
    hidden by a body drawn as a point, then at its spot); markers ranked
    below the focus keeping its label out; a ship resting at a body
    hidden by it, so unlabeled and not pickable; lines over a disk not
    pickable; line picks missing curves that bow toward the eye or lie
    off the view's axis; a ship refused mid-design showing no reason;
    labels a frame behind the map view's size when it changes. All
    fixed, each with a test (the last: labels stretch with the stale
    image in that frame). HiDPI was checked at scales 1, 1.5 and 2.
  - Found in damascene 0.8: `rect_of_key` does not find keyed nodes in
    a virtual list's rows (its key index is rebuilt before layout
    realizes them). Hit-testing and scrolling to a row key work.

- Step 6: the HDR chain, the sky, the bodies' orbits, rotation models and
  maps, atmospheres, the Sun's limb, eclipses and Saturn's rings. As
  built, see the step 6 plan below; where it differs:
  - Points of light (stars, bodies under a few pixels) are as bright
    to the viewer's eye as their sources would be, at the scene's one
    exposure, lifted with the sky over it (Realism; decided 2026-10-15). Until then they and the
    Milky Way showed at a fixed sky exposure where it exceeded the
    frame's (7.5e5), capped at a saturated star's brightness, and drawn
    as a camera records them. A body growing into a disk fades from its
    point over 1–3 px in radius (the point's share falling, the disk's
    opacity rising). Orbit lines leave a gap where their body is, which
    they otherwise hid.
  - The scene's pass writes radiance already exposed, so that f16 holds
    the range shown (unexposed, the Milky Way's median, 4e-10, lies
    under f16's smallest step, 6e-8); the glare's levels hold means,
    not sums, for the same reason. What lies past f16 once exposed (the
    Sun's disk, at exposures over ~1.3: every focus from Earth outward)
    loses its excess before the glare, so its glare stops growing there
    (1.5× under its energy at Earth's exposure, 120× at Saturn's). Kept
    (decided 2026-10-15): the glare is bright enough as it is; an f32
    target was the alternative. Since part 10 the Sun is dimmed 6 stops
    by default, which leaves it clipped from about Saturn's exposure out
    (~2× there).
  - The Milky Way: SVS's composite is 0.50 × its background + 1.00 ×
    its bright-star layer (fitted, residual 0.0005), and that layer's
    stars integrate to 2.14e-4 × 10^(−0.4 V) (124 unsaturated stars, V
    4.5–6.5): one stored unit is 1.48e-7 radiance units. Over the sky's
    area its median is 24.6 mag/arcsec², its brightest percent brighter
    than 21.3, and an eighth of it under the 8-bit map's first step (26.9;
    corrected 2026-10-15 from 22.8). Tycho-2's stars fainter than
    Hipparcos are left out (a few percent of the background's light).
  - Orbits are sampled at most 10 days apart (the Sun's reflex motion:
    Neptune's line missed it by 290 km at a 384th of its period), 6
    hours for small bodies (1996 FG3's by 460 m at a day).
  - Earth turns by the IERS rotation angle (UT1 = TT − 69 s), not the
    IAU's linear W (0.05° off at J2000). The Moon's pole in `ephem`
    comes from the IAU model, not Cassini's laws; Earth's stays the
    fixed J2000 axis its J2 acts about.
  - Earth's map is Blue Marble taken as reflectance (its ice 0.8–0.96,
    oceans near black: their blue is the air's); thick clouds 0.8;
    elevation clipped at 6,400 m (GEBCO's scaling). Saturn, Venus,
    Uranus and Neptune have no maps (no Cassini global map of Saturn was
    found; the others are near featureless in visible light).
  - Mars's dust at optical depth 0.35 (the clear season); Mars's and
    Venus's air are estimates, stated in `sandbox::atmospheres`. (0.5
    had been dropped for hiding the surface toward the terminator, a
    fault of the shader's: part 8.)
  - Not done: shape models (Phobos, Deimos, Bennu, Ryugu: a mesh
    pipeline, and sources each needing care).
  - Checks: Sirius, Betelgeuse and Polaris within 0.003 px of their
    SIMBAD places at 10″ pixels, Barnard's star 0.2 px (62 px from its
    J2000 place); orbit lines through their bodies within 0.02 px seen
    from 10 km over them; sub-observer points against Horizons within
    0.0005° (the Moon and Earth 0.003°, Jupiter and Saturn 0.002°); a
    map's mark within 0.06 px of its place; relief within 0.3%; the
    atmosphere's zenith transmittance within 1e-5 of analytic; an
    umbra within 0.17 px of the Sun–Moon axis, the penumbra within
    0.0002 of the covered share; rings within 0.04% of the slab's
    formulas. The 2045-08-12 eclipse falls off Florida at 17:43 TDB,
    the axis 1,346 km from Earth's center (NASA: 17:42:39, γ 0.2116).
  - Cost, GPU at 1920×1080: the post chain 0.19 ms; the sky 0.08 ms;
    Earth filling the view with its maps 0.05 ms more, with its air
    0.55 ms more (0.80 ms in all).
  - Data: `data/sky` 15 MB, `data/bodies` 65 MB (the repository's
    `.git` 93 MB). Every map seen stays on the GPU while the app holds
    it (all of them: ~860 MB), and decoded in memory too (~785 MB with
    the sky's).
  - Saturn's rings' optical depth is one occultation's, noisy where it is
    low (σ ≈ 0.085 a 10 km bin, and spans off by up to −0.1): kept as
    measured, negatives included, so that gaps average empty; the C
    ring's faint parts are uncertain either way.
  - Found in review and fixed: the spot over a growing disk was hidden
    by the disk; bloom saturated f16 near the Sun; bright points were
    clipped before exposure; points left out clouds (Earth's 4× too
    faint) and lunar-Lambert shading (the Moon's up to 1.5×); maps were
    uploaded again each time a body came back into view (70–100 ms for
    Earth's); one unreadable map left every body plain; the rings'
    profile wrapped at its edges, and clipping its noise at 0 made every
    gap a faint ring.
  - Open: translucent air and rings write depth, so lines behind them
    are hidden (Saturn's orbit line at the rings' edge), and from inside
    an atmosphere the sky would hide lines and points; a ringed body as
    a point carries no ring light; a body as a point before the Sun
    adds light rather than hiding it.
  - Part 8: Mars was washed out toward its terminator. Against a Monte
    Carlo of the same dust (`tools/atmosphere/`), the air's multiple
    scattering is now spread by direction, the ground lit by its sky and
    seen by reciprocity; see the part 8 plan below.

- Step 4: plans designed in the background and drawn as lines in fixed
  or rotating frames, with markers for ships and events, and playback.
  As built, see the step 4 plan below (amended in three places, each
  saying why).
  - Gate item 2's check for lines: a low lunar orbit in a frame scaled
    to 1 au and centered far from it (as the Sun–Earth frame is), seen
    from 100 km to 5 au, draws within a pixel of its f64 projection (lit
    pixels within 0.70–1.06 px of the curve, for a line 1.5 px wide; the
    curve within 0.71 px of a lit pixel). With the high parts alone it
    is ~9 km off.
  - Between samples the line follows the Hermite curve: from 30 km over
    LEO within 0.81 px where the chords bow 22 px; from 300 km, cut on
    the GPU into 8–16 pieces, within 0.81 px; a 650,000 km cruise
    segment from 10 km and a 13,000 km NRHO segment bowing ~80 km from
    18 km, cut and placed by the CPU, within 1.0 and 1.2 px (on the GPU
    alone, 14.5 px off). A segment passing behind the eye draws its
    front part only. Past and future meet within a pixel of where the
    curve is at the moment: on curving segments, on one receding from
    100 to 10,000 km, and 100 days into a low orbit seen from 30 km.
  - Frames: in all five, samples' velocities match their positions'
    change (5e-8), and lines stay within 0.2 m of the tracks between
    samples (a ship's marker is drawn on its line, within 1 m of
    `Track::at`). The station loops (L1, L2, NRHO, DRO) close; the
    NRHO's perilune is 2,733 km from the Moon's center.
  - Each of these tests fails with the feature it checks removed
    (checked by breaking each in turn).
  - Cost, GPU timestamps: the published plan (104,670 samples) 0.37–0.84
    ms at 1920×1080. Two million segments (20 low orbits of 100,000
    samples): seen whole, 2.4–2.5 ms GPU and 0.8 ms CPU a frame; from 10
    km inside their tangle, 3.7–4.5 ms GPU and 3.0 ms CPU. Building a
    plan's paths in another frame: 4–13 ms for the published plan, and at
    most 82 ms while every core is designing it. In a
    window, the plan added nothing measurable (0.85 ms of GPU a frame
    against 0.89 without); frames came every 18.5 ms both ways, because
    the compositor put the window on another output (headroom 1.23×).
  - Shapes, in the crew rotation: in the Earth–Moon frame the outbound
    and return legs swing either side of the Earth–Moon line and the
    NRHO stays retrace their loop; in the unscaled frame, the low lunar
    orbits a week apart draw ~16,000 km apart, as the Moon's distance
    changes (the scaled frame keeps them at the Moon).
  - A review (a sub-agent, with runtime probes) found: a crash focusing
    a small body with any time before 2040 in view or in the plan; the
    window stalling up to 2.7 s while a plan designed (the UI's rebuilds
    queued behind the designer on rayon's pool); past and future split
    across the screen rather than along the curve (100+ px on receding
    segments), and drifting with f32 times (200 px after 100 days); f32
    not precise enough near long segments (20 px from 10 km); the bow
    estimate missing S-shaped segments; ascent markers off their jump
    times; a ship whose design panicked shown as designing forever (and
    the planner panicking on a small body before 2040). All fixed, each
    with a test; the line fixes are the plan's three amendments.
  - Left as they are: a landing straight from a plan's start, or an
    ascent as its last step, has no track to place a marker on (they
    are still jump targets), and a ship's marker is not drawn during a
    landing or an ascent, which have no path.

- Step 3: the renderer draws the ephemeris's bodies as ray-cast spheres
  in 64-bit positions, with a camera that follows a body. As built, see
  the step 3 plan below.
  - Gate item 2's check passes: a ship's marker in low lunar orbit lands
    within 0.04–0.22 px of its 64-bit projection, the same to four
    decimals at every distance from 100 km to 5 au (the dot's own
    rasterization, no precision lost with scale).
  - The Moon's limb from 100 and 10 km up sits within 0.18 px of the
    horizon computed in 64 bits.
  - Radiance: a lit center reads 0.49976 for 0.5, the Sun 46,208 for
    46,239 (16-bit float rounding). Flux: a resolved sphere's pixels sum
    to the same image integrated in f64 within 0.05%; a point carries
    the distant sphere's flux within 0.2%, at the screen's center and
    near its corner. (The distant-sphere formula itself runs 2–7% low
    for spheres 40–200 px across: up close, less than a hemisphere is
    visible.)
  - At true brightness and exposure 1.0, the planets seen from afar are
    far too faint to see, and only the Sun shows. Finding them is for
    labels (step 5); exposure is step 6.
  - Screenshots into 8-bit sRGB show steps one code apart across smooth
    gradients (the Moon's surface from low orbit). The float swapchain
    has none; dithering for SDR goes with step 6's tonemap.
  - In a window, frames at the display's refresh (4.3–4.4 ms).
  - A review (a sub-agent) checked the ray casting against an f64 one
    over 396 setups (0.001 px). It found points near the screen's
    corners up to 46% too faint (a pixel there subtends less sky);
    a panic focusing an asteroid before 2040; a jump when refocusing
    mid-flight; small bodies' first view inside the 10 km floor; and
    the screenshot flags bypassing the camera's limits. All fixed, the
    first three with tests. Also from it: spheres wholly off the screen
    are culled, and a frame's step is capped at 0.25 s.

- Step 2: `crates/render` (the scene target, the empty scene, GPU
  timers, a windowless device) and `crates/sandbox` (the window, the
  shell UI on damascene 0.8, and `sandbox --screenshot`/`--frames`).
  As built, see the step 2 plan below.
  - The host is ours, on damascene's exported host layers: they
    negotiated HDR with the compositor on the development machine (an
    `Rgba16Float` scRGB swapchain, 203-nit reference white, 400-nit
    peak: headroom 1.97×). The scene's linear values pass through to
    it; tonemapping to the headroom is step 6.
  - Three runs of 300 frames of the empty scene with the UI. Two got
    frames every 4.4–4.6 ms on average (the display's refresh, under
    Fifo), with 0.4–0.9 ms of GPU time each. The third, its window
    placed elsewhere by the compositor, got frames every 50 ms, 3.2 ms
    of GPU time, and a headroom of 1.23×. An idle GPU's clocks swing its
    times by several fold, so gate item 3 is measured under load.
  - Tests: the scene target reads back exactly, HDR values included
    (1.5 and 7.0); offscreen, the scene's color fills the map view's
    rect and the bars around it show the UI; the target follows the map
    view's size; timestamps time a frame.
  - Left out of our host, for now: AccessKit, the clipboard and primary
    selection, touch and pinch, the soft keyboard, file drops, and the
    app's announcement, link and plot requests, which the stock host
    wires and the sandbox does not need yet.
  - A review (a sub-agent) found the timer reporting an older frame as
    the latest when several finished at once, and a camera drag left
    running when the button came up outside the window; both fixed, the
    first with a test.

- Step 1: mission plans (`data/missions/`, `orbit::mission`), the places
  they name (`data/places.toml`, `orbit::places`, which `orbit gate` and
  `orbit routes` now read too: the regenerated `routes.toml` is
  byte-identical), paths sampled for drawing (`orbit::track`), and
  `orbit mission FILE`. As built, see the step 1 plan below.
  - Gate item 1, its first half (drawing waits for the view): the legs
    M1a checked against published values fly as
    `data/missions/published.toml`, 18 ships, all in 44 s on 24 cores.
    Their Δv matches M1a's designs to a few m/s (the Mars handbook
    departures 3,576 / 3,550 / 3,523 / 3,583 m/s, 2000 SG344 4,038,
    Phobos 1,205 each way, NRHO to low lunar orbit 684).
  - A sensitivity: a station's reference is rebuilt for each visit, from
    its start, and a leg's design depends on it. With a longer reference
    for the NRHO visit (an earlier build), the same NRHO leg found
    another local minimum: 795 m/s, with a 286 m/s departure burn
    against 31.
  - The crew rotation (LEO, 24 days in the NRHO, a week at the south
    pole, home by aerocapture): 9,343 m/s over 51.5 days, designed in
    10 s. In the earlier build its leg from low lunar orbit back to the
    NRHO found no design in its first window, which is why windows are
    retried. Probed, every start within a band of a few hours failed
    (all 8 seeds), and quarter-day starts around it all converged: the
    step 10 sweep's 2% for this leg (216 of 220 dates).
  - Shapes, plotted from `--tracks`: the outbound and return legs
    swing either side of the Earth–Moon line, the NRHO's stays retrace
    their loop in the rotating frame, with perilune ~3,000 km over the
    north pole and apolune ~70,000 km south. Consecutive drawn items
    meet exactly, except a stay in low lunar orbit coasted back and
    forth over 8 days (7 m).
  - Samples: ~31 s apart in LEO, 6 hours in cruise; between them,
    Hermite interpolation stays within 0.1 m of the integration in LEO
    (the test's bound; theory says 3 cm).
  - Known gap, as planned: arriving in a low orbit and leaving it again,
    the next leg starts from its own orbit. LEO to low lunar orbit and
    back: the return leaves 3,494 km from the ship, in a plane 6.9° off
    (197 m/s to change), reported and not flown. It can be much worse:
    the same trip from 2033-03-30 leaves low lunar orbit in a plane 166°
    off (3.2 km/s), and a departure for Mars on arrival in LEO is 85°
    off. Decided 2026-10-14: it stays reported through M1c and is fixed
    before M1b (DESIGN.md, Milestones); 2026-10-15: fixed in M1c, as
    step 7's last part. Done (step 7, part 8): the first two leave from
    the ship's own orbit, at once and after 13.3 days; the third still
    jumps.
  - Long stays in a low orbit are sampled ~2,900 times a day, so a wait
    through a synodic period in LEO would be ~2 million samples. Step 4's
    lines will need a lighter form for long coasts. (Step 7, part 8: a
    stay is drawn and picked a revolution about the clock, its first or
    last outside it (`render::Path::window`); still sampled whole.)
  - Stays in a station orbit cost the upkeep simulated on the visit's
    reference, from 4 runs: 0.5–1.9 m/s per year for the NRHO across
    visits (step 5d's 1.09 came from 20), and none on references under 4
    revolutions (an L2 halo visit of a revolution and a half).
  - A review (a sub-agent) found the jump at a junction lost when the
    leg left the moment the ship arrived; fixed, with a test.

## Why

The map view is the game's center interface: the real solar system in
3D, at true scale, as realistic as we can afford, on HDR displays. The
model is KSP's map view rather than its flight view. It replaces the 2D
heliocentric map with a cislunar inset that DESIGN.md first planned
(decided 2026-10-13; DESIGN.md, UI). The 2D plan was chosen before the
n-body sandbox existed, when the map only had to show patched conics. In
the sandbox the interesting things are 3D: halo orbits, the
near-polar NRHO, lunar flybys, the planes of departures. And the map is
where the player spends the game.

M1c builds that view first, as a creative mode with no economy: load a
mission plan, let the autopilot design and fly it, and watch it travel.
This:

- stress-tests the renderer on real trajectories before the
  progression mechanics are built on top of it;
- builds "a mission is a chain of legs" for real, which M1b needs anyway;
- looks at the shapes of trajectories. Every check in M1a compares a total
  (Δv, time) with a published number; nothing has yet looked at a path.

## Scope

In:

- mission plans: a file format, and the engine pieces that plan and fly
  them;
- the 3D map view: playback with a time scrubber and time warp, reference
  frames, bodies, trajectories, labels, picking;
- panels: the plan's legs, their Δv and durations, failures and why;
- planning (step 7, grown from editing a leg's window or duration,
  decided 2026-10-15): a timeline of the plan's ships, the windows
  between planets, each leg's options along its frontier, editing and
  re-planning in place, saving and loading plans.

Out:

- flying low over terrain (KSP's flight view);
- ship models (later, as flavor; ships are markers);
- mass and propellant (M1b);
- the economy;
- hand-placed burns. A burn editor is open (DESIGN.md, Open questions);
  creative mode is where it would go.

## Mission plans

A plan is a ship's start and a chain of legs:

```toml
name = "Gateway crew rotation"
start = { orbit = "leo", epoch = "2045-03-01" }

[[leg]]
to = { orbit = "nrho" }         # a member of a family in data/orbits.toml
window = ["2045-03-01", "2045-03-31"]
durations = [5.0]               # days, or `longest` for a frontier

[[leg]]
wait = 30                       # days in the NRHO, kept by station-keeping

[[leg]]
to = { surface = "moon/south-pole" }
```

The format above was the first sketch; step 1 settled it (Step 1 plan,
item 2: a plan holds ships). Plans live in `data/missions/`.

Missing from the engine today:

- **One entry point for every leg kind.** `legs::plan` designs legs
  within a planetary system at given durations, and
  `interplanetary::plan` legs between systems at given dates; nothing
  picks the dates, or chooses between the two.
- **Chaining.** Each leg starts where and when the previous one ended. A
  wait coasts in the station orbit with station-keeping
  (`stationkeeping`).
- **Paths for drawing.** A designed path keeps only its nodes' states.
  The view needs samples spaced by the local dynamics: about a minute
  apart in LEO, a few a day in cruise. They come from re-integrating with
  dense output, and are stored with velocities so that playback can
  interpolate between them.
- **Events that are not flown.** Landing, ascent and aerocapture are Δv
  rules (`surface`), so they draw as markers with a jump, not as paths.

Designing a leg takes 0.1–12 s, so plans are designed in the background,
and each leg appears when it is ready.

## The view

### Scale

Distances run from a 100 km lunar orbit to the outer planets, ~1e9 km.
Two standard techniques cover that in one pass, with no second camera
for distant objects (KSP's "scaled space"):

- **Camera-relative positions.** Positions stay 64-bit on the CPU, as the
  ephemeris and the paths already are. Each frame subtracts the camera's
  position before handing 32-bit offsets to the GPU. Near the camera this
  is exact. Far away, the rounding is ~6e-8 of the distance, ~1e-4 of a
  pixel.
- **Reversed depth with no far plane**, in a 32-bit float buffer. Depth
  precision is then roughly constant relative to distance (~1e-7), from a
  metre to the outer planets.

Trajectories are stored in chunks, each with its own 64-bit anchor and
32-bit offsets, so precision depends on a chunk's size, not on its
distance from the Sun.

### Frames

What a trajectory looks like depends on the frame, and the sandbox has no
single right one. A halo orbit is a closed loop only in the frame turning
with the Earth and Moon. In a fixed frame it spirals along the Moon's
path.

- **Fixed, centered on any body.** The default follows the camera's
  focus, as in KSP: focusing on the Moon draws paths relative to the
  Moon.
- **Turning with two bodies:** Earth–Moon, or Sun–planet. The origin is
  their barycenter, x points from the first body to the second, and z lies
  along their orbit's angular momentum. Optionally the frame is scaled by
  their distance, as in the CR3BP, where catalog orbits close.

Each sample of a path is drawn in the frame at that sample's own time.
"Moon-centered" means the Moon's position when the ship was there, not
now. So a path's vertices depend on the frame, and are recomputed on the
CPU when the frame changes.

### Realism

As realistic as we can afford, in stages, while the map stays readable.
Each departure from realism is a setting, in stops, whose 0 is true to
life (decided 2026-10-15): the background lift, the body lift, the
night lights and the Sun's dimming, below.

**Light.**
- The Sun is the only light. Radiometry is physical, so Mars is lit at
  43% of Earth's sunlight and Jupiter at 4%.
- Exposure adapts to what is in focus.
- The scene is rendered in linear HDR. Exposure, the viewer's eye's
  glare (step 6, part 10) and a tonemap fit it to the output's headroom: up to the panel's peak on an HDR output (a
  16-bit float scRGB swapchain), and a filmic curve to 1.0 on SDR. The
  curve takes a color's largest channel, the others scaled with it, so
  that a color keeps its hue however bright (decided 2026-10-15: on each
  channel alone it showed every bright planet's point white, Mars's
  too).
- One exposure for the whole scene (decided 2026-10-15: as accurate as
  we can first; departures where play shows the need). Exposed for a
  sunlit body, as a camera would be, the true sky shows nothing; over
  Earth's exposure, +19 stops shows the Milky Way's band faintly, +22
  clearly. Rejected: a fixed exposure for the stars and the Milky Way
  where it exceeded the frame's, which showed them beside a sunlit body
  but which the exposure keys did not move.
- The sky (the stars and the Milky Way) is lifted over that exposure by
  a setting, the background lift: 15 stops by default, 0 (true) to 25
  (decided 2026-10-15: the true sky left the map black beside any
  sunlit body; 17 until the eye's glare, the user's call). The exposure
  keys move it with the rest of the scene. Over Earth's exposure, +15
  shows stars to about magnitude 9.5 and not yet the Milky Way. (Earth's exposure has moved with its measured
  brightness: 1.1 stops down when it took Mallama's curve, 1.0 up when
  Robinson's; the defaults moved with it, 18 between, the look kept.)
- Bodies drawn as points of light are lifted over the scene's exposure
  by their own setting, the body lift: 15 stops by default, as the
  sky's, so that the planets show where they are, at their brightness
  among the stars (decided 2026-10-15). As a body's disk fades in, from
  1 to 3 px in radius, its point's lift falls in stops with its share.
  Rejected: points at the scene's exposure, which left Jupiter's from
  Earth at 0.0008 of the SDR white, invisible beside thousands of
  lifted stars; and points lifted with the sky, by one setting.
- Points of light are as bright to the viewer's eye as their sources
  would be: a star's flux is spread over the sky a pixel subtends at the
  viewer, not the sky it shows of the scene. A logical pixel is taken as
  CSS's reference pixel (0.0213°), so a window 1,080 logical pixels tall
  fills ~23° of the viewer's sight with the view's 50°: drawn as a
  camera records them, stars would show ~5× too faint against the Milky
  Way, whose radiance shows the same either way. A body is such a point
  while the eye could not resolve it (under 1′ in radius: every planet
  seen from Earth); past that its point's gain falls as its peak would,
  to 1 (none) where its image could be resolved.
- Earth's night lights are lifted over their radiance by a setting, 14
  stops by default, so that they show at the day side's exposure. Their
  radiance, ~1e-5 of daylight's at a bright city, is within a factor of
  a few: Black Marble's image is stretched, not calibrated.

**Bodies.**
- Spheroids with the right flattening: Earth 1/298, Mars 1/170, Jupiter
  1/15, Saturn 1/10.
- Each is oriented by IAU rotation models: the pole and prime meridian
  from Archinal et al. 2018. `ephem` has poles today but no prime
  meridians.
- Each carries a surface texture and relief shading from elevation data
  (LOLA for the Moon, MOLA for Mars, ETOPO for Earth).
- Bodies smaller than a pixel are drawn as points with their true
  brightness, from distance, phase and albedo.
- Each body's brightness is held to its published phase curve (decided
  2026-10-15): Mallama and Hilton 2018 for the planets, as JPL Horizons
  computes them, but Earth's; Allen's for the Moon, as Horizons'; the H,
  G system for Phobos, Deimos and the small bodies. Earth's is Robinson
  2025's fit to Earthshine and spacecraft photometry (decided
  2026-10-15): Mallama's full phase, EPOXI's 57.7° extrapolated by a
  model that swapped forward and backward scattering, is 0.77 mag too
  bright (a geometric albedo in V of 0.434 against 0.215, which with its
  own phase integral made Earth reflect 0.57 of its light, against
  0.29); past ~116° Mallama's curve is the fainter, by 1.2 mag at 144°.
  A sphere's shading spreads its light over its disk, and a gain brings the total to the curve at the
  camera's phase angle (held at the curve's range's edge beyond it);
  Saturn's point carries its rings' light. A map's mean is weighed as
  its disk shows it from its equator (the Moon's, its near side's). The
  exposure shows the focus's sunlit side at full phase, its mean, at
  `KEY`.
  - A body's point follows its curve as far as Horizons takes it
    (Mercury's, Venus's and the Moon's to 180°), Earth's to 144°, where
    its fit's data end; its disk's gain is held past 170°, where a
    crescent's shading carries too little light to scale.
  - Checked against Horizons over 2025–2080, from Earth and from bodies
    farther out (to 176° of phase): the curves but Earth's within 0.002
    mag (Mars 0.12, the terms for its longitude seen and its season left
    out); the planets' points from Earth within 0.005, to 177°.
  - Disks, against their curves, seen from 100 radii: bodies without air
    or maps within 0.03 to 120° of phase, Jupiter 0.02 (nearer, a disk
    shows less than a hemisphere: from 12 radii, 0.2 under at 120°).
    Earth with its air −0.25 to +0.02 to 144° (from 100 and 500 radii,
    two dates: its air is drawn over its calibrated surface, below). The Moon from Earth −0.25 to +0.34 to
    150°, its waxing and waning differing by up to 0.33 near 100°, where
    its curve is one for both.
  - Open: the Moon's thinner crescents show limb highlands brighter than
    its near side's mean, which its curve counts already (0.5 mag over at
    166°). Bodies with air add their air's forward scattering, which
    their curves count already, at large phase angles (seen from 500
    radii, Mars 1.3 mag over at 120°, Venus 1.1 at 160°). Saturn's
    rings as drawn show ~2.5× under its curve's with them at small phase
    angles, so resolved Saturn shows 0.35–0.6 mag under its point.
  - Earth's model (its maps, clouds, air and glint) is close to physical
    (2026-10-15): with no gain, 0.02–0.36 mag under its curve to 120° of
    phase, and its spherical albedo (luminance) 0.245–0.247, ~11% under
    Robinson's in V (0.275: the visual 0.294 scaled as the geometric
    albedo is). Its gain is 1.2–1.7 to 90°: at full phase its Lambert
    clouds lack backscatter. Past 90° it climbs (2.7 at 120°, 9.5 at
    144°): its Lambert clouds lack the forward scattering that brightens
    a crescent. Its air is drawn over its surface as calibrated, so its
    disk shows up to 0.25 mag over its curve. Its cloud map's mean
    value, 0.24, is cover weighed by brightness: 60% of the area has
    some (over 0.1 of full). Its aerosols are the global mean's
    (extinction optical depth 0.14 at 550 nm, asymmetry 0.70; Hillaire's,
    a clear day's, 0.010), which brighten its disk by at most 0.06 mag
    to 144°. Before, held to Mallama's curve, its gain was 3.4 at full
    phase.
  - Before: Lambert shading of the Bond albedo, which left the giants
    ~1 mag faint (a geometric albedo of 2/3 of it; theirs is 1.4–2.5×
    that), and Mercury, Mars and the Moon too bright at large phase
    angles, to 1.4–1.6 mag.
- Each body's color is held to its measured color at full phase the same
  way, the gain per channel, its luminance kept (decided 2026-10-15):
  what it reflects of the Sun's light, the Sun white, from spectra
  through the eye's color matching functions
  (`tools/photometry/colors.py`). The planets from their geometric
  albedos in nine bands (Mallama et al. 2017; the Johnson-Cousins and
  Sloan bands alone differ by up to 0.09 in a channel); the Moon's from
  Lane and Irvine 1973, its red as their 1964 data and other studies
  have it; Phobos's and Deimos's as D-type asteroids', which theirs are
  likened to; small bodies' as their class's mean (DeMeo et al. 2009),
  the Sun's where their class is unknown. A map keeps its parts' colors
  over its mean. Earth's color is its air's (Rayleigh scattering's over
  white clouds): its disk keeps its maps' colors, under its air, and
  its point, which has no air, takes its measured color.
  - Points show their colors within 0.001; disks without air within
    0.014 (as the face shown varies). Earth's disk is 0.04 over its
    measured blue (in a channel, of luminance 1), its air drawn over its
    calibrated surface (above). Rejected: Earth's gain per channel as
    the others', which tinted its clouds pale blue. Earth's color is
    Robinson 2025's, with its curve (Mallama et al.'s came from the same
    extrapolation as its full phase), from its means over 100 nm bands
    (taken as their centers' values, the blue came 0.06 short).
  - Before: the maps' means and rough tints. Mars's map reflected 1.7×
    the blue its measured color does, and Uranus and Neptune were paler
    than their measured cyan.
  - A saturated color past the tonemap's knee keeps its hue and loses a
    little luminance: Mars's sunlit side at `KEY` shows ~8% darker on
    SDR than under a shoulder per channel (calculated). The Sun's disk,
    far past it, shows its limb's reddening rather than white.

**Earth and Mars.**
- Atmospheres by precomputed scattering (Bruneton 2017 or Hillaire
  2020), seen from space: the limb glow and the color at the terminator.
- On Earth, clouds and night lights.

**Shadows and small bodies.**
- Eclipses: analytic sphere shadows, such as Earth's on the Moon and
  Phobos's on Mars.
- Saturn's rings, with their shadow.
- Shape models for the moons and small bodies with published ones, made
  up for the rest (step 6, part 9 plan).

**The Sun and the sky.**
- The Sun is a limb-darkened disc with the eye's glare, dimmed by a
  setting (step 6, part 10).
- Stars come from a catalog (Hipparcos, or a bright subset of Gaia), with
  positions, magnitudes and colors from B−V, and a Milky Way map behind
  them.

**What is drawn on the map.**
- Paths: antialiased lines colored by leg, with past and future
  distinguished.
- Station orbits as reference loops.
- The bodies' orbits, each about its parent, through the body (added
  2026-10-14).
- Burns and flyby periapses as markers carrying their Δv and altitude.
- Each class of line shown or hidden in the settings: the ships' paths
  with their events, the station orbits, and the planets', moons' and
  small bodies' orbits (added 2026-10-15); and the labels, all at once
  (what is drawn still picks).

Textures, elevation, star catalogs and shape models come from NASA, USGS,
ESA and PDS. Each source's license is checked when it is imported and
recorded in `data/`, as with the economy's sources.

### Engine

- **Graphics and shaders:** wgpu 30, with WGSL shaders loaded by
  `include_str!`.
  - wgpu is where every reusable piece below lives, and it is damascene's
    main backend.
  - It also opens the scRGB swapchain on Mesa (damascene's
    COLOR_MANAGEMENT.md), so HDR output needs no other API.
  - We give up ray tracing, mesh shaders and bindless resources, which
    this scene does not need.
  - ash is the fallback if wgpu blocks something. vulkano is not used:
    polychora's experience shows it restricts SPIR-V and leaves draws
    `unsafe` anyway.
- **UI:** damascene 0.8 for panels, tables, plots and text, in our own
  host on winit.
  - Damascene's stock host (`damascene-winit-wgpu`) creates its device
    with no optional features, so no GPU timestamps, and passes only
    primary-button drags to the app. (It does have hooks for app
    textures, `gpu_setup` and `before_paint`; corrected 2026-10-14.)
    Our host is built from the layers it exports for custom hosts
    (`host::gfx::WindowGfx`, `host::color`, `host::input`), which
    include its HDR negotiation.
  - The scene renders into an `Rgba16Float` texture shown by a damascene
    `surface()` widget, so panels, menus and popovers layer over the map
    correctly. Damascene encodes the output.
  - Damascene passes only left-button drags to a widget, so raw input
    goes to the camera whenever the pointer is over the viewport
    (volumetric's pattern).
- **Labels:** damascene text nodes, rebuilt each frame at projected
  positions, decluttered greedily by priority (step 5). Measured first:
  ~2.6 µs a label a frame, so Hushfold's distance-field text, the
  planned fallback, was not needed. Polychora's pairwise relaxation, the
  planned declutter, was rejected (2026-10-15): it never drops a label,
  so in a dense cluster the labels spread far from what they name.
- **Crates:**
  - `crates/render`, the renderer. It takes a scene described in 64-bit
    positions, and knows nothing of orbits or missions.
  - `crates/sandbox`, the app: host, UI, plans, playback.
  - `crates/orbit` gains the mission planner.

**Reuse.** Surveyed 2026-10-13. We take:

| From | Piece | Changes |
|---|---|---|
| volumetric | Host glue: one device, the scene in a texture under a damascene widget, input routing (~300 lines, `volumetric_ui_v2/src/host.rs`, `session.rs`) | The pattern only, on damascene 0.8's host layers (volumetric's host is on 0.6 and hard-codes an sRGB swapchain); 16-bit float target |
| volumetric | Line pipeline (~600, `volumetric_renderer/src/pipelines/line.rs`) | Clip at the near plane (it doesn't: segments passing behind the camera draw wrong); camera-relative positions; joins |
| volumetric | Offscreen render and pixel-test harness (~1,700, `offscreen.rs`, `frame_tests.rs`) | New test scenes |
| volumetric | Orbit camera with animated refocus (~1,460, `camera.rs`, `navigation.rs`) | 64-bit eye and target; follow a moving body; zoom from 100 km to 1e10 km; reversed depth |
| Hushfold | HDR, bloom and tonemap chain (~330, `gfx.rs`, `post.wgsl`) | Drop the warm grade and vignette; raise the clamp at 60 so the Sun's energy survives; tonemap to the output's headroom |
| Hushfold | Growable instance buffers, mesh builders, GPU timestamps, screenshot and recording | UVs on vertices |

Skipped: volumetric's deferred renderer and SSAO (an 8-bit CAD look),
Hushfold's procedural sky, polychora's 4D renderers and its vulkano glue.

Planned and not taken (step 5, 2026-10-15): volumetric's points and GPU
picking (points are the scene's own, step 3 plan, item 4; picking runs
on the CPU in f64, since the bodies are spheres and the rest points and
lines); Hushfold's distance-field text and polychora's label declutter
(see Engine, Labels).

## Gate

1. Every leg M1a checked against a published value (steps 6–8) exists as
   a mission file, flies, and is drawn. The halos, the NRHO and the DRO
   close in the Earth–Moon frame.
2. Zooming from 5 au to 100 km above the Moon shows no jitter. An offscreen
   test checks that a ship in low lunar orbit is drawn within a pixel of
   its position projected in 64 bits.
3. A plan with a ship on every Mars window from 2040 to 2075 plays at a
   month per second at the display's refresh rate on the development
   machine (Radeon RX 6900 XT, Mesa RADV), measured by GPU timestamps.
4. On an HDR output the Sun and sunlit limbs use the headroom. On SDR the
   same scene tonemaps without clipping bands.
5. Re-planning an edited leg meets M1a's times (≤ 0.1 s re-planning,
   ≤ 10 s from the playbook) without stalling the view.

## Steps

1. Mission plans in `crates/orbit`:
   - the format;
   - one entry point for every leg kind;
   - chaining and waits;
   - paths sampled for drawing;
   - `orbit mission FILE` printing each leg's Δv and duration.
2. Host and an empty scene:
   - winit, wgpu and damascene 0.8;
   - the scene texture in a surface widget;
   - input routing;
   - the offscreen test harness and GPU timers.
3. Scale:
   - camera-relative positions and reversed depth;
   - the orbit camera following a body across the full zoom range;
   - lit spheres, the Sun, points for bodies under a pixel.
4. Paths:
   - chunked lines for trajectories and station orbits;
   - frames;
   - the playback clock and time warp;
   - markers for burns, passes and events.
5. Panels and labels:
   - measure damascene's label cost first;
   - the plan's legs, Δv and failures;
   - labels with declutter;
   - picking.
6. Realism (done before step 5, decided 2026-10-14: a visual pass
   first):
   - rotation models and textures;
   - the HDR chain;
   - stars and the Milky Way;
   - the bodies' orbits;
   - atmospheres, clouds and night lights;
   - relief, eclipses, rings and shape models.
7. Mission planning (step 7 plan, at the end): the timeline,
   re-planning from the middle of a plan, plans written out, window
   lanes, the options panel, editing, choices beyond picking, and legs
   leaving a low orbit from the ship's own orbit.
8. The gate.

### Step 1 plan (2026-10-14)

1. **Places** (`data/places.toml`, `orbit::places`): the places ships
   go, by the ids the route graph uses (`leo`, `l1`, `nrho`, `llo`,
   `moon-south-pole`, `lmo`, `phobos`, ...), each defined physically: a
   low circular orbit (body, altitude above the mean radius or radius,
   inclination or a free plane; LEO is 6,778 km, 400 km above the
   equator, as the gate flew it), a
   station orbit (a catalog family and the period wanted; the nearest
   member), a small body or moon to meet, or a body's surface. Every
   small body in the ephemeris is a place by its id. Today these
   definitions are spread over `orbit gate` and `orbit routes`; both
   read the file instead.
2. **The format** (`data/missions/*.toml`, `orbit::mission`). A plan
   holds ships, since the gate's plans do (a ship on every Mars window):

   ```toml
   name = "Gateway crew rotation"

   [[ship]]
   name = "Orion"
   start = { at = "leo", epoch = "2045-03-01" }

   [[ship.leg]]
   to = "nrho"
   days = [5.0]        # durations offered, days; the cheapest flies

   [[ship.leg]]
   wait = 30           # days

   [[ship.leg]]
   to = "llo"
   days = [0.5]

   [[ship.leg]]
   to = "moon-south-pole"
   ```

   - A place is an id or written out:
     `{ low = "earth", altitude = 200 }`,
     `{ station = "l2-halo-south", period = 6.57 }`,
     `{ body = "2000-sg344" }`, `{ surface = "moon" }`.
   - `aero = true`: arrive through the atmosphere, by aerocapture into
     a low orbit or entry to the surface.
   - `window = ["2045-03-01", "2045-03-31"]` bounds the departure.
     Without one, the window opens when the ship is ready (arrived,
     plus any waits) and lasts as gate's did: a revolution of the
     station left or reached, 5 days between low orbits about Earth and
     the Moon, a day in Mars's system; between planets, a synodic
     period (at most 3 years), so the leg takes the next window.
   - `days` is required within a system. Between planets it is
     optional; without it the leg takes the cheapest date and duration
     by patched conics in the window.
   - `pin = { depart = "2045-03-01T12:00", days = 190 }` (added in step
     7) chooses one of a leg's options over its `days` and `window`,
     which stay: the departure of a leg between planets or of an event
     (the window becomes that instant), the duration of any leg; either
     may be left out.
3. **One entry point** (`mission::Planner`) by the kinds of the two
   ends:
   - a low orbit and its body's surface: an event priced by `surface`
     (landing, ascent, entry);
   - within a system: `legs::plan`, in the field gate used for that
     system. A window where no design converges is tried again opened
     a quarter of its length later, up to three windows, and the leg
     says so: the gate sweep converged most legs on 98–100% of dates,
     and one bad window should not end a chain;
   - between systems: the cheapest dates by patched conics, each cell
     priced for the actual ends, then `interplanetary::plan`, trying
     the next-cheapest dates when one fails. Station orbits as ends
     between planets wait for step 9's open part: an error says to fly
     through LEO.
4. **Chaining.** Each leg leaves from where and when the last arrived.
   - A ship in a station orbit follows one reference per visit, built
     before the leg that arrives, long enough for the stay and the leg
     that leaves. A stay there costs that reference's upkeep
     (`stationkeeping::upkeep`) per day.
   - A ship in a low orbit coasts in the system's field.
   - After an aerocapture the ship coasts up to the low orbit's radius
     and circularizes there, so the path is drawn without a jump. The
     speed leaving the pass is aimed in the field so the apoapsis is at
     the low orbit's radius (half a Keplerian period missed it by 12 km
     at Mars, from its J2).
   - Known gap: a leg leaving a low orbit picks its own plane and phase
     (the node is free in `End::Low`), so after arriving in one the next
     departure does not start from the ship's own orbit. The planner
     reports the jump at each such junction (km, and the plane change
     it would take); fixing it is playbook work (departures from a set
     plane). Leaving after a launch or an ascent, the plane is free in
     fact: launch timing picks it. (Fixed in step 7, part 8.)
5. **Paths for drawing** (`orbit::track`): a design records each arc's
   integration frame, and `Leg::track` integrates its arcs again with
   dense output. Samples go every 2° of the motion about the body
   pulling hardest, and at most 6 hours apart (~30 s in LEO, a few hours
   in cruise), with velocities, so that cubic Hermite interpolation
   between them stays well under a meter in LEO. Stays are sampled the
   same way. Burns, flyby periapses, passes and surface events are
   markers.
6. **`orbit mission FILE`** prints each item as it is designed: the leg,
   its dates, Δv per burn and in total, the options offered, the time
   taken, and why a leg failed. `--tracks DIR` writes the samples as
   CSV, for looking at shapes before the view exists.
7. **Missions** in `data/missions/`: the crew rotation above, and the
   legs M1a checked against published values (gate item 1).

### Step 2 plan (2026-10-14)

1. **Crates.**
   - `crates/render`, wgpu only: the scene target (`Rgba16Float`, linear,
     1.0 the SDR reference white, sized to the viewport), a reversed-Z
     `Depth32Float` buffer for step 3, the empty scene (cleared to
     space's black), GPU timers, and an offscreen device for tests.
   - `crates/sandbox`, the `sandbox` binary: the host, the damascene app,
     and the frame function that the window and the tests share.
2. **Host.** Our own winit loop, built from damascene-winit-wgpu 0.8's
   host layers: `WindowGfx` for the surface, damascene's renderer, MSAA
   and the color negotiation (including live renegotiation when the
   window moves between outputs); `host::input` for mapping events.
   - The device requests timestamp queries, inside encoders too, when
     the adapter has them.
   - The app asks for `ColorPreferences::hdr_extended()`, so an HDR
     output gets an scRGB float swapchain, and SDR an 8-bit sRGB one.
3. **Frame.** One encoder and one submit per frame:
   - build the UI, with the scene target shown by a `surface()` keyed
     `viewport`, and prepare it;
   - read the viewport's rect; if its size in pixels changed,
     reallocate the target (the tree shows the new one from the next
     frame, so a resize draws one more frame);
   - render the scene into the target, then the UI, which composites
     it.
   The same function draws into the swapchain or into an offscreen
   texture.
4. **Input.** A press over the viewport (damascene's hover target is
   within `viewport`) goes to the camera, and to damascene too (a click
   on the map still takes focus from a text field); the wheel over it
   goes to the camera alone; everything else goes to damascene. In step 2 the camera is a stub that accumulates
   orbit, pan and zoom and shows them; step 3 builds the real one. As
   in KSP's map view: right drag orbits, middle drag pans, the wheel
   zooms, a left click picks (step 5).
5. **Shell UI.** A top bar (title, whether HDR is active, the output's
   headroom), the viewport filling the rest, and a status line: frame
   time, GPU time for the scene and the whole frame, the camera stub.
6. **GPU timers.** Timestamps at the frame's start, after the scene and
   after the UI (inside the encoder where the device allows; otherwise
   around the scene pass only). They are read back through a ring of
   three buffers, so measuring never stalls the pipeline.
7. **Offscreen harness.** A windowless device runs the same frame
   function.
   - Tests render the scene alone (read back as linear floats) and the
     whole frame (UI and scene, 8-bit sRGB).
   - `sandbox --screenshot PATH` writes the whole frame as a PNG.
   - Checks: the target's clear value reads back exactly; in the whole
     frame the viewport's rect shows the scene's color and the rest
     shows the UI; the timers report positive times where supported.

### Step 3 plan (2026-10-14)

1. **Positions.** The scene is described in 64-bit km in the
   ephemeris's frame (solar-system barycentric, ecliptic J2000). The CPU
   subtracts the eye's position and rotates into view space in 64 bits,
   and only then hands 32-bit vectors to the GPU. Depth is reversed with
   an infinite far plane (depth = near / distance, near at 1 m).
2. **Spheres** are ray-cast in the fragment shader, not tessellated: the
   silhouette is exact from 100 km above the Moon to a dot. A sphere's
   quad covers the cone of rays that hit it (the whole screen when the
   camera is close). Spheres wholly off the screen are culled. The ray's hit distance uses the camera's
   altitude above the sphere, computed in 64 bits
   (`t = h(2r + h) / (b + √(b² − h(2r + h)))`), which stays exact where
   `b − √…` would cancel: looking straight down from low orbit. Depth is
   written per pixel.
3. **Light and units.** Linear radiance, 1.0 the radiance of a white
   Lambertian surface under the Sun at 1 au (SDR white until step 6's
   exposure). A body is Lambertian with its Bond albedo and a tint (NASA
   fact sheets), lit at (1 au / its distance from the Sun)². The Sun is
   a sphere of constant radiance (1 au / R☉)² ≈ 46,000. Limb darkening,
   bloom, exposure and tonemapping are step 6.
4. **Points.** A sphere under a pixel is drawn as a point carrying its
   true flux: the disk-average radiance of a Lambertian sphere at its
   phase angle, (2/3)·albedo·E·Φ(α), times its solid angle over the
   point's. The flux is continuous where a sphere shrinks to a point,
   anywhere on the screen: off the axis a pixel subtends pixel²·cos³θ.
5. **Markers** for ships (step 4 uses them): a fixed size in pixels,
   depth-tested against the bodies.
6. **Camera** (`sandbox`): focused on a body and following it; right
   drag orbits as a turntable about ecliptic north; the wheel zooms by
   distance, from 10 km above the body's surface to 1e10 km. A new focus
   is reached by a 0.6 s animation (position along the line between the
   two moving bodies; distance in log space). The focus is picked from a
   select in the top bar. Panning is dropped: the map view always looks
   at something, as KSP's does.
7. **Time.** A minimal clock so following can be seen: space pauses, `.`
   and `,` change the rate by tenfold; the date and rate in the status
   line. Step 4 builds playback.
8. **Tests** (offscreen):
   - Gate item 2's check: a marker in low lunar orbit lands within a
     pixel of its position projected in 64 bits, with the camera 100 km
     to 5 au away.
   - The Moon's limb, seen from 100 km up, within a pixel of the
     horizon computed in 64 bits.
   - A lit sphere's center pixel has the radiance the units say, and
     the Sun's disk its radiance.
   - A point's summed flux matches the sphere it stands in for.

### Step 4 plan (2026-10-14)

Sizes measured first: the missions in `data/missions/` hold ~116k
samples in all (the crew rotation's first stay in LEO, 2.6 days, is
7,685); an ephemeris frame at one moment takes ~240 ns.

1. **Lines** (`render::paths`). A path is samples (time, position,
   velocity) in some frame's coordinates, and a pose per frame at the
   moment drawn maps them into the world (origin, axes, scale).
   - Positions reach the GPU as two f32s per coordinate (high and low
     parts), and the eye, in the same frame's coordinates, likewise;
     the vertex shader subtracts the eye first. The error is then
     ~6e-8 of the distance from the eye, as for bodies. This replaces
     The view's "chunks with 64-bit anchors and 32-bit offsets": cruise
     samples are 650,000 km apart, so an anchored chunk of a few dozen
     of them is off by ~10 km; holding that under a pixel 10 km from a
     ship needs chunks under ~45,000 km, and thousands of anchors moved
     every frame.
   - Chunks of 64 segments stay, as the units of culling and of detail:
     each frame the CPU culls them against the view in f64. Where a
     straight segment's chord would bow visibly from the Hermite curve
     (over 0.25 px; 2° of LEO bows ~1 km), its segments are cut into
     pieces along the curve on the GPU, a power of two up to 32, the
     chunk's parts of 8 segments each cut as finely as it needs
     (amended after measuring: cutting every segment of a near chunk in
     32 took 18.8 ms of GPU time for two million segments seen from 10
     km).
     The bow is bounded as 4/27·(|h·v₀ − Δp| + |h·v₁ − Δp|), which also
     bounds the curve for culling (amended in review: the midpoint's
     offset alone missed an S-shaped segment).
   - Where f32 is not enough, the CPU places the pieces (amended in
     review): segments whose pieces would number over 32, or whose
     Hermite terms (up to a segment's length, 650,000 km in cruise)
     would round by more than 0.25 px seen from the eye, are cut and
     made relative to the eye in f64, a few dozen near the eye. In f32
     a cruise segment seen from 10 km is off by ~20 px between samples.
     So a ship's marker on its line stays on it at any zoom.
   - Segments are clipped at the near plane before projection, drawn as
     antialiased capsules a fixed width in pixels, depth-tested against
     the bodies. Two samples at one time (a burn, or two motions
     meeting) are not joined.
   - Each path has two colors, past and future. The CPU finds each
     path's segment and fraction at the clock's time in f64 each frame,
     and the fragment shader colors by that (amended in review: times
     as f32 seconds since a path's start drift by a second after 30
     days, 2 km of LEO; and the fraction along a piece is recovered
     perspective-correctly from the screen).
   - Lines and markers are drawn in a second pass over the scene's
     depth; step 6 moves that pass after the tonemap, so exposure does
     not touch them.
   - Paths are uploaded when they change (a new item, another frame),
     and stay on the GPU between frames.
2. **Frames** (`sandbox::frames`):
   - **Focus** (the default): fixed ecliptic axes centered on the
     camera's focus, as in KSP.
   - **Rotating with two bodies**: Earth–Moon, Sun–Earth, Sun–Mars;
     the origin at their barycenter, axes from `reference::EarthMoonFrame`
     (renamed `RotatingFrame`, since it serves any two bodies);
     optionally scaled by their distance ("pulsating").
   - A path's samples are mapped into the frame at their own times
     (positions and velocities, in parallel), and drawn by the frame's
     pose now.
   - The camera's turntable turns with the frame: its azimuth and
     elevation are about the frame's axes now, so in the Earth–Moon
     frame a halo stays still while the ship moves along it. Changing
     the frame keeps the view's direction.
   - A select in the top bar picks the frame; `--frame` on the command
     line.
3. **Station orbits as reference loops.** Each station place's catalog
   orbit, one revolution in the CR3BP, drawn in the Earth–Moon
   pulsating frame at its pose now: in that frame it is still, and in
   any other it turns with the Moon. Faint, in every view.
4. **Plans in the sandbox.** `sandbox --plan FILE` reads a plan and
   designs its ships on a background thread (ships in parallel), and
   each item appears when it is ready. Paths are colored by leg (a
   categorical palette in plan order); a stay takes the color of the
   leg that arrived, dimmer, and before the first leg it is gray. A
   stay resting on or at a body (its track is the body's own motion)
   draws no line. The status line counts ships designed and failures;
   the panels are step 5.
5. **Markers.** Shapes: a disc for each ship where it is now (none
   before its first item or after its last); a diamond for a burn; a
   ring for a flyby or an atmospheric pass; a square for a landing
   (where the ship leaves its orbit) or an ascent (where it reaches
   orbit); a cross for a junction's jump. Each sits on its path, mapped
   like the samples; past ones dimmer. Δv and altitude labels are step
   5's.
6. **Playback** (`clock.rs`):
   - time warp in named steps: 1 s, 10 s, 1 min, 10 min, 1 h, 6 h,
     1 day, 1 week, 1 month and 1 year per second (`,` and `.`), and
     space pauses;
   - a scrubber under the map spanning the plan (the ephemeris without
     one); dragging it sets the time;
   - `[` and `]` jump to the previous and next marker;
   - with a plan, the clock starts at its first ship's start.
7. **Tests** (offscreen unless said):
   - A line about the Moon, in a rotated and scaled frame, seen from
     100 km to 5 au: its pixels within a pixel of the path projected in
     f64 (gate item 2's check, for lines).
   - From 10 km off a LEO path, the drawn line passes within a pixel of
     the Hermite curve between samples.
   - A segment passing behind the eye draws only its part in front.
   - The past and future colors meet within a pixel of the clock's
     time.
   - Frames (unit): poses invert; the Moon on the rotating frame's x
     axis; samples' velocities in a rotating frame match their
     positions' change; a catalog loop closes.
   - Playback (unit): warp steps, jumps to markers, the scrubber's
     mapping.
   - GPU time for the published plan's paths, and for a synthetic two
     million segments (gate item 3 needs headroom).
8. **Not here:** the panels, labels and picking (step 5); a lighter
   form for long stays' tracks themselves (each sample is 56 bytes in
   `Track`, 48 on the GPU; at two million samples, worth revisiting when
   step 8's plan is written).

### Step 5 plan (2026-10-15)

Measured first, with a probe over the sandbox's UI at 1920×1080 (release
build, labels moving every frame): damascene's text nodes cost ~2.6 µs
a label a frame on the CPU, mostly in its paint pass. That is 0.6 ms for
200 labels, 1.4 ms for 500 and 2.6 ms for 1,000; text that changes every
frame costs no more. So labels are damascene text, and Hushfold's
distance-field text is not needed. Only the labels left after declutter
are built.

1. **Labels** (`sandbox::labels`):
   - What is labeled:
     - bodies, by name;
     - ships where they are, by name;
     - markers, by what they are: a burn's Δv; a flyby's body,
       altitude and Δv; a pass's body and altitude; a landing or an
       ascent; a jump's gap and plane change;
     - station loops, by their place, at the loop's first sample.
   - Each is projected in f64 through the scene's camera onto the map
     view. None is labeled behind the eye, off the view, or behind a
     body (where the ray from the eye enters a sphere before it).
   - A body's label sits beside its disk. A body whose disk is taller
     than a quarter of the view has none: it is what the view is about,
     and the status line names it.
   - Declutter is greedy by priority. Each label is tried right of,
     left of, above and below its anchor, and kept at the first place
     that overlaps no label already placed and no anchor shown; else it
     is dropped. Priorities, highest first: ships; the focus; the Sun and
     planets; moons; markers ahead of the clock; small bodies; markers
     behind it; station loops. Within one, nearer the eye first.
   - This replaces polychora's pairwise relaxation (the reuse table).
     It pushes overlapping labels apart and never drops one, so with
     the inner planets seen from Saturn their labels would spread
     across the screen, far from what they name.
   - Drawn as damascene text over the map view, at the caption's size,
     on a dark translucent backing; markers' labels in their marker's
     color.
2. **Panel** (`sandbox::panel`). With a plan loaded, a panel right of
   the map shows it (`p` toggles):
   - each ship: its name, whether it is designing, designed or failed,
     its total Δv and its span;
   - each item: its kind, label, start, duration and Δv, and a leg's
     burns; a failure in red, with why; a junction's jump, with its
     gap and plane change; the item's note;
   - the item each ship is in at the clock's time, marked;
   - clicking an item seeks the clock to its start and selects it.
3. **Picking** (`sandbox::pick`), by a left click on the map, takes the
   first of:
   - a label, for what it names;
   - the nearest ship or marker within 8 px;
   - the nearest body whose disk is under the pointer, or whose point
     is within 8 px;
   - the nearest path within 4 px, for its item.

   A body picked is focused (the camera flies to it). A ship, marker or
   path selects its item in the panel, scrolled into view; for a ship,
   the item it is in. A click on empty space clears the selection. A
   selected item's markers' labels come first in the declutter.

   Picking runs on the CPU in f64, from the projections the labels
   already make. GPU picking (volumetric's, in the reuse table) is not
   needed: the bodies are spheres, and the rest are points and lines.
4. **Tests**:
   - Declutter (unit): placed labels overlap neither each other nor
     anchors shown; of two in conflict, the higher priority stays; the
     same input places the same labels.
   - Labels (unit): a body's label lands beside its projected point
     within 1 px; none is placed behind a body, behind the eye or off
     the view.
   - Picking (unit): a click on a body's center, on a marker beside a
     body, on a path, on a label and on empty space each picks what it
     should.
   - Panel (unit): a designed plan's rows carry each item's Δv and
     duration, and a failure's reason.
   - Offscreen: labels are drawn where they were placed.
   - CPU time for labels, the panel and a pick with the published plan,
     in a window.
5. **Not here:** the camera following a ship; a selected leg's line
   highlighted; hover tooltips; editing (step 7).

### Step 6 plan (2026-10-14)

Moved ahead of step 5 (decided 2026-10-14), with the bodies' orbits
added. Built assets (textures, the star catalog, the Milky Way) are
committed under `data/`, as `de440.bin` is, rebuilt by scripts in
`tools/` (decided 2026-10-14; ~80–100 MB in all). Each source's license
or terms are recorded beside it when imported.

The renderer as step 4 left it: one HDR pass (ray-cast spheres, then
points as 2 px squares of light, additive), then an overlay pass
(lines, markers) over its depth, straight into the texture damascene
shows. A clear color for the sky, Lambertian spheres of one albedo each,
no tonemap.

Order of work, each part committed with its tests:

1. **The HDR chain.**
   - The scene renders into its own HDR texture; a post pass writes the
     texture damascene shows: exposure, bloom, a tonemap to the output's
     headroom (from the host; 1.0 on SDR), and on SDR a dither of one
     8-bit step against banding. The overlay pass then draws over the
     tonemapped image, against the scene's depth, so exposure does not
     touch lines and markers.
   - Bloom: Hushfold's chain (13-tap downsample, tent upsample), with
     no threshold and a small weight (energy kept, as a lens's glare),
     and no clamp below f16's range: the Sun's 46,000 survives.
   - Tonemap: linear up to a knee, then a smooth shoulder that
     approaches the headroom; per channel, so the Sun goes white. (Since
     2026-10-15, on a color's largest channel, its hue kept: Realism.)
   - Exposure follows the focus: its sunlit side at normal incidence
     maps to a fixed level (key / (albedo × irradiance)), eased over
     0.6 s on refocus. `-` and `=` shift it by a stop.
   - Points of light (stars, and bodies under a pixel) are a Gaussian
     spot carrying the same flux, not a 2 px square, and take the
     stars' fixed exposure ("Realism": stars get their own), so planets
     seen from afar look like the bright stars they are. Where a body
     grows from a point to a disk (1–3 px), the two cross-fade.
2. **The sky.**
   - Stars: the Hipparcos catalog (118,000 stars, to V ≈ 9; CDS
     I/239): direction in ecliptic J2000, proper motion applied on the
     GPU for the scene's date, flux from V, color from B−V (blackbody
     temperature, Ballesteros 2012). Parallax from anywhere in the
     solar system is under 1″, and aberration ~20″: both left out.
     `tools/sky/` builds `data/sky/stars.bin`.
   - The Milky Way: NASA SVS's Deep Star Maps 2020 background (Gaia
     DR2's faint stars and the diffuse light, in celestial coordinates,
     with Hipparcos and Tycho stars removed, so none is drawn twice),
     resampled into a texture. Its scale against the stars' fluxes is
     calibrated from SVS's matching bright-star image, so both share
     units. Tycho-2's stars fainter than Hipparcos (V 9–11.5) are
     splatted into it at build time if the gap shows.
   - The sky is fixed in the ICRF. In a rotating frame the camera turns
     with the frame, so the stars turn across the view: once a month in
     the Earth–Moon frame.
3. **The bodies' orbits** (new, in `sandbox`). Each body's path about
   its parent (planets and small bodies about the Sun, the Moon about
   Earth, Phobos and Deimos about Mars) over one period around now, from
   the ephemeris, drawn as paths with a pose per parent placed at the
   parent's position now, as KSP does: the line passes through the body
   in every frame. Windows step by 1/16 of a period, so rebuilds are
   rare. They are a second set of paths, uploaded apart from the plan's,
   dim, with past and future showing the direction of motion.
4. **Rotation models and textures.**
   - `orbit::rotation`: each body's orientation from IAU WGCCRE 2015
     (Archinal et al. 2018): pole and prime meridian, with the Moon's
     and Neptune's periodic terms. Earth's rotation uses UT1 = TT −
     ΔT, ΔT taken as 69 s (its 2024 value; it cannot be predicted, and
     each 10 s is 0.04°). Checked against Horizons's sub-observer
     longitudes and latitudes, fetched as fixtures. `ephem::pole` is
     made to read it, if the mission tests hold; else reported.
   - Textures, equirectangular, east longitude, 0° at the center, with
     mipmaps; the level picked from the pixel's footprint on the
     sphere (no seam at the date line):
     - Earth: Blue Marble Next Generation (NASA Earth Observatory);
     - the Moon: SVS CGI Moon Kit (LROC WAC, LOLA);
     - Mars: Viking colorized mosaic (USGS), MOLA;
     - Mercury: MESSENGER MDIS (USGS);
     - Jupiter: Cassini (PIA07782); Saturn from Cassini if a global
       map is found, else bands;
     - Venus, Uranus and Neptune near uniform, as they look in
       visible light.
   - Each texture is scaled so its mean matches the body's albedo, so a
     resolved body and its point agree.
   - Shading: Lambert for clouded bodies; for the Moon, Mercury and
     small bodies, the lunar-Lambert law (McEwen), which keeps the full
     Moon flat to its limb.
   - Relief: normals from elevation (LOLA, MOLA, ETOPO), lit, not
     displaced.
5. **Atmospheres.** Earth, Mars and Venus, by Hillaire's 2020 method:
   transmittance and multiple-scattering tables computed on the CPU at
   start (in f64, so tests can check them), and a ray march per pixel
   through the shell, from the sphere's own quad grown to the
   atmosphere's top. The surface is seen through it (transmittance and
   in-scattering), and beyond the limb it blends over the sky. Lit from
   behind, the shell glows as a ring.
   - Earth: Rayleigh, Mie, ozone (Hillaire's values; built, the Mie the
     global mean's, Realism); clouds folded into
     the surface's albedo; night lights (Black Marble 2016) shown with a
     fixed boost, like the stars, since at the day side's exposure they
     would not show; a sun glint off the oceans.
   - Mars: dust (Mie) and CO₂, from published parameters; Venus: a haze
     over a bright cloud deck.
6. **Shadows, rings, the Sun, shape models.**
   - Eclipses: the Sun's visible fraction from each surface point, as
     the overlap of the Sun's disk with up to four occluding spheres the
     CPU picks (Earth on the Moon, the Moon on Earth, Phobos on Mars).
   - Saturn's rings: a radial profile of color and opacity (Cassini,
     PDS Ring-Moon Systems Node), lit, with the planet's shadow on them
     and theirs on the planet.
   - The Sun: limb darkening that keeps its total flux; bloom does the
     glare.
   - Shape models for Phobos, Deimos, Bennu and Ryugu (PDS; Ryugu's
     terms checked), drawn as meshes once over a few pixels.
7. **Tests**, beside each part:
   - The tonemap: monotonic, linear below its knee, under the headroom;
     SDR has no flat run of more than an 8-bit step in a smooth ramp.
   - Exposure leaves lines' and markers' colors alone.
   - Stars: Sirius, Betelgeuse and Polaris land within a pixel of their
     positions projected in f64; a star's spot sums to its flux.
   - Orbits: each body's line passes within a pixel of the body in
     every frame.
   - Rotation: sub-observer points against Horizons, per body.
   - Textures: a known feature (Tycho crater, Olympus Mons, the Strait
     of Gibraltar) lands within a pixel of its projected position.
   - Atmospheres: the tables against analytic optical depths; the limb
     brighter than the disk's center; a ring from the night side.
   - Eclipses: the Moon's shadow on Earth at 2045-08-12's total eclipse
     falls where the Besselian elements put it.
   - GPU time at 1920×1080 for each part, for gate item 3.
8. **Not here:** Earth's clouds moving, cities growing, Jupiter's
   features drifting (the textures are fixed in time); shadows on ships.

## Step 6, part 8 plan (2026-10-15): air against a Monte Carlo

Mars looked washed out toward its terminator. A Monte Carlo of the same
dust (plane-parallel, its first order checked against the analytic slab
and its local estimates against binned escapes) found three faults in
the shader, all of them in how light reaches and leaves the ground or
scatters more than once (single scattering agreed within 5%):
Hillaire's isotropic multiple scattering is 1.2–4× too bright for
forward-scattering dust; the ground has no skylight (near the
terminator 2.5× its direct light); and ground light the dust scatters
forward toward the eye is counted lost.

The fix, kept by the user (2026-10-15):

1. **Delta-scaling** (δ-Eddington, with Nakajima and Tanaka's
   single-scattering correction): the share f = g² of Mie scattering in
   the phase function's forward peak is taken as unscattered (δ-
   Eddington's share for Cornette–Shanks's Henyey–Greenstein factor; its
   second Legendre moment χ₂ would also take the factor 1 + cos²θ's
   symmetric 0.1, which is no peak: f 0.1 at g = 0). The scaled medium
   (Mie scattering × (1 − f), its residual phase function
   Cornette–Shanks with the residual's asymmetry) carries everything but
   single scattering:
   - multiple scattering: the table is computed in the scaled medium,
     and added along the view ray with its σ′ₛ and the scaled
     transmittance;
   - the ground: lit by the scaled direct beam and the scaled skylight,
     and seen through the scaled transmittance (forward-scattered ground
     light is ground light, blurred by less than a pixel from orbit).
   Single scattering keeps the true medium (its forward aureole, the
   night-side halo), as does what the air lets through of whatever is
   behind it (stars, other bodies, the Sun's disk). The transmittance
   table's spare channel holds the Mie column along each ray, so the
   shader gets the scaled transmittance as T · e^(f σₘ column).
2. **Skylight**: a table of the scaled sky's irradiance on the ground by
   the Sun's cosine (twilight included), per channel, integrated on the
   CPU from the same model; the ground adds albedo × it (× (1 + n·up)/2
   on relief), dimmed by eclipses.
3. **Test**: the Monte Carlo becomes `tools/atmosphere/` (with its two
   checks) and writes a fixture of reference radiances for pixels of a
   uniform Mars (albedo 0.25) at phase angles 40°, 90° and 130°; a
   sandbox test renders the same pixels and holds the air alone (black
   ground) and air with ground to it.

As built (2026-10-15), where it differs, each found against the fixture:

- Multiple scattering in the scaled medium was still isotropic, and the
  true one is not: the Sun at a cosine of 0.3 and the eye at 0.6, it
  falls ~5× from 54° to 160° from the Sun's direction (Henyey–Greenstein
  of asymmetry 0.4–0.5 fits it, as two scatterings by the dust give:
  χ₁² = 0.51). So it stays Hillaire's, in
  the true medium, spread by Henyey–Greenstein with the square of the
  local mean cosine (blended from isotropic by Mie's share, evaluated
  once a ray). Isotropic in the scaled medium, the haze was 0.57–1.34×
  the Monte Carlo's; spread, 0.78–1.10× (the Sun over 0.2 in cosine).
- The ground seen through the scaled beam alone lost what the air
  scatters out of the view and back in from beside it (diffuse ground
  light: the Monte Carlo's ground was seen by 0.89 where the beam gave
  0.70). It is seen through T(μ) + E(μ)/μ instead: by reciprocity, the
  skylight a beam from the eye would give it (for an eye inside the
  air, in proportion to the column crossed). Hillaire's ground bounce
  is dropped from the multiple scattering (it counted that light again),
  and with it the atmosphere's `ground_albedo`.
- The skylight's multiple scattering is spread the same way.
- Against the fixture, where the Sun is over 0.2 in cosine: the ground's
  part within 8%, the haze within 22%, the whole within 5% but at 130°
  phase (−19%, the haze). Nearer the terminator the Monte Carlo's slab
  dims the Sun more than the shell does (1.14× at 0.19, 1.7× at 0.11):
  there the render runs up to 1.2× it. The skylight table: within 0.4%
  of τ/2 under a thin Rayleigh sky on a flat planet.
- Cost: Earth filling a 1080p view with its maps and air, 1.05 ms (0.80
  before).
- Earth's air shares all of it, unchecked against a Monte Carlo. Its
  aerosols, the global mean's since 2026-10-15, are 14× Hillaire's in
  optical depth: its large phase angles lean on the haze, which at 130°
  left Mars 19% under its Monte Carlo (above).

## Step 6, part 9 plan (2026-10-15): shapes, small bodies' maps and spins

The moons and small bodies with published shape models are drawn with
them, and the rest with made-up ones (the user, 2026-10-15: sources
with convenient licenses, the remaining bodies approximated); with the
small bodies' maps and spins, and the planets' maps still missing.
Sources and their terms: the research of 2026-10-15, recorded in
`tools/shapes/` and beside each file in `data/` as it is imported.

**Decided with the user (2026-10-15): shaped bodies are radius maps,
ray-marched in the spheres' own pass.** The models chosen are radius
grids already: Gaskell's implicitly connected quadrilaterals, 6 × 64²
quads (49,152 facets), one radius per direction from the center; the
radar models are near enough. Rejected: rasterized meshes. The scene's
pass has no MSAA and blends far to near (air and edges composite in
order), so each resolved body would need its own MSAA render off screen
composited at its place in that order, a depth-only redraw for the
overlay's lines, and a shadow map for its self-shadowing. Marched, a
shape inherits the order, the air's blending, eclipses, the edges'
coverage and the fade from its point; it costs fragment time where one
is large on screen, and needs bodies seen whole from their centers
(star-shaped).

Order of work, each part committed with its tests:

1. **The shape path** (`render`).
   - `render::shape::Shape`: a star-shaped body's radius by direction in
     its frame (`Sphere::axes`), km, on a cube map's six faces (N²
     texels each, in wgpu's face order and orientation); with its least
     and greatest radii, its area and volume (the map triangulated), and
     a bound on the slope of |p| − r(p̂), which the march's steps are
     divided by.
   - On the GPU: an `Rgba16Float` cube texture with mips, held with the
     body's maps (`Maps::shape`; a shaped body without a color map gets a
     plain one): the radius between the least and the greatest (alpha)
     and the normal in the body's frame (rgb), from the radius map's
     differences; each level the mean of the one below.
   - `shape_fs`, a second entry point over the sphere's quad (grown to
     the greatest radius), drawn in the same pass and order: the ray
     sphere-traced from where it enters the bounding sphere (or from
     the eye, within it), each step |p| − r(p̂) over the slope bound and
     at least a share of the pixel's footprint, to the first crossing,
     refined by bisection. Its edge's coverage from the ray's closest
     miss, in pixels of the footprint there, as a sphere's limb's.
     Self-shadowing: a second march, toward the Sun, its penumbra the
     Sun's angular size (the least miss over distance along it, over the
     Sun's angular radius). Then `lit`, `sun_seen` (eclipses by other
     bodies), `written`, depth and opacity as a sphere's. Its maps'
     coordinates are the hit's longitude and latitude from the center,
     as maps of irregular bodies are published; their screen
     derivatives, for the maps' levels, are the mean sphere's, taken
     before the march (after a loop with a data-dependent exit, control
     flow is not uniform). The shape's own level follows the pixel's
     footprint over its texel, so a body a few pixels across marches a
     smoothed one.
   - `scene.rs`: a shaped sphere reaches its greatest radius (its quad,
     its culling, its spot's place in front), and is skipped only with
     the eye inside the body itself (its radius toward the eye).
     `Sphere::radius` is its volume-equivalent radius: its point's size,
     the fade, an eclipse's occluder.
   - The sandbox: the camera's least altitude scales with the body (a
     tenth of its radius, at most 10 km), over its greatest radius;
     labels and picks reach its greatest radius.
   - Tests: a triaxial ellipsoid (exact intersections) from several
     directions: coverage summed against its projected area, depth and
     Lambert radiance per pixel against exact; two overlapping lobes lit
     from the side: shadowed where a CPU ray says; the eye inside the
     bounding sphere; the cube map's lookup on the GPU against the
     CPU's. GPU time with one filling 1920×1080 (a measurement).
   - As built (2026-10-15), where it differs:
     - The faces are a six-layer `Rg16Float` array, not a cube texture,
       each bordered by a sixteenth of its side sampled past its edges:
       a cube texture's seamless filtering takes the next face's texels,
       whose centers drift along a shared edge by up to half a texel, so
       the radius was off at first order there (1.6e-3 of an ellipsoid's
       at 128² a face) and its slope did not shrink with the texels.
       Bordered, second order to the edges (2.8e-4, 7.0e-5 at 256²). The
       radius is two half floats (a high one and the rest × 1/1024: f16
       alone would step Phobos's by ~3 m), the normal taken in the
       shader by differences, a texel of the level apart.
     - Edges: a miss covers by its least height over the surface (along
       the normal, not radially); a hit near the edge by c² / 2κ, from
       the view's cosine to its normal and the surface's curvature toward
       the eye, measured from the normals a step either side (a sphere's
       of the local radius was 4× off on the ellipsoid's flanks); wholly
       where the ray, out of the body, meets it again. An edge pixel is
       shaded at its covered strip's middle: at the limb, a backlit
       crescent thinner than a pixel showed 16 times its light.
     - The shadow march's least step is half a texel of the level
       marched, as well as half a pixel's footprint, growing past half
       its steps: by the footprint alone, a ray skimming the surface went
       ~100 pixels, and across a body 500 pixels wide, shadows from
       farther were lost; by a 24th of the bounding radius (as first
       built), ridges a few texels wide cast none.
     - Every sphere's quad (not only a shape's) grew by two pixels: its
       old 2% margin is 0.06 pixels on a 3-pixel disk, cutting off the
       edge's coverage (a disk off the axis at 2.5 pixels showed 1.7%
       under its flux; now 0.5%).
     - Checks: the ellipsoid (off its origin, so no plane through the
       origin mirrors it) from five directions at phase 13° to 157°,
       150–300 px across: pixels within its edges within 1% + 0.006 of
       exact (0.006: the map's own normals at 128² a face are 0.12–0.39°
       off where it curves most), sums 0.03–0.51% off (the thinnest
       crescent, 0.02 of light in all, shows none: under a
       hundred-thousandth of its pixels at full light); two lobes under a 10° Sun, ~120 and ~500
       px across: 152 and 4,538 pixels deep in cast shadow dark, lit
       ones within 2%; the eye 0.6 km over the ellipsoid, inside its
       bounding sphere, within 1%; inside it, nothing drawn; markers a
       hair in front of the surface shown, a hair behind hidden.
       Reintroducing each fault found here (the far-part check, the
       strip's middle, the shadow's step), removing self-shadowing,
       mirroring a face, or skipping the eye inside the bounds fails a
       test. On lavapipe as on the GPU.
     - A review (a sub-agent, with runtime probes against exact traces)
       found, all fixed with a test each: holes where a march spent its
       160 steps short of the surface (the slope bound, taken over a face
       corner's texel angle for every texel, was 2.7–3.8 times too loose;
       now each texel's own neighbors' angles; the least step grows past
       half the 192 steps, and a ray still short is taken as a hit) — the
       timing's own view had 13,233 holes in 722,454 pixels, a 3 km
       semi-axis 750 px across 1,496 of 67,729, and the eye 10 m up
       looking along the surface 6,447 of 62,229; ridges a few texels
       wide casting no shadow (above); a small body's crescent drawn short
       (at 140°, 37% at 3 px, 4% at 20 px): a body under 24 px in radius
       now takes ⌈24 / its radius⌉² samples a pixel (to 8 × 8), 4.3% short
       at 3 px and within 2.4% from 4 px; a pixel past a nearer part's
       edge over a farther one shaded as the nearer part's limb alone (28
       of 127 such pixels under 70% of exact; now the farther part is
       shaded for its share: 8). Also: an eclipse's reach takes a shaped
       receiver's greatest radius, a NaN where the eye is inside the
       volume-equivalent sphere is clamped, and a shaped body is asserted
       lit, without air or rings (`shape_fs` draws neither).
     - Not fixed, found there: the plain sphere path's disks, 3 px in
       radius at 140° of phase, show 23% over their flux (16% at 6 px, 2.5%
       at 20 px), where they take over from their points.
     - Checks added: no pixel within the edges wrong from 300 to 3,000 px
       across or 10 m to 1 km up; the near edge over the far part; small
       bodies' flux within 5% from 3 to 20 px at 0° to 140°; a ridge's
       shadow (one pixel in ten at most shown lit, against a fine CPU march:
       4% and 2% now, rays clipping its crest over less than a step); the
       Sun's true size and an eclipse. Each fault above put back fails a
       test, but a spent ray taken as a miss: with the tighter bound, no
       tested ray spends its steps.
     - Cost: the ellipsoid filling 1920×1080, 2.2 ms (1.1 ms as first
       measured, on an image with holes).
2. **Shapes' photometry** (`sandbox::photometry`, `render`).
   - The disk: averaged over orientations, a convex body's brightness is
     a sphere's of the same area (each facet counts alone, whatever its
     law). Its gain is that of a sphere of its volume-equivalent radius
     R, times 4πR² / S, over the share of its light its self-shadowing
     leaves at that phase (averaged over orientations, tabulated per
     shape when built, 0–180° by 10°).
   - Its point: its facets' light summed on the CPU at the geometry (16²
     a face), times that share and the gain: a lightcurve, which a
     sphere's point lacks.
   - Tests: the disk over random orientations against its curve; point
     and disk at the fade; an ellipsoid's lightcurve at full phase
     against the ratio of its cross-sections.
   - As built (2026-10-15): the share is taken from 64 random Suns and
     eyes at each phase over the facets at 16 squares a face, each
     facet's Sun and eye marched for on the map (Phobos's 0.99 at 0°
     (its parts hiding others), 0.98 at 90°, 0.89 at 150°, 0.72 at 170°;
     the smoother Didymos, Nereus and the made-up shapes stay over 0.96
     to 170°). The facets' light is scaled to the map's full area (their
     polyhedron's is smaller), so points average to their curves too.
     A shaped body without maps is shaded lunar-Lambert (Phobos's and
     Deimos's were Lambert, as maps-less spheres).
   - Checks: over 32 random orientations at 30°, 90° and 120°, the disks
     of Phobos, Bennu, Itokawa and Ryugu against their curves with their
     lightcurves there: within 0.06 mag on the mean (0.03–0.05 under at
     120°, at 400 and 1,600 px alike: shadows finer than the tables'
     facets, 1.5% of Bennu's, and pixels at a part's edge over another),
     each within 0.3. Their lightcurves themselves reach ±0.9 mag at 120°
     (the crescent's area turning with the body). At one geometry from
     100 radii, Phobos's and Bennu's disks within 0.08 mag of the same.
     Itokawa's first showed 0.8–1.5 mag over: one slope bound for the
     whole body (65, set by the curtain its overhang makes) left its
     marches spending their steps short, and a ray so spent was taken as
     a hit where it stood. The bounds are now local: each texel holds the
     steepest within a border's width of it, its levels the greatest of
     their texels', and a step goes no farther across (Itokawa's own
     greatest stays 65 at its overhang; elsewhere 2–6); a spent ray is a
     hit only within two pixels over the surface. The ellipsoid filling
     1080p now costs 2.8 ms.
3. **The shapes' data** (`tools/shapes/`: `fetch.sh`, `build.py`). Each
   model ray-cast from its origin into the cube map (256² a face: 16
   texels a model's quad), checked star-shaped (rays meeting the surface
   more than once counted; the outermost hit kept), written to
   `data/bodies/ID/shape.bin` (f32 radii), with `data/bodies/shapes.toml`
   (source, terms, size, radii, area, volume, slope bound, shadow table).
   - Phobos (Gaskell 2006), Deimos (Thomas 1993's 5° grid), Bennu
     (SPOv54), Ryugu (Watanabe et al. 2019; JAXA's terms: credit
     ISAS/JAXA, modifications stated), Itokawa (Gaskell), Didymos (DART
     v003), Apophis (Brozović et al. 2018), 2008 EV5 (Busch et al. 2011),
     Nereus (Brozović et al. 2009): public domain but Ryugu's. Models keep
     their sizes; a shaped body's radius is its model's
     volume-equivalent one (the SBDB's beside it in `shapes.toml`).
   - Made up: 1996 FG3 (none archived: an oblate primary with an
     equatorial ridge, as binary primaries are) and 2009 UY19 (nothing
     published: elongated, 1.4 : 1.1 : 1), each a seeded Gaussian random
     sphere over an ellipsoid, scaled to the SBDB's diameter, marked
     made up.
   - Left out: Phobos's and Deimos's 2023 models (Ernst et al.; terms
     unstated, "All rights reserved": APL could be asked); Dimorphos
     (not among the game's bodies).
   - As built (2026-10-15), at 128² a face (0.5 MB each, 5.5 MB in all;
     the build ~7 minutes, most of it the shadow tables):
     - Thomas's Deimos grid is in west longitude, though its label says
       only "planetocentric": against Ernst et al.'s model (not imported:
       used as a check), 364 m rms off read so, 640 m at best read east,
       over longitude offsets ±30°.
     - Not star-shaped: Itokawa (181 texels meet it twice: its head's
       overhang over its neck, the outermost kept, a curtain down to the
       neck) and Ryugu (15). The volume-equivalent radii against the
       SBDB's: Ryugu, Apophis, EV5 and Nereus within 1–2%, Bennu 1.2%
       over, Itokawa 1.9% under, Didymos 0.365 km against 0.39 (DART's).
       1996 FG3's SBDB diameter, 1.2 km, is the NEOWISE one; radar and
       lightcurve studies give ~1.7 km (Yu et al. 2014): kept as the
       game's, the made-up shape scaled to it.
     - The sandbox reads `shapes.toml` with the maps; a shaped body's
       radius is its shape's once read, its extent (what the camera keeps
       out of, labels and picks reach) its greatest radius. The camera
       stops a tenth of a body's extent over it, at most 10 km (was 10 km
       for every body; a new focus starts five extents out); small bodies'
       orbits are sampled every 3 hours (6 left 2009 UY19's line 2.6 px
       off at the closest the camera now comes); `--altitude` waits for
       the shapes. Screenshots: Phobos, Itokawa and Bennu look as their
       spacecraft saw them (untextured, their spins not yet).
4. **Spins** (`orbit::rotation`, `tools/rotation/`). The small bodies
   have none yet (their axes are the scene's). Bennu (`bennu_v17.tpc`,
   with its YORP term), Ryugu (`ryugu_v10.tpc`), Itokawa (Gaskell's PCK,
   its rate recast to Lowry et al. 2014's period and YORP), Didymos
   (`didymos_system_15.tpc`'s pole, longitude 0 at the DART frame's
   boulder), 2008 EV5 and Nereus (their radar spin states: ecliptic pole,
   period and phase at an epoch), 1996 FG3 (Scheirich et al. 2015's
   period, its pole the mutual orbit's, its phase made up), 2009 UY19
   (made up). Apophis tumbles: Brozović et al.'s spin state carried by
   Euler's equations, torque-free, with its model's moments of inertia.
   Phases in the 2040s past their data's reach (Ryugu's, Didymos's,
   EV5's, Nereus's) are as their models extrapolate, noted in
   `rotation.toml`.
5. **Small bodies' maps** (`tools/textures/`): grayscale where
   published, in their class's color (`photometry::color` gives it):
   Phobos (USGS and Stooke's Viking mosaic), Deimos (Stooke), Bennu
   (USGS normal albedo to ±55°, the panchromatic mosaic beyond), Itokawa
   (Stooke), Ryugu (Hirata et al. 2026, CC BY 4.0: its b, v and w bands
   a near-true color). Stooke's and the Viking mosaics carry their
   images' shading: judged on screenshots, flattened at large scales if
   it reads as a second Sun. The rest: a seeded albedo variation (±10%),
   made up.
6. **The planets' maps still missing**: Saturn (Cassini ISS's global
   color map, PDS; its gaps past 78°N and 82°S and in the rings' shadow
   filled from HST OPAL 2025, CC BY 4.0), Uranus and Neptune (their
   bands from OPAL's composites, averaged along longitude; their color
   the measured one), Venus left plain (no visible-light map exists; its
   cloud deck is near featureless there).

## Step 6, part 10 (2026-10-15): the eye's glare

Bloom was Hushfold's chain spreading 1% of the image's light, a weight
chosen by eye; its shape, equal light in each octave of distance from 2
to 256 px (~1/r²), nobody chose, and its reach was in the target's
pixels, not the viewer's sight. Replaced by the viewer's eye's glare
(the user, 2026-10-15: "try the principled change"). As built:

- The CIE's glare spread function (Vos and van den Berg 1999, CIE
  135/1), as McCann and Vonikakis 2018 give it (eq. 2) but its core's
  half-width read 0.0046°, not their 0.046° (at 0.046° its integral is
  37, not ~1, and its tail 900/θ³ against the CIE general equation's
  10/θ³). A 25-year-old brown-eyed viewer's; normalized over the sphere
  (integral 1.047). In angle at the viewer's eye (`Look::pixel`), so it
  follows the display's scale: at CSS's reference pixel, 23% of a
  point's light stays within half a pixel, about half lands 0.5–8 px
  off, 10% past 32 px (2.8% past 2,048 px, 44°, off any screen).
- Of the light past what the output shows only: what the tonemap takes
  off, per texel. The viewer's own eye scatters what the output does
  show; spreading that too would count it twice. A body within the
  output's range has no glare (the bloom hazed it at 1%).
- On the same chain of half-size levels, now down to 2 px, each level
  weighed by a Richardson–Lucy fit (10 rounds) of the levels' spread,
  as measured (each alone at 1920×1080), to the eye's share in each
  octave of distance. Tested: 4–1,024 px within 15% an octave (measured
  within 12%); 1–4 px within 45%, the first level's one shape (up to
  42% over at 2–4 px at a display scale of 2).
- Found building it: taken of the taps' 2×2 means, the excess lost half
  a small source's light past the knee (a lit texel among dark ones
  fell under it). Found in review: its texels found by rounding the
  pixel's uv jumped 3 texels at an odd-sized image's center, losing up
  to half a point's glare in a band through it (where the focus sits);
  now the 2×2 under the pixel, at the cost of up to a texel's shift at
  the far edges of odd sizes.
- Cost at 1920×1080 (RX 6900 XT): the post chain 0.65–0.9 ms, from 0.21
  ms: the excess pass loads 36 texels a half-size pixel. A full-size
  excess pass would cut it; not done.
- Lifted light glares as shown (the user, 2026-10-15: kept): bright
  stars carry halos, larger at the outer planets, whose exposures show
  the sky up to 6–7 stops brighter than Earth's.
- Tuned for play (the user, 2026-10-15): the background and body lifts
  17 → 15 stops; the Sun dimmed by a setting, its disk and its point,
  −6 stops by default (0 true, to −20; the default chosen on
  screenshots of the Sun past Earth's limb: at 0 its glare veils the
  whole view, Earth's night side with it; at −6 it glares around the
  Sun and the night side's lights show beside it). Focused on the Sun,
  the exposure keys to its disk as shown, so that dimmed, the rest of
  the view brightens.
- Found: the Sun drawn as a point (past ~4 au at the default field of
  view) took the body lift with the planets, 17 stops; it takes its
  own setting instead.

## Step 6, part 11 plan (2026-10-15): the Sun's photosphere

The Sun is a disk of one radiance, darkened toward its limb by a linear
law per channel. It gets a surface and its measured colors across the
disk (the user, 2026-10-15: "a better texture, and the right color
distribution").

**Decided with the user (2026-10-15): procedural, calibrated to
measurements.** Rejected: a global map stitched from SDO/HMI continuum
images, whose spots would be one past rotation's frozen into 2045, at
~1,000 km a texel (granulation blurred when near).

**Color.** The renderer's white is the Sun's whole-disk light
(`tools/color.py`): the disk's mean stays white, at the radiance the
scene is lit by. Across the disk, each channel's center-to-limb profile
comes from the measured limb darkening by wavelength, taken through the
channels (`tools/sun/`): a polynomial in μ per channel, the disk's mean
kept. Contrasts on the surface (granulation, spots, faculae) are given
as temperatures, and their colors follow through the channels too (a
table of each channel's intensity against temperature, relative to the
photosphere's).

**The surface**, evaluated per pixel in the sphere's pass, in the Sun's
body frame (its IAU rotation, 14.1844°/day: the Carrington rate), each
feature a temperature turned into the channels' light by a table
(3,000–7,500 K every 100 K, 1 at the quiet photosphere's 6,247 K: the
disk center's brightness temperature in green):
1. Granulation: cells on the sphere, their mean area 1.36 Mm² (Sánchez
   Cuberes et al. 2000; the modal diameter ~1.4 Mm), each living ~6
   min (Hirzberger et al. 1999's mean; exponentially distributed),
   born small and shrinking away (each cell weighed in the cellular
   distance by its life, so the pattern changes continuously). 255 K
   rms, which gives 18% rms contrast in green at disk center, 15.5% in
   red and 22.6% in blue (measured), against 3D simulations' 18%, 14%
   and 25%
   (Wedemeyer-Böhm and Rouppe van der Voort 2009); falling toward the
   limb as their simulations do (×0.84 at μ 0.5, ×0.60 at 0.2, ×0.35
   at 0.05). Faded out where a pixel spans more than ~0.35 Mm
   (point-sampled, a granule needs ~4 pixels across, or it aliases),
   and where a frame spans more than a few percent of a lifetime (time
   warp).
2. Active regions from a model of the solar cycle at the date:
   - The sunspot number (version 2) as the sum of cycles in Hathaway
     2011's shape (b from the amplitude in version 1 numbers, ×0.66):
     cycle 25 from 2020.1 at 160.9 (SILSO's maximum, October 2024),
     26 from 2030.7 at 140 (the middle of published predictions,
     121–158; Gerçeker et al. 2025's maximum in June 2035), later
     ones every 11.04 years at 179 (cycles 1–24's means). March 2045:
     cycle 27's rise, SN ≈ 155.
   - The visible hemisphere's spot area, 9.08 SN + 0.0114 SN² MSH
     (Nagovitsyn and Osipova 2021): ~1,700 MSH then.
   - Groups born at the rate that keeps that area, each day's births
     from that day's seed (deterministic in the date); their largest
     areas log-normal (Baumann and Solanki 2005: median 62.2 MSH, ln σ
     = ln 2.45), capped at 5,000; growing over 2 days, then decaying
     at 10 MSH/day (Gnevyshev–Waldmeier).
   - Latitude: of the cycle chosen in proportion to the cycles' sunspot
     numbers then, ±(28° e^(−t/90 months)) (Hathaway 2011, t from the
     cycle's start), spread 1.5° + 3.8° (1 − e^(−A/400)) with A the
     hemisphere's area (Hathaway 2015); either hemisphere alike.
     Longitude uniform at birth, then carried at 14.551 − 2.87 sin²φ
     °/day (Balthasar et al. 1986, spots), against the frame's rate.
   - Each group a leader spot of 60% of its area and 1–3 followers
     sharing the rest, the leader ahead in rotation and toward the
     equator, their axis tilted by Joy's law, 0.27 φ ± 5° (Dasi-Espuig
     et al. 2010), 3° + 0.008° A apart (made up: a 1,000 MSH group's
     11°). Each spot an umbra of 0.4 its radius (penumbra 5–6× the
     umbra's area: Hathaway 2013) at 4,000 K (12% of the quiet Sun at
     600 nm; Maltby et al. 1986's darkest large cores 3,900 K; Borrero
     and Ichimoto 2011: 5–25% in the visible) and a penumbra at 5,765 K
     (75% at 600 nm; theirs 65–85%). Spot shares, follower counts and
     separations are made up; real umbrae of small spots run brighter.
   - Faculae: a plage around each group, 10× its spots' area (SATIRE's
     filling factors at maximum, ~3% against ~0.3%), lasting twice as
     long, its excess temperature against μ from Yeo et al. 2013's fit
     at 617 nm at a plage's 300 G (+9 K at disk center, +128 K at μ
     0.4, +24 K at 0.1: 6–8% at its peak, near μ 0.3–0.4).
3. The disk's mean is not renormalized: spots and faculae move the
   Sun's light by ~0.1%, as they do (Kopp 2016).

**Tests**, beside each part: each channel's profile against its
integration; the disk's mean with no spots; granulation's contrast at
full resolution, and its fade when under a pixel; the same date giving
the same spots; spot area against the sunspot number, latitudes against
the cycle's phase; a spot drifting with differential rotation.
Screenshots: the Sun from 1 au, from 0.05 au, and from just over it.

As built (2026-10-15), where it differs or adds:
- The limb: the linear law it replaced had the center at 1.25, 1.29 and
  1.33 of the mean and μ 0.1 at 0.58, 0.51 and 0.43; measured, 1.21,
  1.25 and 1.32, and 0.54, 0.47 and 0.36 (the lines in the spectrum
  change them under 0.3%). The 5th-order fits are exact to 4 places.
- Granulation: every cell lives 6 min (the distribution's spread left
  out), its life offset its own; its share of the cellular distance
  0.5–1 and its brightness 0.3–1 with its life, and each granule's own
  0.7–1; a dome, smoothstep(0, 0.8, 1 − F1/F2). Its mean and rms
  measured on the GPU (0.227, 0.235). 255 K rms gives 18.1% in green,
  15.5% in red and 22.6% in blue, the disk 1.2% brighter. Faded from a
  pixel spanning 0.15 of a grid spacing to 0.6, and from a frame
  spanning 3% of a life to 15%.
- Spots are smooth round umbrae and penumbrae (no penumbral filaments),
  each covering its share of a pixel's footprint (a disk of its area,
  or of a twentieth of the spot's radius, softening a near spot's
  edge), blended in light; faculae a Gaussian plage around each group.
  The date's surface moves the disk's light by +0.01% to +0.11%
  whether the disk is 450 pixels in radius or 4. Found in review:
  spread over a pixel's width and blended in temperature, a spot under
  a pixel took half of it (−2.4% for a 4-pixel disk).
- Groups' lives: the Gnevyshev–Waldmeier rule's linear decay gives the
  largest (3,000 MSH) ~300 days, against ~100–135 observed for giant
  groups (the review's reading; Hathaway and Choudhary 2008's decay
  rising with area, a secondary source's, would shorten them). The
  area over a day runs 0.25–1.9× its target (5–95%); over 60 years,
  0.96. Kept, noted.
- Latitudes fold at the equator. The model's groups are drawn once a
  day (0.18 ms each time).
- The model: SN 162 at October 2024 (SILSO's 160.9), 141 at June 2035,
  155 in March 2045; then ~15 groups on the whole Sun (~7 seen: version
  1's Wolf number gives ~6), the spot area over 2046 within 17% of its
  target, latitudes 19.5° early in a cycle and 8.6° late.
- On the GPU the glows are a uniform buffer (a stage's storage buffers
  are at most 8): at most 4 glowing spheres and 512 features; features
  under the eye's horizon on the sphere are left out. The Sun filling a
  1920×1080 view up close: 0.9 or 1.5 ms, frame to frame of the same
  scene (0.09 ms plain), most of it the granulation.
- Found in research: Yeo et al. 2013's fit's left vector runs (B/μ)⁰ to
  (B/μ)³ (read as ¹ to ⁴, faculae came out at 1,000%).
- Open: on SDR, the Sun's exposure key leaves its disk's center at 1.5×
  the white, in the tonemap's shoulder, which washes its surface out
  near (two stops down shows it); network and polar faculae; small
  spots' brighter umbrae; eclipses of the Sun from a surface still take
  its disk as even.

## Step 7 plan (2026-10-15): mission planning

Decided 2026-10-15: step 7 grows from editing a leg's window or duration
into the planning interface, and comes before M1b. It takes from M3 the
two panels that need no economy, the window calendar and the leg
options panel (DESIGN.md, UI); inventory projections and the ledger
stay in M3, after M1b. The choices offered are the autopilot's, along
each leg's frontier; past them, other ways to choose are tried (part
7); a burn editor stays out. M1b's first item, legs leaving a low orbit
from the ship's own orbit, moves here, last.

Each part is committed on its own.

1. **Timeline** (`sandbox::timeline`), under the map and the panel,
   replacing the scrubber.
   - Its own time axis. It opens fitted to the plan (else to a year
     about the clock); the wheel zooms about the pointer, from the
     ephemeris's span down to an hour across; a drag on the lanes pans.
     Ticks on a calendar ladder (years, months, days, hours), labeled
     at the coarsest step that leaves room. A click or drag on the
     axis's row seeks the clock. While playing, when the clock leaves
     the span shown, the span pages after it.
   - A lane per ship, its name in a gutter at left. Its items are bars:
     legs, landings and ascents in their paths' colors (`paths`), stays
     dimmer and thinner, a failure in its marker's color. Burns, flybys,
     passes and jumps are ticks in their markers' colors. A bar is at
     least 2 px wide.
   - A cursor at the clock's time across the lanes.
   - A click on a bar selects its item, as the panel and the map do,
     and seeks the clock to where it landed; the selected item is
     outlined. Hovering on a bar shows the item's label, dates and Δv.
   - Built from damascene elements placed by a layout closure, as the
     labels are, one keyed element per bar drawn, so that damascene
     routes clicks, hovers and the wheel. Damascene 0.8's plot widget
     has lanes and a time axis, but no bars and no color per mark.
   - Lanes past the strip's height scroll; the strip takes at most a
     third of the window.
2. **Re-planning from the middle** (`orbit::mission`).
   - Each item names the plan's step that made it; the stay before a
     leg belongs to the leg.
   - The planner keeps a checkpoint before each step: where the ship
     is, since when, and when it is ready. A planner resumes from a
     checkpoint with the ship's steps after it edited.
   - Where an edit takes effect: at the edited step, except that a
     station's visit is built long enough for the steps after it (its
     waits, the next leg's window and longest duration), so an edit to
     those takes effect at the leg that arrived at the station. An edit
     to a ship's start takes effect at its start.
   - In the sandbox each ship's design has a generation. An edit bumps
     it, keeps the items before where it takes effect, and starts
     designing from that checkpoint. A design of an older generation
     finishes the leg it is on (a design cannot be interrupted) and its
     items are dropped.
3. **Plans written out** (`orbit::mission`).
   - `Plan::to_toml`, which `Plan::parse` reads back to the same plan.
     Places keep the TOML they were written as.
   - `depart = "YYYY-MM-DDTHH:MM"` pins a leg's departure, with one
     duration in `days`: what choosing an option writes. The window and
     the frontier shown stay those the leg has without the pin.
   - The sandbox saves to the file it loaded (Ctrl+S) and opens plans
     from `data/missions/` through a menu, or starts an empty one.
4. **Window lanes**, under the ships' lanes.
   - One per kind of leg between planets in the plan: its two ends, and
     whether it arrives through the atmosphere.
   - For each day's departures across the timeline's span, the least Δv
     by patched conics over the durations, as the design's search
     prices them (`interplanetary::patched`), on one sequential ramp
     with a legend in km/s. Past a ceiling the lane is empty.
   - Computed in the background a lane at a time, and kept. Each
     ship's departure is marked in its lane.
   - Measured first: the cost of a day of departures over the
     durations.
5. **Options panel**, for the selected leg, below the plan's list.
   - Between planets: the porkchop over the leg's window (departure
     against duration, shaded as the window lanes are, with contours at
     round values); the dates the design tried, marked converged or
     not; the one flown, ringed. A click on a cell pins the leg there
     (part 3) and re-plans from it (part 2). The design keeps its grid
     (`between`'s cells already are one) in the item.
   - Within a system: Δv against duration at the offered durations. A
     sweep (a dozen durations around them) is designed in the
     background on request, its points shown as they converge. A click
     on a point pins that duration.
6. **Editing**, in the plan's panel.
   - A leg's destination (a menu of places), window, durations and
     aerocapture; a wait's days; a ship's start and epoch.
   - Adding a leg or a wait after any item, removing one, moving one
     up or down; adding, renaming and removing ships; undo.
   - Each edit re-plans from where it takes effect; rows not yet
     designed again say so.
   - An edit that cannot be flown (a surface reached from an orbit
     about another body, a window closing before the ship is ready)
     says why at once, from the plan's own checks, without a design.
7. **Beyond picking** (asked 2026-10-15: past the autopilot's options,
   look for other ways to choose). To try, keeping what works and
   listing the rest:
   - goals rather than points: the cheapest leaving by a date,
     arriving by a date, the fastest under a Δv; each a choice over the
     frontier the leg already has;
   - dragging a leg's bar on the timeline to move its departure,
     snapping to designs;
   - the next or the previous window, for a leg between planets;
   - moving a ship's start moving its pinned legs with it.
8. **Leaving a low orbit from the ship's own orbit** (from M1b, decided
   2026-10-15). A leg that leaves a low orbit the ship arrived in by a
   flown leg starts from that orbit: its plane fixed, its departure
   time chosen in the window for its phase. Until then the jump is
   reported (step 1 plan, item 4). It is playbook work, of a size not
   yet known.
9. **Tests and costs:**
   - the timeline's mapping between time and pixels, its zoom about the
     pointer and its ticks; bars drawn at their items' times within a
     pixel (offscreen); a click on a bar selecting its item;
   - a plan edited and re-planned from the middle gives the items a
     fresh design of the edited plan gives;
   - a plan written out and read back is the same plan;
   - a porkchop's cell picked flies at that cell;
   - the timeline's CPU time a frame with the published plan; a
     re-plan after an edit, against gate item 5.

Then step 8, the gate.

## Step 8 plan (2026-10-16): the gate

1. **Item 1.** The published plan flown again at the end of step 7
   (part 8 changed legs leaving a low orbit), against M1a's values.
2. **Item 2.** The offscreen checks for a marker and for lines (steps 3
   and 4) stand; nothing new.
3. **Item 3.** `data/missions/mars-windows.toml`: a ship on each of the
   17 Earth–Mars windows `orbit windows earth mars` finds from 2040 to
   2075 (2041-09 to 2075-11), LEO to a 400 km low Mars orbit, one way.
   Round trips would add stays of ~500 days in low Mars orbit, sampled
   whole (step 1's known gap), a different question. `sandbox --warp`
   starts the clock at a rate; measured in a window (`--frames`: wall
   and GPU time a frame) around the Sun and near Mars, and by an
   offscreen test (CPU and GPU time a frame, stepping a month a second
   at the display's refresh).
4. **Item 4.** HDR checked on the development machine's OLED
   (2026-10-16, with prism's gamut tools). SDR: the gate's scenes shot
   through the SDR tonemap, checked for clipped bands. Open from step
   6: the Sun's disk washed out on SDR.
5. **Item 5.** Each leg of the published plan designed alone, against
   10 s; in the sandbox, an edit's time to its re-planned items, and
   the frame's CPU time while it designs. M1a's 0.1 s is for
   re-planning a standing route's departure (`legs::replan`); the
   sandbox has no standing routes (M1b's), and an edit designs afresh.
