"""Builds data/neas.toml from the raw downloads of fetch.sh.

Each body's motion becomes a sequence of two-body orbits. A segment is a
least-squares fit of 7 parameters (a, e, i, node, peri, M at EPOCH, n) to the
Horizons positions it covers; it splits in half while its worst residual
exceeds TOL and it is longer than 2 * MIN_DAYS. The free mean motion absorbs
the average of perturbations that a single osculating orbit gets wrong (the
Sun's own wobble around the barycenter drifts osculating heliocentric orbits
by ~0.1 au over the game's span).

Usage: python3 tools/neas/build.py RAW_DIR > data/neas.toml
"""

import json
import os
import re
import sys

import numpy as np
from scipy.optimize import least_squares

AU = 149597870.7
GM = 132712440041.279 * 86400**2 / AU**3  # au³/day²
EPOCH = 2466154.5  # 2040-01-01 00:00 TDB
TOL = 0.002  # au
MIN_DAYS = 360

# Classifications from the literature where SBDB has none.
SPECTRAL_OVERRIDES = {"2008-ev5": ("C", "Reddy et al. 2012 (not in SBDB)")}


def horizons_rows(path):
    text = open(path).read().split("$$SOE")[1].split("$$EOE")[0]
    return [line.split(",") for line in text.strip().splitlines()]


def load_vectors(path):
    rows = np.array([[float(f[0])] + [float(x) for x in f[2:8]] for f in horizons_rows(path)])
    return rows[:, 0], rows[:, 1:4] / AU, rows[:, 4:7] * 86400 / AU


def rv_to_params(r, v, jd):
    h = np.cross(r, v)
    hn = np.linalg.norm(h)
    rn = np.linalg.norm(r)
    evec = np.cross(v, h) / GM - r / rn
    e = np.linalg.norm(evec)
    a = 1 / (2 / rn - v @ v / GM)
    node_v = np.cross([0.0, 0.0, 1.0], h)
    i = np.degrees(np.arccos(h[2] / hn))
    om = np.degrees(np.arctan2(node_v[1], node_v[0]))
    w = np.degrees(np.arctan2(evec @ np.cross(h, node_v) / hn, evec @ node_v))
    nu = np.arctan2(np.cross(evec, r) @ h / hn, evec @ r)
    ecc_anom = 2 * np.arctan(np.sqrt((1 - e) / (1 + e)) * np.tan(nu / 2))
    m = np.degrees(ecc_anom - e * np.sin(ecc_anom))
    n = np.degrees(np.sqrt(GM / a**3))
    return np.array([a, e, i, om, w, m - n * (jd - EPOCH), n])


def positions(p, jd):
    """Must match orbit::kepler::Elements::state."""
    a, e, i, om, w, m0, n = p
    i, om, w = np.radians([i, om, w])
    m = np.radians(m0 + n * (jd - EPOCH))
    m = (m + np.pi) % (2 * np.pi) - np.pi
    ecc = np.where(e < 0.8, m, np.pi)
    for _ in range(50):
        ecc = ecc - (ecc - e * np.sin(ecc) - m) / (1 - e * np.cos(ecc))
    x = a * (np.cos(ecc) - e)
    y = a * np.sqrt(1 - e * e) * np.sin(ecc)
    si, ci, so, co, sw, cw = np.sin(i), np.cos(i), np.sin(om), np.cos(om), np.sin(w), np.cos(w)
    px = np.array([co * cw - so * sw * ci, so * cw + co * sw * ci, sw * si])
    py = np.array([-co * sw - so * cw * ci, -so * sw + co * cw * ci, cw * si])
    return np.outer(x, px) + np.outer(y, py)


def fit(jd, r, v):
    mid = len(jd) // 2
    sol = least_squares(
        lambda p: (positions(p, jd) - r).ravel(),
        rv_to_params(r[mid], v[mid], jd[mid]),
        x_scale=[1e-3, 1e-3, 1e-2, 1e-2, 1e-2, 1e-2, 1e-5],
        xtol=1e-14, ftol=1e-14, gtol=1e-14, max_nfev=4000,
    )
    return sol.x, np.linalg.norm(positions(sol.x, jd) - r, axis=1).max()


