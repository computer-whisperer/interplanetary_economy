"""Builds data/bodies/saturn/rings.toml (M1c.md, step 6 plan, item 6)
from what fetch.sh downloads into DIR: the rings' normal optical depth
every 10 km from 74,000 to 141,000 km (the C ring's inner edge to past
the F ring), from Cassini UVIS's Gamma Crucis occultation. Depths below 0
(noise in gaps) are taken as 0, and above what the occultation could
detect (the B ring's core) as that limit.

Also the ring particles' albedo by region, estimated: the C ring and the
Cassini Division darker and grayer, the A and B rings brighter and tan
(Cuzzi et al. 2009's reflectances, roughly).

Usage: python3 build.py DIR > ../../data/bodies/saturn/rings.toml
"""

import sys

import numpy as np

INNER, OUTER, STEP = 74_000.0, 141_000.0, 10.0

# Regions' outer radii (km) and particle albedos (red, green, blue).
REGIONS = [
    ("C ring", 91_983.0, [0.24, 0.22, 0.20]),
    ("B ring", 117_516.0, [0.55, 0.47, 0.37]),
    ("Cassini Division", 122_053.0, [0.28, 0.26, 0.24]),
    ("A ring", 136_774.0, [0.50, 0.43, 0.35]),
    ("F ring and beyond", OUTER, [0.55, 0.53, 0.50]),
]


def main(src):
    rows = np.genfromtxt(f"{src}/gamcru.TAB", delimiter=",")
    radius, tau, limit = rows[:, 0], rows[:, 4], rows[:, 5]
    grid = np.arange(INNER, OUTER, STEP)
    # The bins drift from an exact 10 km grid (steps of 9.98-10.02 km).
    missing = np.interp(grid, radius, ((tau < -0.5) | (limit < 0)).astype(float)) > 0
    t = np.interp(grid, radius, tau)
    cap = np.interp(grid, radius, limit)
    t = np.where(missing, cap.clip(0), np.clip(t, 0, cap))
    print("# Saturn's rings (M1c.md, step 6 plan, item 6), built by tools/rings/build.py:")
    print("# normal optical depth every `step_km` from `inner_km`, from Cassini UVIS's")
    print("# occultation of Gamma Crucis (2008-238, ingress; PDS Ring-Moon Systems")
    print("# Node, COUVIS_8001; NASA, public domain), negative depths taken as 0 and")
    print("# those past detection as the limit; and the ring particles' albedo by")
    print("# region, estimated (Cuzzi et al. 2009, roughly).")
    print(f"inner_km = {INNER}")
    print(f"step_km = {STEP}")
    print(f"# {len(t)} values; {int(missing.sum())} missing, taken as the limit.")
    print("tau = [")
    for i in range(0, len(t), 12):
        print("  " + ", ".join(f"{x:.3f}" for x in t[i : i + 12]) + ",")
    print("]")
    for name, outer, albedo in REGIONS:
        print()
        print("[[region]]")
        print(f"# {name}")
        print(f"outer_km = {outer}")
        print(f"albedo = {albedo}")
    print(
        f"# Optical depth by region (mean): "
        + ", ".join(
            f"{name} {t[(grid >= lo) & (grid < hi)].mean():.2f}"
            for (name, hi, _), lo in zip(REGIONS, [INNER] + [r[1] for r in REGIONS[:-1]])
        ),
        file=sys.stderr,
    )


if __name__ == "__main__":
    main(sys.argv[1])
