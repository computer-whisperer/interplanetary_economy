#!/usr/bin/env bash
# Downloads the raw input for data/rotation.toml into DIR: NAIF's text
# PCK pck00011.tpc, the IAU WGCCRE 2015 rotation models (Archinal et al.
# 2018) as SPICE reads them.
# Usage: tools/rotation/fetch.sh DIR
set -euo pipefail
dir=${1:?usage: fetch.sh DIR}
mkdir -p "$dir"
curl -sSfL -o "$dir/pck00011.tpc" https://naif.jpl.nasa.gov/pub/naif/generic_kernels/pck/pck00011.tpc
