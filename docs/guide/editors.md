# Neovim and Emacs

Search the archive and read cards without leaving your editor. The Neovim plugin (`editors/nvim`) and the Emacs package (`editors/emacs/carrel.el`) come with Carrel and are versioned with it. Both run the `carrel` program and read its `--json` output ([Commands](commands.md#archives)), so they search exactly as `carrel search` does, words, meaning and filters alike, and need nothing else running.

You need `carrel` on your `PATH` (or set where it is, below), and an archive: the plugins use the archive the current folder is in, else the one chosen with `carrel open`, unless you set one.

## Neovim

Neovim 0.9 or later. No other plugins are needed; [Telescope](https://github.com/nvim-telescope/telescope.nvim) is used for results when it's installed.

| Command | What it does |
| --- | --- |
| `:CarrelSearch QUERY` | Searches, and lists the results with the passage that matched: in Telescope if it's installed, else in the quickfix list. Choosing one opens its file; a card-only item opens its card, a collection its design document, a glossary fact the glossary at its term. Filters go straight to `carrel search`: `:CarrelSearch tag:receipts after:2025-01 desk chair`. |
| `:CarrelSearch! QUERY` | The same, but choosing a result opens its card. |
| `:CarrelShow [PATH]` | Shows the card of the item or card in the current buffer (or `PATH`) in a split: its fields, summary, problems and history. In it, `o` opens the item and `c` the card file beside it, and `q` closes it. |
| `:CarrelIndex` | Says how much of the archive the meaning index covers, from `carrel index status`. |

In Telescope, `Enter` opens the item and `Ctrl-e` its card, and the preview shows the card. With `ui = "float"`, results are listed in a floating window instead: `Enter` opens the item, `c` its card, `q` or `Esc` close it.

### Installing the Neovim plugin

The plugin is in the `editors/nvim` folder of the Carrel repository, so plugin managers are told to add that folder.

With [lazy.nvim](https://github.com/folke/lazy.nvim):

```lua
{
  "Horizon-Labs/carrel",
  cmd = { "CarrelSearch", "CarrelShow", "CarrelIndex" },
  config = function(plugin)
    vim.opt.rtp:append(plugin.dir .. "/editors/nvim")
    vim.cmd.runtime("plugin/carrel.lua")
    require("carrel").setup({})
  end,
}
```

With [packer.nvim](https://github.com/wbthomason/packer.nvim):

```lua
use({
  "Horizon-Labs/carrel",
  rtp = "editors/nvim",
  config = function()
    require("carrel").setup({})
  end,
})
```

By hand, link the folder into a package from a clone of the repository:

```bash
git clone https://github.com/Horizon-Labs/carrel ~/src/carrel
```

```bash
mkdir -p ~/.local/share/nvim/site/pack/carrel/start
```

```bash
ln -s ~/src/carrel/editors/nvim ~/.local/share/nvim/site/pack/carrel/start/carrel
```

Then `:helptags ALL` makes `:help carrel` work.

### Settings for Neovim

`setup()` takes these, all optional:

```lua
require("carrel").setup({
  cmd = { "carrel" },     -- the program, or a path to it, with any arguments
  archive = nil,          -- an archive's folder; nil finds it as `carrel` does
  ui = "auto",            -- "auto" (Telescope if installed), "telescope", "quickfix" or "float"
  split = "vsplit",       -- how :CarrelShow opens its window: "split", "vsplit", "tabnew"…
  open = function(path)   -- how a result's file is opened
    vim.cmd.edit(vim.fn.fnameescape(path))
  end,
})
```

Items such as PDFs and photos are better opened in their own application: on Neovim 0.10, `open = vim.ui.open` does that. A mapping for each command is up to you, for example `vim.keymap.set("n", "<leader>cs", ":CarrelSearch ")`.

## Emacs

Emacs 27.1 or later.

| Command | What it does |
| --- | --- |
| `M-x carrel-search` | Asks for a query and lists the results with `completing-read`, so vertico, ivy, helm and the default completion all work. Each is its title, a mark for sensitive and restricted items, its collection, date and kind, and the passage that matched with your words highlighted, in Carrel's order. Choosing one opens its file; with `C-u`, its card. Filters go straight to `carrel search`, and quoted phrases stay together: `tag:receipts collection:"Tax returns" 2024`. |
| `M-x carrel-show` | Shows the card of the file in the current buffer, or of the file at point in Dired (the folder itself, which may be an item, when point is on none), in another window: its fields, summary, problems and history. In it, `o` opens the item and `c` the card file in another window, `g` shows it again, and `q` closes it. |
| `M-x carrel-index-status` | Says how much of the archive the meaning index covers, from `carrel index status`. |

### Installing the Emacs package

With `use-package`, from a clone of the repository:

```elisp
(use-package carrel
  :load-path "~/src/carrel/editors/emacs"
  :commands (carrel-search carrel-show carrel-index-status)
  :bind (("C-c a s" . carrel-search)
         ("C-c a c" . carrel-show)))
```

On Emacs 30, `use-package` can fetch it itself:

```elisp
(use-package carrel
  :vc (:url "https://github.com/Horizon-Labs/carrel" :lisp-dir "editors/emacs")
  :bind (("C-c a s" . carrel-search)
         ("C-c a c" . carrel-show)))
```

With [straight.el](https://github.com/radian-software/straight.el):

```elisp
(straight-use-package
 '(carrel :type git :host github :repo "Horizon-Labs/carrel"
          :files ("editors/emacs/carrel.el")))
```

By hand, add the folder to `load-path` and load it: `(add-to-list 'load-path "~/src/carrel/editors/emacs")` and `(require 'carrel)`.

### Settings for Emacs

`M-x customize-group RET carrel` lists them:

| Setting | What it is |
| --- | --- |
| `carrel-program` | The `carrel` program: a name on `exec-path`, or a path (`"carrel"`) |
| `carrel-archive` | An archive's folder, or `nil` to find it as `carrel` does |
| `carrel-open-function` | How a result's file is opened (`find-file`); `browse-url-of-file` opens items such as PDFs in their own application |

The faces `carrel-match`, `carrel-title` and `carrel-about` color the matched words, titles, and where and when.

## How they work

Each command runs `carrel --json` and reads what it prints: `carrel search QUERY...` for results, `carrel show PATH` for a card, run from the file's folder so the archive it's in is used, and `carrel index status`. Paths in the output are relative to the archive's folder, which both commands give as `root`. Errors are the `{"error": …}` that `carrel` prints, shown as they are. Search may take a moment the first time, while `carrel` reads the text of items it hasn't read yet; in Neovim it runs in the background.

Neither plugin changes the archive: they search, show cards and open files. Editing a card's fields the way Carrel checks them is still `carrel edit ITEM`.

## Testing the plugins

`editors/test/run.sh` tests both, with `nvim --headless` and with ERT under `emacs --batch`, against a fake `carrel` that prints fixture JSON. It also byte-compiles `carrel.el` with warnings as errors and runs `checkdoc`, and `package-lint` when it's installed. CI runs it on Ubuntu with Neovim and Emacs from apt. See [editors/test](../../editors/test/README.md).
