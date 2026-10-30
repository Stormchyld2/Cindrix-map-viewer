> **Superseded 2026-10-30.** This document was written before the viewer was compared with the main program. The `.cmap` format here (a ZIP with `manifest.json`) is **not** the format CindrixOps writes; see [CMAP_FORMAT.md](CMAP_FORMAT.md). Kept for history.

# CindrixOps CMAP Viewer — Windows-First Architecture

**Status:** Windows application foundation committed; map viewer functionality remains under development  
**Version:** 1.1 (Windows foundation)  
**Date:** 2026-10-22  
**Repository:** `Stormchyld2/Cindrix-map-viewer` (standalone viewer)
**Upstream architecture:** `Stormchyld2/Cindrix-Ops/docs/CINDRIXOPS_CMAP_VIEWER_ARCHITECTURE.md`  
**Owner decisions:** Windows first; standalone viewer; Tauri 2 Android adaptation second; Svelte, TypeScript, Rust, JSON, Markdown, filesystem; native `.cmap` map packages.

## 1. Objective and boundaries

Deliver a small **CindrixOps Map Viewer** that opens self-contained offline `.cmap` map packages without requiring the main CindrixOps application, an AI model, a network connection, or a server. The existing CindrixOps Maps area will eventually export compatible packages. First complete and verify the Windows viewer; then adapt the shared implementation to Android with Tauri 2, including optional Hub transfer via local Wi-Fi/hotspot and Bluetooth where supported.

**Do not confuse:** Existing GPX route export is not a full map export. `.cmap` must contain tiles and all rendering assets. This document authorizes beginning a scoped implementation milestone, not unrelated changes to CindrixOps.

## 2. Verified existing repository baseline

Inspected 2026-10-22:
- `package.json`: Svelte 5, TypeScript, Vite 6, Tauri 2, `maplibre-gl` 5, `pmtiles` 4, `@protomaps/basemaps` 5.
- `src-tauri/Cargo.toml`: Rust/Tauri 2, serde/serde_json, existing `cindrix-core` and `cindrix-crypto` path crates.
- `src/views/Maps.svelte`: dynamic MapLibre/PMTiles/Protomaps imports; map pack discovery and `tileUrl`; optional local glyph resources; POI layers; pins, ordered routes, no-go polygons; map save/load; GPX route export.
- `src/lib/api.ts`: map APIs imported by the Maps view.

**New viewer repository at kickoff:** empty, default branch `main`; no app source or CI exists yet. Upstream not yet verified: backend tile server internals, map pack discovery paths, cross-platform file permissions, current tests/CI outcomes, Android build support, or any existing whole-map exporter. Inspect these before editing. Do not overwrite or replace the current Maps interface.

## 3. Repository layout and component boundaries

Since this is a dedicated repository, place the Tauri viewer at repository root; add narrowly scoped shared modules within this repository only when needed:

```text
src/
    App.svelte
    components/MapCanvas.svelte
    components/MapLibrary.svelte
    lib/types.ts
src-tauri/
  src/lib.rs
  tauri.conf.json
README.md
shared/                        # only if needed after review
  cmap-schema/
  map-rendering/
  map-model/
```

Keep app UI in Svelte/TypeScript, package IO and validation in Rust, and structured configuration in JSON. Keep Markdown architecture and user instructions. Preserve the current application's build scripts and dependency graph. This viewer repository is initially empty; use a conventional single-project root layout rather than creating a nested application workspace.

### Runtime responsibilities

- **Viewer UI:** import/open/list/remove maps; pan/zoom; optional GNSS location; show routes, pins, no-go polygons, POIs, attribution, storage usage, and meaningful errors.
- **Shared map rendering:** reuse MapLibre style and Protomaps layer conventions where possible; never reference remote style/glyph/sprite URLs for an offline package.
- **Rust `.cmap` core:** create/read/validate/install/export archives, enforce size and path limits, check hashes, atomic installation and recovery.
- **Platform adapters:** Windows file dialogs and filesystem initially; Android storage permissions and hotspot/Bluetooth adapters only after Windows release validation.
- **Transfer service:** later, authenticated local HTTP file delivery with user-approved pairing, resumability, and hash validation. No public internet dependency.

