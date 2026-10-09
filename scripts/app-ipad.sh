#!/bin/sh
# Builds the app, installs it on the connected iPad, and starts it.
# You do not need to open Xcode.
#
# Usage: scripts/app-ipad.sh
# Set DEVELOPMENT_TEAM to use a different Apple developer team.
exec "$(dirname "$0")/ipad-run.sh" vale-app ValeApp dev.vale.app "Vale"
