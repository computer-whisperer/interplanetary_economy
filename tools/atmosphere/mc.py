"""Reference radiances for the renderer's atmospheres (M1c.md, step 6,
part 8 plan): a Monte Carlo of a plane-parallel atmosphere over Lambert
ground, its medium the one scene.wgsl draws (Rayleigh and Mie scattering,
Mie and ozone absorption, each with its height profile, to a top), with
no delta-scaling or other shortcut: what the renderer's must come close
to.

Photons enter at the top along the Sun's direction and are followed
through scattering (Rayleigh's phase function, Cornette-Shanks's for Mie)
and Lambert reflection at the ground, absorption taken as weight. At
every scattering and every reflection, a local estimate adds what that
event sends toward the eye, attenuated on the way out. The radiance is in
the renderer's units: a white Lambert surface at normal incidence under
the Sun at 1 au is 1.

Parts: the haze (light scattered toward the eye before it ever met the
ground: what a black ground shows), the ground lit by the Sun's beam, the
ground lit by the sky, and ground light the air sends on toward the eye.

Usage:
  python3 -I mc.py GEOMETRY.csv [PHOTONS] > FIXTURE.csv
      GEOMETRY.csv as `sandbox`'s test `atmospheres` writes it (the
      medium in its header; rows of pixels with their mu0, mu and cosine
      of the phase angle); the fixture adds each channel's parts.
  python3 -I mc.py --check GEOMETRY.csv
      the two checks: first-order scattering against its quadrature, and
      the local estimates against photons escaping into a cone around
      the view, binned.
"""

import sys

import numpy as np

LAYERS = 4000


def cornette_shanks(g, c):
    return 3 / (8 * np.pi) * (1 - g * g) * (1 + c * c) / ((2 + g * g) * (1 + g * g - 2 * g * c) ** 1.5)


def rayleigh(c):
    return 3 / (16 * np.pi) * (1 + c * c)


def sample_cornette_shanks(rng, g, n):
    """Scattering cosines: Henyey-Greenstein's, kept with probability
    (1 + c^2) / 2."""
    out = np.empty(n)
    todo = np.arange(n)
    while len(todo):
        u = rng.random(len(todo))
        c = (1 + g * g - ((1 - g * g) / (1 - g + 2 * g * u)) ** 2) / (2 * g)
        keep = rng.random(len(todo)) < (1 + c * c) / 2
        out[todo[keep]] = c[keep]
        todo = todo[~keep]
    return out


def sample_rayleigh(rng, n):
    out = np.empty(n)
    todo = np.arange(n)
    while len(todo):
        c = rng.random(len(todo)) * 2 - 1
        keep = rng.random(len(todo)) < (1 + c * c) / 2
        out[todo[keep]] = c[keep]
        todo = todo[~keep]
    return out


def turn(d, c, rng):
    """Directions at angle acos(c) from d, uniform in azimuth about it."""
    phi = 2 * np.pi * rng.random(len(c))
    s = np.sqrt(np.maximum(1 - c * c, 0))
    a = np.where(np.abs(d[:, 2:3]) < 0.9, np.array([[0, 0, 1.0]]), np.array([[1.0, 0, 0]]))
    e1 = np.cross(d, a)
    e1 /= np.linalg.norm(e1, axis=1, keepdims=True)
    e2 = np.cross(d, e1)
    return d * c[:, None] + (e1 * np.cos(phi)[:, None] + e2 * np.sin(phi)[:, None]) * s[:, None]


class Medium:
    """One channel's medium, by height: per km, Rayleigh scattering, Mie
    scattering and extinction (absorption included), on a grid from the
    top down, with the optical depth from the top."""

    def __init__(self, p, ch):
        top = p["top"]
        self.z = np.linspace(top, 0, LAYERS + 1)
        z = self.z
        ozone = np.maximum(1 - np.abs(z - p["absorption_center"]) / p["absorption_width"], 0)
        self.rayleigh = p["rayleigh"][ch] * np.exp(-z / p["rayleigh_height"])
        self.mie = p["mie_scattering"][ch] * np.exp(-z / p["mie_height"])
        self.extinction = (
            self.rayleigh
            + self.mie
            + p["mie_absorption"][ch] * np.exp(-z / p["mie_height"])
            + p["absorption"][ch] * ozone
        )
        dz = top / LAYERS
        # Trapezoid in height, the grid fine against the scale heights.
        self.tau = np.concatenate([[0], np.cumsum((self.extinction[1:] + self.extinction[:-1]) / 2 * dz)])
        self.total = self.tau[-1]
        self.g = p["mie_g"]

    def at(self, tau):
        """Rayleigh's and Mie's shares of extinction at optical depth tau."""
        r = np.interp(tau, self.tau, self.rayleigh / self.extinction)
        m = np.interp(tau, self.tau, self.mie / self.extinction)
        return r, m


