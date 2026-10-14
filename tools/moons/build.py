"""Builds data/moons.toml from the raw downloads of fetch.sh: Phobos and
Deimos as precessing ellipses about Mars, fitted to JPL Horizons over
2025-2080 (crates/orbit, ephem.rs).

Each moon's ellipse lies in a plane that precesses about its Laplace pole
(near Mars's pole; for Deimos tilted toward Mars's orbit by the Sun): its
node and periapsis turn at constant rates, and its mean longitude has a
quadratic term (Phobos's tidal acceleration). The pole's two angles from
Mars's (IAU 2000) pole, and a, e, i, node, periapsis, mean longitude with
their rates, are fitted to positions by least squares.

Usage: python3 tools/moons/build.py RAW_DIR > data/moons.toml
"""

import math
import sys

import numpy as np
from scipy.optimize import least_squares

EPOCH = 2460676.5  # 2025-01-01 00:00 TDB
GM_MARS = 42828.375214  # km^3/s^2, DE440's Mars system
OBLIQUITY = math.radians(84381.448 / 3600.0)
# Mean radii (km) and GMs (km^3/s^2), Archinal et al. 2018 and Jacobson 2010.
MOONS = [("phobos", "Phobos", 11.08, 7.087e-4), ("deimos", "Deimos", 6.27, 9.6e-5)]


def rows(path):
    text = open(path).read().split("$$SOE")[1].split("$$EOE")[0]
    out = []
    for line in text.strip().splitlines():
        f = [x.strip() for x in line.split(",")]
        out.append([float(f[0])] + [float(x) for x in f[2:8]])
    return np.array(out)


def mars_pole(t_centuries):
    """Mars's north pole, IAU 2000 (WGCCRE): ecliptic J2000 unit vector."""
    ra = math.radians(317.68143 - 0.1061 * t_centuries)
    dec = math.radians(52.88650 - 0.0609 * t_centuries)
    v = np.array([math.cos(dec) * math.cos(ra), math.cos(dec) * math.sin(ra), math.sin(dec)])
    c, s = math.cos(OBLIQUITY), math.sin(OBLIQUITY)
    return np.array([v[0], c * v[1] + s * v[2], -s * v[1] + c * v[2]])


def frame(pole):
    """Axes with z along `pole`, x toward its node on the ecliptic."""
    z = pole / np.linalg.norm(pole)
    x = np.cross([0.0, 0.0, 1.0], z)
    x /= np.linalg.norm(x)
    return np.array([x, np.cross(z, x), z])


def tilted(base, tx, ty):
    """`base`'s frame turned so its pole leans by (tx, ty) (radians, small)
    toward its x and y axes."""
    x, y, z = base
    return frame(z + tx * x + ty * y)


def positions(p, days, base):
    """The model's positions (ecliptic, km) at `days` after EPOCH."""
    tx, ty, a, e, inc, node, node_rate, peri, peri_rate, lam, n, ndot = p
    axes = tilted(base, tx, ty)
    om = node + node_rate * days
    w = peri + peri_rate * days - om
    m = lam + n * days + 0.5 * ndot * days**2 - (peri + peri_rate * days)
    ecc = m.copy()
    for _ in range(30):
        ecc -= (ecc - e * np.sin(ecc) - m) / (1.0 - e * np.cos(ecc))
    xp, yp = a * (np.cos(ecc) - e), a * math.sqrt(1.0 - e * e) * np.sin(ecc)
    cw, sw, co, so, ci, si = np.cos(w), np.sin(w), np.cos(om), np.sin(om), math.cos(inc), math.sin(inc)
    px = np.stack([co * cw - so * sw * ci, so * cw + co * sw * ci, sw * si])
    py = np.stack([-co * sw - so * cw * ci, -so * sw + co * cw * ci, cw * si])
    local = px * xp + py * yp
    return (axes.T @ local).T


