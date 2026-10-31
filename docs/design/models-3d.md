# Design: reading 3D models

Status: accepted 2026-10-30, with the five recommendations below (revised the same day to draw STEP and IGES now, and to log the file types nothing reads). Issue #182.

## What this adds

Today a 3D model has no text to read: `curtain_glider_tabhook_1010.stl` is filed by its name alone, and names like `part_v3_final.stl` say nothing. The image reader (`docs/design/images.md`) gives Carrel a way to see. A model can be drawn, and the drawing read like any other image.

A prototype on four of the owner's test files rendered four views of each with SceneKit in under a second. Given only the renders and the size, with no file name, Apple's on-device model answered in 2–5 seconds:

| File | Size (mm) | What the model said |
| --- | --- | --- |
| `spw605_coozie_base.stl` | 86 × 86 × 19 | "a circular container with a rim … for holding small items or liquids" |
| `spw605_coozie_collar.stl` | 86 × 86 × 16 | "a ring-shaped component, likely used as a seal or connector" |
| `curtain_glider_tabhook_1010.stl` | 12 × 14 × 25.5 | "a corner bracket with a rectangular base and a vertical extension" |
| `curtain_glider_pin_1010.stl` | 12 × 16.9 × 25.5 | "a small mechanical component … a protruding tab … and a cylindrical shaft" |

The drafting model then has the shape and the size as well as the name, and can write "Coozie base, 86 mm" rather than guessing.

When the prototype also gave the model the file name, its answers mostly repeated the name ("a 3D-printable tabhook … for hanging small items such as labels"). So the description step doesn't get the name; the card step sees both.

## A model reader in the Apple provider

- **A new extract offer, `model3d`,** in `carrel-provider-apple`, for the formats ModelIO reads: STL, OBJ, PLY, and USD/USDZ (see "Media types" below).
- **It does three things,** and returns them as one text, as the image reader does:
  1. **Geometry** through ModelIO: the bounding size, the triangle count, and how many separate parts it has.
  2. **Four views** drawn by SceneKit off-screen (angled, front, top, side), in light grey on white, each 512 pixels, in one 2 × 2 picture of 1,024 pixels. Z is taken as up, as in most STL files and slicers.
  3. **A description** of that picture from Apple's on-device model, given the views and the size, but not the file name.
- **The text looks like:**

  ```text
  3D model: STL, 86 × 86 × 19 (mm, as stored), 10,220 triangles, 1 part
  Shows: A circular container with a rim, likely for holding small items or liquids.
  ```

- **The method is `rendered`** when a description was written, `geometry` otherwise.
- **Without image input** (before macOS 27, or Apple Intelligence off), the geometry still goes in; it takes well under a second.
- **Units:** STL has none. Most files are in millimetres, so the text says "mm, as stored", leaving the model to notice when a 0.09 × 0.09 × 0.02 part is in metres.

## Media types

- `detect` maps `.stl` to `model/stl`, `.obj` to `model/obj`, `.ply` to `model/ply`, `.usdz` to `model/vnd.usdz+zip`, `.usd`/`.usda`/`.usdc` to `model/vnd.usd`, and `.3mf` to `model/3mf`, before the "reads as UTF-8 text" rule that would call an ASCII STL or OBJ `text/plain`.

## CAD and slicer files, everywhere

- **3MF** files (from Bambu Studio, PrusaSlicer, Cura) are zipped XML, and say more than a mesh: a title, a designer, a description, a license, the printer and filament, and often a thumbnail. ModelIO can't read them.
- **G-code** starts with comments a slicer wrote: the printer, filament, layer height, print time and the model's name.
- **STEP** (`.step`, `.stp`) files from CAD programs are text with a header that names the file, when it was exported, the author and organisation, and the program, followed by the names of its parts. The owner's test files show how much that says:

  | File | From its header and parts |
  | --- | --- |
  | `sample holder.step` | Onshape, 2026-10-11; "Left OLED Holder", "Middle Sample Holder", "Rear Body", "Right OLED Holder" |
  | `HZ-65000-G.STEP` | SolidWorks 2022, 2022-12-01; "HZ Camera Body (Fan)", "Gpixel GMAX3265 Sensor", … |
  | `OBIS Laser.STEP` | SolidWorks 2012, 2012-01-18, organisation "coherent" |

- **IGES** (`.igs`, `.iges`) has a similar start section with the program and date.
- All four are text formats, so they can be read **in `carrel-provider-extract`**, on every platform, without rendering: the metadata as text, the export date as `created`, and for 3MF the embedded thumbnail, when there's one, handed to the image reader on a Mac. STEP files are often tens of megabytes; the header is at the start, and part names are found by scanning for `PRODUCT(` without parsing the geometry.

## Drawing STEP and IGES

