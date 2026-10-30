> **Superseded 2026-10-30.** This document was written before the viewer was compared with the main program. The `.cmap` format here (a ZIP with `manifest.json`) is **not** the format CindrixOps writes; see [CMAP_FORMAT.md](CMAP_FORMAT.md). Kept for history.

# M0 — Repository Audit and Integration Checkpoint

**Date:** 2026-10-22  
**Status:** Initial source audit; implementation not yet started.

## Viewer repository
The standalone repository was empty at inspection. The architecture file is its first commit. Use a root-level Svelte 5 + TypeScript + Vite + Tauri 2/Rust application rather than a nested monorepo.

## Upstream CindrixOps code inspected
- `package.json`: Svelte 5, Vite 6, MapLibre GL 5, PMTiles 4, Protomaps basemaps 5 and Tauri 2.
- `src/views/Maps.svelte`: MapLibre/PMTiles rendering, PMTiles header-derived bounds and zoom, optional local glyphs, POI display, saved pins, ordered routes, no-go polygons, and GPX route export.
- `src/lib/api.ts`: map list/load/save/delete, GPX route export, pack discovery, and custom `cindrix-tiles` URL conversion. GPX is not a complete offline map export.
- `src-tauri/src/lib.rs`: core map-pack discovery, saved-map commands and custom range-serving tile protocol. This should inform an isolated, read-only tile adapter in the viewer.

## M0 conclusions
1. Reuse presentation and map-rendering patterns, but do not couple the viewer to CindrixOps app state, drive discovery, or its runtime.
2. The viewer needs its own managed local map library, safe read-only PMTiles byte-range serving, and bundled offline styles/fonts/sprites.
3. Implement `.cmap` v1 as a Rust writer/validator with a versioned JSON manifest, integrity hashes, safe extraction, and hostile-archive unit tests.
4. Integrate the CindrixOps exporter later without breaking the existing GPX export.
5. Validate Windows first; Android and Wi-Fi/Bluetooth transfers follow verified desktop milestones.

## M1 entry criteria and acceptance
Create a conventional project scaffold, Rust package-format code and tests, frontend checks and Windows GitHub Actions. Before claiming completion, verify manifest-schema tests, round trips, traversal/duplicate/symlink/zip-bomb rejection, hash and size validation, and clean rollback. Record run IDs and artifacts.

## Remaining questions
Precise upstream map backend/storage layout, current CI outcomes, Windows minimum OS, optional encryption/signing, and final `.cmap` byte-level schema must be verified or decided before feature freeze.

**Protection note:** The `.cmap` extension by itself provides neither encryption nor DRM.
## M1 progress checkpoint (2026-10-22)

- Initial Windows-first Tauri 2/Svelte 5/TypeScript application scaffold committed, named **CindrixOps CMAP Viewer**.
- Fullscreen startup configured with a visible fullscreen toggle; responsive shell anticipates tablet/Android layout.
- Windows GitHub Actions workflow added: frontend tests, Svelte checks, Vite build, Rust tests, NSIS installer, artifact upload. **Actual workflow success and installer artifact not yet verified.**
- Rust `src-tauri/src/cmap.rs` now implements a `.cmap` v1 ZIP-compatible writer and read-only validator, JSON manifest, SHA-256 verification, path and archive-size checks, and unit tests for round trip, traversal, duplicate declarations, unsupported version, tampering and overwrite refusal.
- **Still outstanding before M1 signoff:** confirm GitHub Actions compilation and test results; correct failures; add binary fixtures, symlink and zip-bomb cases, robust atomic installation/recovery; freeze the schema; add actual viewer import and map rendering in M2. The writer currently accepts in-memory payloads; production multi-GB maps will require streaming IO rather than loading entire PMTiles files into RAM.
- `.cmap` is a structured package with integrity checks, not encryption. Protection/encryption policy remains an open owner decision.
