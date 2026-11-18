#!/usr/bin/env bash
# Renders the screens of strip.html with Chromium. Run it from the root of
# the repository. The pictures go to docs/mockups/pad-brush-strip/screens.
set -euo pipefail

chrome=${CHROME:-chromium}
dir=$(cd docs/mockups/pad-brush-strip && pwd)
mkdir -p "$dir/screens"

shoot() {
  "$chrome" --headless --no-sandbox --allow-file-access-from-files --hide-scrollbars \
    --force-device-scale-factor=2 --window-size="$1" --virtual-time-budget=2000 \
    --screenshot="$dir/screens/$2.png" "file://$dir/$3" 2>/dev/null
}
for screen in panel swap; do
  shoot 1194,834 "$screen" "strip.html#$screen"
done
