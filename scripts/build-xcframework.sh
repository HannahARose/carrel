#!/usr/bin/env bash
# Builds carrel-ffi for macOS, iOS and the iOS Simulator, packages it as
# apple/CarrelKit/CarrelFFI.xcframework, and generates the Swift bindings into the
# CarrelKit package. Run it after changing carrel-ffi; the outputs aren't committed.
#
#   scripts/build-xcframework.sh            release build
#   PROFILE=dev scripts/build-xcframework.sh  faster debug build
#
# Needs the Apple targets: rustup target add aarch64-apple-ios aarch64-apple-ios-sim
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

for target in $targets; do
  cargo build --locked -p carrel-ffi --lib --profile "$profile" --target "$target"
done

# Generate the Swift side from the macOS library, which carries the interface metadata.
rm -rf "$work"
mkdir -p "$work/generated" "$work/headers"
cargo run --locked -q -p uniffi-bindgen -- generate \
  --library "target/aarch64-apple-darwin/$out/libcarrel_ffi.dylib" \
  --language swift --out-dir "$work/generated"
cp "$work/generated/carrel_ffiFFI.h" "$work/headers/"
cp "$work/generated/carrel_ffiFFI.modulemap" "$work/headers/module.modulemap"
mkdir -p "$package/Sources/CarrelKit/Generated"
cp "$work/generated/carrel_ffi.swift" "$package/Sources/CarrelKit/Generated/"

rm -rf "$package/CarrelFFI.xcframework"
args=()
for target in $targets; do
  args+=(-library "target/$target/$out/libcarrel_ffi.a" -headers "$work/headers")
done
xcodebuild -create-xcframework "${args[@]}" -output "$package/CarrelFFI.xcframework" >/dev/null
echo "Built $package/CarrelFFI.xcframework and the Swift bindings ($profile)"
