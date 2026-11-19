#!/usr/bin/env bash
# Renders the screens of stops.html with Chromium. Run it from the root of the
# repository. The pictures go to docs/mockups/pad-stops/screens.
set -euo pipefail

chrome=${CHROME:-chromium}
dir=$(cd docs/mockups/pad-stops && pwd)
mkdir -p "$dir/screens"

for screen in now rest many many-selected many-scrub; do
  "$chrome" --headless=new --no-sandbox --disable-gpu --allow-file-access-from-files --hide-scrollbars \
    --force-device-scale-factor=2 --window-size=1194,834 --virtual-time-budget=1500 \
    --screenshot="$dir/screens/$screen.png" "file://$dir/stops.html#$screen" \
    >/dev/null 2>&1
done
