// THE EVENT VOCABULARY, pinned across the two sides that have to agree about it.
//
// The device pushes SSE frames carrying `{"ev": "<name>"}`; the panel's SSE layer turns each into
// `window.dispatchEvent(new CustomEvent("summrise-" + frame.ev))`. So the EMITTER side cannot drift — the
// name is derived, not duplicated. The LISTENER side is literals:
//
//     window.addEventListener("summrise-sessions-changed", …)
//
// and that is where a typo is silent. Nothing throws, nothing warns; the feature simply never fires
// again, and the symptom is stale data rather than an error.
//
// Round 94 found the other direction of the same hole: `summrise-write-failed` was DISPATCHED by
// TerminalPane when a keystroke write rejected, and NOTHING LISTENED — anywhere in the repo, including
// the Electron shell and the tests. The catch was there to keep the write chain alive (right), and it
// said nothing at all (wrong): an operator typing into a session whose agent had gone away watched
// their keystrokes vanish. It is wired to the status line now, and this file is what stops the next
// one from being born unattended.
import { describe, it, expect } from "vitest";
import { readFileSync, readdirSync, statSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const PANEL_SRC = path.resolve(HERE, "..");
const AGENT_SRC = path.resolve(HERE, "..", "..", "..", "..", "src");

function filesUnder(dir: string, match: (f: string) => boolean, out: string[] = []): string[] {
  for (const name of readdirSync(dir)) {
    const full = path.join(dir, name);
    if (statSync(full).isDirectory()) filesUnder(full, match, out);
    else if (match(full)) out.push(full);
  }
  return out;
}

const panelFiles = filesUnder(PANEL_SRC, (f) => /\.tsx?$/.test(f) && !f.includes("__tests__"));
const panelText = panelFiles.map((f) => readFileSync(f, "utf8")).join("\n");
const rustText = filesUnder(AGENT_SRC, (f) => f.endsWith(".rs"))
  .map((f) => readFileSync(f, "utf8"))
  .join("\n");

const listeners = new Set([...panelText.matchAll(/addEventListener\(\s*"(summrise-[a-z-]+)"/g)].map((m) => m[1]));
const dispatched = new Set([...panelText.matchAll(/CustomEvent\(\s*"(summrise-[a-z-]+)"/g)].map((m) => m[1]));
/** The derived path — the dispatch builds its event name from the FRAME's own `ev`, or the whole SSE vocabulary is
 *  broken. Round 54 extracted that `ev` into a local (`const ev = frame.ev as Frame`) so the generated vocabulary types
 *  it, so the pattern accepts either spelling: what this test is about is that the name is DERIVED rather than written
 *  out, not how many lines the derivation takes. */
const derivesFromFrames =
  /CustomEvent\(\s*`summrise-\$\{frame\.ev\}/.test(panelText) ||
  /const ev = frame\.ev as Frame;[\s\S]{0,120}CustomEvent\(\s*`summrise-\$\{ev\}`/.test(panelText);
// LITERALS AND CONSTANTS. The device writes `{"ev": "monitor-change"}` in some places and
// `{"ev": ACTIONS_CHANGED_EVENT}` in others — and the constant form is invisible to a literal-only
// pattern, which made this contract report `summrise-browser-actions-changed` as an orphan when the bus
// emits it on every recorded action. A checker that cannot see how the device actually writes a name
// is worth nothing here, so constants are resolved from their own definitions.
const rustConsts = new Map(
  [...rustText.matchAll(/const\s+([A-Z_]+)\s*:\s*&str\s*=\s*"([a-z-]+)"/g)].map((m) => [m[1], m[2]]),
);
const deviceEvents = new Set(
  [...rustText.matchAll(/"ev":\s*"([a-z-]+)"/g)].map((m) => `summrise-${m[1]}`),
);
for (const m of rustText.matchAll(/"ev":\s*([A-Z_]+)/g)) {
  const resolved = rustConsts.get(m[1]);
  // A constant this scan cannot resolve is a hole in the checker, not a missing emitter — say so
  // rather than counting it as absent (the direction that produced a false finding).
  if (resolved) deviceEvents.add(`summrise-${resolved}`);
  else deviceEvents.add(`summrise-<unresolved:${m[1]}>`);
}

describe("the panel's window-event vocabulary", () => {
  it("reads both sides, or it proves nothing", () => {
    expect(listeners.size, "no listeners parsed — the pattern or the tree changed").toBeGreaterThanOrEqual(4);
    expect(deviceEvents.size, "no device emitters parsed").toBeGreaterThanOrEqual(3);
    expect(derivesFromFrames, "the derived SSE dispatch is gone — check useSSE before trusting this").toBe(true);
  });

  it("every event the panel listens for is one somebody emits", () => {
    const orphans = [...listeners].filter((n) => !deviceEvents.has(n) && !dispatched.has(n));
    expect(
      orphans,
      "a listener for an event nothing emits never fires, and nothing says so",
    ).toEqual([]);
  });

  it("every event the panel dispatches has a listener — in the panel or in the shell", () => {
    // The Electron shell has its own bundle, so its listeners are found by the same pattern over that
    // tree; a dispatch addressed to nobody is round 94's defect.
    const shellDir = path.resolve(HERE, "..", "..", "..", "..", "summrise-desktop-electron");
    let shellText = "";
    try {
      shellText = filesUnder(shellDir, (f) => /\.js$/.test(f) && !f.includes("node_modules"))
        .map((f) => readFileSync(f, "utf8"))
        .join("\n");
    } catch {
      // The shell is a separate package; if it is not present the check still means something.
    }
    const shellListeners = new Set(
      [...shellText.matchAll(/addEventListener\(\s*"(summrise-[a-z-]+)"/g)].map((m) => m[1]),
    );
    const unattended = [...dispatched].filter(
      (n) => !listeners.has(n) && !shellListeners.has(n) && n !== "summrise-term-output",
    );
    expect(
      unattended,
      "a dispatched event nobody listens for is a failure the operator never sees",
    ).toEqual([]);
  });
});