STEP and IGES hold exact surfaces, not triangles, and nothing in macOS reads them. OpenCascade (LGPL-2.1, on macOS, Linux and Windows: `brew install opencascade`, `apt install libocct-*`, vcpkg) turns them into triangles.

- **A new optional provider, `carrel-provider-cad`,** in Rust, built like `carrel-provider-llama`: only when asked for, since it needs OpenCascade installed to build. It offers `text/extract` for STEP and IGES.
- **A small C++ shim,** compiled with the `cc` crate against the installed OpenCascade, reads the file (`STEPControl_Reader`, `IGESControl_Reader`), meshes it coarsely (`BRepMesh_IncrementalMesh`, about 1% of the part's size), and hands back triangles and part names. A binding crate would be more code to trust than the hundred lines Carrel needs.
- **It draws the four views itself,** with a small software renderer in Rust (flat shading and a depth buffer, grey on white, as decision 2 describes), so it needs no GPU and works the same everywhere.
- **The text** has the header and parts (as above), plus the size in the file's units, the part count and the volume, which a mesh can't give reliably but OpenCascade can.
- **The views go back as pictures** for another offer to describe; see below. Without OpenCascade installed, `carrel-provider-extract` still reads STEP's header and part names.

## Pictures to describe

The views of a STEP file, and a 3MF's thumbnail, come from providers that can't see. So:

- **An extraction result may include `pictures`:** temporary PNG files the provider wrote, each with a short `context` such as "four views of a 3D model, 86 × 86 × 19 mm".
- **Carrel passes each picture to the best image reader available** (on a Mac, the Apple provider's `image` offer) with that context and without the file's name, and adds what it says to the text as a `Shows:` line. Then it deletes the pictures.
- **With no image reader,** the pictures are dropped, and the text stands on its own.

The same path will serve Linux and Windows once a local model can see (Gemma 4 E4B through llama.cpp, a separate design), and it lets the portable renderer draw STL there too.

## Files nothing reads

Carrel says "nothing on this device reads model/3mf" for a single item, but there's no record across runs of what's missing, so there's no telling which reader to build next.

- **Each device keeps a log of the file types it couldn't read:** `config/unreadable.json` in Carrel's own folder, since it depends on what's installed. One entry per media type and extension, with how many items, when first and last seen, and why (`no reader`, `reader failed`, `read but empty`).
- **No file names or paths,** so the log can be shared in an issue without showing what's in the archive.
- **`carrel formats`** lists what this device reads and by which provider, then the log, most frequent first, with what would read each when Carrel knows (such as "install OpenCascade and build `carrel-provider-cad`").
- **An ingest summary mentions new ones:** "3 items of a type nothing reads (`.f3d`); see `carrel formats`."

## Previews

- **The renders aren't kept.** A preview beside the card would help when browsing, but it's a new kind of file in the archive (FORMAT.md) and belongs with the Mac app's browsing work in v0.5.

## Building it

1. `detect`'s media types for 3D formats.
2. The `model3d` offer: geometry, with tests on small generated STL, OBJ and PLY files.
3. The four views and the description, behind the vision check, sharing the image reader's model call.
4. 3MF, G-code, STEP and IGES metadata in `carrel-provider-extract`.
5. `pictures` on extraction results, and Carrel passing them to an image reader.
6. `carrel-provider-cad`: the OpenCascade shim, the software renderer, and CI that builds it on macOS and Linux with OpenCascade from the package manager.
7. The log of unreadable types and `carrel formats`. This can come first, since it needs none of the rest.
8. Benchmark items: a few invented models (a bracket, a box, a ring, a figurine-like blob) with known destinations.
9. The user guide and `docs/providers.md`.

## Decisions (accepted 2026-10-30)

1. **One `model3d` offer in the Apple provider that returns geometry and a description of four rendered views as one text.** Recommended: it reuses the image reader's description step, and needs nothing from the card, routing or rating steps.
2. **Four views in one 2 × 2 picture, Z up, grey on white.** Recommended: one model call per model, and the views together show what one angle hides. The alternative, one call per view, is four times slower and gives four descriptions to reconcile.
3. **The description step doesn't see the file name; the card step sees both.** Recommended: with the name, the prototype's descriptions repeated it instead of looking.
4. **CAD and slicer files are read everywhere, and STEP and IGES drawn now:** their own text in `carrel-provider-extract`, and drawings from a new optional `carrel-provider-cad` that meshes through OpenCascade and renders in software, with the pictures described by whichever image reader the device has. Recommended: the text works with no download; the drawings are portable, and the `pictures` path also serves 3MF thumbnails and, later, other platforms. The cost is a C++ build step behind an optional feature.
5. **Each device logs the file types it couldn't read, without file names, and `carrel formats` shows them.** Recommended: it says which reader to build next, and stays shareable. Previews of models stay out of the archive until the Mac app's browsing work (v0.5).
