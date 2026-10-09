#!/bin/sh
# Builds the globe prototype, installs it on the connected iPad, and starts it.
# You do not need to open Xcode.
#
# Usage: scripts/globe-proto-ipad.sh
# Set DEVELOPMENT_TEAM to use a different Apple developer team.
set -eu

root="$(cd "$(dirname "$0")/.." && pwd)"
ios="$root/crates/vale-globe-proto/ios"
derived="$root/target/globe-proto/xcode"
bundle_id=dev.vale.globe-proto

fail() {
    printf '\nStopped: %s\n' "$1" >&2
    exit 1
}

command -v xcodegen >/dev/null || fail "xcodegen is missing. Run: brew install xcodegen"
rustup target list --installed | grep -qx aarch64-apple-ios ||
    fail "The Rust target for iOS is missing. Run: rustup target add aarch64-apple-ios"

# The team comes from a signing certificate in the keychain.
team="${DEVELOPMENT_TEAM:-}"
for name in "Apple Development" "Developer ID Application"; do
    if [ -z "$team" ]; then
        team="$(security find-certificate -a -c "$name" -p 2>/dev/null |
            openssl x509 -noout -subject 2>/dev/null |
            sed -n 's/.*OU *= *\([A-Z0-9]\{10\}\).*/\1/p' | head -1)"
    fi
done
[ -n "$team" ] || fail "No Apple developer team found. Open Xcode, then Settings, then Accounts, and sign in. Then run this script again."

echo "1 of 4: Find the iPad"
devices="$(mktemp)"
xcrun devicectl list devices --json-output "$devices" >/dev/null 2>&1 || true
udid="$(python3 - "$devices" <<'EOF'
import json, sys
try:
    devices = json.load(open(sys.argv[1]))["result"]["devices"]
except Exception:
    devices = []
for d in devices:
    hardware = d.get("hardwareProperties", {})
    connection = d.get("connectionProperties", {})
    if hardware.get("deviceType") != "iPad":
        continue
    if connection.get("tunnelState") == "unavailable":
        continue
    print(hardware.get("udid") or d.get("identifier"))
    break
EOF
)"
rm -f "$devices"
[ -n "$udid" ] || fail "No iPad found. Connect the iPad with a cable, unlock it, and tap Trust. Then run this script again."

echo "2 of 4: Build the app (the first build takes a few minutes)"
cd "$ios"
xcodegen generate --quiet
log="$root/target/globe-proto/xcodebuild.log"
if ! xcodebuild -project ValeGlobeProto.xcodeproj -scheme ValeGlobeProto \
    -configuration Release -destination "id=$udid" -derivedDataPath "$derived" \
    -allowProvisioningUpdates -allowProvisioningDeviceRegistration \
    DEVELOPMENT_TEAM="$team" build >"$log" 2>&1; then
    grep -E "error:|error\[|^env: |is not installed|Developer Mode|nonzero exit code" "$log" | sort -u | head -20 >&2
    if grep -q "Developer Mode disabled" "$log"; then
        fail "Developer Mode is off on the iPad. On the iPad, open Settings, then Privacy & Security, then Developer Mode. Turn it on, tap Restart, and after the restart tap Turn On. Then run this script again."
    fi
    if grep -q "Unable to log in with account" "$log"; then
        fail "Xcode must sign in to your Apple account again. Open the Xcode app. In the menu bar, select Xcode, then Settings, then Accounts. Select your account, and sign in again. Then run this script again."
    fi
    if grep -q "is not installed" "$log"; then
        fail "Xcode does not have the iOS platform. Run: xcodebuild -downloadPlatform iOS"
    fi
    fail "The build failed. The full log is in $log"
fi

echo "3 of 4: Install the app on the iPad"
xcrun devicectl device install app --device "$udid" \
    "$derived/Build/Products/Release-iphoneos/ValeGlobeProto.app" >/dev/null ||
    fail "The install failed. Unlock the iPad and make sure that Developer Mode is on."

echo "4 of 4: Start the app"
xcrun devicectl device process launch --device "$udid" "$bundle_id" >/dev/null ||
    fail "The app is installed, but it did not start. Unlock the iPad and tap the Vale Globe icon."

echo "Done. Vale Globe runs on the iPad."
