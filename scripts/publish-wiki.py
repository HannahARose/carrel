#!/usr/bin/env python3
"""Publishes written documentation from docs/ to the GitHub wiki (issue #49).

Each page listed in PAGES becomes a wiki page: its first heading is dropped (the wiki shows
the page name as the title), a note says where to edit it, links to other published pages
become wiki links, and links to anything else in the repository point to the file on
GitHub. Pages not listed here, and the wiki's own pages (Home, the sidebar, the design
pages), are left alone.

    scripts/publish-wiki.py WIKI_DIR     write the pages into a checkout of the wiki
    scripts/publish-wiki.py --check      only check that every relative link resolves

It fails, writing nothing, if a relative link points to a file that isn't in the repository.
"""

import posixpath
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
REPO_URL = "https://github.com/HannahARose/carrel"

# Each published file, and the wiki page it becomes (the page's file name, without .md).
PAGES = {
    "docs/guide/getting-started.md": "Getting-Started",
    "docs/guide/user-guide.md": "User-Guide",
    "docs/guide/commands.md": "Command-Line Tool",
    "docs/dev/architecture.md": "Architecture",
    "docs/dev/code-map.md": "Code-Map",
    "docs/dev/overhead.md": "Overhead",
}

LINK = re.compile(r"\[([^\]]+)\]\(([^)\s]+)\)")


def wiki_url(page):
    return page.replace(" ", "-")


def convert(source, text, problems):
    """`text`, from the file `source`, as a wiki page."""
    base = posixpath.dirname(source)

    def fix(match):
        label, target = match.group(1), match.group(2)
        if re.match(r"[a-z]+:|#", target):
            return match.group(0)
        path, _, anchor = target.partition("#")
        anchor = f"#{anchor}" if anchor else ""
        resolved = posixpath.normpath(posixpath.join(base, path))
        if resolved in PAGES:
            return f"[{label}]({wiki_url(PAGES[resolved])}{anchor})"
        on_disk = ROOT / resolved
        if not on_disk.exists():
            problems.append(f"{source}: {target} isn't in the repository")
            return match.group(0)
        kind = "tree" if on_disk.is_dir() else "blob"
        return f"[{label}]({REPO_URL}/{kind}/dev/{resolved}{anchor})"

    lines = LINK.sub(fix, text).split("\n")
    if lines and lines[0].startswith("# "):
        lines = lines[1:]
    note = (
        f"> This page is published from [`{source}`]({REPO_URL}/blob/dev/{source}) in the "
        "repository. To change it, edit that file in a pull request."
    )
    return note + "\n\n" + "\n".join(lines).lstrip("\n")


def main(argv):
    if len(argv) != 2:
        print(__doc__.strip().split("\n\n")[1], file=sys.stderr)
        return 2
    problems = []
    pages = {}
    for source, page in PAGES.items():
        text = (ROOT / source).read_text(encoding="utf-8")
        pages[page] = convert(source, text, problems)
    if problems:
        print("\n".join(problems), file=sys.stderr)
        return 1
    if argv[1] == "--check":
        print(f"{len(pages)} pages convert cleanly")
        return 0
    wiki = Path(argv[1])
    for page, text in pages.items():
        (wiki / f"{page}.md").write_text(text, encoding="utf-8")
    print(f"wrote {len(pages)} pages to {wiki}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
