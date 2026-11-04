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
});

describe("scopeLabel", () => {
  it("says this machine only for the sheet's own machine", () => {
    expect(scopeLabel(token(), MACHINES, "device-1").label).toBe("this machine");
  });

  it("names the other machine a device token was minted for", () => {
    expect(scopeLabel(token({ device_id: "device-2" }), MACHINES, "device-1").label).toBe("attic");
  });

  it("counts the machines an aggregate token was minted for, not 'all machines'", () => {
    const fleet = token({ device_id: "", device_ids: ["device-1", "device-2", "device-3"] });
    const words = scopeLabel(fleet, MACHINES, "device-1");

    expect(words.label).toBe("3 machines it was minted for");
    expect(words.label).not.toBe("all machines");
    // `/mcp/all` enforces the recorded ids literally; the title is what stops the count reading as
    // "every machine, including one enrolled tomorrow".
    expect(words.title).toContain("enrolled after");
  });

  it("says one machine, not one machines", () => {
    expect(scopeLabel(token({ device_id: "", device_ids: ["device-1"] })).label).toBe(
      "1 machine it was minted for"
    );
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
