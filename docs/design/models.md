# Design: downloading models (v0.2)

Status: draft for the owner. Issue #99.

## What this adds

The llama.cpp provider offers every GGUF file in Carrel's models folder (`carrel_core::local::models_dir()`), but today the owner has to find a model and put it there by hand. On Linux and Windows, where Apple's on-device model isn't available, this means no cards get drafted until they do. This design adds `carrel models`, which lists a short catalog of models known to work, and downloads one when the owner asks: never automatically.

## The network helper

The design spec keeps every part of Carrel offline except one helper process, the only component allowed online. It downloads models and uploads offsite backups, and never reads archive files. This is the first feature that needs it.

- **A separate program, `carrel-fetch`:** a small Rust binary in its own crate, the only one in the workspace that links an HTTP client (`ureq` with `rustls`). `carrel`, `carrel-core` and the providers keep no network code, and `cargo deny` checks that no other crate depends on an HTTP client.
- **Found like providers:** next to the `carrel` executable, then on `PATH`. The release builds (#46) ship it beside `carrel`, and the Mac app (v0.5) bundles it as the only part with a network entitlement.
- **A narrow job:** `carrel` starts it with one request on standard input, as JSON: the URL, the expected size and SHA-256, and the file to write. It reports progress and the result as JSON lines on standard output. It only writes to the file it was given, and only accepts `https` URLs on hosts in the catalog, following redirects only to the hosts the catalog lists for that source (Hugging Face serves files from its CDN). It doesn't take a path inside an archive.
- **In the core:** `carrel_core::models` holds the catalog and runs the helper through `std::process`, as `backup` runs Kopia. A test double lets the tests run without the network.

## The catalog

- **Built in, pinned:** each entry gives the model's name, the file's URL at a fixed Hugging Face commit (not `main`, which can change), its size and SHA-256, its license, the memory it needs, and what it's for. The catalog changes only with a Carrel release, like the pinned Kopia checksums.
- **Only models it can vouch for:** open weights under a permissive license (Apache 2.0 or MIT), not gated behind an account, and loadable by the llama.cpp version Carrel ships.
- **Starting candidates:**

  | Name | File | Size | For |
  | --- | --- | --- | --- |
  | `granite-4.2-3b` | IBM Granite 4.2 3B, Q4_K_M | 2.2 GB | machines with 8 GB of memory |
  | `qwen3.5-4b` | Qwen3.5 4B, Q4_K_M | 2.7 GB | machines with 8 GB of memory |
  | `gemma-4-e4b` | Gemma 4 E4B, Google's QAT Q4_0 | 5.2 GB | machines with 16 GB |
  | `granite-4.2-8b` | IBM Granite 4.2 8B, Q4_K_M | 5.4 GB | machines with 16 GB |
  | `qwen3.5-9b` | Qwen3.5 9B, Q4_K_M | 5.7 GB | machines with 16 GB or more |

  The benchmark (#33) decides which are **recommended** for each memory size. Until it runs, none is recommended and `carrel models` says the list is provisional.

## Commands

- `carrel models` lists the catalog with each model's size, memory needs, license and whether it's downloaded, then the GGUF files already in the models folder that the catalog doesn't know (owner-added models still work). It marks the recommended one for this machine's memory.
- `carrel models download NAME` shows the size, license and destination, and asks before downloading (`--yes` skips the question, for scripts). It refuses if the disk doesn't have the space plus a margin.
- `carrel models remove NAME` deletes a downloaded model, after asking.
- `--json` works on all three, as elsewhere.

## Downloading safely

- **Checked before it's used:** the helper writes `NAME.gguf.part` and hashes as it goes. Only when the size and SHA-256 both match does Carrel rename it to `NAME.gguf`, so the provider never sees a partial or altered file. A mismatch deletes the part file and says so.
- **Resumable:** an interrupted download keeps its `.part` file, and the next `download` continues it with an HTTP range request, re-hashing what's already there first.
- **Logged:** every request the helper makes is appended to `logs/network.log` in Carrel's configuration folder as a JSON line (time, URL, bytes, SHA-256, result). That's a device log, not part of an archive. `carrel models` shows where it is.
- **Progress:** a progress line on a terminal; with `--json`, one line per update.

## Building it

1. `carrel_core::models`: the catalog, the request and progress types, the checks, and a test double for the helper.
2. The `carrel-fetch` crate, with tests against a local HTTP server: a full download, a resume, a checksum mismatch and a refused host.
3. `carrel models`, `download` and `remove`, and a pointer from the llama.cpp provider's "no model yet" message.
4. `cargo deny` rules that keep HTTP clients out of every other crate.
5. The benchmark (#33) marks the recommended models.

## Decisions for the owner

1. **A separate `carrel-fetch` program** as the only networked component, as the spec requires, instead of an HTTP client inside `carrel`. Recommended: it costs one more small binary and keeps the boundary checkable today.
2. **A built-in, pinned catalog** that changes only with releases, instead of a list fetched online. Recommended: no extra trust in a server, and every file is checked against a hash Carrel shipped with.
3. **Ask before every download** and show size, license and destination, with `--yes` for scripts. Recommended.
4. **Log every network request** to a device log in the configuration folder, not in the archive. Recommended.
5. **Permissive, ungated models only**, with the five candidates above, and recommendations left to the benchmark (#33). Recommended.
