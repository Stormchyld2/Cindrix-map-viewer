// Tiles reach the map engine through its own tile loader instead of straight from the address, so the program,
// not the web view, unpacks them. Street tiles are usually gzip-packed inside a .cmap file; unpacking them here
// works the same on Windows and Android whatever the web view does with packed answers.
import type * as MapLibre from 'maplibre-gl';

let registered = false;
let blank: Promise<ArrayBuffer> | null = null;

/** gzip data starts with the two bytes 1f 8b. */
export function isGzip(b: ArrayBuffer): boolean {
  if (b.byteLength < 2) return false;
  const v = new Uint8Array(b, 0, 2);
  return v[0] === 0x1f && v[1] === 0x8b;
}

async function gunzip(b: ArrayBuffer): Promise<ArrayBuffer> {
  const stream = new Blob([b]).stream().pipeThrough(new DecompressionStream('gzip'));
  return new Response(stream).arrayBuffer();
}

/** A clear one-pixel picture, for a square of a picture map that the file does not hold. */
function clearPicture(): Promise<ArrayBuffer> {
  blank ??= new Promise<ArrayBuffer>((ok, bad) => {
    const c = document.createElement('canvas');
    c.width = 1;
    c.height = 1;
    c.toBlob((blob) => (blob ? blob.arrayBuffer().then(ok, bad) : bad(new Error('no picture'))), 'image/png');
  });
  return blank;
}

/** Make `cmaptile://<kind>/<z>/<x>/<y>` addresses work. `base` is where the program serves the open map's tiles. */
export function registerTileProtocol(ml: typeof MapLibre, base: string): void {
  if (registered) return;
  registered = true;
  ml.addProtocol('cmaptile', async (params, abort) => {
    const rest = params.url.slice('cmaptile://'.length);
    const kind = rest.split('/')[0];
    const res = await fetch(base + 'tiles/' + rest, { signal: abort.signal });
    // A square the map does not hold (outside its area, or finer than it was cut): empty, not a fault.
    if (res.status === 404 || res.status === 204) return { data: kind === 'street' ? new ArrayBuffer(0) : await clearPicture() };
    if (!res.ok) throw new Error('The map file could not give a tile (' + res.status + ').');
    let data = await res.arrayBuffer();
    if (isGzip(data)) data = await gunzip(data);
    return { data };
  });
}
