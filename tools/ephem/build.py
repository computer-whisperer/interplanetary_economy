"""Builds data/de440.bin from JPL's DE440 (see fetch.sh).

Usage: python3 build.py DIR OUT

Keeps the Chebyshev series of the Sun, the planetary system barycenters
(Mercury to Neptune), the Earth-Moon barycenter and the Moon (relative to
the Earth-Moon barycenter), for 2025-01-01 to 2080-01-01 TDB, rotated from
the ICRF equator to the ecliptic of J2000 (obliquity 84381.448", as JPL
Horizons uses). The coefficients are DE440's own, not refitted.

Output, little-endian:
  b"DE440ECL", u32 version (1)
  u32 n; n x (16-byte NUL-padded name, f64 value): constants. GMs in km^3/s^2
      (GMS, GM1, GM2, GMB, GM4..GM8), EMRAT, AU in km, RE in km, J2E.
  u32 n; n x series:
      i32 target, i32 center (NAIF ids), f64 init (TDB seconds past J2000),
      f64 intlen (s), u32 ncoef, u32 nrec,
      nrec x [x coefficients, y coefficients, z coefficients] (f64, km).
      Record k covers init + k*intlen to init + (k+1)*intlen.
"""

import struct
import sys
from datetime import date

import numpy as np

TARGETS = [(10, 0), (1, 0), (2, 0), (3, 0), (4, 0), (5, 0), (6, 0), (7, 0), (8, 0), (301, 3)]
CONSTANTS = ["GMS", "GM1", "GM2", "GMB", "GM4", "GM5", "GM6", "GM7", "GM8", "EMRAT", "AU", "RE", "J2E"]
DAY = 86400.0
J2000_JD = 2451545.0
# 00:00 TDB on 2025-01-01 and 2080-01-01.
START_JD = (date(2025, 1, 1) - date(2000, 1, 1)).days + 2451544.5
END_JD = (date(2080, 1, 1) - date(2000, 1, 1)).days + 2451544.5
OBLIQUITY = np.radians(84381.448 / 3600.0)


def header_constants(path):
    """Group 1040 (names) and 1041 (values) of a DE ASCII header."""
    words = open(path).read().split()
    g40 = words.index("1040", words.index("GROUP"))
    n = int(words[g40 + 1])
    names = words[g40 + 2 : g40 + 2 + n]
    g41 = words.index("1041")
    values = [float(w.replace("D", "E")) for w in words[g41 + 2 : g41 + 2 + n]]
    assert int(words[g41 + 1]) == n
    return dict(zip(names, values))


def spk_segments(path):
    """(target, center, frame, type, words) for every SPK segment."""
    data = open(path, "rb").read()
    assert data[:8] == b"DAF/SPK ", data[:8]
    assert data[88:96] == b"LTL-IEEE", data[88:96]
    nd, ni = struct.unpack_from("<ii", data, 8)
    assert (nd, ni) == (2, 6)
    fward = struct.unpack_from("<i", data, 76)[0]
    words = np.frombuffer(data, dtype="<f8")
    segments = []
    record = fward
    while record:
        base = (record - 1) * 128
        nxt, _prev, nsum = words[base : base + 3]
        for k in range(int(nsum)):
            off = (base + 3 + k * 5) * 8
            _t0, _t1 = struct.unpack_from("<dd", data, off)
            target, center, frame, kind, begin, end = struct.unpack_from("<6i", data, off + 16)
            segments.append((target, center, frame, kind, words[begin - 1 : end]))
        record = int(nxt)
    return segments


def series(seg_words):
    """A type 2 segment trimmed to [START_JD, END_JD], rotated to the ecliptic."""
    init, intlen, rsize, n = seg_words[-4:]
    rsize, n = int(rsize), int(n)
    ncoef = (rsize - 2) // 3
    records = seg_words[: rsize * n].reshape(n, rsize)
    # Uniform records: each midpoint and radius follow from init and intlen.
    mids = init + (np.arange(n) + 0.5) * intlen
    assert np.allclose(records[:, 0], mids, rtol=0, atol=1e-3), "non-uniform records"
    assert np.allclose(records[:, 1], intlen / 2, rtol=0, atol=1e-6)
    t0, t1 = (START_JD - J2000_JD) * DAY, (END_JD - J2000_JD) * DAY
    first = int(np.floor((t0 - init) / intlen))
    last = int(np.ceil((t1 - init) / intlen))
    assert 0 <= first and last <= n, "segment does not cover the range"
    coef = records[first:last, 2:].reshape(-1, 3, ncoef)
    c, s = np.cos(OBLIQUITY), np.sin(OBLIQUITY)
    x, y, z = coef[:, 0], coef[:, 1], coef[:, 2]
    ecl = np.stack([x, c * y + s * z, -s * y + c * z], axis=1)
    return init + first * intlen, intlen, ncoef, last - first, ecl


def main():
    src, out = sys.argv[1], sys.argv[2]
    k = header_constants(f"{src}/header.440")
    au = k["AU"]
    gm = au**3 / DAY**2  # au^3/day^2 -> km^3/s^2
    consts = [(name, k[name] * gm if name.startswith("GM") else k[name]) for name in CONSTANTS]

    segs = spk_segments(f"{src}/de440s.bsp")
    chosen = []
    for target, center in TARGETS:
        found = [s for s in segs if s[0] == target and s[1] == center]
        assert len(found) == 1, (target, center, len(found))
        _, _, frame, kind, words = found[0]
        assert frame == 1 and kind == 2, (target, frame, kind)
        chosen.append((target, center) + series(words))

    with open(out, "wb") as f:
        f.write(b"DE440ECL" + struct.pack("<I", 1))
        f.write(struct.pack("<I", len(consts)))
        for name, value in consts:
            f.write(name.encode().ljust(16, b"\0") + struct.pack("<d", value))
        f.write(struct.pack("<I", len(chosen)))
        for target, center, init, intlen, ncoef, nrec, coef in chosen:
            f.write(struct.pack("<iiddII", target, center, init, intlen, ncoef, nrec))
            f.write(np.ascontiguousarray(coef, dtype="<f8").tobytes())
    for target, center, init, intlen, ncoef, nrec, _ in chosen:
        print(f"{target:4} wrt {center}: {nrec} records of {intlen / DAY:g} d, {ncoef} coefficients", file=sys.stderr)


if __name__ == "__main__":
    main()
