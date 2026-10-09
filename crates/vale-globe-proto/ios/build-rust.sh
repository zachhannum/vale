#!/bin/sh
# Xcode runs this script before it links the app. It builds the Rust library
# for the device or for the simulator.
set -eu

case "${PLATFORM_NAME:-iphoneos}" in
    iphonesimulator) target=aarch64-apple-ios-sim ;;
    *) target=aarch64-apple-ios ;;
esac

# Xcode sets SDKROOT and other variables for iOS. They break the parts of the
# build that run on the Mac, so cargo gets a clean environment.
cd "$(dirname "$0")/../../.."
env -i HOME="$HOME" PATH="$HOME/.cargo/bin:/opt/homebrew/opt/rustup/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin" \
    cargo build --release -p vale-globe-proto --lib --target "$target"
