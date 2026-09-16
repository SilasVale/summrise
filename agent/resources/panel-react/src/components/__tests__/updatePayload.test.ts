// THE UPDATE PAYLOAD, in the shape the DEVICE sends it.
//
// Written after two rounds (110-111) spent on a card that would not render. The lesson those rounds
// paid for is in the first test: `useUpdateStatus` requires `ok === true` before it will believe a
// reply, and it does not fail loudly when the key is absent — it sets `failed`, and the card renders
// "The device did not answer, so its update state could not be read." A fixture built from the
// PARSER's fields (current, latest, update_available …) therefore looks perfectly right and produces a
// failure card. The device sends it: `update_status` in agent/src/plugins/update/tools.rs has `ok:
// true` in BOTH of its branches — the no-channel one and the answered one.
//
// So the shape below carries the envelope, and the parser's own job is the part that must survive a
// body it cannot use.
import { describe, it, expect } from "vitest";
import { parseUpdateStatus } from "../UpdateCard";

/** Exactly what `update_status` returns when a channel answered and a newer release exists. */
const answered = {
  ok: true,
  current: "1.2.403",
  channel: "https://agent.saisi.online/vale-agent",
  latest: "1.2.433",
  update_available: true,
  pinned_to: null,
  busy: false,
  error: null,
};

describe("parseUpdateStatus", () => {
  it("reads the real payload, including the envelope the hook demands", () => {
    expect(answered.ok, "useUpdateStatus refuses a reply without this").toBe(true);
    const s = parseUpdateStatus(answered);
    expect(s.current).toBe("1.2.403");
    expect(s.latest).toBe("1.2.433");
    expect(s.channel).toContain("agent.saisi.online");
    expect(s.updateAvailable).toBe(true);
    expect(s.busy).toBe(false);
    expect(s.pinnedTo).toBeNull();
    expect(s.error).toBeNull();
  });

  it("treats 'no release server configured' as UNKNOWN, never as up to date", () => {
    // The device's other branch: no channel at all. `latest` is null and `update_available` false —
    // and this must not be presented as "you are current", which is the claim the card's own comment
    // calls out. The parser keeps them distinguishable: no latest, nothing to say.
    const noChannel = { ok: true, current: "1.2.403", channel: null, latest: null, update_available: false, pinned_to: null, busy: false, error: null };
    const s = parseUpdateStatus(noChannel);
    expect(s.latest).toBeNull();
    expect(s.updateAvailable).toBe(false);
    expect(s.channel).toBeNull();
  });

  it("survives a body it cannot use, and does not invent a version", () => {
    for (const junk of [null, undefined, 42, "text", {}, { ok: true }]) {
      const s = parseUpdateStatus(junk);
      expect(s.current, "an unreadable body is an EMPTY state, never a version").toBe("");
      expect(s.latest).toBeNull();
      expect(s.updateAvailable).toBe(false);
    }
  });

  it("reads busy and a pinned release, the two states an operator watches during an update", () => {
    const applying = parseUpdateStatus({ ...answered, busy: true, latest: "1.2.403", update_available: false });
    expect(applying.busy).toBe(true);
    expect(applying.updateAvailable).toBe(false);
    const pinned = parseUpdateStatus({ ...answered, pinned_to: "1.2.400", latest: "1.2.400", update_available: false });
    expect(pinned.pinnedTo).toBe("1.2.400");
  });
});
