#!/usr/bin/env bash
# Downloads the raw inputs for data/neas.toml into DIR:
#   DIR/nhats.json        NHATS accessible-NEA list (for size estimates)
#   DIR/sbdb/<key>.json   SBDB physical parameters
#   DIR/state/<key>.txt   Horizons state at 2040-01-01 00:00 TDB, relative to
#                         the solar-system barycenter, ecliptic of J2000
# Usage: tools/neas/fetch.sh DIR
set -euo pipefail
dir=${1:?usage: fetch.sh DIR}
here=$(dirname "$0")
mkdir -p "$dir/sbdb" "$dir/state"

curl -sSf "https://ssd-api.jpl.nasa.gov/nhats.api?dv=6&dur=450&stay=8&h=26" -o "$dir/nhats.json"

# candidates.txt: id <TAB> Horizons command <TAB> SBDB designation
while IFS=$'\t' read -r id cmd des; do
  [[ -z "$id" || "$id" == \#* ]] && continue
  curl -sSf -G "https://ssd-api.jpl.nasa.gov/sbdb.api" --data-urlencode "sstr=$des" \
    --data-urlencode "phys-par=1" -o "$dir/sbdb/$id.json"
  curl -sSf -G "https://ssd.jpl.nasa.gov/api/horizons.api" \
    --data-urlencode "format=text" --data-urlencode "COMMAND='$cmd'" \
    --data-urlencode "OBJ_DATA='NO'" --data-urlencode "MAKE_EPHEM='YES'" \
    --data-urlencode "EPHEM_TYPE='VECTORS'" --data-urlencode "CENTER='500@0'" \
    --data-urlencode "START_TIME='2040-01-01'" --data-urlencode "STOP_TIME='2040-01-02'" \
    --data-urlencode "STEP_SIZE='1 d'" --data-urlencode "REF_PLANE='ECLIPTIC'" \
    --data-urlencode "REF_SYSTEM='ICRF'" --data-urlencode "OUT_UNITS='KM-S'" \
    --data-urlencode "VEC_TABLE='2'" --data-urlencode "CSV_FORMAT='YES'" -o "$dir/state/$id.txt"
  sleep 0.3
done < "$here/candidates.txt"
