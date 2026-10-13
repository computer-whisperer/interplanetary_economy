#!/usr/bin/env bash
# Downloads the Earth-Moon periodic-orbit families that data/orbits.toml
# samples, from JPL's Three-Body Periodic Orbits API, into DIR/<family>.json.
# Usage: tools/orbits/fetch.sh DIR
set -euo pipefail
dir=${1:?usage: fetch.sh DIR}
mkdir -p "$dir"
api="https://ssd-api.jpl.nasa.gov/periodic_orbits.api?sys=earth-moon"
fetch() { curl -sSf "$api&$2" -o "$dir/$1.json"; sleep 0.3; }
fetch l1-lyapunov "family=lyapunov&libr=1"
fetch l2-lyapunov "family=lyapunov&libr=2"
fetch l1-halo-north "family=halo&libr=1&branch=N"
fetch l1-halo-south "family=halo&libr=1&branch=S"
fetch l2-halo-north "family=halo&libr=2&branch=N"
fetch l2-halo-south "family=halo&libr=2&branch=S"
fetch dro "family=dro"
