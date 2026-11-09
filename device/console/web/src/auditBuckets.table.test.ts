
import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import path from "node:path";
import { isChannelBoundary, isClientCall, isConsoleRead, isPoll } from "./activitySummary";
import type { AuditLine } from "./api";

/**
 * **ONE TABLE OF CASES, BOTH SIDES RUN IT (the forty-fifth list, item 2).**
 *
 * `is_client_call` lives in `device/console/src/audit.rs` and is re-implemented here; the two agree
 * today only because nobody has changed one alone. `docs/superpowers/specs/audit-buckets.json` is the
 * rule as CASES, and the Rust test `the_bucketing_rule_is_the_shared_table` reads the same file. A
 * case one side answers differently is drift, and it fails in whichever repo changed.
 *
 * The table is data rather than prose on purpose: three lists in a row found a comparison between two
 * intervals, and every one of them was a reader (or an auditor) parsing an English sentence.
 */
// jsdom gives `import.meta.url` an http scheme, so the table is read by PATH from this file's own
// directory — the same hop the layout contract's test uses to reach the repo's specs.
const TABLE = JSON.parse(
  readFileSync(
    path.join(__dirname, "..", "..", "..", "..", "docs", "superpowers", "specs", "audit-buckets.json"),
    "utf8"
  )
) as {
  cases: {
    why: string;
    line: { caller?: string; kind?: string; tool?: string; token_id?: string };
    client: boolean;
    console: boolean;
    poll: boolean;
    boundary: boolean;
  }[];
};

describe("the audit's bucketing rule, as the shared table states it", () => {
  it("answers every case the way the console's own rule does", () => {
    for (const entry of TABLE.cases) {
      // **THE LINE AS THE PREDICATES READ IT.** The table's cases carry only the fields the rule
      // looks at, and the three predicates each take a `Pick` of the audit line; this is the subset
      // they can see, with the audit's own optionality (`undefined`, not `null`).
      const line = {
        caller: entry.line.caller,
        kind: entry.line.kind,
        tool: entry.line.tool,
        token_id: entry.line.token_id
      } as Pick<AuditLine, "caller" | "kind" | "tool" | "token_id">;
      expect(isChannelBoundary(line), `${entry.why} (boundary)`).toBe(entry.boundary);
      expect(isPoll(line), `${entry.why} (poll)`).toBe(entry.poll);
      expect(isClientCall(line), `${entry.why} (client)`).toBe(entry.client);
      expect(isConsoleRead(line), `${entry.why} (console)`).toBe(entry.console);
    }
  });

  it("counts the table's cases into one bucket each, and the boundary into none", () => {
    // The four buckets are the page's day line, and they PARTITION the work: every line is a client
    // call, a console read, a poll failure or unattributed — never two of them.
    let client = 0;
    let console_ = 0;
    let poll = 0;
    let unattributed = 0;
    for (const entry of TABLE.cases) {
      const line = {
        caller: entry.line.caller,
        kind: entry.line.kind,
        tool: entry.line.tool,
        token_id: entry.line.token_id
      } as Pick<AuditLine, "caller" | "kind" | "tool" | "token_id">;
      if (isChannelBoundary(line)) continue;
      if (isPoll(line)) poll += 1;
      else if (isClientCall(line)) client += 1;
      else if (isConsoleRead(line)) console_ += 1;
      else unattributed += 1;
    }
    expect(client + console_ + poll + unattributed).toBe(
      TABLE.cases.filter((entry) => !entry.boundary).length
    );
  });
});
