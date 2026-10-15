"""Builds data/bodies/ (M1c.md, step 6 plan, item 4) from what fetch.sh
downloads into DIR: each body's maps, equirectangular, east longitude
from -180 at the left edge, north up, and data/bodies/bodies.toml, which
says how they are read (albedo, elevation's encoding, shading) and where
they come from.

- color.jpg: 8-bit sRGB. The renderer multiplies it by `albedo`, chosen
  here so that its area-weighted mean (linear luminance) is the body's
  albedo for its shading: the geometric albedo under lunar-Lambert (whose
  full disk is as bright as its normal reflectance), the Bond albedo
  under Lambert. Earth's is taken as it is (its ice reads 0.8-0.96, its
  deserts ~0.4: reflectance already; its oceans are near black, their
  blue from space being the atmosphere's), its thick clouds at 0.8.
  The sandbox then holds each body with a published phase curve to it
  (crates/sandbox/src/photometry.rs), which cancels `albedo`: it counts
  only where it sets a map against clouds (Earth's), or for a body
  without a curve.
- elevation.png: 16-bit, metres = value x `metres_per_unit` + `offset_m`.
- clouds.jpg, water.png, night.jpg: one channel (Earth). Black Marble's
  faint moonlit snow and desert (up to ~55 of 255) is taken off its
  lights.

Each map's orientation is checked against landmarks, and printed.

Needs numpy, Pillow and vips. Usage: python3 build.py DIR ../../data/bodies
"""

import os
import subprocess
import sys

import numpy as np
from PIL import Image

Image.MAX_IMAGE_PIXELS = None

COLOR = (8192, 4096)
SMALL = (4096, 2048)
LUMINANCE = np.array([0.2126, 0.7152, 0.0722])


def linear(encoded):
    c = encoded / 255.0
    return np.where(c <= 0.04045, c / 12.92, ((c + 0.055) / 1.055) ** 2.4)


def mean_albedo(rgb):
    """Area-weighted mean linear luminance of an equirectangular image."""
    h = rgb.shape[0]
    lat = np.radians(90 - (np.arange(h) + 0.5) / h * 180)
    lum = linear(rgb.astype(np.float64)) @ LUMINANCE
    w = np.cos(lat)[:, None] * np.ones(rgb.shape[1])[None, :]
    return float((lum * w).sum() / w.sum())


def resized_linear(src, out, size, quality=92):
    """`src` resized to `size` in linear light (vips), as an sRGB JPEG."""
    tmp = out + ".v"
    w, _ = vips_size(src)
    scale = size[0] / w
    subprocess.run(["vips", "colourspace", src, tmp + "1.v", "scrgb"], check=True)
    subprocess.run(["vips", "resize", tmp + "1.v", tmp + "2.v", str(scale), "--kernel", "lanczos3"], check=True)
    subprocess.run(["vips", "colourspace", tmp + "2.v", f"{out}[Q={quality},subsample-mode=off]", "srgb"], check=True)
    for t in (tmp + "1.v", tmp + "2.v"):
        os.remove(t)
    got = vips_size(out)
    assert got == size, f"{out}: {got}"


def vips_size(path):
    w = int(subprocess.run(["vipsheader", "-f", "width", path], capture_output=True, text=True, check=True).stdout)
    h = int(subprocess.run(["vipsheader", "-f", "height", path], capture_output=True, text=True, check=True).stdout)
    return w, h


def box(array, size):
    """A float array resized by area averaging."""
    return np.asarray(Image.fromarray(array.astype(np.float32), "F").resize(size, Image.BOX))


def save_elevation(metres, path, step, offset):
    v = np.round((metres - offset) / step)
    assert v.min() >= 0 and v.max() <= 65535, (v.min(), v.max())
    Image.fromarray(v.astype(np.uint16)).save(path, optimize=True)


def at(array, lat, lon):
    h, w = array.shape[:2]
    return array[int((90 - lat) / 180 * h) % h, int((lon + 180) / 360 * w) % w]


def where_extreme(array, f):
    """Latitude and east longitude of the array's argmax (or argmin)."""
    h, w = array.shape[:2]
    y, x = np.unravel_index(f(array), array.shape[:2])
    return 90 - (y + 0.5) / h * 180, (x + 0.5) / w * 360 - 180


def tiles(dir, rows, cols):
    """Trek's tiles assembled: rows x cols of 256."""
    out = Image.new("RGB", (cols * 256, rows * 256))
    for r in range(rows):
        for c in range(cols):
            out.paste(Image.open(f"{dir}/{r}-{c}.jpg").convert("RGB"), (c * 256, r * 256))
    return out


