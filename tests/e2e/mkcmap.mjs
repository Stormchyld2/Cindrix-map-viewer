// Builds .cmap files exactly as the main CindrixOps program lays them out (docs/32-phone-map.md there),
// with real vector tiles (gzip packed, like a Protomaps street map) and picture tiles, so the viewer can be tried
// in a browser without a phone or a Windows machine.
import { gzipSync, deflateSync } from "node:zlib";

// ---------- tiles ----------
const varint = (n) => {
  const out = [];
  while (n > 127) { out.push((n & 127) | 128); n = Math.floor(n / 128); }
  out.push(n);
  return Buffer.from(out);
};
const field = (num, wire, body) => Buffer.concat([varint(num * 8 + wire), body]);
const lenField = (num, buf) => field(num, 2, Buffer.concat([varint(buf.length), buf]));
const zig = (v) => (v << 1) ^ (v >> 31);
const packed = (arr) => Buffer.concat(arr.map((v) => varint(v)));

/** A vector tile layer holding one polygon: a rectangle [x0,y0,x1,y1] in 0..4096 tile units. */
function polygonLayer(name, [x0, y0, x1, y1]) {
  const g = [9, zig(x0), zig(y0), 26, zig(x1 - x0), zig(0), zig(0), zig(y1 - y0), zig(x0 - x1), zig(0), 15];
  const feature = Buffer.concat([field(3, 0, varint(3)), lenField(4, packed(g))]);
  return lenField(3, Buffer.concat([lenField(1, Buffer.from(name)), lenField(2, feature), field(5, 0, varint(4096)), field(15, 0, varint(2))]));
}

/** Earth everywhere, and water over the top half of the square. */
export function streetTile() {
  return Buffer.concat([polygonLayer("earth", [0, 0, 4096, 4096]), polygonLayer("water", [0, 0, 4096, 2048])]);
}

const crcTable = (() => {
  const t = [];
  for (let n = 0; n < 256; n++) { let c = n; for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; t[n] = c >>> 0; }
  return t;
})();
const crc32 = (buf) => { let c = 0xffffffff; for (const b of buf) c = crcTable[(c ^ b) & 255] ^ (c >>> 8); return (c ^ 0xffffffff) >>> 0; };

/** A 256 x 256 PNG in one flat colour. */
export function pngTile([r, g, b]) {
  const chunk = (type, data) => {
    const body = Buffer.concat([Buffer.from(type), data]);
    const len = Buffer.alloc(4); len.writeUInt32BE(data.length);
    const crc = Buffer.alloc(4); crc.writeUInt32BE(crc32(body));
    return Buffer.concat([len, body, crc]);
  };
  const ihdr = Buffer.alloc(13); ihdr.writeUInt32BE(256, 0); ihdr.writeUInt32BE(256, 4); ihdr[8] = 8; ihdr[9] = 2;
  const row = Buffer.concat([Buffer.from([0]), Buffer.alloc(256 * 3).fill(Buffer.from([r, g, b]))]);
  // fill() with a 3-byte buffer repeats it only partly in older Node; build the row explicitly
  const px = Buffer.alloc(256 * 3);
  for (let i = 0; i < 256; i++) { px[i * 3] = r; px[i * 3 + 1] = g; px[i * 3 + 2] = b; }
  const raw = Buffer.concat(Array.from({ length: 256 }, () => Buffer.concat([Buffer.from([0]), px])));
  void row;
  return Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk("IHDR", ihdr), chunk("IDAT", deflateSync(raw)), chunk("IEND", Buffer.alloc(0))]);
}

export const gz = (b) => gzipSync(b);

// ---------- tile numbers ----------
export function tileOf(lat, lon, z) {
  const n = 2 ** z;
  const x = Math.floor(((lon + 180) / 360) * n);
  const y = Math.floor(((1 - Math.asinh(Math.tan((lat * Math.PI) / 180)) / Math.PI) / 2) * n);
  return [x, y];
}
/** All tiles in a (2r+1) x (2r+1) block around a point, for each zoom in lo..hi. */
export function block(lat, lon, lo, hi, r, make) {
  const out = [];
  for (let z = lo; z <= hi; z++) {
    const [cx, cy] = tileOf(lat, lon, z);
    for (let dx = -r; dx <= r; dx++) for (let dy = -r; dy <= r; dy++) out.push({ z, x: cx + dx, y: cy + dy, bytes: make(z) });
  }
  return out;
}

// ---------- the file ----------
/**
 * layers: [{ kind, tile_type, compression, tiles: [{z,x,y,bytes}] }]
 * glyphs: [{ font, range, bytes }]
 */
export function writeCmap({ name, bbox, view, layers, glyphs = [], map = {} }) {
  const parts = [Buffer.from("CMAP0001")];
  let pos = 8;
  const put = (b) => { parts.push(b); pos += b.length; };
  const layerJson = [];
  for (const l of layers) {
    const dataStart = pos;
    const index = [];
    for (const t of l.tiles) {
      index.push([t.z, t.x, t.y, pos - dataStart, t.bytes.length]);
      put(t.bytes);
    }
    const dataLen = pos - dataStart;
    const indexOffset = pos;
    for (const e of index) { const b = Buffer.alloc(20); e.forEach((v, i) => b.writeUInt32LE(v, i * 4)); put(b); }
    const zs = index.map((e) => e[0]);
    layerJson.push({ kind: l.kind, tile_type: l.tile_type, compression: l.compression, min_zoom: Math.min(...zs), max_zoom: Math.max(...zs), tiles: index.length, data_offset: dataStart, data_length: dataLen, index_offset: indexOffset, index_count: index.length });
  }
  const glyphJson = glyphs.map((g) => { const off = pos; put(g.bytes); return { font: g.font, range: g.range, offset: off, length: g.bytes.length }; });
  const manifest = Buffer.from(JSON.stringify({ format: 1, name, created: 1760000000, bbox, view, layers: layerJson, glyphs: glyphJson, map }));
  put(manifest);
  const len = Buffer.alloc(4); len.writeUInt32LE(manifest.length); put(len);
  put(Buffer.from("CMAPEND1"));
  return Buffer.concat(parts);
}

// ---------- reading it back (the test serves tiles from the finished file, as the program does) ----------
export function readCmap(buf) {
  if (buf.subarray(0, 8).toString("latin1") !== "CMAP0001" || buf.subarray(buf.length - 8).toString("latin1") !== "CMAPEND1") throw new Error("not a cmap");
  const len = buf.readUInt32LE(buf.length - 12);
  const manifest = JSON.parse(buf.subarray(buf.length - 12 - len, buf.length - 12).toString("utf8"));
  const lists = manifest.layers.map((l) => {
    const m = new Map();
    for (let i = 0; i < l.index_count; i++) {
      const at = l.index_offset + i * 20;
      const [z, x, y, off, n] = [0, 1, 2, 3, 4].map((k) => buf.readUInt32LE(at + k * 4));
      m.set(z + "/" + x + "/" + y, buf.subarray(l.data_offset + off, l.data_offset + off + n));
    }
    return m;
  });
  const tile = (kind, z, x, y) => { const i = manifest.layers.findIndex((l) => l.kind === kind); return i < 0 ? null : (lists[i].get(z + "/" + x + "/" + y) ?? null); };
  const glyph = (font, range) => { const g = manifest.glyphs.find((q) => q.font === font && q.range === range); return g ? buf.subarray(g.offset, g.offset + g.length) : null; };
  return { manifest, tile, glyph };
}
