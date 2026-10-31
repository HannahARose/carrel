#!/usr/bin/env bash
# Generates the Swift side of carrel-ffi with uniffi-bindgen: the Swift bindings, committed in
# apple/CarrelKit/Sources/CarrelKit/Generated, and the C header and module map the
# XCFramework needs, written to target/xcframework/headers. Run it after changing carrel-ffi's
# interface and commit the result; CI fails while the committed bindings are out of date.
#
#   scripts/generate-bindings.sh            builds carrel-ffi for this machine first
#   LIBRARY=path scripts/generate-bindings.sh  reads the interface from a library already built
#
# scripts/build-xcframework.sh runs it too, so a full build also updates the bindings.
set -euo pipefail

cd "$(dirname "$0")/.."
work=target/xcframework
library="${LIBRARY:-}"
if [ -z "$library" ]; then
  cargo build --locked -p carrel-ffi --lib
  case "$(uname -s)" in
    Darwin) library=target/debug/libcarrel_ffi.dylib ;;
    *) library=target/debug/libcarrel_ffi.so ;;
  esac
fi

rm -rf "$work/generated" "$work/headers"
mkdir -p "$work/generated" "$work/headers"
cargo run --locked -q -p uniffi-bindgen -- generate \
  --library "$library" --language swift --out-dir "$work/generated"
cp "$work/generated/carrel_ffiFFI.h" "$work/headers/"
cp "$work/generated/carrel_ffiFFI.modulemap" "$work/headers/module.modulemap"
mkdir -p apple/CarrelKit/Sources/CarrelKit/Generated
cp "$work/generated/carrel_ffi.swift" apple/CarrelKit/Sources/CarrelKit/Generated/
echo "Generated the Swift bindings in apple/CarrelKit/Sources/CarrelKit/Generated"
