// Drawing on the map: the shapes a person can add (lines, areas, boxes, circles, text), and the arithmetic behind
// measuring them. Nothing here touches the screen, so it can be checked on its own.
import { distanceKm } from './mapkinds';

export type LatLon = [number, number];
export type DrawKind = 'line' | 'shape' | 'rect' | 'circle' | 'text';

export type Drawing = {
  id: number;
  kind: DrawKind;
  /** The name shown on the map (for a text mark, the text itself). */
  label: string;
  color: string;
  /** Line thickness in pixels. */
  width: number;
  /** line and shape: every corner in order. rect: two opposite corners. circle: the middle. text: where it sits. */
  points: LatLon[];
  /** circle only: the radius in metres. */
  radiusM?: number;
};

export const DRAW_COLORS: { name: string; hex: string }[] = [
  { name: 'Red', hex: '#d93025' },
  { name: 'Orange', hex: '#f29900' },
  { name: 'Yellow', hex: '#e8c600' },
  { name: 'Green', hex: '#188038' },
  { name: 'Blue', hex: '#1a73e8' },
  { name: 'Purple', hex: '#9334e6' },
  { name: 'Black', hex: '#202124' },
  { name: 'White', hex: '#ffffff' },
];
export const DRAW_WIDTHS: { name: string; px: number }[] = [
  { name: 'Thin', px: 2 },
  { name: 'Medium', px: 4 },
  { name: 'Thick', px: 7 },
];
export const DEFAULT_COLOR = DRAW_COLORS[4].hex;

const R_M = 6371008.8;
const rad = (d: number) => (d * Math.PI) / 180;
const deg = (r: number) => (r * 180) / Math.PI;

/** Length of a path in kilometres. */
export function pathKm(points: LatLon[]): number {
  let t = 0;
  for (let i = 1; i < points.length; i++) t += distanceKm(points[i - 1], points[i]);
  return t;
}

/** Area enclosed by a ring of corners, in square kilometres (flat-earth near the shape; fine up to a few hundred km). */
export function areaKm2(points: LatLon[]): number {
  if (points.length < 3) return 0;
  const lat0 = points.reduce((a, p) => a + p[0], 0) / points.length;
  const kx = Math.cos(rad(lat0));
  const xy = points.map(([la, lo]) => [(R_M / 1000) * rad(lo) * kx, (R_M / 1000) * rad(la)]);
  let s = 0;
  for (let i = 0; i < xy.length; i++) {
    const [x1, y1] = xy[i];
    const [x2, y2] = xy[(i + 1) % xy.length];
    s += x1 * y2 - x2 * y1;
  }
  return Math.abs(s) / 2;
}

/** The ring of points of a circle with the given middle and radius (metres). */
export function circlePoints(center: LatLon, radiusM: number, steps = 72): LatLon[] {
  const out: LatLon[] = [];
  const la = rad(center[0]);
  const lo = rad(center[1]);
  const d = radiusM / R_M;
  for (let i = 0; i < steps; i++) {
    const b = (2 * Math.PI * i) / steps;
    const la2 = Math.asin(Math.sin(la) * Math.cos(d) + Math.cos(la) * Math.sin(d) * Math.cos(b));
    const lo2 = lo + Math.atan2(Math.sin(b) * Math.sin(d) * Math.cos(la), Math.cos(d) - Math.sin(la) * Math.sin(la2));
    out.push([deg(la2), ((deg(lo2) + 540) % 360) - 180]);
  }
  return out;
}

/** The four corners of a box drawn between two opposite corners. */
export function rectPoints(a: LatLon, b: LatLon): LatLon[] {
  return [a, [a[0], b[1]], b, [b[0], a[1]]];
}

/** The ring (closed list of corners) a shape is drawn with, or null for lines and text. */
export function ringOf(d: Drawing): LatLon[] | null {
  if (d.kind === 'shape' && d.points.length >= 3) return d.points;
  if (d.kind === 'rect' && d.points.length >= 2) return rectPoints(d.points[0], d.points[1]);
  if (d.kind === 'circle' && d.points.length >= 1 && (d.radiusM ?? 0) > 0) return circlePoints(d.points[0], d.radiusM as number);
  return null;
}

/** A point to hang a name on: the middle of a shape's corners, or the first point of a line. */
export function labelPoint(d: Drawing): LatLon | null {
  if (!d.points.length) return null;
  const ring = ringOf(d);
  if (ring) {
    return [ring.reduce((a, p) => a + p[0], 0) / ring.length, ring.reduce((a, p) => a + p[1], 0) / ring.length];
  }
  if (d.kind === 'line') return d.points[Math.floor(d.points.length / 2)];
  return d.points[0];
}

