# Core-map parity implementation plan

Source of truth: `Stormchyld2/Cindrix-Ops` on its default branch. The standalone viewer must reuse the actual map implementation rather than recreate a simplified mock.

## Source modules and responsibilities

| Main application | Standalone target | Notes |
| --- | --- | --- |
| `src/views/Maps.svelte` | `src/views/Maps.svelte` | Port map screen, adapt only imports/navigation |
| `src/lib/mapkinds.ts` | `src/lib/mapkinds.ts` | Ported directly; keep POI classifications aligned |
| `core/src/maps.rs` | `src-tauri/src/maps.rs` | Port saved maps, validation, distances, GPX, tile-range helpers and tests; replace only app-specific fsio dependency |
| `src/lib/api.ts` (map exports) | `src/lib/maps-api.ts` | Narrow adapter preserving names and DTO shapes |
| `src-tauri/src/lib.rs` (map commands) | `src-tauri/src/lib.rs` | Register map commands and restricted asynchronous `cindrix-tiles` protocol |
| `docs/18-maps.md` | documentation / acceptance criteria | Read the documented limitations; don't advertise mockup-only features |

## Integration order

1. Port the Rust map core and its existing tests, preserving safe path resolution and byte-range handling. Make the standalone map storage root explicit and portable.
2. Register `maps_list`, `map_load`, `map_save`, `map_delete`, `map_export_gpx`, `route_stats`, and `map_packs`; add `cindrix-tiles` protocol. Avoid exposing arbitrary filesystem paths.
3. Implement a narrow frontend API adapter with the same DTOs. Port the full `Maps.svelte` component, removing main-app settings/navigation dependencies without dropping capabilities.
4. Validate real local PMTiles rendering with offline network disabled; verify pan, zoom, POI filters, adding/editing pins, rendezvous points, routes, no-go polygons, saving/reloading, notes, and GPX export.
5. Implement `.cmap` package install/open/export as an additional boundary around the working map implementation, not as a substitute for it. Validate hashes and reject unsafe archive paths; do not load entire multi-GB archives into RAM.
6. Keep fullscreen Windows behavior and tablet-friendly controls. Build Android later without compromising the Windows version.

## Important limitations in the upstream source

The main application's `docs/18-maps.md` explicitly states the tile viewer was not verified against real PMTiles at the time of that document, and that street-road routing, GPS, satellite/topographic rendering, print/export-image and certain mockup features were not complete. Inspect current code and verify each feature before claiming parity. Straight-line route distance is not road navigation.

## Verification gate

The milestone is **not complete** merely because GitHub Actions is green. It requires a working real offline map, a round-trip saved map, safe range-based PMTiles access, a real GPX export, and a successful Windows installer smoke test. Preserve the current passing CI baseline while adding these capabilities.
