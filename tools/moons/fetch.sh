#!/usr/bin/env bash
# Downloads the raw inputs for data/moons.toml into DIR: JPL Horizons state
# vectors of Phobos and Deimos relative to Mars's center, ecliptic of J2000,
# TDB, daily over 2025-2080 (DIR/<id>.txt).
# Usage: tools/moons/fetch.sh DIR
set -euo pipefail
dir=${1:?usage: fetch.sh DIR}
mkdir -p "$dir"
for pair in phobos:401 deimos:402; do
  id=${pair%%:*}; cmd=${pair##*:}
  curl -sSf -G "https://ssd.jpl.nasa.gov/api/horizons.api" \
    --data-urlencode "format=text" --data-urlencode "COMMAND='$cmd'" \
    --data-urlencode "OBJ_DATA='NO'" --data-urlencode "MAKE_EPHEM='YES'" \
    --data-urlencode "EPHEM_TYPE='VECTORS'" --data-urlencode "CENTER='500@499'" \
    --data-urlencode "START_TIME='2025-01-02'" --data-urlencode "STOP_TIME='2079-12-30'" \
    --data-urlencode "STEP_SIZE='1 d'" --data-urlencode "REF_PLANE='ECLIPTIC'" \
    --data-urlencode "REF_SYSTEM='ICRF'" --data-urlencode "OUT_UNITS='KM-S'" \
    --data-urlencode "VEC_TABLE='2'" --data-urlencode "CSV_FORMAT='YES'" \
    --data-urlencode "TIME_TYPE='TDB'" -o "$dir/$id.txt"
  sleep 0.5
done
