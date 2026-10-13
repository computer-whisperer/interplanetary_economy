#!/usr/bin/env bash
# Fetches JPL Horizons vectors that crates/orbit/tests/ephemeris.rs checks
# the integrated asteroids against, into OUTDIR (the tests' fixtures).
# Usage: tools/neas/fixtures.sh OUTDIR
set -euo pipefail
out=${1:?usage: fixtures.sh OUTDIR}

fetch() { # ID COMMAND
  local raw
  raw=$(curl -sSf -G "https://ssd.jpl.nasa.gov/api/horizons.api" \
    --data-urlencode "format=text" --data-urlencode "COMMAND='$2'" \
    --data-urlencode "OBJ_DATA='NO'" --data-urlencode "MAKE_EPHEM='YES'" \
    --data-urlencode "EPHEM_TYPE='VECTORS'" --data-urlencode "CENTER='500@10'" \
    --data-urlencode "START_TIME='2040-01-01'" --data-urlencode "STOP_TIME='2079-12-31'" \
    --data-urlencode "STEP_SIZE='73 d'" --data-urlencode "REF_PLANE='ECLIPTIC'" \
    --data-urlencode "REF_SYSTEM='ICRF'" --data-urlencode "OUT_UNITS='KM-S'" \
    --data-urlencode "VEC_TABLE='2'" --data-urlencode "CSV_FORMAT='YES'")
  {
    echo "# JPL Horizons, fetched $(date -u +%F) by tools/neas/fixtures.sh: $1 ($2), heliocentric"
    echo "# ecliptic of J2000, TDB; columns: jd_tdb, x, y, z [km], vx, vy, vz [km/s]"
    echo "$raw" | sed -n '/^\$\$SOE/,/^\$\$EOE/p' | sed '1d;$d' |
      awk -F, '{gsub(/ /, ""); print $1 "," $3 "," $4 "," $5 "," $6 "," $7 "," $8}'
  } > "$out/horizons_$1.csv"
  sleep 0.3
}

# Quiet orbits, and ones with close Earth flybys in the game's span
# (Bennu 2060, 2000 SG344, 2017 SV19, Ryugu in the 2070s).
fetch ryugu "162173;"
fetch 2008-ev5 "341843;"
fetch bennu "101955;"
fetch apophis "99942;"
fetch 2000-sg344 "DES=2000 SG344;"
fetch 2017-sv19 "DES=2017 SV19;"
fetch 2001-fr85 "DES=2001 FR85;"
