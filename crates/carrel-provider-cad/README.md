# carrel-provider-cad

Carrel's CAD provider ([protocol](../../docs/providers.md), [design](../../docs/design/models-3d.md)): STEP and IGES models read through [OpenCascade](https://dev.opencascade.org), meshed coarsely, measured and drawn.

- **The text:** the file's header and part names (read as `carrel-provider-extract` reads them), then its size in millimetres, how many solids it has, and their volume.
- **The drawing:** four views (angled, front, top, side), grey on white, 512 pixels each in one picture. A small software renderer draws them, so no GPU is needed and they look the same everywhere. The picture goes back for an image reader to describe (`pictures` in the protocol); Carrel deletes it afterwards.
- **Without OpenCascade** it still builds, and offers drawing as unavailable, saying how to install it.

## Building

Install OpenCascade, then the provider:

```bash
brew install opencascade
```

On Debian or Ubuntu:

```bash
sudo apt install libocct-data-exchange-dev libocct-modeling-algorithms-dev
```

```bash
cargo install --path crates/carrel-provider-cad --locked
```

`build.rs` looks for OpenCascade in Homebrew's and the system's usual places. Set `OCCT_INCLUDE_DIR` and `OCCT_LIB_DIR` for another install. A C++17 compiler builds the shim (`shim/carrel_cad.cpp`), which is the only code that calls OpenCascade.

OpenCascade is licensed under the LGPL 2.1 and is linked dynamically, from where it's installed; it isn't built into Carrel.

## Tests

`tests/data` has an invented bracket and pin as STEP and IGES, written by `tests/data/make.cpp`. Without OpenCascade the drawing tests check only that the provider says how to install it, unless `CARREL_REQUIRE_OPENCASCADE` is set, as in CI.
