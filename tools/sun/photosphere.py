"""The Sun's photosphere in the renderer's colors (M1c.md, step 6, part 11
plan): what crates/sandbox/src/sun.rs takes as constants, printed.

The renderer's linear sRGB has the Sun's whole-disk light white
(tools/color.py). Across the disk:

- Limb darkening: Neckel and Labs 1994 (Sol. Phys. 153, 91, Table I),
  I(mu)/I(1) a 5th-order polynomial in mu at each of their continuum
  wavelengths, interpolated in wavelength (of I(mu), not of the
  coefficients, which jump from one wavelength to the next). Their data
  stop at mu ~0.12; the polynomials are extrapolated past it.
- The disk center's spectrum, which weighs each wavelength within a
  channel: Neckel and Labs 1984 (Sol. Phys. 90, 205, Table VII), Planck
  at their center's radiation temperatures, linear between their nodes;
  times the light the absorption lines leave of the continuum, 0.74 at
  400-410 nm, 0.94 at 550-560 and 0.96 at 651-669 (from their Tables
  Xa/Xb), linear between and held past the ends.

Each channel's profile is the light it takes at mu over its disk mean:
fitted by a 5th-order polynomial in mu, weighed by the disk's area (mu
dmu), and scaled to keep the mean exactly 1.

Contrasts on the surface are given as temperatures: a feature at T shows
in each channel at that channel's light from a blackbody at T over that
from the disk center's spectrum (`ratio`), tabulated every 100 K.

- Granulation: the rms temperature fluctuation that gives 18% rms
  contrast in green at disk center, where 3D simulations give 25%, 18%
  and 14% at 450, 555 and 668 nm (Wedemeyer-Bohm and Rouppe van der
  Voort 2009, A&A 503, 225, Table 3; the deconvolved Hinode images
  agree). Taken as Gaussian in temperature.
- Spots: the umbra at 4,000 K (12% of the quiet Sun at 600 nm; Maltby
  et al. 1986's darkest large cores 3,900 K; Borrero and Ichimoto 2011:
  5-25% in the visible), the penumbra at 75% at 600 nm (theirs 65-85%).
  Printed beside them, for comparison, Mathew et al. 2007's (A&A 465,
  291, Table 2) mean umbrae at 677 nm by size, as MDI's 2" pixels blur
  them: far brighter.
- Faculae: their contrast at 617 nm against mu (Yeo et al. 2013, A&A
  550, A95, Eq. 3) at a plage's 300 G, to temperatures. (Its left
  vector is 10^-2 (B/mu)^0, 10^-3 (B/mu)^1, 10^-6 (B/mu)^2, 10^-9
  (B/mu)^3.)

Usage: python3 -I tools/sun/photosphere.py
"""

import os
import sys

import numpy as np

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
from color import LUMINANCE, NM, XYZ_TO_SRGB, cie  # noqa: E402

# Neckel and Labs 1994, Table I: nm, then A0..A5 (I(mu)/I(1) = sum A_k mu^k).
NL94 = np.array(
    [
        [374.086, 0.12579, 0.85402, 0.54601, -1.15048, 0.88928, -0.26462],
        [390.915, 0.12995, 0.91836, -0.07566, 0.19149, -0.28712, 0.12298],
        [401.970, 0.12323, 1.08648, -0.43974, 0.45912, -0.32759, 0.09850],
        [416.319, 0.12814, 1.19947, -0.84407, 1.07201, -0.79537, 0.23982],
        [427.930, 0.14249, 1.28796, -1.19564, 1.68603, -1.36658, 0.44572],
        [443.885, 0.16220, 1.24893, -0.92165, 0.89978, -0.50148, 0.11220],
        [445.125, 0.15248, 1.38517, -1.49615, 1.99886, -1.48155, 0.44119],
        [457.345, 0.16604, 1.38544, -1.52275, 2.00232, -1.45969, 0.42864],
        [477.427, 0.19571, 1.30551, -1.25845, 1.50626, -1.05472, 0.30570],
        [492.905, 0.20924, 1.30798, -1.20411, 1.21505, -0.67196, 0.14381],
        [519.930, 0.23695, 1.29927, -1.28034, 1.37760, -0.85054, 0.21706],
        [541.760, 0.26073, 1.27428, -1.30352, 1.47085, -0.96618, 0.26384],
        [559.950, 0.26892, 1.34319, -1.58427, 1.91271, -1.31350, 0.37295],
        [579.880, 0.28392, 1.36896, -1.75998, 2.22154, -1.56074, 0.44630],
        [610.975, 0.30854, 1.36620, -1.83572, 2.33221, -1.63082, 0.45959],
        [640.970, 0.33644, 1.30590, -1.79238, 2.45040, -1.89979, 0.59943],
        [669.400, 0.34685, 1.37539, -2.04425, 2.70493, -1.94290, 0.55999],
        [700.875, 0.37885, 1.25553, -1.70908, 2.19647, -1.59554, 0.47378],
        [748.710, 0.40627, 1.22842, -1.67877, 2.05535, -1.39972, 0.38845],
        [811.760, 0.42977, 1.25182, -1.85164, 2.31949, -1.59101, 0.44155],
    ]
)

