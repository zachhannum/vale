#!/usr/bin/env bash
# Renders the screens of ramps.html with Chromium. Run it from the root of the
# repository. The pictures go to docs/mockups/pad-ramps/screens.
set -euo pipefail

chrome=${CHROME:-chromium}
dir=$(cd docs/mockups/pad-ramps && pwd)
mkdir -p "$dir/screens"

for screen in card-keep card-new editor editor-end; do
  "$chrome" --headless=new --no-sandbox --disable-gpu --allow-file-access-from-files --hide-scrollbars \
    --force-device-scale-factor=2 --window-size=1194,834 --virtual-time-budget=1500 \
    --screenshot="$dir/screens/$screen.png" "file://$dir/ramps.html#$screen" \
    >/dev/null 2>&1
done
