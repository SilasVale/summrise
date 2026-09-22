// ── the CORS allowlist is hand-maintained in THREE files, and its own comment
//    asserts they agree ────────────────────────────────────────────────────────
//
// `src/http.ts`'s `ALLOWED_ORIGINS` carries a comment naming where each entry
// comes from — `CONSOLE_HOST` in wrangler.jsonc — and ends with "(Mirrors
// proxies/zen-go-proxy/src/index.js.)". Those are claims about OTHER files, and
// nothing read any of them.
//
// A SECOND SOURCE WAS `extension/manifest.json` until round 243 removed that extension: the console host plus
// the manifest's host_permissions WAS the expected set, which is how `https://dsh.summrise.test` got in — it was
// there for the extension's content script and for nothing else. Checked before cutting, in this order: the
// comment in http.ts names the manifest; the harness's own browser client posts to `http://dsh.internal` rather
// than to this API; and the DSH plugin in this repo fetches no API at all. So the origin went, and this test now
// compares against CONSOLE_HOST alone — which is a STRICTER check than before, because a stray origin can no
// longer be legitimised by a manifest nobody reads.
//
// The stake is concrete rather than theoretical. CONSOLE_HOST is a wrangler
// binding that the file itself says may be overridden from the dashboard, so the
// console's real origin can move without a commit — and when it does, the new
// origin is absent from this allowlist and the console silently loses CORS in the
// browser while every server-side test stays green. The zen-go copy is the same
// hazard with no override story at all: two lists declared to mirror each other,
// and only one of them edited.
//
// This reads all three as data and compares them, so none of the three restates
// another — the same shape as agent/tests/gateway_code_contract.rs.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { join } from "node:path";

const ROOT = fileURLToPath(new URL("..", import.meta.url));

/** The origins in `export const ALLOWED_ORIGINS: ReadonlySet<string> = new Set([ … ])`. */
function gatewayOrigins() {
  const src = readFileSync(join(ROOT, "src", "http.ts"), "utf8");
  const start = src.indexOf("export const ALLOWED_ORIGINS");
  assert.notEqual(start, -1, "src/http.ts declares ALLOWED_ORIGINS");
  const open = src.indexOf("new Set([", start);
  const close = src.indexOf("])", open);
  assert.ok(open !== -1 && close !== -1, "the allowlist is a `new Set([ … ])` literal");
  return [...src.slice(open, close).matchAll(/"([^"]+)"/g)].map((m) => m[1]).sort();
}

/** The origins in the zen-go proxy's `const ALLOWED_ORIGINS = new Set([ … ])`. */
function zenGoOrigins() {
  const src = readFileSync(
    join(ROOT, "..", "proxies", "zen-go-proxy", "src", "index.js"),
    "utf8",
  );
  const start = src.indexOf("const ALLOWED_ORIGINS");
  assert.notEqual(start, -1, "the zen-go proxy declares ALLOWED_ORIGINS");
  const open = src.indexOf("new Set([", start);
  const close = src.indexOf("])", open);
  return [...src.slice(open, close).matchAll(/"([^"]+)"/g)].map((m) => m[1]).sort();
}

test("CORS: the gateway's allowlist is exactly what its comment says it mirrors", () => {
  const gateway = gatewayOrigins();

  // CONSOLE_HOST is a comma-separated HOST list (no scheme) in wrangler.jsonc.
  const wrangler = JSON.parse(
    readFileSync(join(ROOT, "wrangler.jsonc"), "utf8")
      .replace(/^\s*\/\/.*$/gm, ""), // jsonc: strip whole-line comments
  );
  const consoleHost = wrangler.vars?.CONSOLE_HOST ?? wrangler.env?.production?.vars?.CONSOLE_HOST;
  assert.ok(consoleHost, "wrangler.jsonc declares CONSOLE_HOST");
  const fromConsoleHost = consoleHost
    .split(",")
    .map((h) => `https://${h.trim()}`)
    .sort();

  const expected = [...new Set(fromConsoleHost)].sort();

  assert.deepEqual(
    gateway,
    expected,
    "src/http.ts's ALLOWED_ORIGINS no longer matches the files its own comment names. " +
      "A missing origin means that console silently loses CORS in the browser while every " +
      "server-side test stays green; a surplus one stays allowlisted after its host moved. " +
      `allowlist=${gateway.join(",")} expected=${expected.join(",")}`,
  );
});

test("CORS: the gateway and the zen-go proxy allow the same origins", () => {
  assert.deepEqual(
    gatewayOrigins(),
    zenGoOrigins(),
    "src/http.ts says its allowlist `(Mirrors proxies/zen-go-proxy/src/index.js.)` and the " +
      "two are separate copies, so a change to one silently diverges: the proxy starts " +
      "refusing a browser origin the gateway still allows, or the reverse. Edit both.",
  );
});
