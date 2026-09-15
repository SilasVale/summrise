// WHICH SESSIONS IS NOBODY USING — the rule, and its two refusals.
//
// The threshold is an hour because the measured distribution on the device is bimodal: sessions in
// use are silent for seconds, forgotten ones for hours (eleven of sixteen were past five hours, one
// past eleven). These tests pin the boundaries, and the cases that must NOT be offered.
import { describe, it, expect } from "vitest";
import { IDLE_OFFER_MS, idleOfferText, idleSessions } from "../idleSessions";
import type { Session } from "../../hooks/useSessions";

const s = (over: Partial<Session>): Session =>
  ({
    sid: "s1",
    label: "pwsh",
    kind: "pty",
    closed: false,
    savedOnly: false,
    active: false,
    idleMs: 0,
    firstSeenAt: 0,
    closedAt: null,
    heldByHuman: false,
    approvalRequired: false,
    pendingApproval: null,
    approvalGrants: [],
    goal: null,
    plan: [],
    ...over,
  }) as Session;

describe("idle sessions", () => {
  it("offers only what is silent past the threshold", () => {
    const fresh = s({ sid: "a", idleMs: 1000 });
    const borderline = s({ sid: "b", idleMs: IDLE_OFFER_MS });
    const idle = s({ sid: "c", idleMs: IDLE_OFFER_MS + 1 });
    const ancient = s({ sid: "d", idleMs: 11 * 3600_000 });
    const got = idleSessions([fresh, borderline, idle, ancient]).map((x) => x.sid);
    // `>` not `>=`: a session silent for EXACTLY the threshold is not yet offered — the bar exists
    // to catch forgotten sessions, and an offer that fires on the boundary is one that fires early.
    expect(got).toEqual(["c", "d"]);
  });

  it("never offers a CLOSED session: there is no shell left to release", () => {
    // Counting them would also make the number wrong — closed tabs stay in the list as tombstones.
    const closed = s({ sid: "closed", closed: true, idleMs: 5 * 3600_000 });
    const saved = s({ sid: "saved", savedOnly: true, idleMs: 5 * 3600_000 });
    expect(idleSessions([closed, saved])).toEqual([]);
  });

  it("says how many, how long, and which", () => {
    const text = idleOfferText([
      s({ sid: "a", label: "d1", idleMs: 2 * 3600_000 }),
      s({ sid: "b", label: "serial:COM4", idleMs: 11 * 3600_000 }),
    ]);
    expect(text).toContain("2 sessions");
    expect(text).toContain("11h00m");
    expect(text).toContain("d1");
    expect(text).toContain("serial:COM4");
    // One is singular, and a long list is summarized rather than printed in full.
    expect(idleOfferText([s({ sid: "a", idleMs: 2 * 3600_000 })])).toContain("1 session idle");
    const many = idleOfferText(["a", "b", "c", "d", "e"].map((id) => s({ sid: id, label: id, idleMs: 2 * 3600_000 })));
    expect(many).toContain("+2 more");
    // Nothing to offer is an empty string, not a sentence about nothing.
    expect(idleOfferText([])).toBe("");
  });
});
