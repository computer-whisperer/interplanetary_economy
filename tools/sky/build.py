"""Builds data/sky/ (M1c.md, step 6 plan, item 2) from what fetch.sh
downloads into DIR:

- stars.bin: the Hipparcos catalog's stars, each a direction (ICRS, at
  J2000), a proper motion, a flux and a color (layout below);
- milkyway.jpg: SVS's Milky Way background, 8192 x 4096, celestial
  coordinates (right ascension 0h at the center, increasing to the left;
  north up), 8-bit sRGB;
- sky.toml: the background's calibration against the stars' fluxes, and
  the sources and their terms.

Units: a flux is in radiance units x sr, where the radiance unit is a
white Lambertian surface's under the Sun at 1 au (E_sun / pi), so a star
of magnitude V has pi x 10^(-0.4 (V - V_sun)), V_sun = -26.74.

The background's scale: SVS's composite is 0.5 x the background plus the
bright-star layer (fitted here), and the bright-star layer's stars
integrate to k x 10^(-0.4 V) (k fitted here on unsaturated, isolated
Hipparcos stars of V 4.5-6.5). So one stored unit of the background is
0.5 x pi / (k x 10^(0.4 x 26.74)) radiance units.

stars.bin, little-endian: b"STARS001", a u32 count, a u32 of zero, then
per star 32 bytes: direction (3 f32, unit, ICRS at J2000), flux (f32),
proper motion (3 f32, radians per Julian year, a tangent vector), color
(u32: linear r, g, b in its low three bytes, the largest 255; the flux is
scaled so that flux x color / 255 is the star's radiance-units x sr per
channel, with the Sun's color white). Sorted brightest first.

Usage: python3 build.py DIR ../../data/sky
"""

import sys

import numpy as np
import OpenEXR
import Imath
from PIL import Image
from scipy.spatial import cKDTree

V_SUN = -26.74
# Hipparcos positions are at J1991.25.
CATALOG_EPOCH = 1991.25
MAS = np.pi / 180 / 3600 / 1000


def exr(path):
    f = OpenEXR.InputFile(path)
    dw = f.header()["dataWindow"]
    w, h = dw.max.x - dw.min.x + 1, dw.max.y - dw.min.y + 1
    half = Imath.PixelType(Imath.PixelType.HALF)
    return np.stack(
        [
            np.frombuffer(f.channel(c, half), dtype=np.float16).reshape(h, w).astype(np.float32)
            for c in "RGB"
        ],
        -1,
    )


def sexagesimal(s, hours):
    sign = -1.0 if s.strip().startswith("-") else 1.0
    a, b, c = (abs(float(x)) for x in s.split())
    v = a + b / 60 + c / 3600
    return sign * v * (15.0 if hours else 1.0)


def hipparcos(path):
    """HIP, V, RA, Dec (degrees, J1991.25), pmRA* and pmDE (mas/yr), B-V."""
    rows = []
    for line in open(path, encoding="latin-1"):
        f = line.split("|")
        if not f[5].strip():
            continue
        try:
            ra, de = float(f[8]), float(f[9])
        except ValueError:
            # A few dozen stars lack the degree fields: their sexagesimal
            # ones, without proper motion.
            ra, de = sexagesimal(f[3], True), sexagesimal(f[4], False)
        pm = [float(x) if x.strip() else 0.0 for x in (f[12], f[13])]
        bv = float(f[37]) if f[37].strip() else np.nan
        rows.append((int(f[1]), float(f[5]), ra, de, pm[0], pm[1], bv))
    return np.array(rows)


def unit(ra, de):
    return np.stack([np.cos(de) * np.cos(ra), np.cos(de) * np.sin(ra), np.sin(de)], -1)


def lobe(lam, mu, s1, s2):
    s = np.where(lam < mu, s1, s2)
    return np.exp(-0.5 * ((lam - mu) / s) ** 2)


