#!/usr/bin/env bash
# Renders the screens of the inner screen of iPhone Duo with vale-app.
# Run it from the root of the repository. The pictures go to
# docs/mockups/duo-inner/screens.
set -euo pipefail

app=${VALE_APP:-target/release/vale-app}
out=docs/mockups/duo-inner/screens
mkdir -p "$out"
height=docs/mockups/duo-inner/assets/height.png

# The start screen lists the project files of this folder.
projects=$(mktemp -d)
for world in Kareth Vesperan; do
  "$app" --layout pad --project "$projects/$world.gpkg" --size 626x890 \
    --screenshot "$(mktemp --suffix=.png)" >/dev/null
done

# The screen is 1878 by 2670 pixels. At 3 pixels per point, it is 626 by
# 890 points. The safe areas are estimates.
portrait=(--size 626x890 --safe-area 32,0,20,0)
landscape=(--size 890x626 --safe-area 24,0,20,0)

screens=(
  "start|--start-screen --projects-dir $projects"
  "globe|"
  "globe-layers|--panel layers"
  "globe-dock|--panel layers --panel attributes --panel toolbox"
  "globe-dock-hidden|--panel layers --dock-hidden"
  "globe-modes|--import-heightmap $height --face-size 256 --brush-modes"
  "globe-panels-menu|--panels-menu"
  "globe-view-menu|--view-menu"
  "globe-worlds-menu|--worlds-menu"
  "globe-world-form|--world-form new"
  "globe-stops|--import-heightmap $height --face-size 256 --panel layers --stops-page"
  "globe-height|--import-heightmap $height --face-size 256 --panel layers"
  "globe-measure|--measure '0,0 30,20 60,-10'"
  "flat|--view flat"
  "flat-layers|--view flat --panel layers"
  "atlas|--workspace atlas"
  "atlas-frame|--workspace atlas --frame 1"
  "atlas-labels|--workspace atlas --frame 1 --panel labels"
  "atlas-pages|--workspace atlas --panel pages"
)

for entry in "${screens[@]}"; do
  name=${entry%%|*}
  flags=${entry#*|}
  args=()
  for orientation in portrait landscape; do
    if [ "$orientation" = portrait ]; then size=("${portrait[@]}"); else size=("${landscape[@]}"); fi
    eval "args=($flags)"
    "$app" --layout pad "${args[@]}" "${size[@]}" --pixel-ratio 2 \
      --screenshot "$out/$name-$orientation.png" ||
      echo "failed: $name-$orientation" >&2
  done
done
