#!/usr/bin/env bash
# Decides which of CI's areas a change touches, for the "Changes" job in ci.yml. Writes one
# `<area>=true|false` line per area to $GITHUB_OUTPUT, and a summary to standard output, plus:
#   cache=true|false  restore caches (false for pull requests into main: a clean build)
#   save=true|false   save caches (only on dev, outside pull requests)
#
# Everything runs for the weekly schedule, a manual run, a pull request or merge queue group
# into main, a change to any workflow, or when the changed files can't be worked out.
#
# Reads EVENT (github.event_name), BASE_REF (the pull request's or merge queue group's base
# branch), REF (github.ref), BEFORE (github.event.before), BASE_SHA
# (github.event.merge_group.base_sha) and SHA (github.sha). Needs the full history checked out. To try it
# locally: EVENT=push BEFORE=<commit> SHA=HEAD .github/scripts/ci-changes.sh
set -euo pipefail

out=${GITHUB_OUTPUT:-/dev/null}
all=false
files=()
# A merge queue names its base as refs/heads/<branch>.
base=${BASE_REF:-}
base=${base#refs/heads/}
into_main=false
if { [ "$EVENT" = pull_request ] || [ "$EVENT" = merge_group ]; } && [ "$base" = main ]; then
  into_main=true
fi

if [ "$into_main" = true ]; then
  echo "A $EVENT into main: running everything, from a clean build."
  all=true
elif [ "$EVENT" = pull_request ]; then
  # The checkout is GitHub's merge of the pull request into its base, so the first parent is
  # the base as merged against.
  range=(HEAD^1 HEAD)
elif [ "$EVENT" = merge_group ] && [ -n "${BASE_SHA:-}" ] \
  && git cat-file -e "$BASE_SHA^{commit}" 2> /dev/null; then
  # The queue's combined commit, against the base branch it will land on.
  range=("$BASE_SHA" "$SHA")
elif [ "$EVENT" = push ] && [ -n "${BEFORE:-}" ] && [ "${BEFORE//0/}" != "" ] \
  && git cat-file -e "$BEFORE^{commit}" 2> /dev/null; then
  range=("$BEFORE" "$SHA")
else
  echo "A $EVENT run: running everything."
  all=true
fi

if [ "$all" = false ]; then
  # Both sides of a rename count. A failed diff fails the job, and so "CI passed".
  changed=$(git diff --name-only --no-renames "${range[@]}")
  # A loop rather than mapfile, so the script also runs with macOS's bash 3.
  while IFS= read -r file; do
    [ -n "$file" ] && files+=("$file")
  done <<< "$changed"
  echo "Changed files:"
  printf '  %s\n' ${files[@]+"${files[@]}"}
fi

# Whether any changed file matches one of the patterns. In [[ ]], * also matches /.
touches() {
  local file pattern
  for file in ${files[@]+"${files[@]}"}; do
    for pattern in "$@"; do
      # shellcheck disable=SC2053 # the pattern is a glob on purpose
      [[ $file == $pattern ]] && return 0
    done
  done
  return 1
}

if [ "$all" = false ] && touches '.github/workflows/*'; then
  echo "A workflow changed: running everything."
  all=true
fi

area() {
  local name=$1 value=false
  shift
  if [ "$all" = true ] || touches "$@"; then
    value=true
  fi
  echo "$name=$value" >> "$out"
  echo "$name: $value"
}

# What every Rust build depends on.
cargo=(Cargo.toml Cargo.lock rust-toolchain.toml '.cargo/*')
core=('crates/carrel-core/*' "${cargo[@]}")
# The XCFramework and its Swift bindings.
ffi=("${core[@]}" 'crates/carrel-ffi/*' 'crates/uniffi-bindgen/*' 'apple/CarrelKit/*'
  scripts/build-xcframework.sh scripts/generate-bindings.sh)

# Clippy, the tests on three platforms and rustdoc. The core's tests read the fixtures, the
# format and card schema, and two Swift files that copy the model catalog.
area rust 'crates/*' "${cargo[@]}" 'fixtures/*' FORMAT.md 'schemas/*' \
  apple/CarrelMLX/Sources/CarrelMLXProvider/EmbeddingCatalog.swift \
  apple/CarrelProviders/Sources/CarrelAppleProviders/SentenceEmbedding.swift
# CarrelKit, then the Apple provider, through the core's tests and `carrel providers`.
area swift "${ffi[@]}" 'apple/CarrelProviders/*' 'crates/carrel-cli/*' 'fixtures/*'
area app "${ffi[@]}" 'apple/CarrelApp/*'
area mlx 'apple/CarrelMLX/*' 'apple/CarrelProviders/*'
area swift_format 'apple/*' .swift-format scripts/swift-format.sh
# Each provider is tested on its own and, for llama.cpp, through `carrel providers`.
area llama "${core[@]}" 'crates/carrel-provider-llama/*' 'crates/carrel-cli/*'
area cad "${core[@]}" 'crates/carrel-provider-cad/*' 'crates/carrel-provider-extract/*'
area extract "${core[@]}" 'crates/carrel-provider-extract/*'
area kopia "${core[@]}"
area editors 'editors/*' 'crates/carrel-cli/*'
area deny Cargo.toml Cargo.lock 'crates/*/Cargo.toml' deny.toml
area workflows '.github/*'

cache=true
if [ "$into_main" = true ]; then
  cache=false
fi
# A merge queue group's ref is refs/heads/gh-readonly-queue/..., so it saves nothing either.
save=false
if [ "$EVENT" != pull_request ] && [ "${REF:-}" = refs/heads/dev ]; then
  save=true
fi
echo "cache=$cache" >> "$out"
echo "save=$save" >> "$out"
echo "cache: $cache, save: $save"
