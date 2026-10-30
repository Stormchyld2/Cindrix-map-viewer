// Reads the person's own map data out of a .cmap file. The file is not trusted: everything is checked and
// cleaned here so the screen only ever sees well-formed pins, areas and drawings.
import type { Drawing, DrawKind, LatLon } from './mapdraw';

export type Pin = { id: number; label: string; kind: 'pin' | 'rendezvous'; lat: number; lon: number; tag: string; color: string };
export type Area = { label: string; points: LatLon[] };
export type DirStep = { name: string; km: number; minutes: number };
export type Dir = { points: LatLon[]; km: number; minutes: number; steps: DirStep[] };
export type Overlays = { pins: Pin[]; route: number[]; nogo: Area[]; drawings: Drawing[]; directions: Dir | null; notes: string };

const MAX_POINTS = 50_000;
const KINDS: DrawKind[] = ['line', 'shape', 'rect', 'circle', 'text'];

const isObj = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v);
const num = (v: unknown): number | null => (typeof v === 'number' && Number.isFinite(v) ? v : null);
const str = (v: unknown, max = 200): string => (typeof v === 'string' ? v.slice(0, max) : '');

export function point(v: unknown): LatLon | null {
  if (!Array.isArray(v) || v.length < 2) return null;
  const la = num(v[0]);
  const lo = num(v[1]);
  if (la === null || lo === null || la < -90 || la > 90 || lo < -180 || lo > 180) return null;
  return [la, lo];
}

function points(v: unknown): LatLon[] {
  if (!Array.isArray(v)) return [];
  const out: LatLon[] = [];
  for (const p of v.slice(0, MAX_POINTS)) {
    const q = point(p);
    if (q) out.push(q);
  }
  return out;
}

export function colour(v: unknown, fallback: string): string {
  return typeof v === 'string' && /^#[0-9a-fA-F]{3,8}$/.test(v) ? v : fallback;
}

export const pinColor = (kind: string): string => (kind === 'rendezvous' ? '#6fbf73' : '#5aa9e6');

export function readOverlays(raw: unknown): Overlays {
  const o: Overlays = { pins: [], route: [], nogo: [], drawings: [], directions: null, notes: '' };
  if (!isObj(raw)) return o;
  o.notes = str(raw.notes, 20000);
  if (Array.isArray(raw.pins)) {
    for (const p of raw.pins.slice(0, 5000)) {
      if (!isObj(p) || p.hidden === true) continue;
      const la = num(p.lat);
      const lo = num(p.lon);
      if (la === null || lo === null || Math.abs(la) > 90 || Math.abs(lo) > 180) continue;
      const kind = p.kind === 'rendezvous' ? 'rendezvous' : 'pin';
      o.pins.push({ id: num(p.id) ?? o.pins.length + 1, label: str(p.label) || 'Pin', kind, lat: la, lon: lo, tag: str(p.tag, 60), color: colour(p.color, pinColor(kind)) });
    }
  }
  if (Array.isArray(raw.route)) o.route = raw.route.map(num).filter((n): n is number => n !== null).slice(0, 5000);
  if (Array.isArray(raw.nogo)) {
    for (const a of raw.nogo.slice(0, 1000)) {
      if (!isObj(a)) continue;
      const pts = points(a.points);
      if (pts.length >= 3) o.nogo.push({ label: str(a.label) || 'Area to avoid', points: pts });
    }
  }
  if (Array.isArray(raw.drawings)) {
    for (const d of raw.drawings.slice(0, 5000)) {
      if (!isObj(d) || typeof d.kind !== 'string' || !KINDS.includes(d.kind as DrawKind)) continue;
      const pts = points(d.points);
      if (!pts.length) continue;
      const r = num(d.radiusM);
      o.drawings.push({
        id: num(d.id) ?? o.drawings.length + 1,
        kind: d.kind as DrawKind,
        label: str(d.label),
        color: colour(d.color, '#1a73e8'),
        width: Math.min(12, Math.max(1, num(d.width) ?? 4)),
        points: pts,
        ...(r !== null && r > 0 && r < 5_000_000 ? { radiusM: r } : {}),
      });
    }
  }
  if (isObj(raw.directions)) {
    const pts = points(raw.directions.points);
    if (pts.length >= 2) {
      const steps: DirStep[] = [];
      if (Array.isArray(raw.directions.steps)) {
        for (const s of raw.directions.steps.slice(0, 2000)) {
          if (isObj(s)) steps.push({ name: str(s.name), km: num(s.km) ?? 0, minutes: num(s.minutes) ?? 0 });
        }
      }
      o.directions = { points: pts, km: num(raw.directions.km) ?? 0, minutes: num(raw.directions.minutes) ?? 0, steps };
    }
  }
  return o;
}

/** The line a saved route draws when there are no directions: the route's pins in order. */
export function routeLine(o: Overlays): LatLon[] {
  return o.route.map((id) => o.pins.find((p) => p.id === id)).filter((p): p is Pin => !!p).map((p) => [p.lat, p.lon] as LatLon);
}
