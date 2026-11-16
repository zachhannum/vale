#!/usr/bin/env bash
# Renders the screens of toolbox.html with Chromium. Run it from the root of
# the repository. The pictures go to docs/mockups/pad-toolbox/screens.
set -euo pipefail

chrome=${CHROME:-chromium}
dir=$(cd docs/mockups/pad-toolbox && pwd)
mkdir -p "$dir/screens"

for screen in grid page; do
  "$chrome" --headless --no-sandbox --allow-file-access-from-files --hide-scrollbars \
    --force-device-scale-factor=2 --window-size=1194,834 --virtual-time-budget=2000 \
    --screenshot="$dir/screens/$screen.png" "file://$dir/toolbox.html#$screen" \
    2>/dev/null
done
