# Editor plugin tests

`run.sh` tests the Neovim plugin (`editors/nvim`) with `nvim --headless` and the Emacs package (`editors/emacs`) with `emacs --batch`: ERT tests, a byte-compile with warnings as errors, `checkdoc`, and `package-lint` when it's installed. Both run against `bin/carrel`, a fake that prints the JSON in `fixtures/`, with paths in `archive/`, so no real archive or `carrel` is needed. `HOME` and the XDG folders point to a temporary folder for the run, so nothing writes your own configuration.

```bash
editors/test/run.sh          # both
editors/test/run.sh nvim     # or one
```
