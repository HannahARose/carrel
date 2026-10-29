# Contributing to Carrel

Carrel is in early development. This page explains how work is organized.

## Where things live

| What | Where |
| --- | --- |
| Bugs, features, tasks, open questions | [Issues](https://github.com/HannahARose/carrel/issues), using the templates |
| Planning and status | The Carrel [project board](https://github.com/users/HannahARose/projects) |
| Releases | Milestones: v1.0, Beta 2, Beta 3 |
| Design and reference docs | The [wiki](https://github.com/HannahARose/carrel/wiki) |
| The archive format spec | `FORMAT.md` in this repository (once written) |

## Labels

- **type:** bug, feature, docs, task, question, research, epic
- **area:** which part of Carrel (format, core, cli, ingest, models, history, backups, checkout, explorer, librarian, restructure, sync, apple-apps, privacy, design)
- **priority:** p0 blocks the milestone, p1 is needed for it, p2 is nice to have
- **status:** needs design, blocked

Each roadmap phase has an epic issue that lists its work as sub-issues.

## Workflow

1. Pick or open an issue; discuss design first if it carries `status: needs design`.
2. Work on a branch named `type/short-description` (for example `feat/card-parser`, `fix/manifest-rename`).
3. Open a pull request using the template and link the issue (`Closes #123`).
4. Changes to the archive format update `FORMAT.md` in the same pull request.

## Privacy

Never attach private documents to issues or pull requests. Describe the file instead, and use test fixtures with public or invented content.
