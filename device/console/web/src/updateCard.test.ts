import { describe, expect, it } from "vitest";
import { exactMoment, isoDate, clockTime } from "./format";
import { updateRows } from "./updateCard";

/**
 * THE MACHINE TAB'S UPDATE CARD, AS THE PANEL'S GRID.
 *
 * Every word below comes from the fleet read: the device's state document (running version, last
 * target, outcome, result, check), its boot journal (channel, when the process started) and its own
 * verdict sentence. The console adds the absent words (`not recorded`, `not reported`) and nothing
 * else. The three shapes this file pins are the ones the page draws: the applied version is the
 * running one, the running version is behind the last target, and a device with no update record.
 */
const NOW = 1_800_000_000_000;
const MINUTE = 60_000;
const HOUR = 3_600_000;

const RESULT_AT = NOW - 2 * HOUR;
const CHECK_AT = NOW - 10 * MINUTE;
const STARTED_AT = NOW - 3 * HOUR;

const HOST = {
  restarts: [{ at_ms: STARTED_AT, version: "0.3.290", channel: "canary" }],
  update: { current: { version: "0.3.290", channel: "canary" } }
};

describe("updateRows — the panel's UPDATES grid, one version per row", () => {
  it("puts the applied version first when it is the one running", () => {
    const rows = updateRows({
      version: "0.3.290",
      // The applied target is the running release. `0.3.290.0` is the same release spelled the
      // other way, which is what `versionEqual` is for.
      update: {
        current: "0.3.290",
        last_update: { to: "0.3.290.0", outcome: "applied", at_ms: RESULT_AT },
        last_check: { at_ms: CHECK_AT, trigger: "schedule" }
      },
      host: HOST
    });

    expect(rows.map((row) => row.label)).toEqual([
      "Applied",
      "Last outcome",
      "Last result",
      "Last check",
      "Channel"
    ]);
    expect(rows[0]).toMatchObject({ key: "applied", text: "0.3.290" });
    expect(rows[1].text).toBe("applied");
    expect(rows[2]).toMatchObject({
      text: `${isoDate(RESULT_AT)} ${clockTime(RESULT_AT)}`,
      exact: exactMoment(RESULT_AT)
    });
    expect(rows[3]).toMatchObject({
      text: `${isoDate(CHECK_AT)} ${clockTime(CHECK_AT)}`,
      exact: exactMoment(CHECK_AT)
    });
    expect(rows[4]).toMatchObject({ text: "canary" });
    // ONE VERSION PER ROW, AND THIS VERSION ON ONE ROW: no `Running`/`Last target` row repeats it.
    expect(rows.filter((row) => row.text.includes("0.3.290"))).toHaveLength(1);
    expect(rows.map((row) => row.label)).not.toContain("Running");
    expect(rows.map((row) => row.label)).not.toContain("Last target");
  });

  it("puts the running version and the target on their own rows when the machine is behind", () => {
    // The behind shape: the updater applied 0.3.290 but the process still runs 0.3.280. Each version
    // is a row, so the gap is read, not buried in a clause.
    const rows = updateRows({
      version: "0.3.280",
      update: {
        current: "0.3.280",
        last_update: { to: "0.3.290", outcome: "applied", at_ms: RESULT_AT },
        last_check: { at_ms: CHECK_AT, trigger: "channel" }
      },
      host: {
        restarts: [{ at_ms: STARTED_AT, version: "0.3.280", channel: "canary" }],
        update: { current: null, previous: null, pending: null, last_swap: null }
      }
    });

    expect(rows.map((row) => row.label)).toEqual([
      "Running",
      "Last target",
      "Last outcome",
      "Last result",
      "Last check",
      "Channel"
    ]);
    expect(rows[0].text).toBe("0.3.280");
    expect(rows[1].text).toBe("0.3.290");
    expect(rows[2].text).toBe("applied");
    // The channel falls back to the journal's newest entry when the launcher has no generation.
    expect(rows[5].text).toBe("canary");
  });

  it("says what a device with no update record did not send, rather than drawing an empty card", () => {
    const rows = updateRows({ update: { current: "0.3.284" } });

    expect(rows.map((row) => row.label)).toEqual([
      "Running",
      "Last target",
      "Last outcome",
      "Last result",
      "Last check",
      "Channel"
    ]);
    expect(rows[0].text).toBe("0.3.284");
    for (const row of rows.slice(1, 5)) {
      expect(row).toMatchObject({ text: "not recorded", absent: true });
    }
    expect(rows[5]).toMatchObject({ text: "not reported", absent: true });
  });

  it("labels an applied answer older than this start as history", () => {
    // The measured 9VH: the process started 06:16, the apply is two days old. The row is the
    // device's record; the qualifier stops it reading as what the running process just did.
    const rows = updateRows({
      version: "0.3.286",
      update: {
        current: "0.3.286",
        verdict: { outcome: "ok", detail: "started 0.3.286" },
        last_update: { to: "0.3.273", outcome: "applied", at_ms: STARTED_AT - 2 * 24 * HOUR },
        last_check: { at_ms: CHECK_AT }
      },
      host: { restarts: [{ at_ms: STARTED_AT, version: "0.3.286" }] }
    });

    expect(rows.find((row) => row.key === "outcome")?.text).toBe(
      "applied · before this start"
    );
  });

  it("keeps an applied answer inside this start as the current outcome", () => {
    const rows = updateRows({
      version: "0.3.277",
      update: {
        current: "0.3.277",
        last_update: { to: "0.3.277", outcome: "applied", at_ms: STARTED_AT + HOUR }
      },
      host: { restarts: [{ at_ms: STARTED_AT, version: "0.3.277" }] }
    });

    expect(rows.map((row) => row.label)).toContain("Applied");
    expect(rows.find((row) => row.key === "outcome")?.text).toBe("applied");
  });

  it("names the version a rollback returned to, and does not repeat it as what runs", () => {
    const rows = updateRows({
      version: "0.3.277",
      update: {
        current: "0.3.277",
        last_update: { to: "0.3.278", outcome: "rolled_back", at_ms: RESULT_AT },
        rolled_back_to: "0.3.277"
      }
    });

    expect(rows.map((row) => row.label)).toEqual([
      "Last target",
      "Last outcome",
      "Rolled back to",
      "Last result",
      "Last check",
      "Channel"
    ]);
    expect(rows[0].text).toBe("0.3.278");
    expect(rows[1].text).toBe("rolled back");
    expect(rows[2].text).toBe("0.3.277");
  });

  it("keeps a refusal's target, word and time as separate facts", () => {
    const rows = updateRows({
      update: {
        current: "0.3.42",
        last_update: { to: "0.3.43", outcome: "refused", at_ms: RESULT_AT },
        last_check: { at_ms: CHECK_AT }
      }
    });

    expect(rows[0]).toMatchObject({ label: "Running", text: "0.3.42" });
    expect(rows[1]).toMatchObject({ label: "Last target", text: "0.3.43" });
    expect(rows[2]).toMatchObject({ label: "Last outcome", text: "refused" });
  });

  it("draws the device's own verdict when no swap answer was folded in", () => {
    // The measured 84T: the only update result was `started 0.3.280`. The verdict is the device's
    // sentence, aged by the journal; without a journal it is shown verbatim and no time is invented.
    const rows = updateRows({
      version: "0.3.280",
      update: {
        current: "0.3.280",
        verdict: { outcome: "ok", detail: "started 0.3.280" }
      },
      host: {
        restarts: [{ at_ms: NOW - 7 * HOUR, version: "0.3.280", channel: "canary" }]
      }
    }, NOW);

    expect(rows.map((row) => row.label)).toEqual([
      "Running",
      "Verdict",
      "Last check",
      "Channel"
    ]);
    expect(rows[1].text).toBe("ok · started 0.3.280 · 7h ago");
    expect(rows[2]).toMatchObject({ text: "not recorded" });
  });

  it("keeps a verdict it cannot age as the device's sentence", () => {
    const rows = updateRows({
      update: {
        current: "0.3.280",
        verdict: { outcome: "ok", detail: "started 0.3.280" }
      }
    }, NOW);

    expect(rows.find((row) => row.key === "verdict")?.text).toBe("ok · started 0.3.280");
  });

  it("says a moment it cannot read as not recorded, never as a time", () => {
    const rows = updateRows({
      update: {
        current: "0.3.284",
        last_update: { to: "0.3.285", outcome: "applied", at_ms: Number.NaN },
        last_check: { at_ms: 0 }
      }
    });

    const result = rows.find((row) => row.key === "result");
    const check = rows.find((row) => row.key === "check");
    expect(result).toMatchObject({ text: "not recorded", absent: true });
    expect(check).toMatchObject({ text: "not recorded", absent: true });
    expect(result?.exact).toBeUndefined();
  });

  it("draws no version row when the device reported no running version", () => {
    const rows = updateRows({ update: { last_check: { at_ms: CHECK_AT } } });

    expect(rows.map((row) => row.label)).toEqual([
      "Last target",
      "Last outcome",
      "Last result",
      "Last check",
      "Channel"
    ]);
    expect(rows[0]).toMatchObject({ text: "not recorded", absent: true });
    expect(rows[3]).toMatchObject({
      text: `${isoDate(CHECK_AT)} ${clockTime(CHECK_AT)}`
    });
  });
});
