#!/usr/bin/env bash
# Downloads the raw input for data/bodies/saturn/rings.toml into DIR:
# Cassini UVIS's occultation of Gamma Crucis by Saturn's rings (ingress,
# 2008-238, seen 62 degrees above the ring plane, so the B ring's opaque
# core is measured), normal optical depth in 10 km bins (PDS Ring-Moon
# Systems Node, COUVIS_8001).
# Usage: tools/rings/fetch.sh DIR
set -euo pipefail
dir=${1:?usage: fetch.sh DIR}
mkdir -p "$dir"
base=https://pds-rings.seti.org/holdings/volumes/COUVIS_8xxx/COUVIS_8001/data
for ext in TAB LBL; do
  curl -sSfL -o "$dir/gamcru.$ext" "$base/UVIS_HSP_2008_238_GAMCRU_I_TAU10KM.$ext"
done
