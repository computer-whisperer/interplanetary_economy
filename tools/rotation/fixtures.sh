#!/usr/bin/env bash
# Downloads the Horizons fixtures the rotation tests check against: each
# body's apparent sub-observer point (west longitude, planetodetic
# latitude) and one-way light time, from Earth's center (Earth's from the
# Moon's), TT, on a few dates. Into crates/orbit/tests/fixtures/.
# Usage: tools/rotation/fixtures.sh
set -euo pipefail
out="$(dirname "$0")/../../crates/orbit/tests/fixtures"
for pair in sun:10:399 mercury:199:399 venus:299:399 moon:301:399 mars:499:399 \
  phobos:401:399 deimos:402:399 jupiter:599:399 saturn:699:399 uranus:799:399 \
  neptune:899:399 earth:399:301; do
  IFS=: read -r id cmd center <<<"$pair"
  # Earth's orientation from observed EOP: dates the file covers.
  if [ "$id" = earth ]; then range=("2026-01-01" "2026-09-28" "39 d"); else range=("2026-01-01" "2036-01-01" "251 d"); fi
  curl -sSf -G "https://ssd.jpl.nasa.gov/api/horizons.api" \
    --data-urlencode "format=text" --data-urlencode "COMMAND='$cmd'" \
    --data-urlencode "OBJ_DATA='NO'" --data-urlencode "MAKE_EPHEM='YES'" \
    --data-urlencode "EPHEM_TYPE='OBSERVER'" --data-urlencode "CENTER='500@$center'" \
    --data-urlencode "START_TIME='${range[0]}'" --data-urlencode "STOP_TIME='${range[1]}'" \
    --data-urlencode "STEP_SIZE='${range[2]}'" --data-urlencode "QUANTITIES='14,21'" \
    --data-urlencode "TIME_TYPE='TT'" --data-urlencode "CSV_FORMAT='YES'" \
    -o "$out/horizons_subobs_$id.txt"
  sleep 0.5
done
