#!/bin/sh
# Tests the editor plugins against the fake carrel in bin/: the Neovim plugin with
# `nvim --headless`, and the Emacs package with ERT, a byte-compile with warnings as errors,
# checkdoc, and package-lint when it's installed. `run.sh nvim` or `run.sh emacs` runs one.
# HOME and the XDG folders point to a temporary folder, so nothing writes your own settings.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
editors=$(dirname "$here")
which=${1:-all}

scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
export HOME="$scratch/home"
export XDG_CONFIG_HOME="$scratch/config" XDG_DATA_HOME="$scratch/data"
export XDG_STATE_HOME="$scratch/state" XDG_CACHE_HOME="$scratch/cache"
mkdir -p "$HOME" "$XDG_CONFIG_HOME" "$XDG_DATA_HOME" "$XDG_STATE_HOME" "$XDG_CACHE_HOME"
export CARREL_FAKE_LOG="$scratch/calls"
export PATH="$here/bin:$PATH"

if [ "$which" = all ] || [ "$which" = nvim ]; then
  echo "Neovim: $(nvim --version | head -n 1)"
  nvim --headless --clean -i NONE -l "$editors/nvim/tests/run.lua"
fi

if [ "$which" = all ] || [ "$which" = emacs ]; then
  echo "Emacs: $(emacs --version | head -n 1)"
  emacs --batch -Q -L "$editors/emacs" -l "$editors/emacs/test/carrel-test.el" \
    -f ert-run-tests-batch-and-exit
  # The compiled file goes to the scratch folder, not beside the source.
  emacs --batch -Q -L "$editors/emacs" -l "$here/lint.el" \
    --eval "(carrel-test-lint \"$editors/emacs/carrel.el\" \"$scratch/carrel.elc\")"
fi
