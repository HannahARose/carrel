#!/usr/bin/env bash
# Writes the third-party license notices for release builds: every Rust crate's license, from
# cargo-about (about.toml, about.hbs), then the licenses of the C libraries some crates build
# in (libgit2, zlib, llama.cpp), which cargo-about doesn't see.
#
#   scripts/third-party-licenses.sh OUT
#
# Needs cargo-about: cargo install --locked cargo-about --version 0.9.2
set -euo pipefail

cd "$(dirname "$0")/.."
out="$1"
cargo about generate --workspace --locked --fail about.hbs > "$out"

# Each vendored library: the crate that builds it in, the license file inside that crate, and
# a name for people.
vendored=(
  "libgit2-sys libgit2/COPYING libgit2"
  "libz-sys src/zlib/LICENSE zlib"
  "llama-cpp-sys-2 llama.cpp/LICENSE llama.cpp"
)
metadata="$(cargo metadata --format-version 1 --locked)"
printf '\n## C libraries built into Rust crates\n' >> "$out"
for entry in "${vendored[@]}"; do
  read -r crate file name <<< "$entry"
  dir="$(jq -r --arg crate "$crate" \
    '[.packages[] | select(.name == $crate)][0].manifest_path // empty | sub("/Cargo.toml$"; "")' \
    <<< "$metadata")"
  if [ -z "$dir" ] || [ ! -f "$dir/$file" ]; then
    echo "third-party-licenses: no $file in $crate" >&2
    exit 1
  fi
  {
    printf '\n### %s (in %s)\n\n```text\n' "$name" "$crate"
    cat "$dir/$file"
    printf '```\n'
  } >> "$out"
done
echo "Wrote $out"
