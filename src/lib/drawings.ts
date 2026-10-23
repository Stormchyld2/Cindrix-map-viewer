/** Editable annotations stored separately from an exported CMAP basemap. */
export type Coordinate = [longitude: number, latitude: number];
export type Drawing =
  | { id: string; kind: 'line' | 'route' | 'freehand'; points: Coordinate[]; color: string; label?: string }
  | { id: string; kind: 'polygon' | 'nogo'; points: Coordinate[]; color: string; label?: string }
  | { id: string; kind: 'circle'; center: Coordinate; radiusMeters: number; color: string; label?: string }
  | { id: string; kind: 'marker' | 'rendezvous' | 'text'; point: Coordinate; color: string; label: string };
export type DrawingDocument = { version: 1; packageId: string; drawings: Drawing[] };
const validPoint = (p: Coordinate) => Array.isArray(p) && p.length === 2 && p.every(Number.isFinite) && Math.abs(p[0]) <= 180 && Math.abs(p[1]) <= 90;
export function validateDrawing(d: Drawing): boolean {
  if (!d.id || !d.color || !/^#[0-9a-fA-F]{6}$/.test(d.color)) return false;
  if ('points' in d) return d.points.length >= (d.kind === 'polygon' || d.kind === 'nogo' ? 3 : 2) && d.points.every(validPoint);
  if ('center' in d) return validPoint(d.center) && Number.isFinite(d.radiusMeters) && d.radiusMeters > 0;
  return validPoint(d.point) && typeof d.label === 'string';
}
export function serializeDrawings(doc: DrawingDocument): string {
  if (doc.version !== 1 || !doc.packageId || !doc.drawings.every(validateDrawing)) throw new Error('Invalid drawing document');
  return JSON.stringify(doc, null, 2);
}
export function parseDrawings(json: string): DrawingDocument {
  const doc: unknown = JSON.parse(json);
  if (!doc || typeof doc !== 'object') throw new Error('Invalid drawing document');
  const d = doc as DrawingDocument;
  if (d.version !== 1 || typeof d.packageId !== 'string' || !d.packageId || !Array.isArray(d.drawings) || !d.drawings.every(validateDrawing)) throw new Error('Invalid drawing document');
  return d;
}
