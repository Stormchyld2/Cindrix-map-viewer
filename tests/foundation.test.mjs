import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
test("app identity and default fullscreen are configured", () => {
  const config = JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf8"));
  assert.equal(config.productName, "CindrixOps CMAP Viewer");
  assert.equal(config.app.windows[0].fullscreen, true);
  assert.equal(config.bundle.active, true);
});
