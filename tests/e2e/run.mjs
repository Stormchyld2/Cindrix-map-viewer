// Opens the built viewer in a real browser (the same MapLibre and Svelte code the program runs) and tries it the
// way a person would: open a phone map, look at street and aerial pictures, use the full screen button, open the
// list of marks. The program's Rust side is stood in for by a small shim that serves tiles from the .cmap file
// with the same addresses the Rust code uses, so everything on the screen side is really exercised.
// Run after `npm run build`:  node tests/e2e/run.mjs
import http from "node:http";
import { readFile, mkdir, writeFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import path from "node:path";
import { inflateSync } from "node:zlib";
import { chromium } from "playwright";
import { block, gz, pngTile, readCmap, streetTile, writeCmap } from "./mkcmap.mjs";

const OUT = path.resolve("tests/e2e/out");
const DIST = path.resolve("dist");
let failed = 0;
// GitHub keeps only 10 annotations of each kind per step, so everything is gathered and sent as one message at the end.
const lines = [];
const note = (m) => { lines.push("   " + m); console.log(m); };
const pass = (m) => { lines.push("PASS " + m); console.log("PASS " + m); };
const fail = (m) => { failed++; lines.push("FAIL " + m); console.log("FAIL " + m); };
const check = (c, m) => (c ? pass(m) : fail(m));

// ---------- the two test maps ----------
const LAT = 39.485, LON = -80.139;
const AERIAL = [200, 60, 40];
const base = {
  bbox: [-80.3, 39.35, -79.97, 39.62],
  view: { center: [LAT, LON], zoom: 12 },
  layers: [
    { kind: "street", tile_type: "mvt", compression: "gzip", tiles: block(LAT, LON, 11, 13, 1, () => gz(streetTile())) },
    { kind: "aerial", tile_type: "png", compression: "none", tiles: block(LAT, LON, 11, 13, 1, () => pngTile(AERIAL)) },
  ],
};
const FILES = {
  "/maps/plain.cmap": writeCmap({ name: "Plain Trail", ...base }),
  "/maps/marked.cmap": writeCmap({
    name: "Marked Trail",
    ...base,
    map: {
      pins: [
        { id: 1, label: "Camp", kind: "pin", lat: 39.486, lon: -80.14, tag: "Other", color: "#5aa9e6" },
        { id: 2, label: "Meet here", kind: "rendezvous", lat: 39.49, lon: -80.13 },
      ],
      route: [1, 2],
      nogo: [{ label: "Bog", points: [[39.48, -80.15], [39.484, -80.15], [39.484, -80.145]] }],
      drawings: [{ id: 1, kind: "text", label: "Spring", color: "#d93025", width: 4, points: [[39.483, -80.135]] }],
      directions: { points: [[39.486, -80.14], [39.49, -80.13]], km: 1.2, minutes: 4, steps: [{ name: "Main Street", km: 1.2, minutes: 4 }] },
      notes: "Bring water",
    },
  }),
};

// ---------- a plain file server for dist ----------
const TYPES = { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".svg": "image/svg+xml", ".png": "image/png", ".json": "application/json", ".woff2": "font/woff2" };
const server = http.createServer(async (req, res) => {
  let p = decodeURIComponent(new URL(req.url, "http://x").pathname);
  if (p === "/") p = "/index.html";
  const f = path.join(DIST, p);
  if (!f.startsWith(DIST) || !existsSync(f)) { res.writeHead(404); res.end(); return; }
  res.writeHead(200, { "content-type": TYPES[path.extname(f)] ?? "application/octet-stream" });
  res.end(await readFile(f));
});
await new Promise((r) => server.listen(0, "127.0.0.1", r));
const ORIGIN = "http://127.0.0.1:" + server.address().port;

// ---------- reading a screenshot ----------
function decodePng(buf) {
  let pos = 8, w = 0, h = 0, ct = 0;
  const idat = [];
  while (pos < buf.length) {
    const len = buf.readUInt32BE(pos);
    const type = buf.toString("latin1", pos + 4, pos + 8);
    const data = buf.subarray(pos + 8, pos + 8 + len);
    if (type === "IHDR") { w = data.readUInt32BE(0); h = data.readUInt32BE(4); ct = data[9]; }
    if (type === "IDAT") idat.push(data);
    pos += 12 + len;
  }
  const bpp = ct === 6 ? 4 : ct === 2 ? 3 : 0;
  if (!bpp) throw new Error("unexpected png type " + ct);
  const raw = inflateSync(Buffer.concat(idat));
  const stride = w * bpp;
  const out = Buffer.alloc(h * stride);
  for (let y = 0; y < h; y++) {
    const f = raw[y * (stride + 1)];
    for (let i = 0; i < stride; i++) {
      const x = raw[y * (stride + 1) + 1 + i];
      const a = i >= bpp ? out[y * stride + i - bpp] : 0;
      const b = y > 0 ? out[(y - 1) * stride + i] : 0;
      const c = i >= bpp && y > 0 ? out[(y - 1) * stride + i - bpp] : 0;
      let v;
      if (f === 0) v = x; else if (f === 1) v = x + a; else if (f === 2) v = x + b; else if (f === 3) v = x + ((a + b) >> 1);
      else { const p = a + b - c, pa = Math.abs(p - a), pb = Math.abs(p - b), pc = Math.abs(p - c); v = x + (pa <= pb && pa <= pc ? a : pb <= pc ? b : c); }
      out[y * stride + i] = v & 255;
    }
  }
  return { w, h, bpp, data: out };
}
function colours(png) {
  const m = new Map();
  for (let i = 0; i < png.w * png.h; i++) {
    const k = (png.data[i * png.bpp] << 16) | (png.data[i * png.bpp + 1] << 8) | png.data[i * png.bpp + 2];
    m.set(k, (m.get(k) ?? 0) + 1);
  }
  const [top, n] = [...m.entries()].sort((a, b) => b[1] - a[1])[0];
  return { distinct: m.size, top: [(top >> 16) & 255, (top >> 8) & 255, top & 255], frac: n / (png.w * png.h) };
}
const hex = (c) => "rgb(" + c.join(",") + ")";
const near = (a, b, t = 14) => a.every((v, i) => Math.abs(v - b[i]) <= t);

// ---------- one visit ----------
async function visit(browser, name, viewport, pick) {
  const ctx = await browser.newContext({ viewport, deviceScaleFactor: 1, hasTouch: viewport.width < 700 });
  let cur = null;
  const seen = { street: 0, aerial: 0, missing: 0, glyph: 0 };
  await ctx.route("http://cmap.localhost/**", (route) => {
    const parts = new URL(route.request().url()).pathname.split("/").filter(Boolean);
    const cors = { "access-control-allow-origin": "*" };
    if (parts[0] === "tiles" && cur) {
      const t = cur.tile(parts[1], +parts[2], +parts[3], +parts[4]);
      if (!t) { seen.missing++; return route.fulfill({ status: 404, headers: cors }); }
      seen[parts[1]]++;
      return route.fulfill({ status: 200, headers: { ...cors, "content-type": "application/octet-stream" }, body: t });
    }
    if (parts[0] === "glyphs") { seen.glyph++; return route.fulfill({ status: 404, headers: cors }); }
    return route.fulfill({ status: 404, headers: cors });
  });
  const page = await ctx.newPage();
  const problems = [];
  page.on("pageerror", (e) => problems.push("page error: " + e.message));
  page.on("console", (m) => { if (m.type() === "error") problems.push("console: " + m.text().slice(0, 300)); });
  await page.exposeFunction("__open", (p) => { cur = readCmap(FILES[p]); return { ...cur.manifest, unsupported: [] }; });
  await page.addInitScript((pickPath) => {
    window.__TAURI_INTERNALS__ = {
      metadata: { currentWindow: { label: "main" }, currentWebview: { windowLabel: "main", label: "main" } },
      convertFileSrc: (p, proto = "asset") => "http://" + proto + ".localhost/" + encodeURIComponent(p),
      invoke: async (cmd, args) => {
        if (cmd === "startup_file") return null;
        if (cmd === "plugin:dialog|open") return pickPath;
        if (cmd === "cmap_open") return await window.__open(args.path);
        return null;
      },
    };
  }, pick);
  await page.goto(ORIGIN + "/");
  return { page, ctx, seen, problems, label: name };
}

const waitReady = async (page, ms = 30000) => { await page.waitForSelector('.map[data-ready="1"]', { timeout: ms }); await page.waitForTimeout(400); };
const shot = async (page, file) => { await mkdir(OUT, { recursive: true }); await page.screenshot({ path: path.join(OUT, file) }); };
const mapColours = async (page) => colours(decodePng(await page.locator(".map").screenshot()));
const insideScreen = async (page, selector) =>
  page.evaluate((sel) => {
    const vw = window.innerWidth, vh = window.innerHeight;
    return [...document.querySelectorAll(sel)].map((e) => { const r = e.getBoundingClientRect(); return { l: r.left, r: r.right, t: r.top, b: r.bottom, w: r.width, h: r.height, vw, vh, txt: (e.getAttribute("aria-label") || e.textContent || "").trim().slice(0, 30) }; });
  }, selector);

// ---------- the checks ----------
const browser = await chromium.launch({ args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist", "--enable-webgl"] });

async function openAndLook(v, label, pick) {
  const { page, ctx, seen, problems } = await visit(browser, label, v, pick);
  try {
    await page.getByText("Open a phone map").waitFor({ timeout: 15000 });
    const st = await insideScreen(page, ".start button");
    check(st.length === 1 && st[0].l >= 0 && st[0].r <= st[0].vw, label + ": the start button fits the screen");
    await page.getByRole("button", { name: "Open a map file" }).click();
    await waitReady(page);
    check((await page.locator(".name").innerText()).length > 0, label + ": the map's name is shown");
    check((await page.locator(".notice").count()) === 0, label + ": no error notice on screen" + (problems.length ? " | " + problems.join(" | ") : ""));
    check(seen.street > 0, label + ": street tiles were read from the file (" + seen.street + ")");
    const c = await mapColours(page);
    note(label + ": street picture has " + c.distinct + " colours, most common " + hex(c.top) + " covers " + c.frac.toFixed(2));
    check(c.distinct >= 3 && c.frac < 0.95, label + ": the street map is actually drawn (not blank)");
    await shot(page, label + "-street.png");
    return { page, ctx, seen, problems };
  } catch (e) {
    fail(label + ": " + String(e).split("\n")[0] + (problems.length ? " | " + problems.join(" | ") : ""));
    try { await shot(page, label + "-failed.png"); } catch { /* ignore */ }
    await ctx.close();
    return null;
  }
}

// desktop: plain map, picture switch, full screen
{
  const r = await openAndLook({ width: 1280, height: 800 }, "desktop", "/maps/plain.cmap");
  if (r) {
    const { page, ctx, seen } = r;
    await page.evaluate(() => document.querySelector(".map").setAttribute("data-ready", "0"));
    await page.locator(".seg button", { hasText: "Aerial" }).click();
    await waitReady(page);
    const a = await mapColours(page);
    note("desktop: aerial picture most common " + hex(a.top) + " covers " + a.frac.toFixed(2) + "; aerial tiles read " + seen.aerial);
    check(seen.aerial > 0, "desktop: aerial tiles were read from the file");
    check(near(a.top, AERIAL) && a.frac > 0.5, "desktop: the Aerial button shows the aerial picture");
    await shot(page, "desktop-aerial.png");
    await page.evaluate(() => document.querySelector(".map").setAttribute("data-ready", "0"));
    await page.locator(".seg button", { hasText: "Street" }).click();
    await waitReady(page);
    const s = await mapColours(page);
    check(!near(s.top, AERIAL), "desktop: the Street button goes back to the street map");

    await page.getByRole("button", { name: "Full screen map" }).click();
    await page.waitForTimeout(300);
    check((await page.locator(".bar").count()) === 0, "desktop: Full screen map hides the top buttons");
    check(await page.getByRole("button", { name: "Leave full screen map" }).isVisible(), "desktop: a button to leave full screen is shown");
    const big = await page.locator(".map").boundingBox();
    check(big.height >= 799 && big.width >= 1279, "desktop: the map fills the whole window in full screen (" + big.width + "x" + big.height + ")");
    await shot(page, "desktop-fullscreen.png");
    await page.getByRole("button", { name: "Leave full screen map" }).click();
    await page.waitForTimeout(300);
    check((await page.locator(".bar").count()) === 1, "desktop: leaving full screen brings the buttons back");
    await ctx.close();
  }
}

// phones: layout, marks list, full screen
for (const v of [{ width: 360, height: 640, label: "phone360" }, { width: 320, height: 568, label: "phone320" }]) {
  const r = await openAndLook({ width: v.width, height: v.height }, v.label, "/maps/marked.cmap");
  if (!r) continue;
  const { page, ctx } = r;
  const sw = await page.evaluate(() => [document.documentElement.scrollWidth, window.innerWidth]);
  check(sw[0] <= sw[1], v.label + ": the page does not scroll sideways (" + sw[0] + " in " + sw[1] + ")");
  const btns = await insideScreen(page, ".bar button");
  const off = btns.filter((b) => b.l < -0.5 || b.r > b.vw + 0.5);
  check(btns.length >= 4 && off.length === 0, v.label + ": every top button is on screen" + (off.length ? " | off screen: " + off.map((b) => b.txt + "@" + Math.round(b.l) + "-" + Math.round(b.r)).join(", ") : ""));
  const small = btns.filter((b) => b.h < 40 || b.w < 40);
  check(small.length === 0, v.label + ": every top button is big enough to touch" + (small.length ? " | small: " + small.map((b) => b.txt + " " + Math.round(b.w) + "x" + Math.round(b.h)).join(", ") : ""));
  check((await page.locator(".pinmark").count()) === 2, v.label + ": both pins are on the map (" + (await page.locator(".pinmark").count()) + ")");
  await page.getByRole("button", { name: "Pins, areas, drawings and directions" }).click();
  await page.waitForSelector(".sheet");
  // the position box sits at the top, clear of the scale bar and the map credit at the bottom
  const clash = await page.evaluate(() => {
    const r = (q) => document.querySelector(q)?.getBoundingClientRect();
    const a = r(".readout"), others = [r(".maplibregl-ctrl-scale"), r(".maplibregl-ctrl-attrib")].filter(Boolean);
    const hit = (p, q) => !(p.right <= q.left || q.right <= p.left || p.bottom <= q.top || q.bottom <= p.top);
    const kids = [...document.querySelectorAll(".readout > *")].map((e) => e.getBoundingClientRect());
    return kids.some((k) => others.some((o) => hit(k, o))) || !a;
  });
  check(!clash, v.label + ": the position box does not overlap the scale bar or the map credit");
  const z0 = await page.locator(".readout span", { hasText: "zoom" }).innerText();
  await page.getByRole("button", { name: "Zoom in" }).click();
  await page.waitForTimeout(700);
  const z1 = await page.locator(".readout span", { hasText: "zoom" }).innerText();
  check(parseFloat(z1.replace("zoom", "")) > parseFloat(z0.replace("zoom", "")), v.label + ": the Zoom in button zooms in (" + z0 + " to " + z1 + ")");
  await page.getByRole("button", { name: "Zoom out" }).click();
  await page.waitForTimeout(700);
  const sh = (await insideScreen(page, ".sheet"))[0];
  check(sh.l >= -0.5 && sh.r <= sh.vw + 0.5 && sh.b <= sh.vh + 0.5, v.label + ": the list of marks fits the screen");
  const txt = await page.locator(".sheet").innerText();
  check(/Camp/.test(txt) && /Meet here/.test(txt) && /Bog/.test(txt) && /Main Street/.test(txt) && /Bring water/.test(txt), v.label + ": the list shows pins, area, directions and notes");
  await shot(page, v.label + "-list.png");
  await page.locator(".sheet li button", { hasText: "Camp" }).click();
  await page.waitForTimeout(500);
  check((await page.locator(".sheet").count()) === 0, v.label + ": choosing a pin closes the list on a phone");
  await page.getByRole("button", { name: "Full screen map" }).click();
  await page.waitForTimeout(300);
  const fs = await insideScreen(page, ".float button");
  check((await page.locator(".bar").count()) === 0 && fs.length === 5 && fs.every((b) => b.l >= 0 && b.r <= b.vw && b.t >= 0 && b.b <= b.vh), v.label + ": full screen shows only the map and its five small buttons, all on screen");
  await shot(page, v.label + "-fullscreen.png");
  await ctx.close();
}

await browser.close();
server.close();
await writeFile(path.join(OUT, "done.txt"), failed ? "failed " + failed : "all passed").catch(() => {});
const enc = (t) => t.replace(/%/g, "%25").replace(/\r/g, "%0D").replace(/\n/g, "%0A");
console.log("::notice title=Browser test results::" + enc(lines.join("\n")));
console.log(failed ? "\n" + failed + " check(s) failed" : "\nall checks passed");
process.exit(failed ? 1 : 0);