## 4. `.cmap` v1 portable format

**File extension:** `.cmap`. **Container:** ZIP-compatible archive with a mandatory root manifest. `.cmap` is the only user-facing full-map package extension; internal PMTiles are implementation details.

```text
region.cmap
├── manifest.json
├── map/
│   ├── region.pmtiles
│   ├── style.json
│   ├── sprites/...
│   └── fonts/...
├── overlays/
│   ├── waypoints.geojson
│   ├── routes.geojson
│   └── nogo.geojson
└── metadata/
    ├── attribution.json
    └── licenses/...
```

### Manifest contract (illustrative; freeze with schema tests before writing production packages)

```json
{
  "format": "cindrix-cmap",
  "formatVersion": 1,
  "packageId": "example-region-uuid",
  "title": "Example Region",
  "createdUtc": "2026-10-22T00:00:00Z",
  "boundsWgs84": [-80.5, 38.0, -79.0, 39.5],
  "map": {
    "tiles": "map/region.pmtiles",
    "style": "map/style.json"
  },
  "overlays": [],
  "entries": [
    {
      "path": "map/region.pmtiles",
      "bytes": 123456,
      "sha256": "<64-lowercase-hex-digest>"
    }
  ],
  "attribution": "metadata/attribution.json",
  "minimumViewerVersion": "1.0.0"
}
```

The example is **not a valid distributable package** until its entry list, hashes, and files are complete. All included payload entries must be represented in the manifest and validated; the manifest itself is the root of the integrity inventory. Freeze path normalization, schema, canonicalization, version compatibility, overlay CRS (WGS84 lon/lat), and archive compression rules in v1 tests. PMTiles readers generally need random access: extract to an app-managed local file, or prove safe direct range access before adopting it.

### Security and protection

A custom extension and ZIP-compatible archive **do not provide secrecy or copy protection**. Use strict validation and SHA-256 for corruption detection; hashes inside the same package are not proof of trusted authorship. Optional **authenticated encryption** for confidential map contents and optional **publisher signatures** for authenticity should be separate, versioned extensions requiring explicit decisions on keys, password recovery, and offline access. Do not invent an encryption algorithm or silently require a password for emergency maps. Existing `cindrix-crypto` must be reviewed before any reuse.

Reject traversal paths, absolute paths, symlinks, duplicate names, archive bombs, oversized entries, missing files, invalid tile/style/GeoJSON assets, hash mismatches, and unsupported versions. Enforce extraction quotas and atomic installs; a failed import must not damage an installed map.

## 5. Export semantics

CindrixOps Maps export should let the user select a map region/pack, optional routes/pins/no-go areas, and a destination. Export must bundle actual local tiles, glyphs, sprites, style, attribution and licenses. If the source pack is larger than the selected region, do not promise clipping until a verified PMTiles extraction/repack pipeline exists; initial v1 may export complete selected packs. GPX export remains available independently.

Viewer imports and stores `.cmap` in a managed local library. Preserve package identity and provenance; handle duplicate package IDs and updates explicitly. Maintain ordinary filesystem backups and permit USB/SD/local import.

## 6. User workflow

**Windows v1:** Open Viewer → Import `.cmap` → validate → install → choose map → navigate offline → inspect overlays/POIs → close/reopen with no internet. Optional future import from CindrixOps Vault through local file transfer.

**Android phase:** Install Viewer APK → join Hub hotspot or local network → pair/approve → receive `.cmap` → validate/install → use offline after disconnect. Bluetooth for small packages and waypoint/route payloads only after testing platform feasibility and permissions. A hotspot alone does not guarantee client-to-host HTTP reachability; test on real devices.

## 7. Windows-first milestones and gates

