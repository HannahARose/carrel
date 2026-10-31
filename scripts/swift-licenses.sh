#!/usr/bin/env bash
# Appends the license notices of a Swift package's dependencies to OUT: each package SwiftPM
# checked out for it, and the C and C++ libraries MLX and mlx-swift-lm vendor (mlx, mlx-c,
# metal-cpp, fmt, nlohmann/json, XGrammar). For carrel-provider-mlx, whose dependencies
# cargo-about doesn't see (scripts/third-party-licenses.sh covers the Rust programs).
#
#   scripts/swift-licenses.sh PACKAGE_DIR OUT
#
# The package's dependencies must be resolved (swift package resolve, or a build).
set -euo pipefail

package="$1"
out="$2"
checkouts="$package/.build/checkouts"
if [ ! -d "$checkouts" ]; then
  echo "swift-licenses: $checkouts is missing; resolve the package first" >&2
  exit 1
fi

notice() {
  local name="$1" file="$2"
  {
    printf '\n### %s\n\n```text\n' "$name"
    cat "$file"
    printf '\n```\n'
  } >> "$out"
}

printf '\n## Swift packages in carrel-provider-mlx\n' >> "$out"
for dir in "$checkouts"/*/; do
  name="$(basename "$dir")"
  found=0
  for file in "$dir"LICENSE* "$dir"NOTICE* "$dir"COPYING*; do
    [ -f "$file" ] || continue
    notice "$name ($(basename "$file"))" "$file"
    found=1
  done
  if [ "$found" = 0 ]; then
    echo "swift-licenses: no license file in $name" >&2
    exit 1
  fi
done

printf '\n## C and C++ libraries vendored by MLX\n' >> "$out"
vendored=(
  "mlx-swift/Source/Cmlx/mlx/LICENSE MLX"
  "mlx-swift/Source/Cmlx/mlx-c/LICENSE mlx-c"
  "mlx-swift/Source/Cmlx/metal-cpp/LICENSE.txt metal-cpp"
  "mlx-swift/Source/Cmlx/fmt/LICENSE fmt"
  "mlx-swift/Source/Cmlx/json/LICENSE.MIT nlohmann/json"
  "mlx-swift-lm/Libraries/MLXCXGrammar/xgrammar/LICENSE XGrammar"
)
for entry in "${vendored[@]}"; do
  read -r file name <<< "$entry"
  if [ ! -f "$checkouts/$file" ]; then
    echo "swift-licenses: no $file for $name" >&2
    exit 1
  fi
  notice "$name" "$checkouts/$file"
done
