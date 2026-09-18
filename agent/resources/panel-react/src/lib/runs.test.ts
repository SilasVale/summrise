// THE RUNS CONTRACT, read against the device's own example (round 66).
//
// The other half of `agent/tests/fixtures/run-event.json`: the Rust test in `agent/src/operation.rs` asserts the
// device SENDS every field on each feed's allowlist, and this asserts the panel READS them — by grouping the
// fixture's own example rows and checking that the run, the plan step, the intent and the alternatives all survived.
//
// WHY IT MATTERS RATHER THAN BEING TIDY: the device's mapping is an allowlist, and its comment says a field missing
// there is "dropped silently with every other test still green". A panel test with its own hand-written payload
// cannot see that — it would keep passing while the wire stopped carrying the fact.
import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { groupOperation } from "./runs";
import type { OperationEvent } from "./runs";

const fixture = JSON.parse(
  readFileSync(path.join(path.dirname(fileURLToPath(import.meta.url)), "../../../../tests/fixtures/run-event.json"), "utf8"),
);

/** The fixture's rows, as the route hands them over — `runs` is the array `/api/operation` returns. */
const rows = (): OperationEvent[] => [
  fixture.example_terminal,
  fixture.example_terminal_end,
  fixture.example_browser,
];

describe("the runs fixture, end to end", () => {
  it("groups the device's own example into ONE run, by the run_id it carries", () => {
    const groups = groupOperation(rows(), []);
    expect(groups.runs.length).toBe(1);
    const g = groups.runs[0];
    expect(g.runId).toBe("run-1000-abc123");
    // BOTH FEEDS LAND IN THE SAME RUN — the whole point of carrying run_id through the merge, and the thing the
    // device's own test pins from its side. The counts are the group's own accounting: rows.length is exactly
    // terminal + browser.
    expect(g.terminal).toBe(2);
    expect(g.browser).toBe(1);
    expect(g.rows.length).toBe(3);
  });

  it("keeps the fields the panel reads per row: plan step, intent, the alternatives, exit code and duration", () => {
    const g = groupOperation(rows(), []).runs[0];
    const start = g.rows.find((r) => r.kind === "command/start");
    const end = g.rows.find((r) => r.kind === "command/end");
    const action = g.rows.find((r) => r.source === "browser");
    expect(start?.planStep).toBe(2);
    expect(start?.intent).toBe("check the agent is alive");
    expect(start?.considered).toEqual(["vale doctor"]);
    expect(start?.session).toBe("term-1");
    expect(start?.seq).toBe(41);
    expect(end?.exitCode).toBe(0);
    expect(end?.durationMs).toBe(298);
    expect(action?.durationMs).toBe(812);
    expect(action?.timedOut).toBe(false);
    expect(action?.screenshots).toEqual(["evidence/1789700001500.png"]);
    expect(action?.script).toBe("mcp: click");
    // THE BROWSER FEED CARRIES NO `text` AND NO `status`, and the panel's own default turns that absence into null
    // rather than into an empty string or a false timeout.
    expect(action?.text).toBeNull();
    expect(action?.status).toBeNull();
  });

  it("reads every field the fixture promises for the panel, on the feed that carries it", () => {
    const g = groupOperation(rows(), []).runs[0];
    const union = new Set(g.rows.flatMap((r) => Object.keys(r)));
    // the panel's own field names, which is what `required_by_panel` was read off
    const promised = [
      "source", "tsMs", "kind", "session", "seq", "command", "text", "status",
      "exitCode", "durationMs", "intent", "considered", "planStep", "runId",
      "script", "timedOut", "screenshots",
    ];
    for (const k of promised) expect(union.has(k), `the panel's row is missing ${k}`).toBe(true);
  });
});
