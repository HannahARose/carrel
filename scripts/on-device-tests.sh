#!/bin/sh
# The tests that need a real Mac with Apple Intelligence and local models (#45): the Apple
# provider and its on-device model, the llama.cpp provider with a real model, and a short
# benchmark that drafts a few items with every installed model. CI runs it on the
# self-hosted runner (.github/workflows/on-device.yml); run it by hand on any Apple-silicon Mac.
#
#     scripts/on-device-tests.sh
#
# Models come from CARREL_MODELS_DIR, else Carrel's models folder for this user. A model
# that isn't installed is skipped, not failed.
set -eu

cd "$(dirname "$0")/.."
export KOPIA_CHECK_FOR_UPDATES=false

echo "== Building"
cargo build --release --locked -p carrel-cli -p carrel-provider-extract -p carrel-provider-llama
swift build -c release --package-path apple/CarrelProviders --product carrel-provider-apple
bin="$(pwd)/target/release"
cp "$(swift build -c release --package-path apple/CarrelProviders --show-bin-path)/carrel-provider-apple" "$bin/"

echo "== Apple provider"
(cd apple/CarrelProviders && swift test)
CARREL_TEST_APPLE_PROVIDER="$bin/carrel-provider-apple" \
    cargo test --release --locked -p carrel-core --test apple_provider

echo "== llama.cpp provider"
models="${CARREL_MODELS_DIR:-}"
if [ -z "$models" ]; then
    models="$("$bin/carrel" --json models list | python3 -c 'import json, sys; print(json.load(sys.stdin)["folder"])')"
fi
CARREL_TEST_LLAMA_MODELS="$models" cargo test --release --locked -p carrel-provider-llama

echo "== Drafting with every installed model"
out="$(mktemp -d)"
"$bin/carrel" bench --fresh --limit 5 --out "$out"
cat "$out/report.md"
