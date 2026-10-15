"""Builds the shaped bodies' radius maps (M1c.md, step 6, part 9 plan,
item 3) from the models fetch.sh downloads into DIR, and two made up:
data/bodies/ID/shape.bin and data/bodies/shapes.toml.

shape.bin is render::shape::Shape's layout: each body's radius, km, by
direction from its model's origin in its frame, on a cube map's six
faces in wgpu's order (+x, -x, +y, -y, +z, -z) and orientation, each
SIZE texels across within its edges and bordered by SIZE / 16 past them
(sampled there, in the face's own coordinates); little-endian f32, face
by face, row by row from the top.

Each texel's radius is where the ray from the origin through its center
meets the model: on each face's plane, by projection from the origin
(which keeps straight lines straight), a facet is a triangle and the
texels whose directions meet it lie within it. A ray meeting the surface
more than once (the model not star-shaped from its origin) takes the
outermost; such texels are counted and printed.

With each, `shadowed`: the share of its light (lunar-Lambert, as the
sandbox shades an airless body) its self-shadowing and the parts of it
hiding others leave, by phase angle 0-180 degrees every 10, averaged over
orientations: for each, 64 random Suns and eyes that far apart, over its
map's facets at 16 squares a face (as render::shape's), each facet's Sun
and eye marched for through the map (photometry::calibrate divides it
out, so that the disk averages to its phase curve).

Made up, as nothing usable is published (M1c.md): 1996 FG3 (an oblate
primary with an equatorial ridge, as binary primaries are; ~1.4 : 1.4 :
1.2) and 2009 UY19 (elongated, 1.4 : 1.1 : 1), each an ellipsoid with
seeded bumps and hollows, scaled to the volume of the SBDB's diameter.

Needs numpy. Usage: python3 -I build.py DIR ../../data/bodies
"""

import math
import os
import sys

import numpy as np

SIZE = 128
BORDER = SIZE // 16
SIDE = SIZE + 2 * BORDER

# Each face's axis, and the directions u (right) and v (down) run along
# it: render::shape::face_direction's d = axis + u U + v V.
FACES = [
    ((1, 0, 0), (0, 0, -1), (0, -1, 0)),
    ((-1, 0, 0), (0, 0, 1), (0, -1, 0)),
    ((0, 1, 0), (1, 0, 0), (0, 0, 1)),
    ((0, -1, 0), (1, 0, 0), (0, 0, -1)),
    ((0, 0, 1), (1, 0, 0), (0, -1, 0)),
    ((0, 0, -1), (-1, 0, 0), (0, -1, 0)),
]

# Where each source's terms are recorded (shapes.toml's comments).
SOURCES = {
    "phobos": (
        "phobos.tab",
        "Gaskell 2006, from Viking and Phobos 2 images (PDS SBN,\n"
        "# gaskell.phobos.shape-model, phobos_ver64q): 49,152 plates. Its frame\n"
        "# pck00006's IAU Phobos frame, +x toward Mars. Public domain.",
    ),
    "deimos": (
        "deimos.tab",
        "Thomas 1993's radius grid, every 5 degrees (PDS SBN,\n"
        "# ast-sat.thomas.shape-models, m2deimos): planetocentric latitude, west longitude\n"
        "# (against Ernst et al. 2023's model, 364 m rms off read so, 640 m at best read\n"
        "# east); ~400 m uncertain at longitudes 200-355. Public domain.",
    ),
    "ryugu": (
        "ryugu.obj",
        "Watanabe et al. 2019 (Science 364, 268), structure from\n"
        "# motion, 49,152 facets (JAXA DARTS, SHAPE_SFM_49k_v20180804). Credit: ISAS/JAXA;\n"
        "# modified here: resampled as radii. JAXA's terms allow commercial use with credit.",
    ),
    "bennu": (
        "bennu.obj",
        "OSIRIS-REx's SPOv54, stereophotoclinometry and lidar, 49,152\n"
        "# facets (PDS SBN, orex.altimetry, g_06320mm_spo_obj_0000n00000_v054; Barnouin,\n"
        "# Daly, Espiritu and the OSIRIS-REx Altimetry Working Group). Public domain.",
    ),
    "itokawa": (
        "itokawa.tab",
        "Gaskell et al. 2008, from Hayabusa AMICA images (PDS SBN,\n"
        "# gaskell.ast-itokawa.shape-model, ver64q): 49,152 plates; +x through the\n"
        "# black rock. Public domain.",
    ),
    "didymos": (
        "didymos.obj",
        "DART's v003 (PDS SBN, dart_shapemodel, didymos_g_9309mm_spc;\n"
        "# Palmer et al.), from approach images: its far side poorly constrained. 49,152\n"
        "# facets. Public domain.",
    ),
    "apophis": (
        "apophis.obj",
        "Brozovic et al. 2018 (Icarus 300, 115), radar, 3,996 facets\n"
        "# (PDS SBN, gbo.ast-apophis.jpl.radar.shape_model, apophis_v233s7, \"preliminary\"):\n"
        "# its principal axes, z the short one. Public domain.",
    ),
    "2008-ev5": (
        "2008-ev5.obj",
        "Busch et al. 2011 (Icarus 212, 649), radar, 3,996 facets\n"
        "# (PDS SBN, gbo.ast.jpl.radar.shape_models, compiled by Lawrence and Benner).\n"
        "# Public domain.",
    ),
    "nereus": (
        "nereus.obj",
        "Brozovic et al. 2009 (Icarus 201, 153), radar, 2,292 facets\n"
        "# (PDS SBN, gbo.ast.jpl.radar.shape_models). Public domain.",
    ),
}