# Neckel and Labs 1984, Table VII: nm, the center's radiation temperature, K.
NL84_CENTER = np.array(
    [
        [378.0, 6410], [390.0, 6465], [400.0, 6490], [415.0, 6490], [430.0, 6480],
        [445.0, 6455], [455.0, 6430], [462.5, 6405], [467.5, 6372], [497.5, 6330],
        [550.0, 6240], [585.0, 6215], [600.0, 6185], [620.0, 6195], [695.0, 6100],
        [750.0, 6085], [760.0, 6060], [850.0, 6060],
    ]
)

# The light the lines leave of the continuum: nm, share.
LINES = np.array([[405.0, 0.74], [555.0, 0.94], [660.0, 0.96]])

MU = np.linspace(0.0, 1.0, 201)

# Each channel's light per unit spectrum over NM.
CHANNELS = cie(NM) @ XYZ_TO_SRGB.T


def planck(nm, t):
    """Blackbody spectral radiance, unnormalized, at `nm` and `t` K."""
    m = nm * 1e-9
    return 1.0 / (m**5 * (np.exp(1.4388e-2 / (m * t)) - 1.0))


def center_spectrum(lines=True):
    """The disk center's spectrum over NM, unnormalized."""
    t = np.interp(NM, NL84_CENTER[:, 0], NL84_CENTER[:, 1])
    s = planck(NM, t)
    if lines:
        s = s * np.interp(NM, LINES[:, 0], LINES[:, 1])
    return s


def darkening():
    """I(lambda, mu)/I(lambda, 1) over NM x MU."""
    at = NL94[:, 1:] @ np.vander(MU, 6, increasing=True).T  # wavelengths x mu
    return np.stack([np.interp(NM, NL94[:, 0], at[:, j]) for j in range(len(MU))], -1)


def profiles(lines=True):
    """Each channel's light at mu over its disk mean: channels x MU."""
    light = (center_spectrum(lines)[:, None] * darkening()).T @ CHANNELS  # MU x 3
    mean = 2.0 * np.trapezoid(light * MU[:, None], MU, axis=0)
    return (light / mean).T


def fit(profile):
    """A 5th-order polynomial in mu, weighed by mu, its disk mean 1."""
    w = np.sqrt(np.maximum(MU, 1e-3))
    a = np.linalg.lstsq(np.vander(MU, 6, increasing=True) * w[:, None], profile * w, rcond=None)[0]
    mean = 2.0 * sum(a[k] / (k + 2) for k in range(6))
    return a / mean


def ratio(t):
    """Each channel's light from a blackbody at `t` K over the disk
    center's (lines left out of both)."""
    return (planck(NM, t) @ CHANNELS) / (center_spectrum(False) @ CHANNELS)


def contrast_rms(dt, rng=np.random.default_rng(1)):
    """Each channel's rms contrast of Gaussian temperature fluctuations of
    rms `dt` about the temperature that matches the center's light in
    green."""
    t0 = brightness_temperature(1.0)
    ts = t0 + dt * rng.standard_normal(20000)
    r = np.array([ratio(t) for t in ts])
    return r.std(0) / r.mean(0)


def brightness_temperature(level, channel=1):
    """The temperature whose `channel` light is `level` times the center's."""
    lo, hi = 1000.0, 20000.0
    for _ in range(60):
        mid = 0.5 * (lo + hi)
        if ratio(mid)[channel] < level:
            lo = mid
        else:
            hi = mid
    return mid


def temperature_at(nm, level):
    """The temperature whose light at `nm` is `level` times the center's."""
    t_center = np.interp(nm, NL84_CENTER[:, 0], NL84_CENTER[:, 1])
    lo, hi = 1000.0, 20000.0
    for _ in range(60):
        mid = 0.5 * (lo + hi)
        if planck(nm, mid) / planck(nm, t_center) < level:
            lo = mid
        else:
            hi = mid
    return mid


