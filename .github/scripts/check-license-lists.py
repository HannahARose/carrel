#!/usr/bin/env python3
"""Check that about.toml accepts every license deny.toml allows, exceptions included.

cargo-deny checks dependencies' licenses on every change; cargo-about, which writes the
third-party notices, runs only when a release is built. A license deny.toml allows but
about.toml doesn't stops the release build, so this keeps the two lists in step.
"""

import sys
import tomllib

with open("deny.toml", "rb") as f:
    deny = tomllib.load(f)["licenses"]
with open("about.toml", "rb") as f:
    about = set(tomllib.load(f)["accepted"])

allowed = set(deny.get("allow", []))
for exception in deny.get("exceptions", []):
    allowed.update(exception.get("allow", []))

missing = sorted(allowed - about)
if missing:
    print(
        "about.toml doesn't accept these licenses that deny.toml allows, so the release's "
        "license notices would fail: " + ", ".join(missing),
        file=sys.stderr,
    )
    sys.exit(1)
print(f"about.toml accepts all {len(allowed)} licenses deny.toml allows")