/** What a drawing measures: a line's length, or a shape's edge length and area. */
export function measureOf(d: Drawing): { km: number; km2: number } {
  if (d.kind === 'line') return { km: pathKm(d.points), km2: 0 };
  if (d.kind === 'circle') {
    const r = (d.radiusM ?? 0) / 1000;
    return { km: 2 * Math.PI * r, km2: Math.PI * r * r };
  }
  const ring = ringOf(d);
  if (ring) return { km: pathKm([...ring, ring[0]]), km2: areaKm2(ring) };
  return { km: 0, km2: 0 };
}

export function fmtLength(km: number, metric: boolean): string {
  if (metric) return km < 1 ? `${Math.round(km * 1000)} m` : `${km.toFixed(km < 10 ? 2 : 1)} km`;
  const ft = km * 3280.84;
  return ft < 1000 ? `${Math.round(ft)} ft` : `${(km * 0.621371).toFixed(km * 0.621371 < 10 ? 2 : 1)} mi`;
}

export function fmtArea(km2: number, metric: boolean): string {
  if (metric) {
    if (km2 < 0.01) return `${Math.round(km2 * 1e6)} m²`;
    return km2 < 1 ? `${(km2 * 100).toFixed(2)} ha` : `${km2.toFixed(2)} km²`;
  }
  const acres = km2 * 247.105;
  return acres < 640 ? `${acres < 10 ? acres.toFixed(2) : acres.toFixed(1)} acres` : `${(km2 * 0.386102).toFixed(2)} sq mi`;
}

type Feature = { type: 'Feature'; properties: Record<string, unknown>; geometry: { type: string; coordinates: unknown } };

const lonlat = (p: LatLon): [number, number] => [p[1], p[0]];

/** Everything drawn, as map features: lines, and shapes (with their fill). Text is drawn separately as labels. */
export function drawingFeatures(list: Drawing[], selectedId: number | null): { type: 'FeatureCollection'; features: Feature[] } {
  const features: Feature[] = [];
  for (const d of list) {
    const props = { id: d.id, color: d.color, width: d.width, sel: d.id === selectedId ? 1 : 0 };
    if (d.kind === 'line' && d.points.length >= 2) {
      features.push({ type: 'Feature', properties: props, geometry: { type: 'LineString', coordinates: d.points.map(lonlat) } });
      continue;
    }
    const ring = ringOf(d);
    if (ring) {
      features.push({ type: 'Feature', properties: props, geometry: { type: 'Polygon', coordinates: [[...ring, ring[0]].map(lonlat)] } });
    }
  }
  return { type: 'FeatureCollection', features };
}

/** The unfinished shape while it is being drawn, and the path being measured. */
export function previewFeatures(points: LatLon[], kind: DrawKind | 'measure', hover: LatLon | null, radiusM: number | null): { type: 'FeatureCollection'; features: Feature[] } {
  const features: Feature[] = [];
  const pts = hover ? [...points, hover] : points;
  if (kind === 'circle' && points.length === 1) {
    const r = radiusM ?? (hover ? distanceKm(points[0], hover) * 1000 : 0);
    if (r > 0) {
      const ring = circlePoints(points[0], r);
      features.push({ type: 'Feature', properties: {}, geometry: { type: 'LineString', coordinates: [...ring, ring[0]].map(lonlat) } });
    }
  } else if (kind === 'rect' && points.length === 1 && hover) {
    const ring = rectPoints(points[0], hover);
    features.push({ type: 'Feature', properties: {}, geometry: { type: 'LineString', coordinates: [...ring, ring[0]].map(lonlat) } });
  } else if (pts.length >= 2) {
    const closed = kind === 'shape' && pts.length >= 3;
    features.push({ type: 'Feature', properties: {}, geometry: { type: 'LineString', coordinates: (closed ? [...pts, pts[0]] : pts).map(lonlat) } });
  }
  for (const p of points) features.push({ type: 'Feature', properties: { pt: 1 }, geometry: { type: 'Point', coordinates: lonlat(p) } });
  return { type: 'FeatureCollection', features };
}

/** The handles a selected drawing can be reshaped by: each corner, or a circle's middle and one point on its edge. */
export function handlesOf(d: Drawing): LatLon[] {
  if (d.kind === 'circle' && d.points.length) {
    const edge = circlePoints(d.points[0], d.radiusM ?? 0, 4)[1]; // due east of the middle
    return [d.points[0], edge];
  }
  return d.points;
}
