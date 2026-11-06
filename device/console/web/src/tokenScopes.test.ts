import { describe, expect, it } from "vitest";
import type { TokenMeta } from "./api";
import { mergeTokenRows, scopeLabel, tokenScope } from "./tokenScopes";

const MACHINES = [
  { id: "device-1", name: "workshop" },
  { id: "device-2", name: "attic" }
];

function token(overrides: Partial<TokenMeta> = {}): TokenMeta {
  return {
    id: "tok-device",
    name: "laptop",
    device_id: "device-1",
    tools: ["files.read"],
    allow_destructive: false,
    created_at_ms: 1_000,
    ...overrides
  };
}

describe("tokenScope", () => {
  it("calls a token with device_ids a fleet token", () => {
    expect(tokenScope(token({ device_id: "", device_ids: ["device-1", "device-2"] }))).toBe("fleet");
  });

  it("calls a token with one device_id a device token", () => {
    expect(tokenScope(token())).toBe("device");
  });

  it("uses the kind the console wrote, even before the scope shape is read", () => {
    expect(tokenScope(token({ kind: "fleet", device_id: "device-1" }))).toBe("fleet");
    expect(tokenScope(token({ kind: "device", device_id: "" }))).toBe("device");
  });

  it("says unknown rather than guessing a door neither fact proves", () => {
    expect(tokenScope(token({ device_id: "" }))).toBe("unknown");
  });
});

describe("scopeLabel", () => {
  it("says this machine only for the sheet's own machine", () => {
    expect(scopeLabel(token(), MACHINES, "device-1").label).toBe("this machine");
  });

  it("names the other machine a device token was minted for", () => {
    expect(scopeLabel(token({ device_id: "device-2" }), MACHINES, "device-1").label).toBe("attic");
  });

  it("names the machines an aggregate token was minted for, not 'all machines'", () => {
    const fleet = token({ device_id: "", device_ids: ["device-1", "device-2", "device-3"] });
    const words = scopeLabel(fleet, MACHINES, "device-1");

    expect(words.label).toBe("workshop, attic, device-3");
    expect(words.label).not.toBe("all machines");
    // `/mcp/all` enforces the recorded ids literally; the title carries the full list and the rule.
    expect(words.title).toContain("Covers 3 machines: workshop, attic, device-3.");
    expect(words.title).toContain("enrolled after");
  });

  it("names one machine, not one machines", () => {
    const words = scopeLabel(token({ device_id: "", device_ids: ["device-1"] }), MACHINES);
    expect(words.label).toBe("workshop");
    expect(words.title).toContain("Covers 1 machine: workshop.");
  });

  it("caps a long list at three names and says how many more the title holds", () => {
    const fleet = token({
      device_id: "",
      device_ids: ["device-1", "device-2", "device-3", "device-4", "device-5"]
    });
    const words = scopeLabel(fleet, MACHINES, null);

    expect(words.label).toBe("workshop, attic, device-3 and 2 more");
    expect(words.title).toContain("Covers 5 machines:");
    expect(words.title).toContain("device-4");
  });

  it("falls back to the short id for a machine the fleet does not know", () => {
    const fleet = token({ device_id: "", device_ids: ["retired-device-0123456789"] });
    const words = scopeLabel(fleet, MACHINES, null);

    expect(words.label).toBe("retired-devi…");
  });

  it("prints the door before the reach: device · /mcp/<short> or fleet · /mcp/all", () => {
    expect(scopeLabel(token(), MACHINES, "device-1").door).toBe("device · /mcp/device-1");
    const fleet = token({ device_id: "", device_ids: ["device-1", "device-2"] });
    expect(scopeLabel(fleet, MACHINES).door).toBe("fleet · /mcp/all");
  });

  it("uses the written kind's door even when the scope shape is absent", () => {
    expect(scopeLabel(token({ kind: "fleet", device_id: "" })).door).toBe("fleet · /mcp/all");
  });

  it("says unknown, never a door, when neither the kind nor the scope proves one", () => {
    const words = scopeLabel(token({ device_id: "" }), MACHINES);

    expect(words.door).toBe("unknown · endpoint not recorded");
    expect(words.label).toBe("scope not recorded");
  });
});

describe("mergeTokenRows", () => {
  it("merges the registry and the created row by creation time, newest first", () => {
    const device = token({ id: "device", created_at_ms: 1_000 });
    const fleet = token({
      id: "fleet",
      name: "fleet",
      device_id: "",
      device_ids: ["device-1"],
      created_at_ms: 2_000
    });
    expect(mergeTokenRows([device], [fleet]).map((entry) => entry.id)).toEqual(["fleet", "device"]);
  });

  it("lists a record that both the read and the created row carry once, keeping the read's copy", () => {
    const fromDevice = token({ name: "from device" });
    const fromAll = token({ name: "from all" });
    const merged = mergeTokenRows([fromDevice], [fromAll]);
    expect(merged).toHaveLength(1);
    expect(merged[0].name).toBe("from device");
  });

  it("has a stable order when two tokens share a creation millisecond", () => {
    const later = token({ id: "b", created_at_ms: 5_000 });
    const earlier = token({ id: "a", created_at_ms: 5_000 });
    expect(mergeTokenRows([later], [earlier]).map((entry) => entry.id)).toEqual(["a", "b"]);
  });
});
