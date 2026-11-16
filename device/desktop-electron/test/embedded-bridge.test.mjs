// THE BRIDGE SURFACE, pinned against the host page's fixture.
//
// `preload.ts` exposes exactly TWO objects to the thin host page (`src/host.html`): `summriseBrowser`
// (the native browser view) and `summriseHarness` (the harness view on this machine's loopback). The
// page calls them from plain functions of its own; both sides work while a renamed member breaks the
// shell silently, and nowhere else — these surfaces only exist behind a contextBridge global, so no
// rendered sweep has ever seen them. This test cannot see their behaviour either. It pins the NAMES,
// which is the half that drifts with nothing to catch it, and it pins the GLOBAL SET, because a third
// `exposeInMainWorld` call would be a third privileged surface with no fixture to describe it.
//
// The preload is parsed as TEXT because importing it needs `electron`, which this suite deliberately
// does not install (it runs on node builtins alone). The shape it parses is a plain object literal of
// arrow functions, which is exactly what the file contains.
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import test from "node:test";
import assert from "node:assert/strict";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const PRELOAD = path.join(HERE, "..", "src", "preload.ts");
const FIXTURE = path.join(HERE, "..", "..", "tests", "fixtures", "embedded-bridge.json");

/** The body of one `contextBridge.exposeInMainWorld("<name>", { … })` object literal, by brace
 *  matching (the nested bounds types inside `place` balance out). */
function bridgeBody(source, name) {
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
  return source.slice(open + 1, end);
}

/** The members of one bridge's object literal. Member names are `name:` at the start of a line —
 *  comments and nested object types are skipped because they are not at that position. */
function exposedMembers(source, name) {
  const names = [];
  for (const m of bridgeBody(source, name).matchAll(/^\s{2}([A-Za-z_$][\w$]*)\s*:/gm)) names.push(m[1]);
  return names;
}

/** Every global the preload exposes, in source order (duplicates are kept so the count is honest). */
function exposedGlobals(source) {
  return [...source.matchAll(/exposeInMainWorld\(\s*"([^"]+)"/g)].map((m) => m[1]);
}

test("the preload exposes exactly the two bridge globals, with the fixture's members", () => {
  const source = readFileSync(PRELOAD, "utf8");
  const fixture = JSON.parse(readFileSync(FIXTURE, "utf8"));
  const exposed = exposedGlobals(source);
  assert.deepEqual(
    exposed.slice().sort(),
    Object.keys(fixture.bridges).sort(),
    `preload.ts must expose exactly the host bridges the fixture names (found: ${exposed.join(", ")}) — a third global is a third privileged surface`,
  );
  // The counts are the contract's: the browser view's 10 members, the harness view's 8.
  assert.equal(fixture.bridges.summriseBrowser.length, 10, "the browser bridge is 10 members");
  assert.equal(fixture.bridges.summriseHarness.length, 8, "the harness bridge is 8 members");
  for (const [bridge, members] of Object.entries(fixture.bridges)) {
    assert.deepEqual(
      exposedMembers(source, bridge).sort(),
      [...members].sort(),
      `${bridge} drifted from the shared fixture — the host page and the preload must expose the SAME members`,
    );
  }
});

test("the subscribing members return a function, not a promise", () => {
  // A subscriber that returned a promise would leave the page unable to unsubscribe, which leaks a
  // listener per mount and shows up only as duplicated navigation events in the desktop app.
  const source = readFileSync(PRELOAD, "utf8");
  const fixture = JSON.parse(readFileSync(FIXTURE, "utf8"));
  for (const [bridge, members] of Object.entries(fixture.bridges)) {
    const body = bridgeBody(source, bridge);
    for (const member of members) {
      if (!/^on[A-Z]/.test(member)) continue;
      const at = body.indexOf(`${member}:`);
      assert.ok(at > -1, `${bridge}.${member} must exist`);
      assert.ok(
        body.slice(at, at + 900).includes("removeListener"),
        `${bridge}.${member} must return an unsubscribe function (no removeListener found)`,
      );
    }
  }
});
