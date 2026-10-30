// Ways of writing a position, for showing on the map: degrees-minutes-seconds and the military grid (MGRS).
// The same arithmetic as core/src/coords.rs (which also reads positions typed in any of these ways).

const A = 6378137;
const F = 1 / 298.257223563;
const K0 = 0.9996;
const BANDS = 'CDEFGHJKLMNPQRSTUVWX';
const ROWS = 'ABCDEFGHJKLMNPQRSTUV';
const COLS = 'ABCDEFGHJKLMNPQRSTUVWXYZ';

function zoneOf(lat: number, lon: number): number {
  let z = Math.min(60, Math.max(1, Math.floor((lon + 180) / 6) + 1));
  if (lat >= 56 && lat < 64 && lon >= 3 && lon < 12) z = 32;
  if (lat >= 72 && lat < 84) {
    if (lon >= 0 && lon < 9) z = 31;
    else if (lon >= 9 && lon < 21) z = 33;
    else if (lon >= 21 && lon < 33) z = 35;
    else if (lon >= 33 && lon < 42) z = 37;
  }
  return z;
}

function forward(lat: number, lon: number, zone: number): [number, number] {
  const e2 = F * (2 - F);
  const ep2 = e2 / (1 - e2);
  const rad = Math.PI / 180;
  const phi = lat * rad;
  const lam = (lon - ((zone - 1) * 6 - 180 + 3)) * rad;
  const n = A / Math.sqrt(1 - e2 * Math.sin(phi) ** 2);
  const t = Math.tan(phi) ** 2;
  const c = ep2 * Math.cos(phi) ** 2;
  const a = lam * Math.cos(phi);
  const m = A * ((1 - e2 / 4 - (3 * e2 * e2) / 64 - (5 * e2 ** 3) / 256) * phi - ((3 * e2) / 8 + (3 * e2 * e2) / 32 + (45 * e2 ** 3) / 1024) * Math.sin(2 * phi) + ((15 * e2 * e2) / 256 + (45 * e2 ** 3) / 1024) * Math.sin(4 * phi) - ((35 * e2 ** 3) / 3072) * Math.sin(6 * phi));
  const x = K0 * n * (a + ((1 - t + c) * a ** 3) / 6 + ((5 - 18 * t + t * t + 72 * c - 58 * ep2) * a ** 5) / 120) + 500000;
  let y = K0 * (m + n * Math.tan(phi) * ((a * a) / 2 + ((5 - t + 9 * c + 4 * c * c) * a ** 4) / 24 + ((61 - 58 * t + t * t + 600 * c - 330 * ep2) * a ** 6) / 720));
  if (lat < 0) y += 10000000;
  return [x, y];
}

/** The military grid reference of a position, for example "17S LD 1234 5678" (10 m squares), or null outside 80°S to 84°N. */
export function toMgrs(lat: number, lon: number, digits = 4): string | null {
  if (!(lat >= -80 && lat <= 84) || !Number.isFinite(lon)) return null;
  const band = BANDS[Math.min(19, Math.floor((lat + 80) / 8))];
  const zone = zoneOf(lat, lon);
  const [x, y] = forward(lat, lon, zone);
  const colIdx = Math.floor(x / 100000) - 1;
  if (colIdx < 0 || colIdx > 7) return null;
  const col = COLS[((zone - 1) % 3) * 8 + colIdx];
  const row = ROWS[(((Math.floor(y / 100000) + (zone % 2 === 0 ? 5 : 0)) % 20) + 20) % 20];
  const d = Math.min(5, Math.max(1, digits));
  const scale = 10 ** (5 - d);
  const e = String(Math.floor((x % 100000) / scale)).padStart(d, '0');
  const n = String(Math.floor((y % 100000) / scale)).padStart(d, '0');
  return `${zone}${band} ${col}${row} ${e} ${n}`;
}

function dmsPart(v: number, pos: string, neg: string): string {
  const tenths = Math.round(Math.abs(v) * 3600 * 10);
  const d = Math.floor(tenths / 36000);
  const rest = tenths % 36000;
  const m = Math.floor(rest / 600);
  const s10 = rest % 600;
  return `${d}°${String(m).padStart(2, '0')}'${String(Math.floor(s10 / 10)).padStart(2, '0')}.${s10 % 10}"${v < 0 ? neg : pos}`;
}

/** Degrees, minutes and seconds: 39°29'06.5"N 80°08'21.4"W */
export function toDms(lat: number, lon: number): string {
  return `${dmsPart(lat, 'N', 'S')} ${dmsPart(lon, 'E', 'W')}`;
}
