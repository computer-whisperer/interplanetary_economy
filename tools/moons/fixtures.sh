#!/usr/bin/env bash
# Fetches JPL Horizons vectors that crates/orbit/tests/ephemeris.rs checks
# Phobos and Deimos against, into OUTDIR (the tests' fixtures): relative to
# Mars's center, every 97.3 days, off the fit's daily grid.
# Usage: tools/moons/fixtures.sh OUTDIR
set -euo pipefail
out=${1:?usage: fixtures.sh OUTDIR}

fetch() { # ID COMMAND
  local raw
  raw=$(curl -sSf -G "https://ssd.jpl.nasa.gov/api/horizons.api" \
    --data-urlencode "format=text" --data-urlencode "COMMAND='$2'" \
    --data-urlencode "OBJ_DATA='NO'" --data-urlencode "MAKE_EPHEM='YES'" \
    --data-urlencode "EPHEM_TYPE='VECTORS'" --data-urlencode "CENTER='500@499'" \
    --data-urlencode "START_TIME='2025-01-03 07:12'" --data-urlencode "STOP_TIME='2079-12-01'" \
    --data-urlencode "STEP_SIZE='140112 m'" --data-urlencode "REF_PLANE='ECLIPTIC'" \
    --data-urlencode "REF_SYSTEM='ICRF'" --data-urlencode "OUT_UNITS='KM-S'" \
    --data-urlencode "VEC_TABLE='2'" --data-urlencode "CSV_FORMAT='YES'" \
    --data-urlencode "TIME_TYPE='TDB'")
  {
    echo "# JPL Horizons, fetched $(date -u +%F) by tools/moons/fixtures.sh: $1 ($2), relative to"
    echo "# Mars's center, ecliptic of J2000, TDB; columns: jd_tdb, x, y, z [km], vx, vy, vz [km/s]"
    echo "$raw" | sed -n '/^\$\$SOE/,/^\$\$EOE/p' | sed '1d;$d' |
      awk -F, '{gsub(/ /, ""); print $1 "," $3 "," $4 "," $5 "," $6 "," $7 "," $8}'
  } > "$out/horizons_$1_mars.csv"
  sleep 0.3
}

fetch phobos 401
fetch deimos 402
