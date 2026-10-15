"""The bodies' colors at full phase (M1c.md, Realism): what each reflects
of the Sun's light, as the renderer's linear sRGB with the Sun white,
normalized to luminance 1 (their brightness is their phase curves').
Printed as the arms of crates/sandbox/src/photometry.rs's `color`.

Each spectrum is taken as linear between the reflectances measured at
these wavelengths (tools/color.py):

- The other planets: geometric albedos in Johnson-Cousins U, B, V, Rc,
  Ic and Sloan u', g', r', i' (Mallama, Krobusek and Pavlov 2017, Icarus
  282, 19-33, Table 7; arXiv 1609.05048), at the bands' effective
  wavelengths (its Tables 1 and 2). The two systems' colors alone differ
  by up to 0.09 in a channel (printed).
- Earth: its geometric albedos in B (400-500 nm), V (500-600) and R
  (600-700) bands (Robinson 2025, PSJ, arXiv 2507.22258, Sec. 5), at the
  bands' centers: the same work as its phase curve. Mallama et al.'s
  were extrapolated to full phase by a model that swapped forward and
  backward scattering (Robinson's Sec. 6.1).
- The Moon: Lane and Irvine 1973 (AJ 78, 267), Table VIII's narrow
  bands, past 600 nm x 0.85: their 1965 data, tabulated, run 10-20% over
  their 1964 data and other studies there (their Sec. III).
- Phobos and Deimos: the D class's mean spectrum (DeMeo et al. 2009, PDS
  EAR-A-VARGBDET-5-BUSDEMEOTAX-V1.0, meanspectra.tab), which theirs are
  likened to (Fornasier et al. 2024, A&A 686, A203). Their own slopes
  run redder than the class mean's ~9%/100 nm (Phobos's red unit 11.9%
  over 550-800 nm, its blue unit about half that; Deimos's 10.8%): a
  slope of 11% would move their color by up to 0.02 in a channel.
- Small bodies: their class's mean spectrum, the same; below 450 nm, its
  450-500 nm slope extended. A small body of unknown class takes the
  Sun's color.

Usage: python3 tools/photometry/colors.py
"""

import os
import sys

import numpy as np

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
from color import reflected_rgb  # noqa: E402

# Effective wavelengths, nm.
JOHNSON_COUSINS = [360, 436, 549, 641, 798]  # U B V Rc Ic
SLOAN = [355, 469, 616, 748]  # u' g' r' i'

# Mallama et al. 2017, Table 7: U B V Rc Ic, then u' g' r' i'.
PLANETS = {
    "mercury": [0.087, 0.105, 0.142, 0.158, 0.180, 0.095, 0.130, 0.169, 0.200],
    "venus": [0.348, 0.658, 0.689, 0.658, 0.640, 0.326, 0.664, 0.712, 0.708],
    "mars": [0.060, 0.088, 0.170, 0.250, 0.285, 0.061, 0.111, 0.245, 0.298],
    "jupiter": [0.358, 0.443, 0.538, 0.513, 0.389, 0.377, 0.509, 0.575, 0.456],
    "saturn": [0.203, 0.339, 0.499, 0.646, 0.543, 0.209, 0.436, 0.618, 0.601],
    "uranus": [0.502, 0.561, 0.488, 0.264, 0.089, 0.541, 0.559, 0.355, 0.120],
    "neptune": [0.578, 0.562, 0.442, 0.226, 0.072, 0.564, 0.533, 0.307, 0.101],
}

# Robinson 2025: (nm, geometric albedo).
EARTH = [(450.0, 0.277), (550.0, 0.226), (650.0, 0.221)]

# Lane and Irvine 1973, Table VIII: (nm, geometric albedo).
MOON = [(359.0, 0.072), (392.6, 0.085), (415.5, 0.088), (457.3, 0.098), (501.2, 0.106),
        (626.4, 0.162), (729.7, 0.179)]
MOON_RED = 0.85

# DeMeo et al. 2009's class means, 450 to 900 nm by 50, normalized at 550.
CLASS_NM = list(range(450, 901, 50))
CLASSES = {
    "B": [0.978, 0.989, 1.000, 1.004, 1.002, 0.995, 0.983, 0.970, 0.955, 0.940],
    "C": [0.965, 0.985, 1.000, 1.007, 1.012, 1.014, 1.014, 1.012, 1.009, 1.005],
    "Cb": [1.009, 1.004, 1.000, 0.998, 1.002, 1.007, 1.016, 1.020, 1.022, 1.024],
    "D": [0.904, 0.953, 1.000, 1.042, 1.086, 1.129, 1.169, 1.197, 1.234, 1.279],
    "S": [0.839, 0.922, 1.000, 1.063, 1.123, 1.173, 1.193, 1.162, 1.110, 1.079],
    "Sq": [0.846, 0.928, 1.000, 1.057, 1.109, 1.148, 1.152, 1.106, 1.048, 1.008],
    "Xe": [0.912, 0.948, 1.010, 1.052, 1.084, 1.107, 1.116, 1.116, 1.115, 1.121],
}


def class_rgb(r):
    slope = (r[1] - r[0]) / (CLASS_NM[1] - CLASS_NM[0])
    return reflected_rgb([(380.0, r[0] - slope * 70.0)] + list(zip(CLASS_NM, r)))


def rust(name, rgb, note):
    return f'        {name} => [{rgb[0]:.3f}, {rgb[1]:.3f}, {rgb[2]:.3f}], // {note}'


def main():
    lines = []
    for id, a in PLANETS.items():
        jc = list(zip(JOHNSON_COUSINS, a[:5]))
        sloan = list(zip(SLOAN, a[5:]))
        both = reflected_rgb(jc + sloan)
        spread = np.abs(reflected_rgb(jc) - reflected_rgb(sloan)).max()
        lines.append(rust(f'"{id}"', both, f"systems differ by {spread:.2f}"))
    lines.insert(2, rust('"earth"', reflected_rgb(EARTH), "Robinson"))
    moon = [(nm, p * (MOON_RED if nm > 600 else 1.0)) for nm, p in MOON]
    lines.append(rust('"moon"', reflected_rgb(moon), "Lane and Irvine"))
    lines.append(rust('"phobos" | "deimos"', class_rgb(CLASSES["D"]), "class D"))
    for c, r in CLASSES.items():
        lines.append(rust(f'"{c}"', class_rgb(r), f"class {c}"))
    print("\n".join(lines))


if __name__ == "__main__":
    main()
