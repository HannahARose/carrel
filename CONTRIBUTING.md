# Contributing to Carrel

Carrel is in early development. This page explains how work is organized.

## Where things live

| What | Where |
| --- | --- |
| Bugs, features, tasks, open questions | [Issues](https://github.com/HannahARose/carrel/issues), using the templates |
| Planning and status | The Carrel [project board](https://github.com/users/HannahARose/projects/2) |
| Versions | One milestone per version, v0.1 through v3.1 (see the wiki's Roadmap page) |
| Design and reference docs | The [wiki](https://github.com/HannahARose/carrel/wiki) |
| The archive format spec | `FORMAT.md` in this repository (once written) |

## Labels

- **type:** bug, feature, docs, task, question, research, epic
- **area:** which part of Carrel (format, core, cli, ingest, models, history, backups, checkout, explorer, librarian, restructure, sync, apple-apps, privacy, design)
- **priority:** p0 blocks the milestone, p1 is needed for it, p2 is nice to have
- **status:** needs design, blocked

Each roadmap phase has an epic issue that lists its work as sub-issues.

## Branches

| Branch | Purpose | How changes land |
| --- | --- | --- |
| `main` | Released versions only; every commit is a release | Merge-commit pull requests from `dev` (releases) or `hotfix/*` |
| `dev` | Integration branch and the repository default | Squash-merged pull requests from work branches |
| `feat/*`, `fix/*`, `docs/*`, `chore/*`, `research/*` | One issue or change each, short-lived | Branched from `dev`, deleted after merge |
| `hotfix/*` | Urgent fix to a release | Branched from `main`, merged into `main`, then `main` merged back into `dev` |

Both `main` and `dev` are protected: changes arrive only through pull requests, force-pushes and deletion are blocked, and review threads must be resolved before merging. Required CI checks will be added once CI exists.

## Workflow

1. Pick or open an issue; discuss design first if it carries `status: needs design`.
2. Branch from `dev` with a name like `feat/card-parser` or `fix/manifest-rename`.
3. Open a pull request into `dev` using the template and link the issue (`Closes #123`). It is squash-merged, so the pull request title becomes the commit message: write it as a short imperative sentence saying what changes, such as "Add a writer lock and require a lease for history commits", with no type prefix or issue number.
4. Changes to the archive format update `FORMAT.md` in the same pull request.
5. To release, open a pull request from `dev` into `main` titled with the version, merge it, tag the merge commit (`v0.1.0`) and publish GitHub release notes.

## Versioning

`MAJOR.MINOR.PATCH`, numbered by how much the app changes for the person using it:

- **MAJOR:** a large change in how you use Carrel (2.0 adds iCloud, iPad and iPhone).
- **MINOR:** a smaller feature (1.2 adds restructure proposals).
- **PATCH:** bug fixes only.

Versions stay at 0.x, one minor per roadmap phase, until 1.0, the first release meant for daily use. Pre-releases use `-alpha.N`, `-beta.N` (including TestFlight builds) and `-rc.N`. The apps and CLI share one version. The archive format is versioned separately in `FORMAT.md` and aims never to break compatibility.

## Privacy

Never attach private documents to issues or pull requests. Describe the file instead, and use test fixtures with public or invented content.
