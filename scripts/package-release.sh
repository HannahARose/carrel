#!/usr/bin/env bash
# Packages one platform's release archive from already-built binaries: carrel, the network
# helper (carrel-fetch), the llama.cpp and extraction providers, the pinned Kopia binary (downloaded and
# checked against its SHA-256), and on macOS the Apple provider. Run by
# .github/workflows/release.yml; it can be run by hand to check an archive.
#
#   scripts/package-release.sh NAME BIN_DIR
#
# NAME is the archive's platform name, such as macos-universal or linux-x86_64. BIN_DIR holds
# the built binaries. The environment gives the version and the Kopia download:
#
#   VERSION         such as 0.2.0
#   KOPIA_ASSET     such as kopia-0.23.1-linux-x64 (the folder inside Kopia's archive)
#   KOPIA_SHA256    the archive's SHA-256, from Kopia's checksums.txt
#   KOPIA_ARCHIVE   tar.gz or zip
#   APPLE_PROVIDER  optional: the built carrel-provider-apple, for macOS
#   MLX_PROVIDER    optional: the built carrel-provider-mlx (Apple silicon), for macOS; the
#                   mlx-swift_Cmlx.bundle beside it, which holds MLX's shaders, goes with it
#   THIRD_PARTY     optional: the license notices from scripts/third-party-licenses.sh
#
# The archive is written to dist/carrel-VERSION-NAME.tar.gz (.zip on Windows).
set -euo pipefail

cd "$(dirname "$0")/.."
name="$1"
bin_dir="$2"
: "${VERSION:?}" "${KOPIA_ASSET:?}" "${KOPIA_SHA256:?}" "${KOPIA_ARCHIVE:?}"
kopia_version="${KOPIA_ASSET#kopia-}"
kopia_version="${kopia_version%%-*}"

exe=""
if [ "$KOPIA_ARCHIVE" = zip ]; then
  exe=".exe"
fi
stage="dist/carrel-$VERSION-$name"
rm -rf "$stage"
mkdir -p "$stage"

for program in carrel carrel-fetch carrel-provider-llama carrel-provider-extract; do
  cp "$bin_dir/$program$exe" "$stage/"
done
if [ -n "${APPLE_PROVIDER:-}" ]; then
  cp "$APPLE_PROVIDER" "$stage/"
fi
if [ -n "${MLX_PROVIDER:-}" ]; then
  cp "$MLX_PROVIDER" "$stage/"
  cp -R "$(dirname "$MLX_PROVIDER")/mlx-swift_Cmlx.bundle" "$stage/"
fi

# Kopia, pinned and checked, beside carrel: Carrel looks there before PATH.
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
download="$work/kopia.$KOPIA_ARCHIVE"
curl -sSfL -o "$download" \
  "https://github.com/kopia/kopia/releases/download/v$kopia_version/$KOPIA_ASSET.$KOPIA_ARCHIVE"
if command -v sha256sum > /dev/null; then
  echo "$KOPIA_SHA256  $download" | sha256sum -c -
else
  echo "$KOPIA_SHA256  $download" | shasum -a 256 -c -
fi
if [ "$KOPIA_ARCHIVE" = zip ]; then
  unzip -q "$download" -d "$work"
else
  tar -xzf "$download" -C "$work"
fi
cp "$work/$KOPIA_ASSET/kopia$exe" "$stage/"
curl -sSfL -o "$stage/LICENSE-kopia" \
  "https://raw.githubusercontent.com/kopia/kopia/v$kopia_version/LICENSE"

cp LICENSE "$stage/LICENSE"
if [ -n "${THIRD_PARTY:-}" ]; then
  cp "$THIRD_PARTY" "$stage/THIRD-PARTY-LICENSES.md"
fi
cp README.md "$stage/README.md"
cat > "$stage/INSTALL.txt" <<EOF
Carrel $VERSION for $name

Put every program in this folder together in one folder on your PATH, such as
~/.local/bin, or C:\\Users\\you\\bin on Windows. Carrel finds its helpers (the network
helper, the providers and Kopia) beside the carrel program.

  carrel                  the command-line tool; run carrel --help
  carrel-fetch            the network helper, the only part that goes online
  carrel-provider-llama   local open models through llama.cpp; see carrel models
  carrel-provider-extract PDF text, and OCR when Tesseract is installed
  kopia                   Kopia $kopia_version, for carrel backup
EOF
if [ -n "${APPLE_PROVIDER:-}" ]; then
  echo "  carrel-provider-apple   Apple's on-device model, PDFKit and Vision" >> "$stage/INSTALL.txt"
fi
if [ -n "${MLX_PROVIDER:-}" ]; then
  cat >> "$stage/INSTALL.txt" <<'MLX'
  carrel-provider-mlx     local open models through MLX, on Apple-silicon Macs only; keep
                          mlx-swift_Cmlx.bundle beside it (see carrel models)
MLX
fi

cd dist
if [ "$KOPIA_ARCHIVE" = zip ]; then
  7z a -bso0 -tzip "carrel-$VERSION-$name.zip" "carrel-$VERSION-$name"
else
  tar -czf "carrel-$VERSION-$name.tar.gz" "carrel-$VERSION-$name"
fi
rm -rf "carrel-$VERSION-$name"
ls -l