def earth(src, out, toml):
    os.makedirs(f"{out}/earth", exist_ok=True)
    resized_linear(f"{src}/earth_color.jpg", f"{out}/earth/color.jpg", COLOR)
    color = np.asarray(Image.open(f"{out}/earth/color.jpg"))
    surface = mean_albedo(color)
    clouds = np.asarray(Image.open(f"{src}/earth_clouds.tif").convert("L"))
    assert clouds.shape == (COLOR[1], COLOR[0])
    Image.fromarray(clouds).save(f"{out}/earth/clouds.jpg", quality=90)
    h = clouds.shape[0]
    lat = np.radians(90 - (np.arange(h) + 0.5) / h * 180)
    w = np.cos(lat)[:, None] * np.ones(clouds.shape[1])[None, :]
    cover = float((clouds / 255.0 * w).sum() / w.sum())
    cloud_albedo = 0.8
    bath = Image.open(f"{src}/earth_bathymetry.png")
    water = (np.asarray(bath.resize(SMALL, Image.NEAREST)) < 255).astype(np.uint8) * 255
    Image.fromarray(water).save(f"{out}/earth/water.png", optimize=True)
    elev = np.asarray(Image.open(f"{src}/earth_elevation.png"), dtype=np.float32) / 255 * 6400
    save_elevation(box(elev, SMALL), f"{out}/earth/elevation.png", 1.0, 0.0)
    night = np.asarray(Image.open(f"{src}/earth_night.jpg").convert("L").resize(COLOR, Image.LANCZOS), dtype=np.float32)
    night = Image.fromarray(np.round(np.clip((night - 55) / 200, 0, 1) * 255).astype(np.uint8))
    night.save(f"{out}/earth/night.jpg", quality=92)
    # Checks: the Sahara bright and dry, the Pacific dark and wet, Everest
    # high, the Nile delta's lights.
    print(f"earth: surface mean {surface:.4f}, cloud cover {cover:.3f}; with clouds {surface * (1 - cover) + cloud_albedo * cover:.3f}")
    print(f"  Sahara {at(color, 23, 13)}, water {at(water, 23, 13)}; Pacific {at(color, 0, -150)}, water {at(water, 0, -150)}")
    print(f"  highest at {where_extreme(elev, np.argmax)} (Himalaya ~28N 87E)")
    print(f"  night: Cairo {at(np.asarray(night), 30.05, 31.25)}, Sahara {at(np.asarray(night), 23, 13)}")
    toml.append(f"""
[body.earth]
# NASA Earth Observatory's Blue Marble Next Generation (July 2004, R.
# Stöckli), its clouds, and GEBCO's elevation (scaled 0-6400 m: higher is
# clipped) and bathymetry (water: any depth); Black Marble 2016 (NASA
# Earth Observatory, M. Román et al.). NASA imagery: free to use, credit
# requested.
color = "color.jpg"
albedo = 1.0
shading = "lambert"
clouds = "clouds.jpg"
cloud_albedo = {cloud_albedo:.4f}
water = "water.png"
night = "night.jpg"
# The lights' radiance at full value, warm: luminance ~1e-5 of
# daylight's, a bright city's (VIIRS's day-night band reads ~150
# nW/cm2/sr there, a white surface in sunlight ~1.7e7). The map's stretch
# is not calibrated, so within a factor of a few. The settings lift it
# (M1c.md, Realism).
night_radiance = [1.22e-5, 0.92e-5, 0.49e-5]
elevation = {{ file = "elevation.png", metres_per_unit = 1.0, offset_m = 0.0 }}
""")


def moon(src, out, toml):
    os.makedirs(f"{out}/moon", exist_ok=True)
    color = np.asarray(Image.open(f"{src}/moon_color.tif").convert("RGB"))
    Image.fromarray(color).save(f"{out}/moon/color.jpg", quality=92, subsampling=0)
    elev = np.asarray(Image.open(f"{src}/moon_elevation.tif"), dtype=np.float32) / 2 - 10000
    small = box(elev, SMALL)
    save_elevation(small, f"{out}/moon/elevation.png", 0.5, -10000.0)
    mean = mean_albedo(color)
    # Checks: the highest point (the Selenean summit, 5.4N 158.6W) and the
    # lowest (in Antoniadi crater, 70S 172W); Mare Imbrium (33N 16W) dark
    # and low, the highlands near Tycho (43S 11W) bright.
    print(f"moon: mean {mean:.4f}")
    print(f"  highest at {where_extreme(small, np.argmax)}, lowest at {where_extreme(small, np.argmin)}")
    print(f"  Imbrium {at(color, 33, -16)} at {at(small, 33, -16):.0f} m; highlands {at(color, -43, -11)} at {at(small, -43, -11):.0f} m")
    toml.append(f"""
[body.moon]
# NASA's Scientific Visualization Studio's CGI Moon Kit (E. Wright, N.
# Petro): LROC WAC color (Hapke-normalized) and LOLA elevation, 16 px a
# degree. Free to use, credit requested.
color = "color.jpg"
albedo = {0.12 / mean:.5f}
shading = "lunar-lambert"
elevation = {{ file = "elevation.png", metres_per_unit = 0.5, offset_m = -10000.0 }}
""")


