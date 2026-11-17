import { describe, expect, it } from "vitest";
import type { Actor } from "./sessionActivity";
import type { SpineRow } from "./sessionSpine";
import { buildSessionSpine, groupEnd, groupHeader } from "./sessionSpine";

const YOU: Actor = {
  label: "You",
  kind: "you",
  title: "This console made the call on your operator session."
};

const CLIENT: Actor = {
  label: "gate-laptop",
  kind: "client",
  title: "A client made this call with the token gate-laptop."
};

const UNKNOWN: Actor = {
  label: "Unattributed",
  kind: "unknown",
  title: "The record names no caller."
};

/** One row of the record, as the console's own row fields describe it. */
function row(overrides: Partial<SpineRow> & { id: string; atMs: number }): SpineRow {
  return { actor: YOU, ...overrides };
}

describe("the session spine", () => {
  it("a_group_header_names_the_purpose_and_its_author", () => {
    const spine = buildSessionSpine([
      row({ id: "begin", atMs: 10, txn: "txn-9c07", purpose: "verify the suite", outcome: "working" }),
      row({ id: "call", atMs: 20, txn: "txn-9c07", outcome: "ok" })
    ]);

    expect(spine.groups).toHaveLength(1);
    const header = groupHeader(spine.groups[0]);
    expect(header.purpose).toBe("verify the suite");
    // The purpose is the caller's claim, and the header carries its author rather than reading as
    // a fact of the machine's own.
    expect(header.badge).toBe("claim");
    expect(header.author).toBe("You");
    expect(header.authorKind).toBe("you");
    expect(header.owner).toBe("you");
    expect(header.claim).toBe("the operator's claim");
  });

  it("a_declared_abort_does_not_relabel_its_calls", () => {
    const spine = buildSessionSpine([
      row({ id: "begin", atMs: 10, txn: "txn-1", purpose: "fix the parser", outcome: "working" }),
      row({
        id: "failed",
        atMs: 20,
        txn: "txn-1",
        outcome: "failed",
        title: "cargo test -p parser"
      }),
      row({
        id: "abort",
        atMs: 30,
        txn: "txn-1",
        outcome: "aborted",
        receipt: {
          txn: "txn-1",
          state: "aborted",
          purpose: "fix the parser",
          reason: "the build was red",
          observed: { calls: 1, failures: 1, refusals: 0, duration_ms: 1_200 }
        }
      })
    ]);

    const group = spine.groups[0];
    expect(group.state).toBe("aborted");
    expect(group.open).toBe(false);
    // The declaration is the group's ending, not a rewrite of what the device observed under it.
    expect(group.calls[0].outcome).toBe("failed");
    expect(group.calls[0].title).toBe("cargo test -p parser");
    const end = groupEnd(group);
    expect(end.open).toBe(false);
    expect(end.sentence).toContain("1 failure");
    expect(end.sentence).toContain("aborted");
  });

  it("an_open_group_says_no_end_was_recorded", () => {
    const spine = buildSessionSpine([
      row({ id: "begin", atMs: 10, txn: "txn-1", purpose: "fix the parser" }),
      row({ id: "call", atMs: 20, txn: "txn-1", outcome: "ok" })
    ]);

    const group = spine.groups[0];
    expect(group.open).toBe(true);
    expect(group.state).toBe("working");
    const end = groupEnd(group);
    expect(end.open).toBe(true);
    expect(end.sentence).toMatch(/no end recorded/i);
    // A group left working is never closed by the page: the device is the only one that may.
    expect(spine.counts.open).toBe(1);
  });

  it("a_goal_divider_sits_where_the_record_changed", () => {
    const spine = buildSessionSpine([
      row({ id: "goal-1", atMs: 10, goal: { text: "make the installer's config readable" } }),
      row({ id: "begin", atMs: 20, txn: "txn-1", purpose: "read the config" }),
      // The same sentence written again is not a change, so it earns no second divider.
      row({ id: "goal-same", atMs: 30, goal: { text: "make the installer's config readable" } }),
      row({
        id: "goal-2",
        atMs: 40,
        goal: { text: "ship the spine" },
        actor: CLIENT
      }),
      row({
        id: "end",
        atMs: 50,
        txn: "txn-1",
        outcome: "committed",
        receipt: { txn: "txn-1", state: "committed", observed: { calls: 0 } }
      })
    ]);

    // Reading order, oldest first: the divider is where the record changed, not at the top of the
    // page. The transaction sits between the two changes.
    expect(spine.entries.map((entry) => entry.kind)).toEqual(["goal", "txn", "goal"]);
    expect(spine.dividers.map((divider) => divider.goal?.text ?? null)).toEqual([
      "make the installer's config readable",
      "ship the spine"
    ]);
    expect(spine.dividers[1].author.label).toBe("gate-laptop");
    expect(spine.goalNow?.text).toBe("ship the spine");
  });

  it("an_unknown_event_renders_neutral_and_is_counted", () => {
    const spine = buildSessionSpine([
      row({
        id: "future",
        atMs: 10,
        outcome: "a-kind-this-build-cannot-place",
        title: "future.thing"
      })
    ]);

    // Never dropped: the reader can see it, and the page can say how many there were.
    expect(spine.counts.unknown).toBe(1);
    expect(spine.entries).toHaveLength(1);
    const entry = spine.entries[0];
    expect(entry.kind).toBe("row");
    expect(entry.kind === "row" && entry.neutral).toBe(true);
    expect(entry.kind === "row" && entry.row.title).toBe("future.thing");
  });

  it("a_client_purpose_is_never_shown_as_the_operators_instruction", () => {
    const spine = buildSessionSpine([
      row({
        id: "begin",
        atMs: 10,
        txn: "txn-1",
        purpose: "deploy the client's build",
        outcome: "working",
        actor: CLIENT
      })
    ]);

    const header = groupHeader(spine.groups[0]);
    expect(header.purpose).toBe("deploy the client's build");
    expect(header.author).toBe("gate-laptop");
    expect(header.authorKind).toBe("client");
    expect(header.owner).toBe("client");
    // The badge and the claim label name whose sentence it is; no surface may call the client's
    // purpose the operator's instruction.
    expect(header.badge).toBe("claim");
    expect(header.claim).toBe("the client's claim");
    expect(`${header.purpose} ${header.author} ${header.claim}`).not.toMatch(/instruction/i);
  });

  it("an orphan group shows the calls it has and says the receipt is not retained", () => {
    const spine = buildSessionSpine([
      // The call is attributed, which is what makes the misattribution possible: the first call's
      // actor must never be printed as the author of a purpose this window did not retain.
      row({ id: "call", atMs: 10, txn: "txn-9", outcome: "ok", title: "files.write", actor: CLIENT })
    ]);

    const group = spine.groups[0];
    expect(group.beginsHere).toBe(false);
    expect(group.open).toBe(false);
    expect(group.purpose).toBeNull();
    expect(group.calls).toHaveLength(1);
    const header = groupHeader(group);
    expect(header.purpose).toMatch(/purpose not retained/i);
    // No begin row means no declaration to attribute: the badge says so and no author is shown.
    expect(header.author).toBeNull();
    expect(header.authorKind).toBeNull();
    expect(header.owner).toBe("unattributed");
    expect(header.badge).toBe("unattributed");
    expect(header.claim).toMatch(/no author retained/i);
    expect(header.claim).not.toMatch(/gate-laptop/);
    const end = groupEnd(group);
    expect(end.sentence).toContain("1 call in this window");
    expect(end.sentence).toMatch(/receipt not retained/i);
  });

  it("a_refused_begin_opens_no_group_and_is_drawn_as_the_call_it_did_not_run", () => {
    const spine = buildSessionSpine([
      // The published class is explicit here; the next row is the console's older shape, where the
      // evidence still carries `purpose` but the only failure signal is the row's tone.
      row({
        id: "refused",
        atMs: 10,
        purpose: "deploy the client's build",
        outcome: "refused",
        tone: "failed"
      }),
      row({
        id: "legacy",
        atMs: 20,
        purpose: "deploy the client's build",
        outcome: "working",
        tone: "failed"
      })
    ]);

    // Classify failure/refusal before purpose: neither row is a begin, so neither draws a header.
    expect(spine.groups).toHaveLength(0);
    expect(spine.entries).toHaveLength(2);
    expect(spine.entries.every((entry) => entry.kind === "row" && entry.neutral === false)).toBe(true);
    expect(spine.counts.calls).toBe(2);
    expect(spine.counts.open).toBe(0);
  });

  it("a_session_boundary_is_a_row_but_not_a_call", () => {
    const spine = buildSessionSpine([
      row({
        id: "opened",
        atMs: 10,
        rowKind: "session.opened",
        outcome: "session",
        title: "terminal session-1 opened"
      }),
      row({ id: "call", atMs: 20, outcome: "ok", title: "files.read" })
    ]);

    // `counts.calls` says "not boundaries": the boundary stays visible as a row and is not counted.
    expect(spine.counts.rows).toBe(2);
    expect(spine.counts.calls).toBe(1);
    expect(spine.entries).toHaveLength(2);
  });

  it("breaks a tie by the order the rows arrived, so one instant keeps its record order", () => {
    const spine = buildSessionSpine([
      row({ id: "begin", atMs: 10, txn: "txn-1", purpose: "one" }),
      row({ id: "call", atMs: 10, txn: "txn-1", outcome: "ok", title: "files.read" })
    ]);

    expect(spine.groups[0].calls.map((call) => call.id)).toEqual(["call"]);
  });

  it("an unattributed purpose is badged as unattributed, never as yours", () => {
    const spine = buildSessionSpine([
      row({
        id: "begin",
        atMs: 10,
        txn: "txn-1",
        purpose: "some work",
        actor: UNKNOWN
      })
    ]);

    const header = groupHeader(spine.groups[0]);
    expect(header.author).toBe("Unattributed");
    expect(header.owner).toBe("unattributed");
    expect(header.claim).toBe("an unattributed claim");
  });
});