def guess(states, base):
    """Starting values: osculating elements of the first state, rates from
    the trend of the angles."""
    r = states[:, 1:4] @ base.T
    v = states[:, 4:7] @ base.T
    h = np.cross(r, v)
    inc = math.acos(h[0, 2] / np.linalg.norm(h[0]))
    nodes = np.unwrap(np.arctan2(h[:, 0], -h[:, 1]))
    days = states[:, 0] - EPOCH
    # Daily samples alias the mean motion (Phobos turns ~3.1 times a day):
    # whole turns come from Kepler's law with the mean semi-major axis.
    energy = np.sum(v * v, axis=1) / 2.0 - GM_MARS / np.linalg.norm(r, axis=1)
    a_mean = float(np.mean(-GM_MARS / (2.0 * energy)))
    n_kepler = math.sqrt(GM_MARS / a_mean**3) * 86400.0
    # Day by day, so that the law's error (J2 shifts the mean motion by
    # ~3e-4) is a fraction of a degree, not thousands over the span.
    angle = np.arctan2(r[:, 1], r[:, 0])
    step = np.diff(angle)
    expected = n_kepler * np.diff(days)
    step += 2.0 * math.pi * np.round((expected - step) / (2.0 * math.pi))
    lam = angle[0] + np.concatenate([[0.0], np.cumsum(step)])
    ev = np.cross(v, h) / GM_MARS - r / np.linalg.norm(r, axis=1)[:, None]
    peri = np.unwrap(np.arctan2(ev[:, 1], ev[:, 0]))
    fit = lambda y: np.polyfit(days, y, 1)
    (node_rate, node0), (peri_rate, peri0), (n, lam0) = fit(nodes), fit(peri), fit(lam)
    e = float(np.median(np.linalg.norm(ev, axis=1)))
    a = (GM_MARS / (n / 86400.0) ** 2) ** (1.0 / 3.0)
    return [0.0, 0.0, a, e, inc, node0, node_rate, peri0, peri_rate, lam0, n, 0.0]


def main():
    raw = sys.argv[1]
    base = frame(mars_pole(0.525))  # mid-span, 2052
    print("# Phobos and Deimos as precessing ellipses about Mars, fitted to JPL")
    print("# Horizons over 2025-2080 by tools/moons/build.py. Angles in degrees,")
    print("# rates per day, `a` in km. Mars's frame: z along its IAU 2000 pole (at")
    print("# 2052), x toward the node of its equator on the ecliptic of J2000. Each")
    print("# ellipse lies in a plane precessing about a Laplace pole leaning")
    print("# (tilt_x, tilt_y) from Mars's pole toward x and y; node and periapsis")
    print("# (a longitude, measured from that frame's x axis) turn at constant")
    print("# rates; the mean longitude has a quadratic term. Their pull is left out")
    print("# (Phobos's escape speed is 11 m/s), like the asteroids'.")
    print(f"epoch_jd_tdb = {EPOCH}")
    print(f"mars_pole = [{', '.join(f'{x:.12f}' for x in mars_pole(0.525))}]")
    for key, name, radius, gm in MOONS:
        states = rows(f"{raw}/{key}.txt")
        days = states[:, 0] - EPOCH
        target = states[:, 1:4]
        p0 = guess(states, base)
        residual = lambda p: (positions(p, days, base) - target).ravel()
        lo = [-0.2, -0.2, 0.5 * p0[2], 0.0, 0.0] + [-np.inf] * 7
        hi = [0.2, 0.2, 2.0 * p0[2], 0.2, 0.5] + [np.inf] * 7
        p0[3] = min(max(p0[3], 1e-6), 0.19)
        fit = least_squares(
            residual, p0, bounds=(lo, hi), x_scale="jac", xtol=1e-15, ftol=1e-15, gtol=1e-15, max_nfev=2000
        )
        err = np.linalg.norm(positions(fit.x, days, base) - target, axis=1)
        p = fit.x
        print()
        print("[[moon]]")
        print(f'id = "{key}"')
        print(f'name = "{name}"')
        print(f"radius_km = {radius}")
        print(f"# Fit residual over 2025-2080: rms {np.sqrt(np.mean(err**2)):.2f} km, max {err.max():.2f} km.")
        print(f"tilt_x = {math.degrees(p[0]):.10f}")
        print(f"tilt_y = {math.degrees(p[1]):.10f}")
        print(f"a_km = {p[2]:.6f}")
        print(f"e = {p[3]:.10f}")
        print(f"i = {math.degrees(p[4]):.10f}")
        print(f"node = {math.degrees(p[5]):.10f}")
        print(f"node_rate = {math.degrees(p[6]):.12f}")
        print(f"periapsis = {math.degrees(p[7]):.10f}")
        print(f"periapsis_rate = {math.degrees(p[8]):.12f}")
        print(f"mean_longitude = {math.degrees(p[9]):.10f}")
        print(f"mean_motion = {math.degrees(p[10]):.12f}")
        print(f"mean_motion_rate = {math.degrees(p[11]):.6e}")
        print(f"  {key}: rms {np.sqrt(np.mean(err**2)):.2f} km, max {err.max():.2f} km, {fit.message}", file=sys.stderr)


if __name__ == "__main__":
    main()