# Made up: (SBDB diameter km, semi-axes' ratios, ridge, seed).
MADE_UP = {
    "1996-fg3": (1.196, (1.0, 1.0, 0.86), 0.06, 175706),
    "2009-uy19": (0.105, (1.4, 1.1, 1.0), 0.0, 2009),
}


def read_tab(path):
    """Gaskell's vertex-plate table: counts, numbered vertices (km), then
    numbered plates of three one-based vertices."""
    with open(path) as f:
        nv, nf = (int(x) for x in f.readline().split()[:2])
        rows = [f.readline().split() for _ in range(nv + nf)]
    vertices = np.array([[float(x) for x in r[1:4]] for r in rows[:nv]])
    faces = np.array([[int(x) - 1 for x in r[1:4]] for r in rows[nv:]])
    return vertices, faces


def read_obj(path):
    vertices, faces = [], []
    with open(path) as f:
        for line in f:
            p = line.split()
            if p and p[0] == "v":
                vertices.append([float(x) for x in p[1:4]])
            elif p and p[0] == "f":
                faces.append([int(x.split("/")[0]) - 1 for x in p[1:4]])
    return np.array(vertices), np.array(faces)


def read_grid(path, east):
    """Thomas's radius grid (latitude, longitude east or west, radius km;
    both 0 and 360 given), joined two triangles a cell."""
    rows = np.loadtxt(path)
    lats = np.unique(rows[:, 0])
    lons = np.unique(rows[:, 1])
    r = {(a, b): c for a, b, c in rows}
    sign = 1.0 if east else -1.0
    vertices, index = [], {}
    for a in lats:
        for b in lons:
            index[(a, b)] = len(vertices)
            la, lo = math.radians(a), sign * math.radians(b)
            vertices.append([r[(a, b)] * math.cos(la) * math.cos(lo),
                             r[(a, b)] * math.cos(la) * math.sin(lo),
                             r[(a, b)] * math.sin(la)])
    faces = []
    for i in range(len(lats) - 1):
        for j in range(len(lons) - 1):
            a, b = index[(lats[i], lons[j])], index[(lats[i], lons[j + 1])]
            c, d = index[(lats[i + 1], lons[j + 1])], index[(lats[i + 1], lons[j])]
            faces += [[a, b, c], [a, c, d]]
    return np.array(vertices), np.array(faces)


def texel_directions():
    """Each face's texel centers' directions (unit): (6, SIDE, SIDE, 3)."""
    c = 2.0 * (np.arange(SIDE) - BORDER + 0.5) / SIZE - 1.0
    u, v = np.meshgrid(c, c)
    out = []
    for axis, U, V in FACES:
        d = np.array(axis)[None, None, :] + u[..., None] * np.array(U) + v[..., None] * np.array(V)
        out.append(d / np.linalg.norm(d, axis=-1, keepdims=True))
    return np.array(out)