def frame(mu0, mu, cos_alpha):
    """The Sun's direction and the eye's, in the ground's frame (z up)."""
    s0 = np.sqrt(1 - mu0 * mu0)
    sun = np.array([s0, 0.0, mu0])
    vx = (cos_alpha - mu * mu0) / s0
    view = np.array([vx, np.sqrt(max(0.0, 1 - mu * mu - vx * vx)), mu])
    return sun, view / np.linalg.norm(view)


def lambert(rng, k):
    c = np.sqrt(rng.random(k))
    phi = 2 * np.pi * rng.random(k)
    s = np.sqrt(1 - c * c)
    return np.stack([s * np.cos(phi), s * np.sin(phi), c], axis=1)


def transport(medium, sun, albedo, n, seed, event):
    """Follows n photons from the top; `event(kind, tau, d, w, scattered,
    grounded)` sees each scattering (d the incoming direction, w the
    weight after it) and each reflection (w after it)."""
    rng = np.random.default_rng(seed)
    total = medium.total
    t = np.zeros(n)
    d = np.tile(-sun, (n, 1))
    w = np.ones(n)
    scattered = np.zeros(n, bool)
    grounded = np.zeros(n, bool)
    while len(t):
        t_new = t + rng.exponential(size=len(t)) * -d[:, 2]
        out = t_new < 0
        event("escape", t_new[out], d[out], w[out], scattered[out], grounded[out])
        hit = t_new >= total
        coll = ~out & ~hit
        if hit.any():
            w[hit] *= albedo
            t_new[hit] = total
            event("ground", t_new[hit], d[hit], w[hit], scattered[hit], grounded[hit])
            d[hit] = lambert(rng, hit.sum())
            grounded[hit] = True
        if coll.any():
            idx = np.nonzero(coll)[0]
            r, m = medium.at(t_new[idx])
            gas = rng.random(len(idx)) * (r + m) < r
            w[idx] *= r + m
            event("scatter", t_new[idx], d[idx], w[idx], scattered[idx], grounded[idx], gas)
            c = np.empty(len(idx))
            c[gas] = sample_rayleigh(rng, gas.sum())
            c[~gas] = sample_cornette_shanks(rng, medium.g, (~gas).sum())
            d[idx] = turn(d[idx], c, rng)
            scattered[idx] = True
        t = t_new
        alive = ~out
        # Russian roulette under a thousandth.
        faint = alive & (w < 1e-3)
        lucky = rng.random(len(w)) < 0.1
        w = np.where(faint & lucky, w * 10, w)
        alive &= ~(faint & ~lucky)
        t, d, w, scattered, grounded = t[alive], d[alive], w[alive], scattered[alive], grounded[alive]


def radiance(medium, mu0, mu, cos_alpha, albedo, irradiance, n, seed):
    """The radiance toward the eye, by parts, by local estimates."""
    sun, view = frame(mu0, mu, cos_alpha)
    parts = dict(haze=0.0, ground_direct=0.0, ground_sky=0.0, after_ground=0.0)

    def event(kind, t, d, w, scattered, grounded, gas=None):
        if kind == "ground":
            # Lambert: w (after reflection) / pi per sr, seen through the
            # whole air.
            e = w / np.pi * np.exp(-medium.total / mu)
            parts["ground_direct"] += e[~scattered].sum()
            parts["ground_sky"] += e[scattered].sum()
        elif kind == "scatter":
            c = d @ view
            p = np.where(gas, rayleigh(c), cornette_shanks(medium.g, c))
            # The layer's light toward the eye, per unit area, over mu.
            e = w * p * np.exp(-t / mu) / mu
            parts["haze"] += e[~grounded].sum()
            parts["after_ground"] += e[grounded].sum()

    transport(medium, sun, albedo, n, seed, event)
    # Each photon carries pi E mu0 / n: a white Lambert surface at normal
    # incidence is then E.
    scale = np.pi * irradiance * mu0 / n
    return {k: v * scale for k, v in parts.items()}


