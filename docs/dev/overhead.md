# What separating the parts costs

Carrel splits its work across separate programs: providers for models and text extraction, a network helper, Kopia for backups, and a background worker. This page measures what that costs in time, memory and disk, and says where it matters and where it doesn't. The [Architecture](architecture.md) page explains why the parts are separate.

The measurements were taken on 2026-10-30 on an M4 Max MacBook Pro with 36 GB of memory, with release builds from `dev`, while the model benchmark (`docs/design/benchmark.md`) was also running. Each figure is the median of 10 to 200 runs. Rerun them with the script at the end.

## Time

| What | Median | Notes |
| --- | --- | --- |
| A bare provider round trip | **13 µs** | A request that's answered at once (an unsupported method), so it's the JSON-RPC line out, parsing and the line back |
| Extracting a 1-page PDF's text through the extraction provider | 1.1 ms | 20 requests to one running provider |
| The same through the Apple provider (PDFKit) | 1.7 ms | |
| Starting the extraction provider and `initialize` | 5 ms | A small Rust program |
| Starting the Apple provider and `initialize` | 35 ms | Swift, loading the Foundation Models framework |
| Starting the llama provider and `initialize` | 77 ms, 6.4 s the first time | It lists the models but doesn't load one; the first start after a build pays for macOS loading Metal |
| Starting `carrel-fetch`, which refuses a request and exits | 5.5 ms | Once per download |
| **Drafting, routing and rating one item** | **7.8 s** (Apple's model) to **25 s** (Granite 8B) | From the benchmark: three or more model calls an item |

Against the model's time, the separation is noise: a provider round trip is about one millionth of an item's drafting time, and starting every provider once costs about a hundredth of one item. Providers start once per process and stay running, so a batch pays the start-up once, not per item.

Where the time does go, and what separation changes:

- **Loading a model** happens on the first request in each provider process, and takes seconds for a 2–6 GB file (it wasn't measured here). The worker pays it once per run of the queue, not per item.
- **The worker** is a process of its own, but it's started only when something is queued, and once per queue rather than per item.
- **Following the worker** from the interactive mode means reading a small status file about once a second. Proposals appear in Review within a second of being saved.
- **Kopia** is started for each snapshot. Its start-up is far below the time a snapshot takes, which grows with how much changed.
- **History** isn't a separate process: libgit2 runs in-process.

## Memory

This is where separation costs the most.

| What | Resident memory |
| --- | --- |
| A llama provider with Qwen3.5 4B loaded | **3.2 GB** |
| Larger catalog models (8–9B) | about their file size, 5–6 GB (estimated, not measured) |
| The Apple provider | small; Apple's model is shared by the system, outside the process |
| `carrel`, the worker, `carrel-fetch`, the extraction provider | tens of MB each |

Each process that needs a model starts its own providers, and so loads its own copy of the model. With a 4B model, two copies would be about 6.5 GB instead of 3.2 GB; with an 8B model, 11–12 GB, too much for a 16 GB machine. Apple's model doesn't have this cost, because the system shares it.

So in normal use only one process loads a model: the **worker**. Answering a term question used to draft its waiting items again inside the interactive mode, alongside the worker; now it puts them back on the queue, at the front, and the worker drafts them. The exceptions are `carrel ingest --wait` while a worker is idle (it drafts in the foreground, as a worker itself) and `carrel bench`, which measures models on purpose.

Further ways to save memory, for later:

1. **Unload when idle.** A provider could drop its model after a few minutes without requests.
2. **One model host per device.** The Mac app's background agent (v0.5) is planned to own the models, with the CLI, the interactive mode and the worker as its clients. Every process would then share one copy.

## Disk

| Program | Size |
| --- | --- |
| `carrel` | 10.1 MB |
| `carrel-provider-llama` | 12.8 MB, most of it llama.cpp |
| `carrel-fetch` | 3.2 MB, most of it the TLS stack |
| `carrel-provider-extract` | 2.5 MB |
| `carrel-provider-apple` | 0.3 MB, since it links Apple's frameworks |

`carrel-core` is linked into each Rust program, so its code is on disk several times: a few megabytes in all. Models are the real disk cost, at 2–6 GB each, and they're kept once per device however many programs use them.

## What the separation buys

The costs above are paid for these:

- **Native tools where they exist.** The Apple provider is ordinary Swift calling Foundation Models, PDFKit and Vision; the Rust core never links them, so it still builds for Linux and Windows.
- **No llama.cpp on Macs that don't need it,** and no Swift elsewhere. Each platform ships only its own providers.
- **Failures stay contained.** A provider that crashes, hangs or panics on a strange PDF fails that request; Carrel records it, tries the next provider and carries on. A model's memory is returned to the system when its provider exits.
- **The network is in one place.** Only `carrel-fetch` can reach the network, and `cargo deny` checks that no other crate links an HTTP client. The rest of Carrel can be audited as offline.
- **Anyone can add a provider** in any language by speaking the protocol, without building Carrel.
- **The worker survives the terminal.** Drafting a large batch doesn't need a window to stay open, and a crash loses one item.

## Work that grows with the archive

The figures above don't depend on an archive's size. What does is measured by the performance benchmarks ([Development Setup](development-setup.md#coverage-and-performance)), which CI runs on each change to `dev` and compares with the last run. A first run, on 2026-11-01 on a 4-core Linux cloud machine (slower than the Mac above), with generated fixtures:

| What | At the largest size | Per item |
| --- | --- | --- |
| Diffing two manifests of a million files | 0.82 s | 0.8 µs |
| Parsing a million-file manifest | 0.47 s | 0.5 µs |
| Searching the embedding index, a million 384-number vectors | 0.89 s | 0.9 µs |
| Opening that index from disk | 0.83 s | 0.8 µs |
| Searching the word index of a million items | 13 ms | |
| Building that word index | 30 s | 30 µs |
| Parsing 100,000 cards | 4.4 s | 44 µs |
| Reading a history of 100,000 changes | 1.8 s | 18 µs |

Every one grows in step with the archive, without a jump at any size. At a million items, searching by meaning by brute force takes about a second, and building the word index, done once per search process, about thirty: those are the two to watch, and the reason [`Neighbours`](https://hrose.me/carrel/carrel_core/index/trait.Neighbours.html) leaves room for an approximate index.

## Measuring it again

The script below starts each installed provider ten times, times `initialize`, then sends requests to a running provider. Run it with the programs in `~/.cargo/bin` and any PDF:

```bash
python3 measure.py path/to/some.pdf
```

```python
import json, os, statistics, subprocess, sys, time

BIN = os.path.expanduser("~/.cargo/bin")
PDF = sys.argv[1]


def start(name):
    began = time.perf_counter()
    p = subprocess.Popen([f"{BIN}/carrel-provider-{name}"], stdin=subprocess.PIPE,
                         stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
    call(p, 1, "initialize", {"protocol": 1, "client": "measure"})
    return p, time.perf_counter() - began


def call(p, i, method, params):
    p.stdin.write(json.dumps({"jsonrpc": "2.0", "id": i, "method": method, "params": params}) + "\n")
    p.stdin.flush()
    return json.loads(p.stdout.readline())


def stop(p):
    p.stdin.close()
    p.wait()


for name in ["extract", "apple", "llama"]:
    times = []
    for _ in range(10):
        p, t = start(name)
        times.append(t)
        stop(p)
    print(f"start and initialize {name}: {statistics.median(times) * 1000:.1f} ms")

p, _ = start("extract")
times = []
for i in range(200):
    began = time.perf_counter()
    call(p, 10 + i, "text/count-tokens", {"offer": "x", "text": "hello"})
    times.append(time.perf_counter() - began)
print(f"bare round trip: {statistics.median(times) * 1e6:.0f} µs")
times = []
for i in range(20):
    began = time.perf_counter()
    call(p, 300 + i, "text/extract", {"offer": "pdf-text", "path": PDF, "media_type": "application/pdf"})
    times.append(time.perf_counter() - began)
print(f"PDF text: {statistics.median(times) * 1000:.2f} ms")
stop(p)
```

Model times come from the benchmark's per-item results in `bench-results/runs/`.
