// The wire → panel mapping for the boot verdict.
//
// `/api/status` carries TWO fields for one fact (round 256): `last_boot`, the sentence
// `logs/startup.log` also gets, and `last_boot_kind`, the same verdict as data. The
// panel branches on the kind — so a typo here would not throw, it would silently
// render nothing, which is exactly the failure this pair of tests is here to prevent.
//
// THREE SHAPES EXIST IN THE FIELD and all three are pinned below:
//   1. this build:      last_boot + last_boot_kind
//   2. 1.2.366:         last_boot only (the kind was not recorded yet)
//   3. a newer agent:   a kind this build does not know
import { describe, expect, it } from "vitest";
import { parseLastBoot } from "../useAgentVitals";

describe("parseLastBoot", () => {
  // EVERY CALL IS `await`ed NOW, because the parse is RUST (P2, 2026-09-29) and the wasm is fetched
  // at the first call. THE ASSERTIONS ARE UNCHANGED — `tsc --noEmit` named these lines, which is the
  // point of running the checker rather than reading the call sites. The boundaries these tests pin
  // are the ones that made the port worth checking: `""` and `"   "` are both no-verdict, `42` is
  // not a sentence, and an unknown kind keeps its sentence and gets `kind: null`.
  it("reads both fields off a current response", async () => {
    const b = await parseLastBoot({
      ok: true,
      last_boot: "run journal: previous run DID NOT EXIT CLEANLY — CRASHED or was killed",
      last_boot_kind: "crashed",
    });
    expect(b).toEqual({
      kind: "crashed",
      detail: "run journal: previous run DID NOT EXIT CLEANLY — CRASHED or was killed",
    });
  });

  it("keeps a 1.2.366 device's verdict, with NO kind invented for it", async () => {
    // The device that has not rebooted since the older build still has a sentence worth
    // showing, and the panel must not guess which verdict it was by reading English.
    const b = await parseLastBoot({ last_boot: "run journal: previous run DID NOT EXIT CLEANLY — REPLACED" });
    expect(b?.kind).toBeNull();
    expect(b?.detail).toContain("REPLACED");
  });

  it("does not coerce a kind it does not know", async () => {
    const b = await parseLastBoot({ last_boot: "run journal: ???", last_boot_kind: "melted" });
    expect(b?.kind).toBeNull();
    expect(b?.detail).toBe("run journal: ???");
  });

  it("knows the reboot verdict apart from a crash", async () => {
    // The device distinguishes "the host went down with the run" from "the agent died";
    // the panel must carry that distinction through rather than flattening it (the chip
    // rules on the other side of this parse depend on it).
    const b = await parseLastBoot({
      last_boot: "run journal: previous run DID NOT EXIT CLEANLY — the MACHINE RESTARTED under it",
      last_boot_kind: "machine-restart",
    });
    expect(b?.kind).toBe("machine-restart");
  });

  it("treats a missing or blank sentence as no verdict at all", async () => {
    // A kind with no sentence is not something to show a human: no chip, no title.
    expect(await parseLastBoot({})).toBeNull();
    expect(await parseLastBoot({ last_boot: "" })).toBeNull();
    expect(await parseLastBoot({ last_boot: "   " })).toBeNull();
    expect(await parseLastBoot({ last_boot_kind: "crashed" })).toBeNull();
    expect(await parseLastBoot({ last_boot: 42 })).toBeNull();
  });
});
