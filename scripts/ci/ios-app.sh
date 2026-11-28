#!/bin/sh
# Writes the crate and the Xcode project of an app of the workspace, as lines
# for GITHUB_ENV.
#
# Usage: scripts/ci/ios-app.sh APP
#   APP  vale or isotherm
set -eu
case "${1:-}" in
    vale) printf 'CRATE=vale-app\nPROJECT=ValeApp\n' ;;
    isotherm) printf 'CRATE=isotherm-app\nPROJECT=IsothermApp\n' ;;
    *) echo "Usage: $0 vale|isotherm" >&2; exit 2 ;;
esac
