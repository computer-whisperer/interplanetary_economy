#!/usr/bin/env bash
# Downloads the Horizons fixtures the bodies' brightness is checked
# against: each planet's and the Moon's apparent visual magnitude (APmag),
# its distances from the Sun and the observer, and its phase angle
# (S-T-O), every 30 days over 2025-2080; from Earth's center, and for the
# outer planets and Earth from a body farther out, where they show large
# phase angles. Into crates/sandbox/tests/fixtures/.
# Usage: tools/photometry/fixtures.sh
set -euo pipefail
out="$(dirname "$0")/../../crates/sandbox/tests/fixtures"
for pair in mercury:199:399 venus:299:399 moon:301:399 mars:499:399 jupiter:599:399 \
  saturn:699:399 uranus:799:399 neptune:899:399 earth:399:499 jupiter:599:699 \
  saturn:699:799 uranus:799:899 neptune:899:999; do
  IFS=: read -r id cmd center <<<"$pair"
  curl -sSf -G "https://ssd.jpl.nasa.gov/api/horizons.api" \
    --data-urlencode "format=text" --data-urlencode "COMMAND='$cmd'" \
    --data-urlencode "OBJ_DATA='NO'" --data-urlencode "MAKE_EPHEM='YES'" \
    --data-urlencode "EPHEM_TYPE='OBSERVER'" --data-urlencode "CENTER='500@$center'" \
    --data-urlencode "START_TIME='2025-01-01'" --data-urlencode "STOP_TIME='2080-01-01'" \
    --data-urlencode "STEP_SIZE='30 d'" --data-urlencode "QUANTITIES='9,19,20,24'" \
    --data-urlencode "TIME_TYPE='TT'" --data-urlencode "CSV_FORMAT='YES'" \
    -o "$out/horizons_mag_${id}_from_${center}.txt"
  sleep 0.5
done
