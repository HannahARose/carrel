#!/usr/bin/env python3
"""Summarizes line coverage for CI's "Coverage" job (ci.yml) as Markdown on standard output.

Reads an LCOV file from `cargo llvm-cov --lcov` and reports the workspace's total, each crate's,
and the lines a change added or modified, from `git diff BASE HEAD`. It reports only: it never
fails on a number, only on a file it can't read.

    coverage-summary.py target/lcov.info [BASE] [HEAD]

Without BASE, the change's lines are left out. Also prints a `::notice` line with the totals,
so they show among the run's annotations.
"""

import os
import re
import subprocess
import sys
from collections import defaultdict


def read_lcov(path, root):
    """{file relative to root: {line: hits}} for every instrumented line."""
    files = defaultdict(dict)
    current = None
    with open(path, encoding="utf-8") as lcov:
        for line in lcov:
            line = line.strip()
            if line.startswith("SF:"):
                name = line[3:]
                if os.path.isabs(name):
                    name = os.path.relpath(name, root)
                current = files[name.replace(os.sep, "/")]
            elif line.startswith("DA:") and current is not None:
                number, hits = line[3:].split(",")[:2]
                number = int(number)
                # A line in several monomorphized copies counts once, covered if any ran.
                current[number] = max(current.get(number, 0), int(hits))
            elif line == "end_of_record":
                current = None
    return files


def changed_lines(base, head):
    """{file: set of line numbers} added or modified in Rust files between base and head."""
    diff = subprocess.run(
        ["git", "diff", "-U0", "--no-renames", "--no-color", base, head, "--", "*.rs"],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    changed = defaultdict(set)
    current = None
    for line in diff.splitlines():
        if line.startswith("+++ "):
            current = line[6:] if line.startswith("+++ b/") else None
        elif line.startswith("@@") and current is not None:
            match = re.match(r"@@ -\S+ \+(\d+)(?:,(\d+))? @@", line)
            if match:
                start, count = int(match.group(1)), int(match.group(2) or "1")
                changed[current].update(range(start, start + count))
    return changed


def crate_of(path):
    parts = path.split("/")
    if len(parts) > 1 and parts[0] == "crates":
        return parts[1]
    return parts[0]


def percent(covered, total):
    return f"{100 * covered / total:.1f}%" if total else "–"


def ranges(numbers):
    """Line numbers as compact ranges: 3–5, 9."""
    out, numbers = [], sorted(numbers)
    while numbers:
        start = end = numbers.pop(0)
        while numbers and numbers[0] == end + 1:
            end = numbers.pop(0)
        out.append(str(start) if start == end else f"{start}–{end}")
    return ", ".join(out)


def main():
    if len(sys.argv) not in (2, 4):
        sys.exit(__doc__)
    files = read_lcov(sys.argv[1], os.getcwd())
    crates = defaultdict(lambda: [0, 0])
    for name, lines in files.items():
        crates[crate_of(name)][0] += sum(1 for hits in lines.values() if hits)
        crates[crate_of(name)][1] += len(lines)
    covered = sum(c for c, _ in crates.values())
    total = sum(t for _, t in crates.values())

    print("## Coverage\n")
    print(f"**{percent(covered, total)}** of the workspace's lines ({covered:,} of {total:,}), "
          "from its tests on Linux.\n")
    print("| Crate | Lines | Covered |\n| --- | ---: | ---: |")
    for name, (c, t) in sorted(crates.items()):
        print(f"| `{name}` | {t:,} | {percent(c, t)} |")

    notice = f"Coverage {percent(covered, total)}"
    if len(sys.argv) == 4:
        changed = changed_lines(sys.argv[2], sys.argv[3])
        hit = missed = 0
        uncovered = {}
        for name, numbers in changed.items():
            lines = files.get(name, {})
            measured = [n for n in numbers if n in lines]
            missing = [n for n in measured if not lines[n]]
            hit += len(measured) - len(missing)
            missed += len(missing)
            if missing:
                uncovered[name] = missing
        print("\n### This change\n")
        if hit + missed == 0:
            print("No changed lines are in code the tests measure.")
        else:
            print(f"**{percent(hit, hit + missed)}** of the {hit + missed:,} changed lines the "
                  f"tests measure are covered.")
            notice += f"; this change's lines {percent(hit, hit + missed)}"
        if uncovered:
            print("\n<details><summary>Changed lines no test runs</summary>\n")
            for name, missing in sorted(uncovered.items()):
                print(f"- `{name}`: {ranges(missing)}")
            print("\n</details>")
    print("\nReport only: no number here fails CI.")
    print(f"::notice title=Coverage::{notice}", file=sys.stderr)


if __name__ == "__main__":
    main()
