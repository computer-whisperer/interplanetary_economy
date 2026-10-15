#!/usr/bin/env bash
# Downloads the raw inputs for data/sky/ into DIR: the Hipparcos main
# catalog (ESA 1997, CDS I/239), and NASA SVS's Deep Star Maps 2020
# (svs.gsfc.nasa.gov/4851) at 8k, celestial coordinates: the Milky Way
# background (its Hipparcos and Tycho stars removed), and at 4k, for
# calibrating it, the bright-star layer and the composite.
# Usage: tools/sky/fetch.sh DIR
set -euo pipefail
dir=${1:?usage: fetch.sh DIR}
mkdir -p "$dir"
curl -sSfL -o "$dir/ReadMe" https://cdsarc.cds.unistra.fr/ftp/I/239/ReadMe
curl -sSfL -o "$dir/hip_main.dat" https://cdsarc.cds.unistra.fr/ftp/I/239/hip_main.dat
svs=https://svs.gsfc.nasa.gov/vis/a000000/a004800/a004851
for f in milkyway_2020_8k.exr hiptyc_2020_4k.exr milkyway_2020_4k.exr starmap_2020_4k.exr; do
  curl -sSfL -o "$dir/$f" "$svs/$f"
done
