#!/bin/sh
# Builds the globe prototype, installs it on the connected iPad, and starts it.
# You do not need to open Xcode.
#
# Usage: scripts/globe-proto-ipad.sh
# Set DEVELOPMENT_TEAM to use a different Apple developer team.
exec "$(dirname "$0")/ipad-run.sh" vale-globe-proto ValeGlobeProto dev.vale.globe-proto "Vale Globe"