def cast(vertices, faces):
    """The model's radius at each texel's direction (outermost), and how
    many texels meet it more than once."""
    tri = vertices[faces]
    normal = np.cross(tri[:, 1] - tri[:, 0], tri[:, 2] - tri[:, 0])
    keep = np.linalg.norm(normal, axis=1) > 1e-15
    tri, normal = tri[keep], normal[keep]
    plane = np.einsum("ij,ij->i", normal, tri[:, 0])
    dirs = texel_directions()
    outer = np.full((6, SIDE, SIDE), -np.inf)
    inner = np.full((6, SIDE, SIDE), np.inf)
    eps = 1e-9
    for f, (axis, U, V) in enumerate(FACES):
        along = tri @ np.array(axis, float)
        ok = (along > 1e-12).all(axis=1)
        t, n, p = tri[ok], normal[ok], plane[ok]
        a = along[ok]
        x = (t @ np.array(U, float) / a + 1.0) / 2.0 * SIZE + BORDER - 0.5
        y = (t @ np.array(V, float) / a + 1.0) / 2.0 * SIZE + BORDER - 0.5
        x0 = np.clip(np.ceil(x.min(1) - eps), 0, SIDE - 1).astype(int)
        x1 = np.clip(np.floor(x.max(1) + eps), 0, SIDE - 1).astype(int)
        y0 = np.clip(np.ceil(y.min(1) - eps), 0, SIDE - 1).astype(int)
        y1 = np.clip(np.floor(y.max(1) + eps), 0, SIDE - 1).astype(int)
        w, h = x1 - x0 + 1, y1 - y0 + 1
        # The triangle's edges, for barycentric tests at texel centers.
        det = (x[:, 1] - x[:, 0]) * (y[:, 2] - y[:, 0]) - (x[:, 2] - x[:, 0]) * (y[:, 1] - y[:, 0])
        for dx in range(int(w.max(initial=0))):
            for dy in range(int(h.max(initial=0))):
                sel = (dx < w) & (dy < h) & (np.abs(det) > 1e-15)
                if not sel.any():
                    continue
                i, j = x0[sel] + dx, y0[sel] + dy
                xs, ys, dd = x[sel], y[sel], det[sel]
                l1 = ((i - xs[:, 0]) * (ys[:, 2] - ys[:, 0]) - (xs[:, 2] - xs[:, 0]) * (j - ys[:, 0])) / dd
                l2 = ((xs[:, 1] - xs[:, 0]) * (j - ys[:, 0]) - (i - xs[:, 0]) * (ys[:, 1] - ys[:, 0])) / dd
                inside = (l1 >= -1e-7) & (l2 >= -1e-7) & (l1 + l2 <= 1 + 1e-7)
                if not inside.any():
                    continue
                i, j = i[inside], j[inside]
                d = dirs[f, j, i]
                nn, pp = n[sel][inside], p[sel][inside]
                r = pp / np.einsum("ij,ij->i", nn, d)
                np.maximum.at(outer[f], (j, i), r)
                np.minimum.at(inner[f], (j, i), r)
    if not np.isfinite(outer).all():
        raise ValueError(f"{int((~np.isfinite(outer)).sum())} texels meet no facet")
    twice = int((outer - inner > 1e-4 * outer).sum())
    return outer, twice


def model_measures(vertices, faces):
    """The model's area (km²) and volume (km³) from its facets."""
    tri = vertices[faces]
    area = 0.5 * np.linalg.norm(np.cross(tri[:, 1] - tri[:, 0], tri[:, 2] - tri[:, 0]), axis=1).sum()
    volume = abs(np.einsum("ij,ij->i", tri[:, 0], np.cross(tri[:, 1], tri[:, 2])).sum()) / 6.0
    return area, volume


def made_up(diameter, ratios, ridge, seed):
    """An ellipsoid of `ratios` with an equatorial `ridge` and seeded
    bumps and hollows of a few sizes, as radii at the texels' directions,
    scaled to a sphere of `diameter`'s volume."""
    rng = np.random.default_rng(seed)
    d = texel_directions()
    a, b, c = ratios
    r = 1.0 / np.sqrt((d[..., 0] / a) ** 2 + (d[..., 1] / b) ** 2 + (d[..., 2] / c) ** 2)
    r *= 1.0 + ridge * np.exp(-(d[..., 2] / 0.12) ** 2)
    for count, width, height in [(6, 0.6, 0.06), (24, 0.25, 0.025), (80, 0.1, 0.01)]:
        for _ in range(count):
            centre = rng.normal(size=3)
            centre /= np.linalg.norm(centre)
            k = rng.uniform(-1.0, 1.0) * height
            r *= 1.0 + k * np.exp(-(1.0 - d @ centre) / (width * width / 2.0))
    # Its volume, from the faces within their edges: the integral of r³/3
    # over directions, each texel's solid angle on its face.
    inner = slice(BORDER, BORDER + SIZE)
    cu = 2.0 * (np.arange(SIZE) + 0.5) / SIZE - 1.0
    uu, vv = np.meshgrid(cu, cu)
    solid = (2.0 / SIZE) ** 2 / (1.0 + uu * uu + vv * vv) ** 1.5
    volume = sum((r[f][inner, inner] ** 3 / 3.0 * solid).sum() for f in range(6))
    return r * ((math.pi * diameter ** 3 / 6.0) / volume) ** (1.0 / 3.0)


