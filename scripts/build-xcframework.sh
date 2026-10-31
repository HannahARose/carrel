#!/usr/bin/env bash
# Builds carrel-ffi for macOS, iOS and the iOS Simulator (Apple silicon only), packages it as
# apple/CarrelKit/CarrelFFI.xcframework, and regenerates the Swift bindings in the CarrelKit
# package (scripts/generate-bindings.sh). Run it after changing carrel-ffi. The framework
# isn't committed; the bindings are.
#
#   scripts/build-xcframework.sh            release build
#   PROFILE=dev scripts/build-xcframework.sh  faster debug build
#
# Needs the Apple targets:
#   rustup target add aarch64-apple-ios aarch64-apple-ios-sim
set -euo pipefail

cd "$(dirname "$0")/.."
profile="${PROFILE:-release}"
case "$profile" in
  dev) out=debug ;;
  *) out="$profile" ;;
esac
# Match the minimum versions in apple/CarrelKit/Package.swift, so Rust and the bundled C
# code (libgit2) are built for them rather than for this Mac.
export MACOSX_DEPLOYMENT_TARGET=15.0
export IPHONEOS_DEPLOYMENT_TARGET=18.0
targets="aarch64-apple-darwin aarch64-apple-ios aarch64-apple-ios-sim"
package=apple/CarrelKit
work=target/xcframework

installed="$(rustup target list --installed)"
for target in $targets; do
  if ! grep -qx "$target" <<<"$installed"; then
    echo "The $target Rust target isn't installed. Add it with:" >&2
    echo "  rustup target add $target" >&2
    exit 1
  fi
done

for target in $targets; do
  cargo build --locked -p carrel-ffi --lib --profile "$profile" --target "$target"
done

# The Swift side comes from the macOS library, which carries the interface metadata.
LIBRARY="target/aarch64-apple-darwin/$out/libcarrel_ffi.dylib" scripts/generate-bindings.sh

rm -rf "$package/CarrelFFI.xcframework"
args=()
for target in $targets; do
  args+=(-library "target/$target/$out/libcarrel_ffi.a" -headers "$work/headers")
done
xcodebuild -create-xcframework "${args[@]}" -output "$package/CarrelFFI.xcframework" >/dev/null
echo "Built $package/CarrelFFI.xcframework and the Swift bindings ($profile)"