**M0 — Audit and checkpoint:** inspect map backend, asset serving, source formats, file storage, CI and latest HEAD; record reusable code, gaps and conflicts. No changes until baseline verified.

**M1 — Package contract:** Rust `.cmap` parser/validator and writer; JSON schema fixtures; round-trip, corruption, archive safety and compatibility tests.

**M2 — Standalone Windows viewer:** isolated Tauri 2/Svelte app, local import library, offline MapLibre rendering, pins/routes/no-go overlays, attribution, map management and error states.

**M3 — Existing-app export integration:** explicit `.cmap` export from CindrixOps Maps, no regression to GPX/map planning; use shared writer/schema.

**M4 — Windows transfer and release:** local filesystem/USB first; authenticated Wi-Fi transfer if scoped; tests, reproducible Windows build, actual offline run and documentation.

**M5 — Android port:** Tauri 2 Android packaging, file access and permissions, offline rendering and storage; Hub Wi-Fi transfer, then optional Bluetooth. Test APK on physical devices, not just CI.

**Stop/go:** advance only when current milestone passes required tests, builds and owner acceptance. Do not claim completed milestones before verification.

## 8. Verification matrix

- Rust: manifest schema, round-trip, deterministic parsing, bad hashes, malicious ZIP paths, duplicates, decompression limits, interrupted install, rollback, version rejection.
- Frontend: map library UI, failed imports, overlay coordinates, no-network rendering, missing glyph/sprite handling.
- Integration: CindrixOps export → viewer import → identical region/route/waypoints/no-go polygons; verify licenses and attribution.
- Windows: clean installation, large PMTiles, disk-full behavior, reopen offline, no unexpected external requests.
- Network (when implemented): pairing/auth, interruption/resume, untrusted sender, wrong checksum, Hub hotspot reachability.
- Android (later): APK, scoped storage, GNSS, background/resume, hotspot and Bluetooth permissions, device tests.
- CI: preserve existing GitHub Actions, record workflow names, run URLs, conclusions and produced artifacts; manual device/UI verification separately documented.

## 9. Implementation handoff and first action

1. Read existing repository architecture and this file.
2. Inspect `src/views/Maps.svelte`, `src/lib/api.ts`, Rust map commands, pack locations, CI workflows, and current HEAD.
3. Produce a brief **M0 audit**: current capabilities, reusable code, missing pieces, proposed file paths, dependencies, risks and test plan.
4. Start M1 only after M0 confirms integration choices; avoid broad refactors and preserve user-facing branding.
5. Report resulting commit, tests, CI, Windows artifact, and next milestone. Never claim a build/test passed without evidence.

**Open decisions:** exact package encryption policy, publisher signing, Windows minimum OS, Android minimum SDK, maximum package sizes, package update behavior, and whether Wi-Fi transfer belongs in Windows v1 or a subsequent milestone.

---

**Change control:** This file is the dedicated implementation architecture for the map viewer. It does not authorize changing unrelated CindrixOps modules or adopting new security requirements without approval.

## 10. Windows-first UI and CI decisions (2026-10-22)

- Product name: **CindrixOps CMAP Viewer**.
- Default startup: **fullscreen** on Windows, with an explicit exit/toggle control. Preserve responsive touch-friendly layouts for later tablets/Android; avoid desktop-only interaction assumptions.
- Primary targets: Windows first, then Android tablets/phones using shared Tauri 2/Svelte/TypeScript/Rust code. Do not remove Windows support after Android launch.
- GitHub Actions: on push/PR/manual dispatch, run frontend tests, Svelte/TypeScript checks, frontend build, Rust tests, and Windows NSIS installer build. Upload installer as artifact only after passing all steps. Do not report green CI or downloadable installers without verifying actual run conclusions and artifacts.
- Current implementation checkpoint: initial project shell, fullscreen configuration and toggle, frontend foundation test, and Windows CI workflow exist. `.cmap` reader/writer, real map rendering, exporter, and transfers are still unimplemented.
