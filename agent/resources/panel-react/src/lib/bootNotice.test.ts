// The boot notice's rule — the part that decides whether the operator sees anything.
//
// WHAT IS BEING PINNED, and why each case is here rather than implied:
//
//   * a CRASH is shown, always — it is the only verdict that asks for attention, and
//     the whole point of round 254's classifier is that this case stops being a
//     sentence nobody reads in a log file;
//   * a REPLACEMENT is shown only while it is NEWS — right after `vale update` the
//     panel should confirm the swap, and forever after that a chip would be chrome;
//   * a FIRST RUN and a CLEAN EXIT say NOTHING — silence is a feature here, and a rule
//     that drifts toward "always say something" is how chips stop being read;
//   * an UNKNOWN kind (a newer agent) says NOTHING — never a warning this build cannot
//     explain;
//   * with no verdict on record, nothing renders at all.
import { describe, expect, it } from "vitest";
import { bootNotice, REPLACED_NOTICE_SECS } from "./bootNotice";

const CRASH = {
  kind: "crashed" as const,
  detail: "run journal: previous run DID NOT EXIT CLEANLY — CRASHED or was killed; survived 12s",
};
const REPLACED = {
  kind: "replaced" as const,
  detail: "run journal: previous run DID NOT EXIT CLEANLY — REPLACED by a restart; survived 15664s",
};

describe("bootNotice", () => {
  it("shouts about a crash, with the device's own sentence", () => {
    const n = bootNotice(CRASH, 3600);
    expect(n?.tone).toBe("warn");
    expect(n?.text).toContain("crashed");
    // The full sentence is the hover — the chip is the headline, never the whole story.
    expect(n?.title).toContain("CRASHED or was killed");
  });

  it("keeps the crash chip for the WHOLE run, however long it has been up", () => {
    // The verdict describes the run BEFORE this one, so "it crashed" stays true until
    // the next clean boot clears it. Expiring it would hide a fault because time passed.
    for (const up of [0, 60, 86_400, 30 * 86_400]) {
      expect(bootNotice(CRASH, up)?.tone, `uptime ${up}`).toBe("warn");
    }
    // Even with no trustworthy uptime at all: the fault does not depend on it.
    expect(bootNotice(CRASH, null)?.tone).toBe("warn");
  });

  it("shows the replacement as news, then drops it", () => {
    expect(bootNotice(REPLACED, 5)?.tone).toBe("info");
    expect(bootNotice(REPLACED, REPLACED_NOTICE_SECS - 1)?.tone).toBe("info");
    // One second past the window: the uptime reading beside it already says the same
    // thing, so the chip goes quiet instead of becoming permanent furniture.
    expect(bootNotice(REPLACED, REPLACED_NOTICE_SECS)).toBeNull();
    expect(bootNotice(REPLACED, 86_400)).toBeNull();
  });

  it("says nothing for a normal boot and nothing for no verdict", () => {
    expect(bootNotice({ kind: "first-run", detail: "no previous run on record" }, 1)).toBeNull();
    expect(bootNotice({ kind: "clean-exit", detail: "exited cleanly" }, 1)).toBeNull();
    expect(bootNotice(null, 10)).toBeNull();
    expect(bootNotice(undefined, 10)).toBeNull();
  });

  it("does NOT cry crash over a machine restart, however long the host was down", () => {
    // The verdict that exists so this chip stays trustworthy: a device that was rebooted
    // or lost power ends its run exactly the way a crash does, and the device tells the
    // two apart (its host booted after the last heartbeat). Raising a warning here would
    // fire on every routine restart — including, for this project, the ones an operator
    // causes on purpose.
    const rebooted = {
      kind: "machine-restart" as const,
      detail: "run journal: previous run DID NOT EXIT CLEANLY — the MACHINE RESTARTED under it",
    };
    for (const up of [0, 30, 600, 86_400]) {
      expect(bootNotice(rebooted, up), `uptime ${up}`).toBeNull();
    }
  });

  it("says nothing about a verdict it cannot explain", () => {
    // A newer agent inventing a kind this build does not know: silence, not a guess.
    expect(bootNotice({ kind: null, detail: "run journal: something new" }, 5)).toBeNull();
  });

  it("will not call a restart 'just now' without a usable uptime", () => {
    // A replacement's whole claim is "this happened seconds ago". With no uptime to
    // check that against, the honest answer is to say nothing.
    expect(bootNotice(REPLACED, null)).toBeNull();
    expect(bootNotice(REPLACED, undefined)).toBeNull();
  });
});
