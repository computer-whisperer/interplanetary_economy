#!/usr/bin/env bash
# Downloads the raw inputs for data/de440.bin into DIR:
#   DIR/de440s.bsp   JPL DE440 planetary ephemeris, 1849–2150 (SPK, ~32 MB)
#   DIR/header.440   DE440 constants (GMs, EMRAT, AU)
# Usage: tools/ephem/fetch.sh DIR
set -euo pipefail
dir=${1:?usage: fetch.sh DIR}
mkdir -p "$dir"
base=https://ssd.jpl.nasa.gov/ftp/eph/planets
curl -sSf "$base/bsp/de440s.bsp" -o "$dir/de440s.bsp"
curl -sSf "$base/ascii/de440/header.440" -o "$dir/header.440"
