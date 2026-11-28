#!/bin/sh
# Builds Isotherm, installs it on the connected iPad or iPhone, and starts it.
# You do not need to open Xcode.
#
# Usage: scripts/isotherm-ipad.sh
# Set DEVELOPMENT_TEAM to use a different Apple developer team.
exec "$(dirname "$0")/ipad-run.sh" isotherm-app IsothermApp dev.vale.isotherm "Isotherm"
