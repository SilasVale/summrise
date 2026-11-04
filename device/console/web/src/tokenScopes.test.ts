import { describe, expect, it } from "vitest";
import type { TokenMeta } from "./api";
import { mergeTokenRows, scopeLabel, tokenScope } from "./tokenScopes";

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
    expect(scopeLabel(token({ device_id: "", device_ids: ["device-1"] }))).toBe("all machines");
  });

  it("calls a token with one device_id a device token", () => {
    expect(tokenScope(token())).toBe("device");
    expect(scopeLabel(token())).toBe("this machine");
  });

  it("treats an empty device_ids list as the device scope, not a fleet token reaching nothing", () => {
    expect(tokenScope(token({ device_id: "", device_ids: [] }))).toBe("device");
  });
});

describe("mergeTokenRows", () => {
  it("merges both sources by creation time, newest first", () => {
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

  it("lists a record that both sources carry once, keeping the first source's copy", () => {
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
