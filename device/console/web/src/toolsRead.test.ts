import { describe, expect, it } from "vitest";
import type { LiveTool } from "./api";
import { toolsReadState } from "./toolsRead";

function tool(name: string): LiveTool {
  return {
    name,
    description: "",
    inputSchema: { type: "object", properties: {} },
    source: "builtin",
    capability: name.split(".")[0],
    destructive: false
  };
}

const TOOLS = [tool("files.read"), tool("terminal.create")];

describe("toolsReadState", () => {
  it("is reading while an empty list is on the wire", () => {
    expect(toolsReadState({ tools: [], loading: true, error: null })).toBe("reading");
  });

  it("is failed when the read did not answer and left no list", () => {
    expect(
      toolsReadState({ tools: [], loading: false, error: "the tools read failed" })
    ).toBe("failed");
  });

  it("is ready once a list is in hand, even while a refresh is in flight", () => {
    expect(toolsReadState({ tools: TOOLS, loading: true, error: null })).toBe("ready");
  });

  it("is ready when the read answered with no tools: that zero is the machine's own", () => {
    expect(toolsReadState({ tools: [], loading: false, error: null })).toBe("ready");
  });
});
