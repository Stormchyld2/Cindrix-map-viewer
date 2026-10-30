# CindrixOps Map Viewer

A small offline viewer for the **phone map** (`.cmap`) that the main CindrixOps program makes
(Maps > **Send to phone**). It shows the street, topo and aerial pictures you chose, your pins, areas to
avoid, drawings and directions, the military grid position of the map centre, and where you are (GPS).
Nothing is sent over the network.

- **Format:** the viewer reads exactly what `Stormchyld2/Cindrix-Ops` writes (`core/src/phonepack.rs`,
  `docs/32-phone-map.md` there). See [docs/CMAP_FORMAT.md](docs/CMAP_FORMAT.md).
- **Map drawing:** the same Protomaps street rules and the same drawing, measuring and MGRS code as the main
  program (`src/lib/mapdraw.ts`, `coords.ts`, `mapkinds.ts` are copies; keep them in step).
- **Small screens:** built phone-first (touch-size buttons, a list that rises from the bottom, a **Full screen
  map** button that hides every button).
- **Windows first**, then Android through Tauri 2.

Not in the viewer (these stay in the main program): planning, saving maps, street search, directions
calculation, downloading maps.

Development: `npm install`, `npm run check`, `npm test`, `npm run tauri dev`.
Windows build: `npm run tauri build -- --no-bundle` (CI does this and uploads the exe).
