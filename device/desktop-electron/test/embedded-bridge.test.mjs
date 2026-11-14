// THE BRIDGE SURFACE, pinned against the panel's fixture.
//
// `preload.ts` exposes three objects to the SPA; the panel calls them from a typed interface of its
// own. Both compile; a renamed member breaks the desktop app silently, and nowhere else — these
// surfaces only exist behind `window.summriseEmbedded`, so no rendered sweep has ever seen them (rounds
// 45 and 50 named them as the project's blind spot). This test cannot see their behaviour either.
// It pins the NAMES, which is the half that drifts with nothing to catch it.
//
// The preload is parsed as TEXT because importing it needs `electron`, which this suite deliberately
// does not install (it runs on node builtins alone, like the CLI suite). The shape it parses is a
// plain object literal of arrow functions, which is exactly what the file contains.
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import test from "node:test";
import assert from "node:assert/strict";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const PRELOAD = path.join(HERE, "..", "src", "preload.ts");
const FIXTURE = path.join(HERE, "..", "..", "tests", "fixtures", "embedded-bridge.json");

/** The members of one `contextBridge.exposeInMainWorld("<name>", { … })` object literal. */
function exposedMembers(source, name) {
  const at = source.indexOf(`exposeInMainWorld("${name}"`);
  assert.ok(at > 0, `preload.ts must expose ${name}`);
  const open = source.indexOf("{", at);
  let depth = 0;
  let end = open;
  for (let i = open; i < source.length; i++) {
    if (source[i] === "{") depth++;
    else if (source[i] === "}") {
      depth--;
      if (depth === 0) {
        end = i;
        break;
      }
    }
  }
  const body = source.slice(open + 1, end);
  // Member names are `name:` at the start of a line inside the literal — comments and nested
  // object types (place's bounds) are skipped because they are not at that position.
  const names = [];
  for (const m of body.matchAll(/^\s{2}([A-Za-z_$][\w$]*)\s*:/gm)) names.push(m[1]);
  return names;
}

test("every bridge member the panel expects exists in the preload", () => {
  const source = readFileSync(PRELOAD, "utf8");
  const fixture = JSON.parse(readFileSync(FIXTURE, "utf8"));
  for (const [bridge, members] of Object.entries(fixture.bridges)) {
    const exposed = exposedMembers(source, bridge);
    assert.deepEqual(
      exposed.sort(),
      [...members].sort(),
      `${bridge} drifted from the shared fixture — the panel and the preload must expose the SAME members`,
    );
  }
});

test("the three subscribing members return a function, not a promise", () => {
  // A subscriber that returned a promise would leave the panel unable to unsubscribe, which leaks a
  // listener per mount and shows up only as duplicated navigation events in the desktop app.
  const source = readFileSync(PRELOAD, "utf8");
  for (const member of ["onNav", "onGone", "onCommand"]) {
    const at = source.indexOf(`${member}:`);
    assert.ok(at > 0, `${member} must exist`);
    const body = source.slice(at, at + 900);
    assert.ok(
      body.includes("removeListener"),
      `${member} must return an unsubscribe function (no removeListener found)`,
    );
  }
});
