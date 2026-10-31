#!/usr/bin/env bash
# Formats the Swift under apple/ with swift-format, which comes with Xcode, using the
# settings in .swift-format. The generated bindings are left as uniffi-bindgen writes them.
#
#   scripts/swift-format.sh          format in place
#   scripts/swift-format.sh --check  only report what isn't formatted (CI)
set -euo pipefail

cd "$(dirname "$0")/.."
files=()
while IFS= read -r file; do
  files+=("$file")
done < <(find apple -name '*.swift' -not -path '*/.build/*' -not -path '*/Generated/*' | sort)

if [ "${1:-}" = --check ]; then
  xcrun swift-format lint --strict --parallel "${files[@]}"
else
  xcrun swift-format format --in-place --parallel "${files[@]}"
fi
