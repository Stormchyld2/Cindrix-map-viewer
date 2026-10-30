// The viewer must read the phone map file that the main CindrixOps program writes (docs/32-phone-map.md there).
// tests/fixtures/real-core-v1.cmap was written by the main program's own writer (core/src/phonepack.rs), not by hand.
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { readOverlays, routeLine, point, colour } from "../src/lib/overlays.ts";

const file = readFileSync(new URL("./fixtures/real-core-v1.cmap", import.meta.url));

function manifestOf(buf) {
  assert.equal(buf.subarray(0, 8).toString("latin1"), "CMAP0001");
  assert.equal(buf.subarray(buf.length - 8).toString("latin1"), "CMAPEND1");
  const len = buf.readUInt32LE(buf.length - 12);
  return JSON.parse(buf.subarray(buf.length - 12 - len, buf.length - 12).toString("utf8"));
}

test("the fixture from the main program has the layout the viewer reads", () => {
  const m = manifestOf(file);
  assert.equal(m.format, 1);
  assert.equal(m.layers[0].kind, "street");
  assert.equal(m.layers[0].index_count, 2);
  // every tile list entry lies inside the layer's data
  for (let i = 0; i < m.layers[0].index_count; i++) {
    const at = m.layers[0].index_offset + i * 20;
    const [z, x, y, off, len] = [0, 1, 2, 3, 4].map((k) => file.readUInt32LE(at + k * 4));
    assert.ok(z >= m.layers[0].min_zoom && z <= m.layers[0].max_zoom, "zoom in range");
    assert.ok(x >= 0 && y >= 0);
    assert.ok(off + len <= m.layers[0].data_length, "tile inside the data");
  }
  const g = m.glyphs[0];
  assert.equal(file.subarray(g.offset, g.offset + g.length).toString(), "glyphs-a");
});

test("the person's own pins come out of the real file", () => {
  const o = readOverlays(manifestOf(file).map);
  assert.equal(o.pins.length, 1);
  assert.equal(o.pins[0].label, "Camp");
  assert.equal(o.notes, "hi");
});

test("a saved map from the Maps screen is read whole", () => {
  const o = readOverlays({
    pins: [
      { id: 1, label: "Start", kind: "pin", lat: 39.5, lon: -80.1, tag: "Other", color: "#112233" },
      { id: 2, label: "Meet", kind: "rendezvous", lat: 39.51, lon: -80.12 },
      { id: 3, label: "Hidden", kind: "pin", lat: 39.5, lon: -80.1, hidden: true },
    ],
    route: [2, 1, 99],
    nogo: [{ label: "Bog", points: [[39.5, -80.1], [39.51, -80.1], [39.51, -80.09]] }, { label: "Too few", points: [[1, 1]] }],
    drawings: [
      { id: 1, kind: "circle", label: "Camp", color: "#d93025", width: 4, points: [[39.5, -80.1]], radiusM: 150 },
      { id: 2, kind: "nonsense", points: [[1, 1]] },
    ],
    directions: { points: [[39.5, -80.1], [39.6, -80.2]], km: 12.5, minutes: 20, steps: [{ name: "Main St", km: 12.5, minutes: 20 }] },
    notes: "Bring water",
  });
  assert.deepEqual(o.pins.map((p) => p.label), ["Start", "Meet"], "hidden pins are not shown");
  assert.equal(o.pins[1].color, "#6fbf73", "a meeting place gets its own colour");
  assert.equal(o.nogo.length, 1);
  assert.equal(o.drawings.length, 1);
  assert.equal(o.drawings[0].radiusM, 150);
  assert.equal(o.directions.steps[0].name, "Main St");
  // the route follows the pins in the order saved and skips ones that are gone
  assert.deepEqual(routeLine(o), [[39.51, -80.12], [39.5, -80.1]]);
});

test("a hostile or broken map never gets through to the screen", () => {
  for (const bad of [null, undefined, 5, "x", [], { pins: "no" }, { pins: [null, 3, "x", {}] }]) {
    const o = readOverlays(bad);
    assert.deepEqual([o.pins, o.nogo, o.drawings, o.directions], [[], [], [], null]);
  }
  const o = readOverlays({
    pins: [
      { label: "<img src=x onerror=alert(1)>", lat: 1, lon: 2, color: "red;background:url(x)" },
      { label: "far", lat: 95, lon: 2 },
      { label: "nan", lat: NaN, lon: 2 },
    ],
    drawings: [{ kind: "line", points: [[1, 1], [2, 2]], color: "url(javascript:1)", width: 9999 }],
  });
  assert.equal(o.pins.length, 1, "out-of-range and non-number positions are dropped");
  assert.equal(o.pins[0].label, "<img src=x onerror=alert(1)>", "text is kept as plain text (the screen never treats it as markup)");
  assert.match(o.pins[0].color, /^#[0-9a-f]{6}$/i, "a colour that is not a plain colour is replaced");
  assert.equal(o.drawings[0].width, 12, "line width is limited");
  assert.equal(o.drawings[0].color, "#1a73e8");
  assert.equal(point([91, 0]), null);
  assert.equal(point([0, 181]), null);
  assert.deepEqual(point([1, 2]), [1, 2]);
  assert.equal(colour("#abc", "#000"), "#abc");
  assert.equal(colour("#xyz", "#000"), "#000");
});

import { isGzip } from "../src/lib/tiles.ts";
import { gzipSync } from "node:zlib";

test("gzip tiles are recognised by their first two bytes", () => {
  const packed = gzipSync(Buffer.from("tile"));
  assert.equal(isGzip(packed.buffer.slice(packed.byteOffset, packed.byteOffset + packed.byteLength)), true);
  assert.equal(isGzip(new Uint8Array([0x1f]).buffer), false);
  assert.equal(isGzip(new ArrayBuffer(0)), false);
  assert.equal(isGzip(new Uint8Array([0x0a, 0x1f, 0x8b]).buffer), false);
});
