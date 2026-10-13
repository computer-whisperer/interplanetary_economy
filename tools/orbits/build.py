"""Builds data/orbits.toml from JPL's periodic-orbit families (fetch.sh).

The catalog lists each family by Jacobi constant, which folds back on
itself for some families (the L2 halos interleave their NRHO and large-halo
branches). So members are ordered by period or Jacobi constant, whichever
moves through the family more evenly, screened (CANDIDATES evenly spaced
ones, less those that pass within MOON_MARGIN_KM of the Moon's surface or
EARTH_MARGIN_KM of Earth's), and split into runs wherever a step between
neighbors exceeds GAP times the median: the game interpolates between
neighbors only within a run. Each family is sampled at about SAMPLES members
in all. Closest approaches come from integrating one period in the CR3BP
(scipy's DOP853); the game corrects members itself (crates/orbit).

Usage: python3 tools/orbits/build.py DIR > data/orbits.toml
"""

import json
import sys

import numpy as np
from scipy.integrate import solve_ivp

FAMILIES = [
    ("l1-lyapunov", "Earth–Moon L1 Lyapunov (planar)"),
    ("l2-lyapunov", "Earth–Moon L2 Lyapunov (planar)"),
    ("l1-halo-north", "Earth–Moon L1 halo, northern"),
    ("l1-halo-south", "Earth–Moon L1 halo, southern"),
    ("l2-halo-north", "Earth–Moon L2 halo, northern"),
    ("l2-halo-south", "Earth–Moon L2 halo, southern (includes the NRHOs)"),
    ("dro", "Distant retrograde orbit about the Moon"),
]
SAMPLES = 60
GAP = 10.0
CANDIDATES = 400  # members screened per family, evenly spaced
R_EARTH_KM, R_MOON_KM = 6378.1, 1737.4
EARTH_MARGIN_KM, MOON_MARGIN_KM = 200.0, 50.0


def closest(mu, lunit, s0, period):
    def f(t, s):
        x, y, z, vx, vy, vz = s
        r1 = np.sqrt((x + mu) ** 2 + y * y + z * z) ** 3
        r2 = np.sqrt((x - 1 + mu) ** 2 + y * y + z * z) ** 3
        return [vx, vy, vz,
                2 * vy + x - (1 - mu) * (x + mu) / r1 - mu * (x - 1 + mu) / r2,
                -2 * vx + y - (1 - mu) * y / r1 - mu * y / r2,
                -(1 - mu) * z / r1 - mu * z / r2]
    sol = solve_ivp(f, (0, period), s0, method="DOP853", rtol=1e-10, atol=1e-13, dense_output=True)
    t = np.linspace(0, period, 4001)
    x, y, z = sol.sol(t)[:3]
    earth = np.sqrt((x + mu) ** 2 + y * y + z * z).min() * lunit
    moon = np.sqrt((x - 1 + mu) ** 2 + y * y + z * z).min() * lunit
    return earth, moon


def unevenness(rows):
    """Largest step between neighbors over the median step, in (x, z, vy)."""
    jumps = np.linalg.norm(np.diff(rows[:, [0, 2, 4]], axis=0), axis=1)
    return jumps.max() / np.median(jumps)


def split_runs(screened):
    """Continuous runs of clear members: a run ends at a member that is not
    clear (None) or at a step over GAP times the median step."""
    clear = [m for m in screened if m is not None]
    steps = [np.linalg.norm(np.subtract(b[:3], a[:3])) for a, b in zip(clear, clear[1:])]
    limit = GAP * np.median(steps)
    runs, run = [], []
    for m in screened:
        if m is None or (run and np.linalg.norm(np.subtract(m[:3], run[-1][:3])) > limit):
            if len(run) > 1:
                runs.append(run)
            run = [] if m is None else [m]
        else:
            run.append(m)
    if len(run) > 1:
        runs.append(run)
    return runs


def main():
    src = sys.argv[1]
    first = json.load(open(f"{src}/{FAMILIES[0][0]}.json"))["system"]
    mu, lunit, tunit = float(first["mass_ratio"]), float(first["lunit"]), float(first["tunit"])
    out = [
        "# Earth–Moon periodic-orbit families, sampled from JPL's Three-Body Periodic",
        "# Orbits catalog (ssd-api.jpl.nasa.gov/periodic_orbits.api) by",
        "# tools/orbits/build.py; edit that, not this file.",
        "#",
        "# CR3BP units: lengths in length_unit_km, times in time_unit_s. Each member",
        "# starts on the x-z plane moving perpendicular to it: state (x, 0, z, 0, vy,",
        "# 0). Members passing within 50 km of the Moon's surface or 200 km of",
        "# Earth's are left out; perigee and perilune are closest-approach radii.",
        "# Members are in order along their family; `run` numbers its continuous",
        "# stretches, and members are interpolated only within a run.",
        "",
        f"mass_ratio = {mu!r}",
        f"length_unit_km = {lunit!r}",
        f"time_unit_s = {tunit!r}",
    ]
    for fid, name in FAMILIES:
        rows = np.array([[float(v) for v in r] for r in json.load(open(f"{src}/{fid}.json"))["data"]])
        rows = min((rows[np.argsort(rows[:, key], kind="stable")] for key in (7, 6)), key=unevenness)
        screened = rows[:: max(1, len(rows) // CANDIDATES)]
        clear = []
        for x, _, z, _, vy, _, jacobi, period, stability in screened:
            earth, moon = closest(mu, lunit, [x, 0.0, z, 0.0, vy, 0.0], period)
            ok = earth >= R_EARTH_KM + EARTH_MARGIN_KM and moon >= R_MOON_KM + MOON_MARGIN_KM
            clear.append((x, z, vy, period, jacobi, stability, earth, moon) if ok else None)
        runs = split_runs(clear)
        step = max(1, sum(len(r) for r in runs) // SAMPLES)
        out += ["", "[[family]]", f'id = "{fid}"', f'name = "{name}"',
                "# x, z, vy, period, jacobi, stability index, perigee km, perilune km, run",
                "members = ["]
        kept = 0
        for k, run in enumerate(runs):
            sample = run[::step]
            if sample[-1] is not run[-1]:
                sample.append(run[-1])
            kept += len(sample)
            out += ["  [" + ", ".join(repr(float(v)) for v in m + (k,)) + "]," for m in sample]
        out.append("]")
        print(f"{fid}: {len(rows)} members, {sum(len(r) for r in runs)} of {len(screened)} screened "
              f"clear of Earth and Moon in {len(runs)} runs, {kept} kept", file=sys.stderr)
    print("\n".join(out))


if __name__ == "__main__":
    main()