def cie(lam):
    """CIE 1931 color matching functions (Wyman, Sloan and Shirley 2013)."""
    x = 1.056 * lobe(lam, 599.8, 37.9, 31.0) + 0.362 * lobe(lam, 442.0, 16.0, 26.7) - 0.065 * lobe(lam, 501.1, 20.4, 26.2)
    y = 0.821 * lobe(lam, 568.8, 46.9, 40.5) + 0.286 * lobe(lam, 530.9, 16.3, 31.1)
    z = 1.217 * lobe(lam, 437.0, 11.8, 36.0) + 0.681 * lobe(lam, 459.0, 26.0, 13.8)
    return np.stack([x, y, z], -1)


XYZ_TO_SRGB = np.array(
    [[3.2406, -1.5372, -0.4986], [-0.9689, 1.8758, 0.0415], [0.0557, -0.2040, 1.0570]]
)
LUMINANCE = np.array([0.2126, 0.7152, 0.0722])


def blackbody_rgb(t):
    """Linear sRGB of blackbodies at temperatures `t`, unnormalized."""
    lam = np.linspace(380.0, 780.0, 401)
    m = lam * 1e-9
    planck = 1.0 / (m[None, :] ** 5 * (np.exp(1.4388e-2 / (m[None, :] * t[:, None])) - 1.0))
    xyz = planck @ cie(lam)
    return xyz @ XYZ_TO_SRGB.T


def temperature(bv):
    """Ballesteros 2012: a star's temperature from its B-V."""
    return 4600.0 * (1.0 / (0.92 * bv + 1.7) + 1.0 / (0.92 * bv + 0.62))


SUN_BV = 0.65


def star_colors(bv):
    """Linear rgb, the Sun's white, each with luminance 1."""
    bv = np.where(np.isnan(bv), SUN_BV, np.clip(bv, -0.4, 2.0))
    rgb = blackbody_rgb(temperature(bv)) / blackbody_rgb(temperature(np.array([SUN_BV])))
    rgb = np.maximum(rgb, 0.0)
    return rgb / (rgb @ LUMINANCE)[:, None]


def pixel_of(ra_deg, de_deg, w, h):
    """SVS's maps: RA 0h at the center, increasing to the left."""
    return (w / 2 - ra_deg / 360 * w) % w, (90 - de_deg) / 180 * h


def calibrate(src, stars):
    """k: the bright-star layer's integral per 10^(-0.4 V); and the
    composite's weights on the background and that layer."""
    ht = exr(f"{src}/hiptyc_2020_4k.exr").mean(-1)
    h, w = ht.shape
    ok = np.abs(stars[:, 3]) < 60
    s = stars[ok]
    x, y = pixel_of(s[:, 2], s[:, 3], w, h)
    tree = cKDTree(np.stack([x * np.cos(np.radians(s[:, 3])), y], -1))
    r = 4
    ratios = []
    for i in np.flatnonzero((s[:, 1] >= 4.5) & (s[:, 1] < 6.5)):
        if any(j != i and s[j, 1] < s[i, 1] + 4 for j in tree.query_ball_point([x[i] * np.cos(np.radians(s[i, 3])), y[i]], 8)):
            continue
        xi, yi = int(x[i]), int(y[i])
        cols = [(xi + d) % w for d in range(-r - 3, r + 4)]
        ring = ht[yi - r - 3 : yi + r + 4][:, cols]
        win = ring[3:-3, 3:-3]
        if win.max() >= 0.95:
            continue
        background = (ring.sum() - win.sum()) / (ring.size - win.size)
        sr = (2 * np.pi / w) * (np.pi / h) * np.cos(np.radians(s[i, 3]))
        ratios.append((win - background).sum() * sr / 10 ** (-0.4 * s[i, 1]))
    ratios = np.array(ratios)
    k = float(np.median(ratios))
    spread = (float(np.percentile(ratios, 25)), float(np.percentile(ratios, 75)))
    composite = exr(f"{src}/starmap_2020_4k.exr").mean(-1)
    mw = exr(f"{src}/milkyway_2020_4k.exr").mean(-1)
    fit = (composite < 0.9) & (mw < 0.9) & (ht < 0.9)
    weights, *_ = np.linalg.lstsq(np.stack([mw[fit], ht[fit]], -1), composite[fit], rcond=None)
    return k, spread, len(ratios), weights