def yeo(mu, field):
    """Faculae's contrast at 617 nm at `mu`, where the line-of-sight field
    over mu (for a vertical field, its strength) is `field` G: Yeo et al.
    2013, Eq. 3, fitted over 0.1 <= mu <= 1 and up to 800 G."""
    s = [1e-2, 1e-3, 1e-6, 1e-9]
    m = [
        [-5.11, 7.74, 0.34, -4.72],
        [-0.04, 3.84, -7.42, 3.90],
        [0.19, -6.27, 12.03, -6.78],
        [-0.08, 3.58, -8.04, 5.04],
    ]
    return sum(s[i] * field**i * m[i][j] * mu**j for i in range(4) for j in range(4))


def main():
    with_lines, without = profiles(True), profiles(False)
    print("Limb darkening: each channel's light over its disk mean (with lines; without)")
    for k, mu in [(200, 1.0), (100, 0.5), (40, 0.2), (20, 0.1), (0, 0.0)]:
        print(f"  mu {mu:.1f}: {np.round(with_lines[:, k], 4)}  {np.round(without[:, k], 4)}")
    coeffs = np.array([fit(p) for p in with_lines])
    worst = max(
        abs(np.polyval(coeffs[c][::-1], MU) - with_lines[c]).max() for c in range(3)
    )
    print(f"  fit's worst error {worst:.4f}; center's color {np.round(with_lines[:, 200], 4)}")
    old = np.array([0.60, 0.67, 0.75])
    print(f"  the linear law it replaces, center {np.round(1 / (1 - old / 3), 4)}, mu 0.1 {np.round((1 - old * 0.9) / (1 - old / 3), 4)}")
    print("\npub const LIMB: [[f32; 6]; 3] = [")
    for c in coeffs:
        print("    [" + ", ".join(f"{x:.5f}" for x in c) + "],")
    print("];")

    t0 = brightness_temperature(1.0)
    print(f"\nThe center's green brightness temperature: {t0:.0f} K")
    temps = np.arange(3000, 7501, 100)
    quiet = ratio(t0)
    table = np.array([ratio(t) / quiet for t in temps])
    print(f"// {temps[0]}-{temps[-1]} K every 100 K, over {t0:.0f} K's.")
    print(f"pub const TEMPERATURES: [[f32; 3]; {len(temps)}] = [")
    for row in table:
        print("    [" + ", ".join(f"{x:.5f}" for x in row) + "],")
    print("];")
    for dt in [200, 225, 250, 275]:
        print(f"  granulation rms {dt} K: contrast {np.round(contrast_rms(dt), 4)}")

    print("\nSpots (Mathew et al. 2007, 677 nm), umbral radius R arcsec:")
    for r in [1, 2, 4, 8, 12]:
        umb = 0.8297 * r**-0.3052
        print(f"  R {r}: umbra {umb:.3f} -> {temperature_at(677.0, umb):.0f} K")
    print(f"  penumbra 0.85 -> {temperature_at(677.0, 0.85):.0f} K")

    print("\nFaculae (Yeo et al. 2013, 617 nm), by B/mu:")
    for field in [100, 200, 300, 500]:
        row = []
        for mu in [1.0, 0.8, 0.6, 0.4, 0.3, 0.2, 0.1]:
            c = yeo(mu, field)
            row.append(f"{mu}: {c:+.3f} ({temperature_at(617.0, 1 + c) - temperature_at(617.0, 1.0):+.0f} K)")
        print(f"  {field} G: " + ", ".join(row))
    field = 300.0
    excess = [
        temperature_at(617.0, 1 + yeo(mu, field)) - temperature_at(617.0, 1.0)
        for mu in np.arange(1, 11) / 10
    ]
    print(f"\n// At mu 0.1, 0.2, ... 1.0, at {field:.0f} G.")
    print("pub const FACULAR_EXCESS: [f32; 10] = [" + ", ".join(f"{x:.0f}.0" for x in excess) + "];")
    for t, name in [(4000.0, "umbra"), (temperature_at(600.0, 0.75), "penumbra")]:
        print(f"{name} {t:.0f} K: {np.round(ratio(t) / quiet, 4)}")
    print(f"\nLuminance of the center's color: {with_lines[:, 200] @ LUMINANCE:.4f}")


if __name__ == "__main__":
    main()
