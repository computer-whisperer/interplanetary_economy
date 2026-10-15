#!/usr/bin/env bash
# Downloads the shape models data/bodies/*/shape.bin are built from into
# DIR (~25 MB; M1c.md, step 6, part 9 plan, item 3). Public domain (NASA
# PDS) but Ryugu's (JAXA: credit ISAS/JAXA, modifications stated):
# - Phobos: Gaskell 2006 (PDS SBN, gaskell.phobos.shape-model), 49,152
#   plates;
# - Deimos: Thomas 1993's radius grid, 5 degrees (PDS SBN,
#   ast-sat.thomas.shape-models);
# - Ryugu: Watanabe et al. 2019's structure-from-motion model, 49,152
#   facets (JAXA DARTS);
# - Bennu: OSIRIS-REx's SPOv54 (stereophotoclinometry and lidar), 49,152
#   facets (PDS SBN, orex.altimetry);
# - Itokawa: Gaskell et al. 2008, 49,152 plates (PDS SBN);
# - Didymos: DART's v003, 49,152 facets (PDS SBN, dart_shapemodel);
# - Apophis: Brozovic et al. 2018's radar model, 3,996 facets (PDS SBN);
# - 2008 EV5: Busch et al. 2011's radar model, 3,996 facets, and Nereus:
#   Brozovic et al. 2009's, 2,292 facets (PDS SBN, the JPL radar shape
#   models compiled by Lawrence and Benner).
# Usage: tools/shapes/fetch.sh DIR
set -euo pipefail
dir=${1:?usage: fetch.sh DIR}
mkdir -p "$dir"
# Each file whole or not at all: an interrupted download is fetched again,
# and a web page in its place (a moved link) is an error.
get() {
  [ -s "$dir/$2" ] && return
  curl -sSfL --retry 5 --retry-all-errors --max-time 300 -o "$dir/$2.part" "$1"
  if [ "$(head -c 1 "$dir/$2.part")" = "<" ]; then
    echo "$1: a web page, not data" >&2
    return 1
  fi
  mv "$dir/$2.part" "$dir/$2"
}
sbn=https://sbnarchive.psi.edu/pds4
get $sbn/non_mission/gaskell.phobos.shape-model/data/phobos_ver64q.tab phobos.tab
get $sbn/non_mission/ast-sat.thomas.shape-models_V1_0/data/m2deimos.tab deimos.tab
get https://data.darts.isas.jaxa.jp/pub/hayabusa2/paper/Watanabe_2019/SHAPE_SFM_49k_v20180804.obj ryugu.obj
get $sbn/orex/orex.altimetry/data_derived_altimetry_global_models/global_digital_terrain_models/SPOv54/g_06320mm_spo_obj_0000n00000_v054.obj bennu.obj
get $sbn/non_mission/gaskell.ast-itokawa.shape-model_V1_1/data/vertex/ver64q.tab itokawa.tab
get https://pdssbn.astro.umd.edu/holdings/pds4-dart_shapemodel-v1.0/data_derived_didymos_model_v003/didymos_g_9309mm_spc_obj_0000n00000_v003.obj didymos.obj
get $sbn/non_mission/gbo.ast-apophis.jpl.radar.shape_model_v1.0/data/apophis_v233s7.obj apophis.obj
radar=$sbn/non_mission/gbo.ast.jpl.radar.shape_models_V1_0/data
get $radar/a341843_2008ev5.obj 2008-ev5.obj
get $radar/a4660nereus.obj nereus.obj