def main(src, out):
    stars = hipparcos(f"{src}/hip_main.dat")
    k, spread, n, weights = calibrate(src, stars)
    print(f"bright-star layer: k = {k:.4e} (IQR {spread[0]:.3e}-{spread[1]:.3e}, {n} stars)")
    print(f"composite = {weights[0]:.4f} x background + {weights[1]:.4f} x bright stars")
    per_unit = weights[0] * np.pi / (k * 10 ** (-0.4 * V_SUN))

    ra, de = np.radians(stars[:, 2]), np.radians(stars[:, 3])
    p = unit(ra, de)
    east = np.stack([-np.sin(ra), np.cos(ra), np.zeros_like(ra)], -1)
    north = np.stack([-np.sin(de) * np.cos(ra), -np.sin(de) * np.sin(ra), np.cos(de)], -1)
    motion = (stars[:, 4:5] * east + stars[:, 5:6] * north) * MAS
    p = p + motion * (2000.0 - CATALOG_EPOCH)
    p /= np.linalg.norm(p, axis=1, keepdims=True)
    flux = np.pi * 10 ** (-0.4 * (stars[:, 1] - V_SUN))
    rgb = star_colors(stars[:, 6])
    top = rgb.max(axis=1)
    color = np.round(rgb / top[:, None] * 255).astype(np.uint32)
    packed = color[:, 0] | (color[:, 1] << 8) | (color[:, 2] << 16)
    order = np.argsort(-flux)
    record = np.zeros(len(stars), dtype=[("p", "<f4", 3), ("flux", "<f4"), ("motion", "<f4", 3), ("color", "<u4")])
    record["p"] = p[order]
    record["flux"] = (flux * top)[order]
    record["motion"] = motion[order]
    record["color"] = packed[order]
    with open(f"{out}/stars.bin", "wb") as f:
        f.write(b"STARS001")
        f.write(np.array([len(stars), 0], dtype="<u4").tobytes())
        f.write(record.tobytes())
    print(f"{len(stars)} stars, V {stars[:, 1].min():.2f} to {stars[:, 1].max():.2f}")

    mw = exr(f"{src}/milkyway_2020_8k.exr")
    encoded = np.where(mw <= 0.0031308, mw * 12.92, 1.055 * np.power(np.clip(mw, 0, 1), 1 / 2.4) - 0.055)
    Image.fromarray(np.round(np.clip(encoded, 0, 1) * 255).astype(np.uint8)).save(
        f"{out}/milkyway.jpg", quality=92, subsampling=0
    )
    with open(f"{out}/sky.toml", "w") as f:
        f.write(
            f"""# The sky (M1c.md, step 6 plan, item 2), built by tools/sky/build.py.
#
# stars.bin: the Hipparcos main catalog (ESA 1997, SP-1200; CDS I/239),
# {len(stars)} stars: V, positions and proper motions (ICRS), B-V. ESA's
# Hipparcos data are free to use with acknowledgement.
#
# milkyway.jpg: "Deep Star Maps 2020", the Milky Way background (Gaia DR2
# with Hipparcos and Tycho-2 stars removed), NASA/Goddard Space Flight
# Center Scientific Visualization Studio (svs.gsfc.nasa.gov/4851). Gaia
# DR2: ESA/Gaia/DPAC. NASA's media guidelines: free to use, credit
# requested. Stars of Tycho-2 fainter than Hipparcos (V ~9-11.5) are in
# neither: a few percent of the background's light.
#
# Calibration: the bright-star layer integrates to k x 10^(-0.4 V) per
# star, k = {k:.4e} (interquartile {spread[0]:.3e}-{spread[1]:.3e}, {n}
# isolated, unsaturated stars of V 4.5-6.5); the composite map is
# {weights[0]:.4f} x the background + {weights[1]:.4f} x the bright-star layer.

# Radiance units (a white Lambertian surface's under the Sun at 1 au) of
# one unit of milkyway.jpg, linear after sRGB decoding.
milky_way_radiance = {per_unit:.6e}
"""
        )
    print(f"background: {per_unit:.4e} radiance units per unit")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
