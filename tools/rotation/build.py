"""Builds data/rotation.toml (M1c.md, step 6 plan, item 4) from NAIF's
pck00011.tpc, which fetch.sh downloads into DIR: the IAU WGCCRE 2015
rotation models (Archinal et al. 2018; the Moon's and Earth's from the
2009 report, which 2015 left unchanged), for the bodies the game draws.

Each body: the pole's right ascension and declination (ICRS, degrees, a
polynomial in Julian centuries T from J2000 TDB), the prime meridian
(degrees, a polynomial in days d), and the periodic terms on each (sines
on the right ascension and the meridian, cosines on the declination) of
its system's angles (degrees, polynomials in T). As SPICE evaluates them:

  ra  = ra[0] + ra[1] T + ra[2] T^2 + sum ra_terms[i] sin(angle[i])
  dec = dec[0] + dec[1] T + dec[2] T^2 + sum dec_terms[i] cos(angle[i])
  w   = pm[0] + pm[1] d + pm[2] d^2 + sum pm_terms[i] sin(angle[i])

Usage: python3 build.py DIR > ../../data/rotation.toml
"""

import re
import sys

# The game's bodies (data ids) by NAIF id.
BODIES = {
    10: "sun",
    199: "mercury",
    299: "venus",
    399: "earth",
    301: "moon",
    499: "mars",
    401: "phobos",
    402: "deimos",
    599: "jupiter",
    699: "saturn",
    799: "uranus",
    899: "neptune",
}


def data_blocks(text):
    """The assignments between \\begindata and \\begintext, as name ->
    list of numbers."""
    values = {}
    for block in re.findall(r"\\begindata(.*?)(?:\\begintext|$)", text, re.S):
        for name, body in re.findall(r"(\w+)\s*=\s*\(([^)]*)\)", block):
            values[name.upper()] = [float(x.replace("D", "E")) for x in body.split()]
        for name, value in re.findall(r"(\w+)\s*=\s*([-+0-9.ED]+)\s*$", block, re.M):
            values[name.upper()] = [float(value.replace("D", "E"))]
    return values


def fmt(xs):
    return "[" + ", ".join(repr(x) for x in xs) + "]"


def main(src):
    values = data_blocks(open(f"{src}/pck00011.tpc", encoding="latin-1").read())
    systems = {}
    print("# Rotation models (M1c.md, step 6 plan, item 4), built by")
    print("# tools/rotation/build.py from NAIF's pck00011.tpc: the IAU WGCCRE 2015")
    print("# models (Archinal et al. 2018; Earth's and the Moon's from the 2009")
    print("# report). See build.py for how the terms combine.")
    for naif, name in BODIES.items():
        key = f"BODY{naif}"
        print()
        print(f"[body.{name}]")
        print(f"naif = {naif}")
        for field, label in (("POLE_RA", "ra"), ("POLE_DEC", "dec"), ("PM", "pm")):
            print(f"{label} = {fmt(values[f'{key}_{field}'])}")
        terms = {
            label: values.get(f"{key}_NUT_PREC_{field}", [])
            for field, label in (("RA", "ra_terms"), ("DEC", "dec_terms"), ("PM", "pm_terms"))
        }
        if any(any(t) for t in terms.values()):
            system = naif // 100 if naif >= 100 else naif
            systems[system] = True
            print(f"system = {system}")
            for label, t in terms.items():
                print(f"{label} = {fmt(t)}")
    for system in systems:
        degree = int(values.get(f"BODY{system}_MAX_PHASE_DEGREE", [1])[0])
        flat = values[f"BODY{system}_NUT_PREC_ANGLES"]
        per = degree + 1
        angles = [flat[i : i + per] + [0.0] * (2 - degree) for i in range(0, len(flat), per)]
        print()
        print(f"[system.{system}]")
        print("# Each angle: degrees, per century, per century squared.")
        print("angles = [")
        for a in angles:
            print(f"  {fmt(a)},")
        print("]")


if __name__ == "__main__":
    main(sys.argv[1])
