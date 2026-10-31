# Documentation

## Using Carrel

| Page | For |
| --- | --- |
| [Getting started](guide/getting-started.md) | Installing Carrel and filing your first items |
| [User guide](guide/user-guide.md) | How each part works: collections, ingest, review, questions, history, backups, models, settings |
| [Commands](guide/commands.md) | Every command, option, key and environment variable |

## Working on Carrel

| Page | For |
| --- | --- |
| [Development setup](dev/development-setup.md) | Setting up a Mac to build Carrel, and the checks CI runs |
| [Architecture](dev/architecture.md) | The parts, how they communicate, what's on disk, the main flows, with diagrams |
| [Code map](dev/code-map.md) | Where each part is in the code, linked to the [code documentation](https://hrose.me/carrel/) |
| [What separating the parts costs](dev/overhead.md) | Measured time, memory and disk, and what the separation buys |
| [The on-device runner](dev/on-device-runner.md) | The tests and weekly benchmark on the owner's Mac, and setting it up |
| [Providers](providers.md) | The protocol for provider programs |
| [Design documents](design/) | The accepted design for each larger feature |
| [The v0.2 gate](v0.2-gate.md) | The checklist for filing a real Downloads sample |
| [The v0.3 gate](v0.3-gate.md) | The checklist for checking a real project out and back |
| [The v0.4 gate](v0.4-gate.md) | The checklist for finding twenty remembered items by browsing and searching |

The archive format is specified in [FORMAT.md](../FORMAT.md), and how to contribute is in [CONTRIBUTING.md](../CONTRIBUTING.md).

## Publishing to the wiki

The pages above in `guide/` and `dev/` are published to the [wiki](https://github.com/HannahARose/carrel/wiki) on every merge to `dev`, by `scripts/publish-wiki.py` and `.github/workflows/wiki.yml`. Edit them here, not on the wiki. To publish a new page, add it to `PAGES` in the script, and link it from the wiki's sidebar and Home, which are edited on the wiki itself. Pull requests that change the docs check that every link resolves.
