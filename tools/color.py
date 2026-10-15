"""Color for the tools: spectra to the renderer's linear sRGB, with the
Sun's light white (as the renderer lights the scene: a white surface
under the Sun is white). Imported by sky/build.py (the stars' colors)
and photometry/colors.py (the bodies').
"""

import numpy as np

LUMINANCE = np.array([0.2126, 0.7152, 0.0722])

XYZ_TO_SRGB = np.array(
    [[3.2406, -1.5372, -0.4986], [-0.9689, 1.8758, 0.0415], [0.0557, -0.2040, 1.0570]]
)

# The visible, nm, where spectra are integrated.
NM = np.linspace(380.0, 780.0, 401)

# The Sun's B-V, by which its color is taken.
SUN_BV = 0.65


def lobe(lam, mu, s1, s2):
    s = np.where(lam < mu, s1, s2)
    return np.exp(-0.5 * ((lam - mu) / s) ** 2)


def cie(lam):
    """CIE 1931 color matching functions (Wyman, Sloan and Shirley 2013)."""
    x = 1.056 * lobe(lam, 599.8, 37.9, 31.0) + 0.362 * lobe(lam, 442.0, 16.0, 26.7) - 0.065 * lobe(lam, 501.1, 20.4, 26.2)
    y = 0.821 * lobe(lam, 568.8, 46.9, 40.5) + 0.286 * lobe(lam, 530.9, 16.3, 31.1)
    z = 1.217 * lobe(lam, 437.0, 11.8, 36.0) + 0.681 * lobe(lam, 459.0, 26.0, 13.8)
    return np.stack([x, y, z], -1)


def planck(t):
    """Blackbodies' spectra over NM at temperatures `t`, unnormalized."""
    m = NM * 1e-9
    return 1.0 / (m[None, :] ** 5 * (np.exp(1.4388e-2 / (m[None, :] * t[:, None])) - 1.0))


def blackbody_rgb(t):
    """Linear sRGB of blackbodies at temperatures `t`, unnormalized."""
    return planck(t) @ cie(NM) @ XYZ_TO_SRGB.T


def temperature(bv):
    """Ballesteros 2012: a star's temperature from its B-V."""
    return 4600.0 * (1.0 / (0.92 * bv + 1.7) + 1.0 / (0.92 * bv + 0.62))


def reflected_rgb(points):
    """The linear sRGB a surface reflects under the Sun (white), from its
    reflectance at (nm, reflectance) `points`: linear between them, held
    past the ends; normalized to luminance 1."""
    nm, r = zip(*sorted(points))
    reflectance = np.interp(NM, nm, r)
    sun = planck(np.array([temperature(SUN_BV)]))[0]
    channels = cie(NM) @ XYZ_TO_SRGB.T
    rgb = (sun * reflectance) @ channels / (sun @ channels)
    return rgb / (rgb @ LUMINANCE)