def read(path):
    p, rows = {}, []
    for line in open(path):
        line = line.strip()
        if line.startswith("# "):
            key, _, value = line[2:].partition(" = ")
            if value:
                v = [float(x) for x in value.strip("[]").split(",")]
                p[key] = v if len(v) > 1 else v[0]
        elif line and line[0].isdigit():
            rows.append([float(x) for x in line.split(",")])
    return p, rows


def main(path, n):
    p, rows = read(path)
    media = [Medium(p, ch) for ch in range(3)]
    print("# Reference radiances by tools/atmosphere/mc.py, from the geometry the")
    print("# test `atmospheres` wrote; the medium and setup as there:")
    for k, v in p.items():
        print(f"# {k} = {list(v) if isinstance(v, list) else v}")
    print(f"# photons = {n}")
    cols = ["haze", "ground_direct", "ground_sky", "after_ground"]
    print("phase,x,y,mu0,mu,cos_alpha," + ",".join(f"{k}_{c}" for c in "rgb" for k in cols))
    for i, (phase, x, y, mu0, mu, ca) in enumerate(rows):
        out = []
        for ch, medium in enumerate(media):
            parts = radiance(medium, mu0, mu, ca, p["albedo"], p["irradiance"], n, seed=1000 * i + ch)
            out += [parts[k] for k in cols]
        print(f"{phase:g},{x:g},{y:g},{mu0:.9f},{mu:.9f},{ca:.9f}," + ",".join(f"{v:.6g}" for v in out))
        sys.stdout.flush()


def check(path):
    p, rows = read(path)
    medium = Medium(p, 1)
    irradiance = p["irradiance"]
    print("first order against its quadrature (green):")
    for phase, x, y, mu0, mu, ca in rows[::4]:
        sun, view = frame(mu0, mu, ca)
        got = [0.0]

        def first(kind, t, d, w, scattered, grounded, gas=None):
            if kind == "scatter":
                c = d @ view
                pp = np.where(gas, rayleigh(c), cornette_shanks(medium.g, c))
                got[0] += (w * pp * np.exp(-t / mu) / mu)[~scattered & ~grounded].sum()

        n = 1_000_000
        transport(medium, sun, 0.0, n, 7, first)
        got = got[0] * np.pi * irradiance * mu0 / n
        c = -sun @ view
        source = medium.rayleigh * rayleigh(c) + medium.mie * cornette_shanks(medium.g, c)
        f = source * np.exp(-medium.tau / mu0 - medium.tau / mu) / mu
        dz = p["top"] / LAYERS
        want = np.pi * irradiance * np.sum((f[1:] + f[:-1]) / 2 * dz)
        print(f"  mu0 {mu0:.3f} mu {mu:.3f}: {got:.5f} vs {want:.5f}, ratio {got / want:.4f}")
    print("local estimates against binned escapes, 6 degree cone (green):")
    for phase, x, y, mu0, mu, ca in rows[::5]:
        sun, view = frame(mu0, mu, ca)
        local = sum(radiance(medium, mu0, mu, ca, p["albedo"], irradiance, 400_000, 3).values())
        cone = np.cos(np.radians(6))
        caught = [0.0]

        def escapes(kind, t, d, w, scattered, grounded, gas=None):
            if kind == "escape":
                caught[0] += w[d @ view > cone].sum()

        n = 4_000_000
        transport(medium, sun, p["albedo"], n, 11, escapes)
        binned = np.pi * irradiance * mu0 * caught[0] / n / (2 * np.pi * (1 - cone) * mu)
        print(f"  mu0 {mu0:.3f} mu {mu:.3f}: local {local:.5f}, binned {binned:.5f}, ratio {binned / local:.3f}")


if __name__ == "__main__":
    if sys.argv[1] == "--check":
        check(sys.argv[2])
    else:
        main(sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 400_000)