def mars(src, out, toml):
    os.makedirs(f"{out}/mars", exist_ok=True)
    color = tiles(f"{src}/mars_color", 16, 32)
    color.save(f"{out}/mars/color.jpg", quality=92, subsampling=0)
    color = np.asarray(color)
    # MEGDR: big-endian int16 metres, from 0 E at the left edge.
    raw = np.fromfile(f"{src}/mars_elevation.img", dtype=">i2").reshape(2880, 5760).astype(np.float32)
    elev = np.roll(raw, 2880, axis=1)
    small = box(elev, SMALL)
    save_elevation(small, f"{out}/mars/elevation.png", 0.5, -9000.0)
    mean = mean_albedo(color)
    # Checks: Olympus Mons the highest (18.65N 133.8W), Hellas the lowest
    # (~42S 70E); Syrtis Major (9N 70E) dark.
    print(f"mars: mean {mean:.4f}")
    print(f"  highest at {where_extreme(small, np.argmax)}, lowest at {where_extreme(small, np.argmin)}")
    print(f"  Syrtis Major {at(color, 9, 70)}, Arabia {at(color, 20, 20)}")
    toml.append(f"""
[body.mars]
# The Viking MDIM 2.1 colorized mosaic (USGS Astrogeology, NASA Ames), as
# NASA Mars Trek's tiles; MOLA's MEGDR elevation, 16 px a degree (PDS
# Geosciences Node). Public domain.
color = "color.jpg"
albedo = {0.25 / mean:.5f}
shading = "lambert"
elevation = {{ file = "elevation.png", metres_per_unit = 0.5, offset_m = -9000.0 }}
""")


def mercury(src, out, toml):
    os.makedirs(f"{out}/mercury", exist_ok=True)
    gray = tiles(f"{src}/mercury_color", 8, 16)
    # Mercury's own color, faintly brown (rather than MDIS's false color).
    tint = np.array([1.0, 0.93, 0.85])
    rgb = np.asarray(gray, dtype=np.float64)
    lin = linear(rgb) * tint / (tint @ LUMINANCE)
    enc = np.where(lin <= 0.0031308, lin * 12.92, 1.055 * np.clip(lin, 0, 1) ** (1 / 2.4) - 0.055)
    color = np.round(np.clip(enc, 0, 1) * 255).astype(np.uint8)
    Image.fromarray(color).save(f"{out}/mercury/color.jpg", quality=92)
    mean = mean_albedo(color)
    print(f"mercury: mean {mean:.4f}; Caloris rim region (31N 162W) {at(color, 31, -162)}")
    toml.append(f"""
[body.mercury]
# MESSENGER MDIS's basemap (NASA, JHU APL, Carnegie Institution), as
# NASA Mercury Trek's tiles; tinted faintly brown. Public domain.
color = "color.jpg"
albedo = {0.142 / mean:.5f}
shading = "lunar-lambert"
""")


def jupiter(src, out, toml):
    os.makedirs(f"{out}/jupiter", exist_ok=True)
    im = Image.open(f"{src}/jupiter_color.tif").convert("RGB").resize(SMALL, Image.LANCZOS)
    im.save(f"{out}/jupiter/color.jpg", quality=92, subsampling=0)
    color = np.asarray(im)
    mean = mean_albedo(color)
    print(f"jupiter: mean {mean:.4f}")
    toml.append(f"""
[body.jupiter]
# Cassini's cylindrical map from its December 2000 flyby (PIA07782,
# NASA/JPL/Space Science Institute). Its features drift: their longitudes
# in the game's years are not the map's. Public domain.
color = "color.jpg"
albedo = {0.343 / mean:.5f}
shading = "lambert"
""")


def main(src, out):
    os.makedirs(out, exist_ok=True)
    toml = [
        "# Bodies' maps (M1c.md, step 6 plan, item 4), built by tools/textures/build.py;",
        "# see it for what each field means. Equirectangular, east longitude from",
        "# -180 at the left edge, north up.",
    ]
    for f in (earth, moon, mars, mercury, jupiter):
        f(src, out, toml)
    with open(f"{out}/bodies.toml", "w") as f:
        f.write("\n".join(toml).rstrip() + "\n")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
