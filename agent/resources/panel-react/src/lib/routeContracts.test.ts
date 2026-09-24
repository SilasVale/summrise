// WHICH DEVICE ROUTES HAVE A SHAPE CONTRACT, and which do not — written down, so the list stops
// being rediscovered one round at a time.
//
// WHY. Rounds 93-97 pinned five boundaries by hand (boot history, SSE event names, SSE frame
// semantics, SSE frame keys, the vitals series), and each round found the same thing: a field name or
// an order that the panel reads SILENTLY, where a rename on the device side costs a feature rather
// than raising an error. Five rounds is a slow way to enumerate twenty routes.
//
// This test does not pin any shape itself. It walks the routes the panel calls and requires each one
// to be either COVERED (a fixture or test pins its shape, named here) or UNPINNED (with the reason it
// has not been done). A new route with neither fails, which is the point: the next boundary gets
// discovered by the person adding it, not by a future round wondering why a card went blank.
import { describe, it, expect } from "vitest";
import { readFileSync, readdirSync, statSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const PANEL_SRC = path.resolve(HERE, "..");
const REPO = path.resolve(HERE, "..", "..", "..", "..", "..");

/** Routes whose shape is pinned by a fixture or test read from BOTH ends, with that file named. */
const COVERED: Record<string, string> = {
  "/api/boots": "agent/tests/fixtures/boot-history.json (runstate.rs + useBootHistory.test.ts)",
  "/api/vitals/history": "agent/tests/fixtures/vitals-series.json (metrics.rs + useVitalsSeries.fixture.test.ts)",
  "/api/sessions": "agent/src/session_log.rs + useSessionArchive's own tests (parse + failure states)",
  "/api/spec": "agent/spec-tools.json (the spec snapshot test pins every device tool)",
  "/api/monitors":
    "monitorsPayload.test.ts pins the row shape the hook reads — a target whole (id, host, port, summary.upPct/upNow), transitions as at_ms/up/lasted_ms, a probe that did not answer keeping a NULL latency, and the push frame that feeds the alert strip. This entry said 'unpinned' until round 108, which is what a coverage list does when nobody re-reads it.",
  "/api/settings":
    "agent/tests/fixtures/settings.json (settings_get_shape asserts the REAL response's key set against it)",
  "/api/status":
    "agent/tests/fixtures/status.json (api_status's own test asserts the REAL response against it, and statusPayload.fixture.test.ts reads the same file)",
};

/** Routes with NO shape contract yet, each with the reason — the honest gap list. */
const UNPINNED: Record<string, string> = {

  "/api/operation":
    "the ENVELOPE is pinned by the device's own test (`the_operation_route_carries_the_envelope_the_panel_reads`, gated on the terminal feature because the route answers Internal without it): events and runs are arrays and cursor_ms is a number. THE RECORD SHAPES NEED NO PIN, and round 107 checked rather than assumed: `OperationEvent` marks every field optional because the feed MERGES two producers (the terminal audit trail and the browser actions), so tolerance is the design, not a gap. What is not tolerant is the merge, and that is already pinned at the hook level — `useOperationRuns.test.ts` covers the cursor (`since_ms` re-sends the record ON the cursor, and double-counting it would inflate every number on the strip), accumulation, no-rewind, failure tolerance, and the no-churn case by OBJECT IDENTITY (`expect(result.current).toBe(first)`), which is the render-skipping contract the merge's early return exists for.",
  "/api/logs": "a text body, not a fielded one — nothing to rename",

  "/api/update": "read by UpdateCard; the update path is exercised end to end on a device instead",
  "/api/plugins/status": "read defensively by usePlugins (its own tests cover 'a body it cannot use')",
  "/api/gateway/connect": "a POST whose response the page reports verbatim",
  "/api/monitors/add": "a POST; the state it changes is re-read through /api/monitors",
  "/api/monitors/probe": "a POST; same",
  "/api/monitors/remove": "a POST; same",
  "/api/tools/agent_update": "a POST through the tool route; the tool contract is the spec snapshot's",
  "/api/tools/terminal_saved_connections": "a POST through the tool route; same",
  "/api/plugins/playwright/${which}": "a POST through a parameterised route",
  "/api/sessions/${encodeURIComponent(sid)}":
    "a session's audit trail; the EVENTS inside it are the trajectory contract and the session row is pinned by session-row.json",
  "/api/sessions/${encodeURIComponent(sid)}/approval":
    "the approval decision POST; the grant vocabulary that matters is pinned by approval-grants.json",
  "/api/sessions/${encodeURIComponent(sid)}/control":
    "the hold/hand-back POST; the boolean it sets is the session row's held_by_human, pinned by session-row.json",
  "/api/sessions/${encodeURIComponent(sid)}/grants":
    "the standing-grant list; its vocabulary is pinned by approval-grants.json",
  "/api/tools/${name}": "the tool route itself — every tool's schema is the spec snapshot's business",
};

function panelSources(): string[] {
  const out: string[] = [];
  const walk = (dir: string) => {
    for (const name of readdirSync(dir)) {
      const full = path.join(dir, name);
      if (statSync(full).isDirectory()) walk(full);
      else if (/\.tsx?$/.test(full) && !full.includes("__tests__") && !full.endsWith(".test.ts")) out.push(full);
    }
  };
  walk(PANEL_SRC);
  return out;
}

/**
 * Comments are stripped BEFORE the scans, because a scan that counts MENTIONS can be satisfied by
 * prose — the rule this repo states in its sibling scan in `agent/src/web/mod.rs` ("Strip comments
 * before counting… a naive scan counts MENTIONS"). Concretely: a doc example or a commented-out
 * `path: "/api/somewhere"` would enter `called`, and "no route that is neither covered nor explained"
 * would then pass while the panel called nothing of the sort.
 *
 * ONLY WHOLE-LINE AND BLOCK COMMENTS, deliberately. A strip at the first `//` would cut every
 * `https://…` string literal in this tree, and a route literal sitting after one on the same line
 * would silently disappear — a false NEGATIVE is worse here than the false positive this guards,
 * because the floor below cannot tell a missing route from a route that moved. So a TRAILING comment
 * (`callApi("/api/x"); // why`) still counts as code, which is right: that call is real.
 */
function stripComments(text: string): string {
  return text
    .replace(/\/\*[\s\S]*?\*\//g, "")
    .split("\n")
    .filter((line) => !/^\s*\/\//.test(line))
    .join("\n");
}

describe("every device route the panel calls is either pinned or named as unpinned", () => {
  const called = new Set<string>();
  for (const f of panelSources()) {
    const text = stripComments(readFileSync(f, "utf8"));
    for (const m of text.matchAll(/(?:callApi|callTool)\(\s*[`"'](\/api\/[^`"']*)/g)) {
      // Normalise the parameterised forms to the shape used in the two lists above.
      called.add(m[1].split("?")[0]);
    }
    // AND THE READ LOOP'S OWN SEAM. A reader that migrated onto `useDeviceRead` hands its route
    // over as `path: "/api/x"` instead of calling `callApi` itself, so the pattern above stopped
    // seeing exactly those five routes — and this list, which exists to notice a route that has no
    // shape contract, would have answered "the panel no longer calls /api/boots" about a hook that
    // still does (found by this test's own "no longer called" case when the migration landed).
    // A quoted `/api/` literal in a `path:` property is a route by definition; the wire's monitor
    // `path` field is a value READ off a body, never a literal in this tree.
    //
    // AND THE FUNCTION FORM IS A ROUTE TOO (same test, same failure, one round later): a
    // cursor-carrying reader passes `path: () => ".../api/operation?since_ms=..."`, whose route is
    // still /api/operation — the `?` is stripped below like every other query. Without the
    // optional arrow the reader would look like it stopped calling the route, which is precisely
    // the false "no longer called" this scan exists to raise.
    for (const m of text.matchAll(/\bpath:\s*(?:\(\s*\)\s*=>\s*)?[`"'](\/api\/[^`"']*)/g)) {
      called.add(m[1].split("?")[0]);
    }
  }

  it("found the panel's routes, or it proves nothing", () => {
    expect(called.size, "no routes parsed — the call pattern or the tree changed").toBeGreaterThanOrEqual(15);
  });

  it("has no route that is neither covered nor explained", () => {
    const unaccounted = [...called].filter((r) => !(r in COVERED) && !(r in UNPINNED)).sort();
    expect(
      unaccounted,
      "a route the panel reads has no shape contract and no reason — add it to COVERED (with the " +
        "fixture that pins it) or to UNPINNED (with why not). A renamed field costs a feature, and " +
        "says nothing.",
    ).toEqual([]);
  });

  it("does not list routes the panel no longer calls", () => {
    // A stale entry is how a coverage list stops describing reality — the same failure the proven-gate
    // ledger and the sweep headers are written to avoid.
    const stale = [...Object.keys(COVERED), ...Object.keys(UNPINNED)].filter((r) => !called.has(r)).sort();
    expect(stale, "listed here but no longer called — drop it, or the list is fiction").toEqual([]);
  });

  it("names a fixture that exists for every COVERED route", () => {
    for (const [route, why] of Object.entries(COVERED)) {
      const file = /\b(agent\/tests\/fixtures\/[\w.-]+\.json)/.exec(why)?.[1];
      if (!file) continue; // covered by a test rather than a fixture, e.g. the spec snapshot
      expect(
        statSync(path.join(REPO, file)).isFile(),
        `${route} claims ${file}, which is not there`,
      ).toBe(true);
    }
  });
});
