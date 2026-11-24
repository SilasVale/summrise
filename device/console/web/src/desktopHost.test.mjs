import { test } from "vitest";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { JSDOM } from "jsdom";

// Console's unit gate provisions jsdom. The mandatory Desktop Node suite uses builtins only.
const desktopRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../../desktop-electron");
const read = (p) => readFileSync(path.join(desktopRoot, p), "utf8");

test("vertical_tool_navigation_moves_focus_and_selection_without_losing_horizontal_keys", async () => {
  const dom = new JSDOM(read("src/host.html"), { url: "https://host.invalid/host.html", runScripts: "outside-only", pretendToBeVisual: true });
  const { window } = dom;
  const ok = () => Promise.resolve({ ok: true });
  const subscribe = () => () => {};
  window.ResizeObserver = class { observe() {} disconnect() {} };
  window.summriseBrowser = {
    navigate: ok, back: ok, fwd: ok, reload: ok, zoom: ok, place: ok, recover: ok,
    state: () => Promise.resolve({ url: "", visible: false, load: null }), onNav: subscribe, onGone: subscribe,
  };
  window.summriseHarness = {
    targets: () => Promise.resolve({ targets: [] }), open: ok, place: ok, go: ok, reload: ok, recover: ok,
    state: () => Promise.resolve({ url: "", visible: false, load: null }), onGone: subscribe,
  };
  try {
    window.eval(read("src/host.js"));
    await new Promise(resolve => setImmediate(resolve));
    const browser = window.document.getElementById("tab-browser");
    const harness = window.document.getElementById("tab-harness");
    browser.focus();
    for (const [key, selected] of [["ArrowDown", harness], ["ArrowUp", browser], ["ArrowRight", harness], ["Home", browser], ["End", harness], ["ArrowLeft", browser]]) {
      window.document.activeElement.dispatchEvent(new window.KeyboardEvent("keydown", { key, bubbles: true, cancelable: true }));
      assert.equal(window.document.activeElement, selected, `${key} focuses its tool`);
      assert.equal(selected.getAttribute("aria-selected"), "true", `${key} selects its tool`);
      assert.equal(selected.tabIndex, 0);
      const other = selected === browser ? harness : browser;
      assert.equal(other.tabIndex, -1);
      assert.equal(window.document.getElementById(selected.getAttribute("aria-controls")).hidden, false);
      assert.equal(window.document.getElementById(other.getAttribute("aria-controls")).hidden, true);
    }
    await new Promise(resolve => setImmediate(resolve));
  } finally { window.close(); }
});
