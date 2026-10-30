# The `.cmap` phone map file

The viewer reads **version 1** of the file the main CindrixOps program writes. The main program's own
description is `docs/32-phone-map.md` in `Stormchyld2/Cindrix-Ops`, and its writer is `core/src/phonepack.rs`.
If the two ever disagree, the main program's writer wins; fix this viewer.

```text
"CMAP0001" | tile data and lettering files | tile lists | manifest (JSON) | manifest length (u32) | "CMAPEND1"
```

All numbers are little-endian. A reader takes the last 12 bytes, checks `CMAPEND1`, reads the manifest length,
then the manifest just before it. Each tile list entry is five u32: `z, x, y, offset, length`; the tile is at
`data_offset + offset`, exactly as it was in the source map file, still packed as the layer's `compression` says.

The manifest holds `format`, `name`, `created`, `bbox`, `view`, `layers[]`, `glyphs[]` and `map` (the person's
own pins, route, areas to avoid, drawings and directions).

## What this viewer does with it

- `src-tauri/src/cmap.rs` reads the file. It checks that every layer and lettering file lies inside the file,
  reads each tile list into memory (20 bytes per tile) and finds a tile by binary search. It never writes.
- `lib.rs` serves tiles and lettering to the map screen at `cmap://localhost/tiles/<kind>/<z>/<x>/<y>` and
  `.../glyphs/<font>/<range>.pbf` (on Windows and Android the address is `http://cmap.localhost/...`).
  A tile the file does not hold is answered 404, which the map engine treats as an empty square.
- `src/lib/overlays.ts` cleans the `map` object (it comes from a file, so it is never trusted).

## Not yet supported

- **zstd tiles** (the viewer says so in plain words and shows the other layers). gzip and uncompressed work.
- **Android file picking:** the Android file picker returns a `content://` address, not a path. Opening a map on
  a phone needs a small native step (copy it into the app's own folder first). Windows opens files directly.
- GPX tracks recorded on the phone (to show them over the map).

## Tests

`tests/fixtures/real-core-v1.cmap` was written by the main program's own writer, not by hand. To make another,
add a throwaway test next to `the_phone_map_is_written_and_every_tile_can_be_found_again` in `phonepack.rs`
that calls `build(...)` into a folder and copy the file here.
