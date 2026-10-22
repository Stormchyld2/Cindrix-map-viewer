# CindrixOps CMAP Viewer

Standalone, Windows-first offline map viewer. Android via Tauri 2 is planned after Windows verification.

**Current status:** Foundation only. The UI launches fullscreen by default and provides a fullscreen toggle. `.cmap` import/export, rendering, and transfers are **not implemented yet**.

Architecture: [docs/CINDRIXOPS_CMAP_VIEWER_ARCHITECTURE.md](docs/CINDRIXOPS_CMAP_VIEWER_ARCHITECTURE.md).

Development: `npm install`, `npm run check`, `npm test`, `npm run tauri dev`. Windows release: `npm run tauri build`. GitHub Actions builds an NSIS installer on Windows and uploads it as a workflow artifact after tests pass.
