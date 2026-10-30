// The only place the screen talks to the backend. A phone map (.cmap) is made by the main CindrixOps program
// (Maps > Send to phone); this viewer only opens and shows it.
import { invoke, convertFileSrc } from '@tauri-apps/api/core';

export type Layer = {
  kind: 'street' | 'topo' | 'aerial' | string;
  tile_type: string;
  compression: string;
  min_zoom: number;
  max_zoom: number;
  tiles: number;
};
export type Glyph = { font: string; range: string };
export type CmapInfo = {
  format: number;
  name: string;
  created: number;
  /** west, south, east, north */
  bbox: [number, number, number, number];
  view: { center: [number, number]; zoom: number };
  layers: Layer[];
  glyphs: Glyph[];
  /** The person's own pins, route, areas to avoid, drawings and directions, exactly as saved. Untrusted: read with overlays.ts. */
  map: unknown;
  unsupported: string[];
};

/** Ask the person for a .cmap file. Returns its path, or null when they cancel. */
export async function pickCmap(): Promise<string | null> {
  const { open } = await import('@tauri-apps/plugin-dialog');
  const r = await open({ multiple: false, directory: false, filters: [{ name: 'CindrixOps phone map', extensions: ['cmap'] }] });
  return typeof r === 'string' ? r : null;
}

export function openCmap(path: string): Promise<CmapInfo> {
  return invoke<CmapInfo>('cmap_open', { path });
}

/** A .cmap file the program was started with (double-click on Windows), if any. */
export function startupFile(): Promise<string | null> {
  return invoke<string | null>('startup_file');
}

/** Where tiles and lettering are served from, with a trailing slash: `<base>tiles/street/{z}/{x}/{y}`. */
export function mapBase(): string {
  return convertFileSrc('x', 'cmap').slice(0, -1);
}

/** Tauri messages arrive as plain text; anything else gets a plain fallback. */
export function plainError(e: unknown): string {
  if (typeof e === 'string' && e) return e;
  if (e instanceof Error && e.message) return e.message;
  return 'The map could not be opened.';
}
