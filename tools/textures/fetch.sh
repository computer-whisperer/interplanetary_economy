#!/usr/bin/env bash
# Downloads the raw inputs for data/bodies/ into DIR (~400 MB):
# - Earth: NASA Earth Observatory's Blue Marble Next Generation (July
#   2004, without relief shading or bathymetry), its cloud layer, its
#   GEBCO elevation and bathymetry, and Black Marble 2016's night lights;
# - the Moon: NASA SVS's CGI Moon Kit (LROC WAC color, LOLA elevation);
# - Mars: the Viking MDIM 2.1 colorized mosaic (USGS / NASA Ames) as
#   NASA Mars Trek's tiles at 8192 x 4096, and MOLA's MEGDR elevation at
#   16 pixels a degree (PDS);
# - Mercury: MESSENGER MDIS's basemap as Mercury Trek's tiles at 4096 x
#   2048;
# - Jupiter: Cassini's cylindrical map (PIA07782).
# Usage: tools/textures/fetch.sh DIR
set -euo pipefail
dir=${1:?usage: fetch.sh DIR}
mkdir -p "$dir"
# Each file whole or not at all: an interrupted download is fetched again,
# and a web page in its place (a moved link) is an error.
get() {
  [ -s "$dir/$2" ] && return
  curl -sSfL --retry 3 -o "$dir/$2.part" "$1"
  if [ "$(head -c 1 "$dir/$2.part")" = "<" ]; then
    echo "$1: a web page, not data" >&2
    return 1
  fi
  mv "$dir/$2.part" "$dir/$2"
}
eo=https://eoimages.gsfc.nasa.gov/images/imagerecords
get $eo/74000/74092/world.200407.3x21600x10800.jpg earth_color.jpg
get $eo/57000/57747/cloud_combined_8192.tif earth_clouds.tif
get $eo/73000/73934/gebco_08_rev_elev_21600x10800.png earth_elevation.png
get $eo/73000/73963/gebco_08_rev_bath_21600x10800.png earth_bathymetry.png
get $eo/144000/144898/BlackMarble_2016_3km.jpg earth_night.jpg
svs=https://svs.gsfc.nasa.gov/vis/a000000/a004700/a004720
get $svs/lroc_color_poles_8k.tif moon_color.tif
get $svs/ldem_16_uint.tif moon_elevation.tif
megdr=https://pds-geosciences.wustl.edu/mgs/mgs-m-mola-5-megdr-l3-v1/mgsl_300x/meg016
get $megdr/megt90n000eb.img mars_elevation.img
get $megdr/megt90n000eb.lbl mars_elevation.lbl
get https://assets.science.nasa.gov/content/dam/science/psd/photojournal/pia/pia07/pia07782/PIA07782.tif jupiter_color.tif
# Trek's equirectangular tiles: level L is 2^(L+1) x 2^L tiles of 256
# pixels, from 180 W at the left and 90 N at the top.
tiles() {
  local base=$1 level=$2 name=$3
  mkdir -p "$dir/$name"
  for ((row = 0; row < 1 << level; row++)); do
    for ((col = 0; col < 2 << level; col++)); do
      get "$base/$level/$row/$col.jpg" "$name/$row-$col.jpg"
    done
  done
}
trek=https://trek.nasa.gov/tiles
tiles $trek/Mars/EQ/Mars_Viking_MDIM21_ClrMosaic_global_232m/1.0.0/default/default028mm 4 mars_color
tiles $trek/Mercury/EQ/Mercury_MESSENGER_MDIS_Basemap_BDR_Mosaic_Global_166m/1.0.0/default/default028mm 3 mercury_color
