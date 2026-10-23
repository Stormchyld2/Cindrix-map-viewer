import { invoke, convertFileSrc, isTauri } from '@tauri-apps/api/core';
// Map-specific API adapted from the core CindrixOps application.
// ---------- saved maps ----------
export type MapMeta = { id: string; name: string; has_picture: boolean; has_route: boolean };
export type LatLon = [number, number];
export type RouteStats = { km: number; miles: number };

let memMaps: { id: string; json: string; notes: string; name: string }[] = [];

function haversineKm(a: LatLon, b: LatLon): number {
  const r = (d: number) => (d * Math.PI) / 180;
  const h = Math.sin(r(b[0] - a[0]) / 2) ** 2 + Math.cos(r(a[0])) * Math.cos(r(b[0])) * Math.sin(r(b[1] - a[1]) / 2) ** 2;
  return 2 * 6371.0088 * Math.asin(Math.sqrt(h));
}

export async function mapsList(): Promise<MapMeta[]> {
  if (isTauri()) return invoke<MapMeta[]>('maps_list');
  return memMaps.map((m) => ({ id: m.id, name: m.name, has_picture: false, has_route: false }));
}
export async function mapLoad(id: string): Promise<{ json: string; notes: string }> {
  if (isTauri()) return invoke<{ json: string; notes: string }>('map_load', { id });
  const m = memMaps.find((x) => x.id === id);
  if (!m) throw 'There is no such saved map.';
  return { json: m.json, notes: m.notes };
}
export async function mapSave(id: string | null, name: string, json: string, notes: string): Promise<string> {
  if (isTauri()) return invoke<string>('map_save', { id, name, json, notes });
  const nid = id ?? 'map-' + (memMaps.length + 1);
  const i = memMaps.findIndex((x) => x.id === nid);
  const row = { id: nid, json, notes, name };
  if (i >= 0) memMaps[i] = row; else memMaps.push(row);
  return nid;
}
export async function mapDelete(id: string): Promise<void> {
  if (isTauri()) return invoke<void>('map_delete', { id });
  memMaps = memMaps.filter((x) => x.id !== id);
}
export async function mapExportGpx(id: string, name: string, points: LatLon[]): Promise<string> {
  if (isTauri()) return invoke<string>('map_export_gpx', { id, name, points });
  return 'user/saved-maps/' + id + '/route.gpx';
}
export async function routeStats(points: LatLon[]): Promise<RouteStats> {
  if (isTauri()) return invoke<RouteStats>('route_stats', { points });
  let km = 0;
  for (let i = 1; i < points.length; i++) km += haversineKm(points[i - 1], points[i]);
  return { km, miles: km * 0.621371192 };
}

// ---------- map packs (the street and topo tiles on the drive) ----------
export type Pack = { rel: string; name: string; kind: string; mb: number };

export async function mapPacks(): Promise<Pack[]> {
  if (isTauri()) return invoke<Pack[]>('map_packs');
  return []; // browser preview: no packs, so the screen shows the plan view
}

// ---------- finding a place (a town, a ZIP code, a park, a latitude and longitude) ----------
export type PlaceHit = { name: string; state: string; kind: string; lat: number; lon: number };
export type PlaceResults = { places: PlaceHit[]; has_list: boolean };

export async function placeSearch(query: string): Promise<PlaceResults> {
  if (isTauri()) return invoke<PlaceResults>('place_search', { query });
  // Browser preview: a few sample places.
  const sample: PlaceHit[] = [
    { name: 'Denver', state: 'CO', kind: 'city', lat: 39.7392, lon: -104.9903 },
    { name: 'Colorado', state: 'CO', kind: 'state', lat: 39.0, lon: -105.5 },
  ];
  const q = query.trim().toLowerCase();
  return { places: q ? sample.filter((p) => p.name.toLowerCase().startsWith(q)) : [], has_list: false };
}

/** The address the map library reads a pack from. Only the desktop app can serve it. */
export async function tileUrl(rel: string): Promise<string> {
  const { convertFileSrc } = await import('@tauri-apps/api/core');
  return convertFileSrc(rel, 'cindrix-tiles');
}

