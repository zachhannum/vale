#!/usr/bin/env bash
# Renders the screens of collections.html with Chromium. Run it from the root
# of the repository. The pictures go to docs/mockups/collections/screens.
set -euo pipefail

chrome=${CHROME:-chromium}
dir=$(cd docs/mockups/collections && pwd)
mkdir -p "$dir/screens"

for screen in layers layers-narrow pick collection-page add-layers add-layers-narrow \
  layer-collections polygonize polygonize-pick atlas-edit globe-map pick-maps \
  manage-collections manage-collections-narrow manage-layers manage-layers-narrow; do
  size=1194,834
  case "$screen" in *-narrow) size=375,834 ;; esac
  "$chrome" --headless=new --no-sandbox --disable-gpu --allow-file-access-from-files --hide-scrollbars \
    --force-device-scale-factor=2 --window-size=$size --virtual-time-budget=1500 \
    --screenshot="$dir/screens/$screen.png" "file://$dir/collections.html#$screen" \
    >/dev/null 2>&1
done