def lookup(radii, d):
    """The radius toward directions `d` (n, 3): bilinear on each one's face,
    as render::shape::Shape::radius takes it."""
    a = np.abs(d)
    face = np.where((a[:, 0] >= a[:, 1]) & (a[:, 0] >= a[:, 2]), np.where(d[:, 0] > 0, 0, 1),
                    np.where(a[:, 1] >= a[:, 2], np.where(d[:, 1] > 0, 2, 3), np.where(d[:, 2] > 0, 4, 5)))
    axes = np.array([f[0] for f in FACES], float)[face]
    us = np.array([f[1] for f in FACES], float)[face]
    vs = np.array([f[2] for f in FACES], float)[face]
    along = (d * axes).sum(1)
    x = ((d * us).sum(1) / along + 1.0) / 2.0 * SIZE + BORDER - 0.5
    y = ((d * vs).sum(1) / along + 1.0) / 2.0 * SIZE + BORDER - 0.5
    i, j = np.floor(x).astype(int), np.floor(y).astype(int)
    fx, fy = x - i, y - j
    at = lambda ii, jj: radii[face, jj, ii]
    top = at(i, j) * (1 - fx) + at(i + 1, j) * fx
    bottom = at(i, j + 1) * (1 - fx) + at(i + 1, j + 1) * fx
    return top * (1 - fy) + bottom * fy


def facets(radii, cells=16):
    """The map's facets at `cells` squares a face within its edges, two
    triangles each: centers (km) and normals × areas (km²)."""
    c = 2.0 * np.arange(cells + 1) / cells - 1.0
    u, v = np.meshgrid(c, c)
    centers, normals = [], []
    for axis, U, V in FACES:
        d = np.array(axis)[None, None, :] + u[..., None] * np.array(U) + v[..., None] * np.array(V)
        d = (d / np.linalg.norm(d, axis=-1, keepdims=True)).reshape(-1, 3)
        p = (d * lookup(radii, d)[:, None]).reshape(cells + 1, cells + 1, 3)
        a, b, cc, e = p[:-1, :-1], p[:-1, 1:], p[1:, 1:], p[1:, :-1]
        for t0, t1, t2 in [(a, b, cc), (a, cc, e)]:
            centers.append(((t0 + t1 + t2) / 3.0).reshape(-1, 3))
            normals.append((np.cross(t1 - t0, t2 - t0) / 2.0).reshape(-1, 3))
    centers, normals = np.concatenate(centers), np.concatenate(normals)
    # Outward: the faces' orientations wind their triangles both ways.
    out = (normals * centers).sum(1) < 0
    normals[out] *= -1.0
    return centers, normals


def lunar_lambert(cos_i, cos_e, alpha):
    """scene.wgsl's `lit`: McEwen's share L(alpha) of Lommel-Seeliger."""
    a = math.degrees(alpha)
    share = min(max(1.0 + a * (-0.019 + a * (2.42e-4 - a * 1.46e-6)), 0.0), 1.0)
    return 2.0 * share * cos_i / np.maximum(cos_i + cos_e, 1e-9) + (1.0 - share) * cos_i


def clear(radii, start, w, greatest):
    """Whether rays from `start` (n, 3) along `w` (3,) leave the body's
    bounding sphere without entering it: marched in 96 steps, closer
    together near their starts."""
    t = greatest * 2.2 * (np.geomspace(1e-3, 1.0, 96))
    out = np.ones(len(start), bool)
    for tk in t:
        q = start + w[None, :] * tk
        r = np.linalg.norm(q, axis=1)
        inside = r < lookup(radii, q / r[:, None])
        out &= ~inside
    return out


