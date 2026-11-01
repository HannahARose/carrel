# Documentation

## Using Carrel

| Page | For |
| --- | --- |
| [Getting started](guide/getting-started.md) | Installing Carrel and filing your first items |
| [User guide](guide/user-guide.md) | How each part works: collections, ingest, review, questions, history, backups, models, settings |
| [Commands](guide/commands.md) | Every command, option, key and environment variable |
| [The Mac app](guide/mac-app.md) | Getting the app, its window and screens, adding items, and the background agent |
| [Neovim and Emacs](guide/editors.md) | Searching the archive and reading cards from your editor |

## Working on Carrel

| Page | For |
| --- | --- |
| [Development setup](dev/development-setup.md) | Setting up a Mac to build Carrel, and the checks CI runs |
| [Architecture](dev/architecture.md) | The parts, how they communicate, what's on disk, the main flows, with diagrams |
| [Code map](dev/code-map.md) | Where each part is in the code, linked to the [code documentation](https://horizon-labs.github.io/carrel/) |
| [Interfaces](dev/interfaces.md) | Every capability in the command line, the interactive mode and the app, and the allowed deviations |
| [What separating the parts costs](dev/overhead.md) | Measured time, memory and disk, and what the separation buys |
| [The on-device runner](dev/on-device-runner.md) | The tests and weekly benchmark on the owner's Mac, and setting it up |
| [Providers](providers.md) | The protocol for provider programs |
| [Design documents](design/) | The accepted design for each larger feature |
| [The v0.2 gate](v0.2-gate.md) | The checklist for filing a real Downloads sample |
| [The v0.3 gate](v0.3-gate.md) | The checklist for checking a real project out and back |
| [The v0.4 gate](v0.4-gate.md) | The checklist for finding twenty remembered items by browsing and searching |
| [The v0.5 gate](v0.5-gate.md) | The checklist for using the Mac app as the everyday interface for a week |
| [The v0.6 gate](v0.6-gate.md) | The checklist for an hour with the librarian and the discovery views, in every interface |
| [The restructure gate](v0.6-restructure-gate.md) | The checklist for the first whole-archive restructure, approved, tested for findability and undone, in every interface |

The archive format is specified in [FORMAT.md](../FORMAT.md), and how to contribute is in [CONTRIBUTING.md](../CONTRIBUTING.md).

## Publishing to the wiki

The pages above in `guide/` and `dev/` are published to the [wiki](https://github.com/Horizon-Labs/carrel/wiki) on every merge to `dev`, by `scripts/publish-wiki.py` and `.github/workflows/wiki.yml`. Edit them here, not on the wiki. To publish a new page, add it to `PAGES` in the script, and link it from the wiki's sidebar and Home, which are edited on the wiki itself. Pull requests that change the docs check that every link resolves.
