#!/usr/bin/env bash
# Renders the screens of layout.html with Chromium. Run it from the root of
# the repository. The pictures go to docs/mockups/duo-layout/screens.
set -euo pipefail

chrome=${CHROME:-chromium}
dir=$(cd docs/mockups/duo-layout && pwd)
mkdir -p "$dir/screens"

poses=(
  "outer-portrait 466,678"
  "outer-landscape 678,466"
  "inner-portrait 669,951"
  "inner-landscape 951,669"
  "split 951,669"
  "tabletop 669,951"
  "book 951,669"
)

for workspace in globe atlas; do
  for entry in "${poses[@]}"; do
    read -r pose size <<<"$entry"
    "$chrome" --headless --no-sandbox --allow-file-access-from-files --hide-scrollbars \
      --force-device-scale-factor=2 --window-size="$size" --virtual-time-budget=2000 \
      --screenshot="$dir/screens/$workspace-$pose.png" "file://$dir/layout.html#$workspace-$pose" \
      2>/dev/null
  done
done
