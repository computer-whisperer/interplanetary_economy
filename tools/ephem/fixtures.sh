#!/usr/bin/env bash
# Fetches JPL Horizons state vectors that crates/orbit/tests/ephemeris.rs
# checks data/de440.bin against, into OUTDIR (the tests' fixtures).
# Usage: tools/ephem/fixtures.sh OUTDIR
set -euo pipefail
out=${1:?usage: fixtures.sh OUTDIR}
here=$(dirname "$0")

fetch() { # COMMAND CENTER STEP NAME DESCRIPTION
  local raw
  raw=$(curl -sSf -G "https://ssd.jpl.nasa.gov/api/horizons.api" \
    --data-urlencode "format=text" --data-urlencode "COMMAND='$1'" \
    --data-urlencode "OBJ_DATA='NO'" --data-urlencode "MAKE_EPHEM='YES'" \
    --data-urlencode "EPHEM_TYPE='VECTORS'" --data-urlencode "CENTER='$2'" \
    --data-urlencode "START_TIME='2025-01-01'" --data-urlencode "STOP_TIME='2079-12-31'" \
    --data-urlencode "STEP_SIZE='$3'" --data-urlencode "REF_PLANE='ECLIPTIC'" \
    --data-urlencode "REF_SYSTEM='ICRF'" --data-urlencode "OUT_UNITS='KM-S'" \
    --data-urlencode "VEC_TABLE='2'" --data-urlencode "CSV_FORMAT='YES'")
  {
    echo "# JPL Horizons, fetched $(date -u +%F) by tools/ephem/fixtures.sh: $5"
    echo "# ecliptic of J2000, TDB; columns: jd_tdb, x, y, z [km], vx, vy, vz [km/s]"
    echo "$raw" | sed -n '/^\$\$SOE/,/^\$\$EOE/p' | sed '1d;$d' |
      awk -F, '{gsub(/ /, ""); print $1 "," $3 "," $4 "," $5 "," $6 "," $7 "," $8}'
  } > "$out/$4"
  sleep 0.3
}

fetch 399 500@10 "97 d" horizons_earth.csv "Earth (399), heliocentric"
fetch 301 500@399 "37 d" horizons_moon.csv "Moon (301), geocentric"
fetch 10 500@0 "101 d" horizons_sun.csv "Sun (10), relative to the solar-system barycenter"
fetch 5 500@10 "199 d" horizons_jupiter.csv "Jupiter barycenter (5), heliocentric"
