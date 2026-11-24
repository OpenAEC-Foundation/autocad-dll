# autocad-dll

Native library that reads the objects of AutoCAD Architecture drawings (`.dwg`): walls, doors,
windows, slabs, roof slabs, curtain walls, stairs, structural members. For use by
[open-bim-suite](https://github.com/OpenAEC-Foundation/open-bim-suite) and other tools.

**Status:** pre-alpha. Of the 111 sample drawings (AutoCAD Architecture 2008 and 2025)
53 hold objects, 31 of them objects that are drawn as bodies; three drawings do not open in the
codec. What is and is not read is listed under "What is read".

## Scope

The library does not read DWG files. A DWG codec opens the drawing and hands over, per custom
object, three things: the class name, the graphics the object carries for other programs, and the
object's record. `aec_read::read_object` says what the object is and gives its geometry in the
drawing's coordinates and units. It works on bytes, has no dependencies beyond a triangulator, no
unsafe code, and compiles for `wasm32-unknown-unknown`.

## Crates

| Crate | Role |
|---|---|
| `aec-core` | Decoding: the graphics stream to triangles and lines, the record to the object's placement and some sizes, a box from extents and axes |
| `aec-read` | Public API: `Class`, `Shape`, `Source`, `read_object` |
| `aec-cli` | `aec` command: `info`, `list`, `records`, `obj` (opens drawings with the acadrust codec) |

## What is read

An AutoCAD Architecture object is a custom object. A drawing keeps two things of it that another
program can use.

**The graphics it carries.** A stream of primitives that stands in for the object where its
application is absent. Shells, meshes and polygons are read as triangles; polylines, circles and
arcs as lines; colours and transforms are applied; text, clip boundaries and lightweight
polylines are skipped. In all but one of the sample drawings this stream holds, for every object,
only **the edges of its extents**: a box along the axes of the drawing, around what the object
showed in the view the drawing was saved in (a flat rectangle when that was a plan view). Real
faces were found in one drawing, on polygon objects.

**Its record.** The fields the application wrote, as a stream of bits. Read from it:

- the placement of the object: an origin and three axes (drawings of the 2025 release), or only
  the sizes of the object along its own axes (the 2008 drawings, and stairs, railings and some
  slabs of 2025);
- of a wall: base height and width, and the length when the wall has the plain layout (no changes
  to roof line, floor line, end caps or body);
- of a door, a window and an opening: width and height.

Everything else in the record is not read: outlines of slabs, the faces of a roof, the flights
and treads of a stair, the grid of a curtain wall, the profile of a member, the shape of a mass
element, the components of a wall and the openings that doors and windows cut in it, styles,
materials, property sets.

**The geometry that follows.** The extents and the axes together give the box along the object's
own axes that has those extents (`aec_core::fit`). `Shape` says per object which of these it got:

| Shape | When | What it is |
|---|---|---|
| `Graphics` | The graphics hold faces | What the application drew |
| `Box` | Wall, door, window, door and window assembly, slab, roof slab, curtain wall and its units, member, mass element, with extents that have height and axes in the record | The object itself when it is plain (a straight wall, a rectangular slab, a straight member); the block around it when it is not (an L-shaped slab, a wall with a gable keeps its base height) |
| `Plate` | Slabs, roof slabs and curtain walls turned half a right angle, such as a roof slab pitched 45 degrees | The box without thickness: the extents then give thickness and width only as their sum |
| `Extents` | Stairs, railings, roofs, openings, spaces, multi-view blocks, annotation; any object whose extents no box along its axes has (a triangular roof slab); objects of the 2008 drawings that do not stand along the axes of the drawing | Lines: the edges of the extents |
| `Nothing` | No graphics | |

A wall in a drawing saved in a plan view has flat extents; with the plain layout it is drawn from
its own numbers (length, width, height), in the middle of its base line.

Known wrong or missing:

- A wall turned exactly half a right angle takes the width of its record, which is the base width
  of the wall and not always the width its style draws.
- Walls have no openings where doors and windows stand; doors and windows are boxes in them.
- Mass elements are drawn as boxes whatever their shape (cylinders, gables, free forms).
- Objects inside blocks are read like any other; placing them is the caller's side.

## Use

```rust
let source = aec_read::Source { class_name: "AEC_WALL", graphics: Some(graphics), record: Some(record) };
if let Some(object) = aec_read::read_object(&source) {
    println!("{} as {}: {} triangles", object.class.label(), object.shape.label(), object.geometry.triangle_count());
}
```

`record` is the object's record as the drawing stores it, from its type code on.

```
aec info drawing.dwg          the objects per class and the shapes they got
aec list drawing.dwg          one line per object
aec records drawing.dwg       the graphics record types per class
aec obj drawing.dwg out.obj   the triangles, to look at in another program
```

## Tests

`cargo test` runs the unit tests, on bytes the tests build themselves (`aec_core::testing`). The
corpus test reads every drawing under the folder `ACA_SAMPLES` points at and prints per file how
many objects it holds and how many gave geometry; without the variable it does nothing.

```
ACA_SAMPLES=/path/to/drawings cargo test -p aec-read --test corpus -- --nocapture
```

## Clean-room rules

The reader is built from black-box analysis of drawings only.

- Never open, disassemble or decompile AutoCAD, its object enablers or other third-party
  binaries; no memory dumps.
- No SDK headers, and no leaked or licensed documentation of the object formats.
- The layout of the DWG container, its bit encodings and the graphics stream come from the
  published Open Design Specification for .dwg files and from the open source codec the command
  line tool uses, which also reads the start of an object record; the rest of what is known of
  the records was found in sample drawings.

## Test data

Never commit `.dwg` or `.dxf` files, sample content, or anything derived from it (exports, OBJ
files, dumps of records). Samples live outside the repository; `.gitignore` refuses the file
types.

## Legal

AutoCAD and DWG are registered trademarks of Autodesk, Inc. This project is not affiliated with
or endorsed by Autodesk, Inc. Before any public release: legal review and a decision on the
licence.
