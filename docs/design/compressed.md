# Design: compressed files

Status: accepted 2026-10-30, with the five recommendations below. Issue #185.

## What this adds

Today a zip file is one opaque item: nothing reads inside it, so its card is drafted from its name, and what it holds can't be browsed in Finder, Obsidian or Carrel. The owner's direction: **compressed files are expanded by default, so their contents are browsable, unless there's a reason or a request to keep them compressed.**

Expanding doesn't save space. Most archives are already compressed internally (in the owner's test folder, a 1.73 GB export expanded to 1.81 GB, and three game downloads of 20 GB shrank by less than 0.5%). It's about being able to see and find what's inside. Saving space is a separate question, for filesystem compression (APFS, Btrfs, ZFS, NTFS) per collection, in a later design.

## Which files

- **Archives:** `.zip`, `.tar`, `.tar.gz`/`.tgz`, `.tar.bz2`, `.tar.xz`, `.tar.zst`, `.7z` and `.rar`.
- **Single compressed files:** `.gz`, `.bz2`, `.xz` and `.zst` around one file, such as `data.csv.gz`.
- **Never treated as archives,** though they're zip files inside: documents and packages whose format is the zip, such as `.docx`, `.xlsx`, `.pptx`, `.odt`, `.epub`, `.3mf`, `.jar`, `.apk`, `.ipa`, `.whl` and `.usdz`. `detect` already tells these apart by extension and media type.

## Reasons to stay compressed

Carrel keeps an archive compressed, and says why on the proposal, when:

- **It's software to install or run:** it holds an app (`.app`), an installer (`.pkg`, `.mpkg`, `.msi`, `.exe`, `.dmg`), or a bundle with a code signature. Expanding these can break them, and their contents aren't worth browsing.
- **It's encrypted** or password-protected. Carrel never asks for archive passwords.
- **It's split** into parts (`.z01`, `.7z.001`, `.part1.rar`).
- **Expanding it looks unsafe or too big:** it would expand to more than 10 times its size and more than 1 GB, or hold more than 100,000 files, or has entries that would land outside its folder (absolute paths, `..`, links that point out). These ask rather than decide.
- **The owner asked:**
  - with a routing rule (`keep_compressed: true`)
  - with a collection's setting
  - with the archive's own setting (`compressed_files: keep`)
  - or for one item in Review, with a "keep compressed" choice
- **Nothing on this device can open it,** such as a `.7z` without 7-Zip installed. It's kept, and logged in `carrel formats` like other unreadable types.

Disk images (`.dmg`, `.iso`) aren't archives here: they stay as they are, and a later design can read their listing.

## When it happens

- **Drafting reads inside without expanding:** the listing (names, sizes, dates), the README or main document when there's one, and the same project markers that make a folder a bundle. The card and route come from what's inside, as for a folder.
- **Review shows the plan:** "will be expanded: 440 files, 1.8 GB, into Projects/Lab/Spectrometer export", or "kept compressed: it holds an installer".
- **Expanding happens when the proposal is approved,** as part of filing, so nothing changes on disk before the owner agrees. A failure leaves the archive file where it was and the proposal open, with the reason.

## What it becomes

- **One file inside** (`report.pdf.zip`, `data.csv.gz`) becomes that file, as a file item.
- **One folder wrapping everything,** as most zips have, is unwrapped: the folder inside becomes the item.
- **Anything else** becomes a folder named after the archive, without its extension: an open folder, or a sealed bundle if the project markers say so.
- **Kept with the contents:** modification times (so dates and work sessions stay right) and execute permissions. Dropped: `__MACOSX/`, `.DS_Store` and AppleDouble (`._`) files.
- **The card records where it came from:** a new optional field, `expanded_from`, with the archive's name, size and checksum (FORMAT.md §5.2), so it's clear the item was a zip and which one.
- **The archive file goes to the system Trash** after the expanded copy is checked against the archive's own checksums (CRC-32 for zip; sizes for tar), never deleted outright, so it can be recovered.

## Safety

Expanding untrusted archives is a well-known source of trouble, so Carrel:

- writes to a temporary folder in the archive's volume and moves the result into place only when everything checks out;
- refuses entries with absolute paths, `..`, or links that point outside the folder ("zip slip");
- stops at the size, ratio and count limits above, even if the listing understated them;
- verifies every entry's checksum while writing;
- decodes names correctly (zip's UTF-8 flag, otherwise CP437) and refuses names the filesystem can't hold rather than mangling them.

## How it's built

- **Portably, in `carrel-core`,** with Rust crates for zip, tar, gzip, bzip2, xz and zstd, so it works the same on macOS, Linux and Windows.
- **7z and RAR** through `7z` or `unar` when installed, found like other helper programs; otherwise kept and logged.
- **Archives already in the archive:** `carrel expand ITEM` expands one on request, with the same checks, as one undoable change. Nothing is expanded behind the owner's back after filing.

## Building it

1. `detect`: compressed formats, and zip-based documents told apart from archives.
2. Reading inside: listings and the README or main document for drafting and routing, for every format.
3. The reasons to stay compressed, the setting, the routing rule option and the Review choice.
4. Expanding on approval: unwrapping, `expanded_from`, the safety checks, and the Trash.
5. 7z and RAR through installed tools.
6. `carrel expand ITEM`.
7. The user guide, FORMAT.md and the benchmark (a few invented zips: a project, a single document, an installer).

## Decisions (accepted 2026-10-30)

1. **Compressed files are expanded by default,** with `compressed_files: expand | keep | ask` for the archive, collections and routing rules, and a per-item choice in Review. Recommended: the owner's direction, with a way to say otherwise at every level.
2. **These are kept compressed automatically, with the reason shown:** software and installers, encrypted or split archives, and anything nothing on the device opens. Expansions that look unsafe or too big (over 10× and 1 GB, over 100,000 files, or paths that escape) ask instead. Recommended: these are the cases where expanding breaks something or risks the disk.
3. **Drafting reads inside without expanding; expanding happens on approval.** Recommended: routing and cards see the contents, and nothing changes on disk until the owner agrees.
4. **The result is unwrapped (one file, or one wrapping folder), keeps modification times, records `expanded_from` on the card, and the original goes to the system Trash after verification.** Recommended: the item is what the owner would have made by hand, its history is kept, and nothing is lost for good.
5. **Expansion is portable, in `carrel-core`;** 7z and RAR use installed tools when present. Recommended: zip, tar and the common compressors work everywhere with no download, and the rest are logged rather than guessed at.
