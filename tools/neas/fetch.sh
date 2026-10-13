#!/usr/bin/env bash
# Downloads the raw inputs for data/neas.toml into DIR:
#   DIR/nhats.json        NHATS accessible-NEA list (for size estimates)
#   DIR/sbdb/<key>.json   SBDB physical parameters
#   DIR/elem/<key>.txt    Horizons osculating elements at 2040-01-01
#   DIR/vec/<key>.txt     Horizons heliocentric vectors, 10-day steps, 2040–2076
# Usage: tools/neas/fetch.sh DIR
set -euo pipefail
dir=${1:?usage: fetch.sh DIR}
here=$(dirname "$0")
mkdir -p "$dir/sbdb" "$dir/elem" "$dir/vec"

horizons() { # COMMAND EPHEM_TYPE STEP OUT_UNITS OUTFILE
  curl -sSf -G "https://ssd.jpl.nasa.gov/api/horizons.api" \
    --data-urlencode "format=text" --data-urlencode "COMMAND='$1'" \
    --data-urlencode "OBJ_DATA='NO'" --data-urlencode "MAKE_EPHEM='YES'" \
    --data-urlencode "EPHEM_TYPE='$2'" --data-urlencode "CENTER='500@10'" \
    --data-urlencode "START_TIME='2040-01-01'" --data-urlencode "STOP_TIME='$3'" \
    --data-urlencode "STEP_SIZE='$4'" --data-urlencode "REF_PLANE='ECLIPTIC'" \
    --data-urlencode "REF_SYSTEM='ICRF'" --data-urlencode "OUT_UNITS='$5'" \
    --data-urlencode "VEC_TABLE='2'" --data-urlencode "CSV_FORMAT='YES'" -o "$6"
}

curl -sSf "https://ssd-api.jpl.nasa.gov/nhats.api?dv=6&dur=450&stay=8&h=26" -o "$dir/nhats.json"

# candidates.txt: id <TAB> Horizons command <TAB> SBDB designation
while IFS=$'\t' read -r id cmd des; do
  [[ -z "$id" || "$id" == \#* ]] && continue
  curl -sSf -G "https://ssd-api.jpl.nasa.gov/sbdb.api" --data-urlencode "sstr=$des" \
    --data-urlencode "phys-par=1" -o "$dir/sbdb/$id.json"
  horizons "$cmd" ELEMENTS 2040-01-02 "1 d" AU-D "$dir/elem/$id.txt"
  horizons "$cmd" VECTORS 2076-01-01 "10 d" KM-S "$dir/vec/$id.txt"
  sleep 0.3
done < "$here/candidates.txt"