def segments(jd, r, v):
    p, worst = fit(jd, r, v)
    if worst <= TOL or jd[-1] - jd[0] < 2 * MIN_DAYS:
        return [(jd[0], jd[-1], p, worst)]
    h = len(jd) // 2
    # Neighbors share the boundary sample.
    return segments(jd[: h + 1], r[: h + 1], v[: h + 1]) + segments(jd[h:], r[h:], v[h:])


def body_toml(ident, raw, nhats):
    name = re.search(r"Target body name: (.*?)\s+\{", open(f"{raw}/elem/{ident}.txt").read()).group(1)
    name = re.sub(r"\s*\(2\d{6}\)$", "", name.strip())  # Horizons' SPK-ID suffix
    sbdb = json.load(open(f"{raw}/sbdb/{ident}.json"))
    phys = {p["name"]: p["value"] for p in sbdb.get("phys_par", [])}
    nh = nhats.get(sbdb["object"]["des"], {})
    lines = ["[[body]]", f'id = "{ident}"', f'name = "{name}"']
    if ident in SPECTRAL_OVERRIDES:
        cls, why = SPECTRAL_OVERRIDES[ident]
        lines.append(f'spectral = "{cls}"  # {why}')
    else:
        lines.append(f'spectral = "{phys.get("spec_B") or phys.get("spec_T") or "unknown"}"')
    if phys.get("diameter"):
        lines.append(f"diameter_km = {float(phys['diameter'])}")
    elif nh:
        lines.append(f"# NHATS size estimate {nh['min_size']}-{nh['max_size']} m (albedo-dependent)")
    h_mag = phys.get("H") or nh.get("h")
    if h_mag:
        lines.append(f"h_mag = {float(h_mag)}")

    jd, r, v = load_vectors(f"{raw}/vec/{ident}.txt")
    segs = segments(jd, r, v)
    worst = max(s[3] for s in segs)
    print(f"{ident:12s} {len(segs):2d} segments, worst residual {worst:.4f} au", file=sys.stderr)
    lines.append(f"# {len(segs)} segments, worst fit residual {worst:.4f} au")
    for start, end, params, _ in segs:
        a, e, i, om, w, m0, n = (float(x) for x in params)
        lines += [
            "", "[[body.segment]]", f"start_jd = {float(start)}", f"end_jd = {float(end)}",
            f"a_au = {a!r}", f"e = {e!r}", f"i_deg = {i!r}", f"node_deg = {om % 360!r}",
            f"peri_deg = {w % 360!r}", f"mean_anomaly_deg = {m0 % 360!r}",
            f"mean_motion_deg_per_day = {n!r}",
        ]
    return "\n".join(lines)


HEADER = """\
# Candidate near-Earth asteroids. Generated by tools/neas/build.py from
# downloads made by tools/neas/fetch.sh; edit those, not this file.
#
# Each body's motion is a sequence of two-body orbits, heliocentric, in the
# ecliptic and mean equinox of J2000. Each segment is a least-squares fit to
# JPL Horizons positions (10-day steps, 2040-01-01 to 2076-01-01); a segment
# was split while its worst residual exceeded 0.002 au, so close Earth flybys
# show up as boundaries where the orbit changes. `mean_anomaly_deg` is
# referred to `epoch_jd_tdb` in every segment.
#
# Spectral class and diameter come from the JPL Small-Body Database where
# characterized. Candidates: NASA's NHATS accessibility list (dv <= 6 km/s,
# H <= 26) plus well-characterized bodies; see tools/neas/candidates.txt.

epoch_jd_tdb = 2466154.5
"""


def main():
    raw = sys.argv[1]
    nhats = {row["des"]: row for row in json.load(open(f"{raw}/nhats.json"))["data"]}
    here = os.path.dirname(os.path.abspath(__file__))
    ids = [
        line.split("\t")[0]
        for line in open(f"{here}/candidates.txt")
        if line.strip() and not line.startswith("#")
    ]
    print(HEADER + "\n" + "\n\n".join(body_toml(i, raw, nhats) for i in ids))


if __name__ == "__main__":
    main()
