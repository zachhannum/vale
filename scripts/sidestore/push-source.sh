#!/bin/sh
# Writes the SideStore source and its page to the `gh-pages` branch.
# CI runs this script. It reads GITHUB_REPOSITORY and GH_TOKEN.
#
# Usage: scripts/sidestore/push-source.sh
set -eu

here="$(cd "$(dirname "$0")" && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

git init -q "$work"
cd "$work"
git remote add origin "https://x-access-token:${GH_TOKEN}@github.com/${GITHUB_REPOSITORY}.git"
git config user.name "github-actions[bot]"
git config user.email "41898282+github-actions[bot]@users.noreply.github.com"

# If a different job pushes at the same time, the push fails. The loop then
# reads the releases again, so the source does not lose a build.
attempt=1
while :; do
    if git fetch -q --depth 1 origin gh-pages 2>/dev/null; then
        git checkout -q -B gh-pages FETCH_HEAD
    else
        git checkout -q --orphan gh-pages
    fi
    python3 "$here/sidestore.py" source --out "$work"
    git add -A
    if git diff --cached --quiet; then
        echo "The source has no changes."
        exit 0
    fi
    git commit -q -m "Update the SideStore source"
    if git push -q origin gh-pages; then
        echo "Pushed the source."
        exit 0
    fi
    [ "$attempt" -lt 5 ] || { echo "The push failed 5 times." >&2; exit 1; }
    attempt=$((attempt + 1))
    sleep 2
done