def shadowed(radii, seed=1):
    """The share of the light its self-shadowing leaves, by phase angle
    every 10 degrees, averaged over 64 random orientations each."""
    rng = np.random.default_rng(seed)
    centers, normals = facets(radii)
    area = np.linalg.norm(normals, axis=1)
    n = normals / area[:, None]
    greatest = radii.max()
    # From the surface over each facet's centroid (a flat facet's centroid
    # lies under a curved surface: every ray started inside), just off it.
    up = centers / np.linalg.norm(centers, axis=1)[:, None]
    lifted = up * lookup(radii, up)[:, None] + n * greatest * 2e-3
    table = []
    for degrees in range(0, 181, 10):
        alpha = math.radians(degrees)
        lit_all, lit_clear = 0.0, 0.0
        for _ in range(64):
            sun = rng.normal(size=3)
            sun /= np.linalg.norm(sun)
            side = np.cross(sun, rng.normal(size=3))
            side /= np.linalg.norm(side)
            eye = sun * math.cos(alpha) + side * math.sin(alpha)
            cos_i, cos_e = n @ sun, n @ eye
            both = (cos_i > 0) & (cos_e > 0)
            light = np.where(both, area * cos_e * lunar_lambert(np.maximum(cos_i, 0), np.maximum(cos_e, 0), alpha), 0.0)
            k = np.nonzero(both)[0]
            ok = np.zeros(len(n), bool)
            if len(k):
                ok[k] = clear(radii, lifted[k], sun, greatest) & clear(radii, lifted[k], eye, greatest)
            lit_all += light.sum()
            lit_clear += light[ok].sum()
        table.append(lit_clear / lit_all if lit_all > 0 else 0.0)
    return table


def write(out_dir, id, radii):
    os.makedirs(os.path.join(out_dir, id), exist_ok=True)
    radii.astype("<f4").tofile(os.path.join(out_dir, id, "shape.bin"))


def main():
    src, out = sys.argv[1], sys.argv[2]
    toml = [
        "# Shaped bodies (M1c.md, step 6, part 9 plan, item 3), built by",
        "# tools/shapes/build.py; see it for shape.bin's layout. `size`: texels across",
        "# a face within its edges. A shaped body's radius is its map's",
        "# volume-equivalent one (its model's beside it; the SBDB's in M1c.md).",
    ]
    for id, (name, credit) in SOURCES.items():
        path = os.path.join(src, name)
        if name.endswith(".obj"):
            vertices, faces = read_obj(path)
        elif id == "deimos":
            vertices, faces = read_grid(path, east=False)
        else:
            vertices, faces = read_tab(path)
        radii, twice = cast(vertices, faces)
        area, volume = model_measures(vertices, faces)
        equivalent = (3.0 * volume / (4.0 * math.pi)) ** (1.0 / 3.0)
        print(f"{id}: {len(faces)} facets, radii {radii.min():.4f}-{radii.max():.4f} km, "
              f"volume-equivalent {equivalent:.4f} km, area {area:.4f} km2; "
              f"{twice} texels meet it more than once")
        write(out, id, radii)
        table = shadowed(radii)
        print(f"  shadowed: {' '.join(f'{x:.3f}' for x in table)}")
        toml += ["", f"[body.{id}]", f"# {credit}", "file = \"shape.bin\"", f"size = {SIZE}",
                 f"# The model's volume-equivalent radius, km: {equivalent:.4f}.",
                 "shadowed = [" + ", ".join(f"{x:.4f}" for x in table) + "]"]
    for id, (diameter, ratios, ridge, seed) in MADE_UP.items():
        radii = made_up(diameter, ratios, ridge, seed)
        print(f"{id}: made up, radii {radii.min():.4f}-{radii.max():.4f} km")
        write(out, id, radii)
        table = shadowed(radii)
        print(f"  shadowed: {' '.join(f'{x:.3f}' for x in table)}")
        toml += ["", f"[body.{id}]",
                 f"# Made up (nothing usable is published): ratios {ratios}, ridge {ridge}, seed",
                 f"# {seed}, scaled to the SBDB's diameter, {diameter} km.",
                 "file = \"shape.bin\"", f"size = {SIZE}", "made_up = true",
                 "shadowed = [" + ", ".join(f"{x:.4f}" for x in table) + "]"]
    with open(os.path.join(out, "shapes.toml"), "w") as f:
        f.write("\n".join(toml) + "\n")


if __name__ == "__main__":
    main()
