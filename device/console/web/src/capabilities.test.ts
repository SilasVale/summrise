import { describe, expect, it } from "vitest";
import type { LiveTool } from "./api";
import { filterTools, groupByCapability, summariseSchema } from "./capabilities";

function tool(overrides: Partial<LiveTool> & { name: string }): LiveTool {
  return {
    description: "",
    inputSchema: { type: "object", properties: {} },
    source: "builtin",
    capability: overrides.name.split(".")[0],
    destructive: false,
    ...overrides
  };
}

const TOOLS: LiveTool[] = [
  tool({ name: "files.write", description: "write a file", destructive: true }),
  tool({ name: "files.read", description: "read a file" }),
  tool({ name: "terminal.create", description: "open a shell", destructive: true }),
  tool({ name: "host.facts", description: "read the machine" }),
  tool({ name: "runs.exec", description: "run a command", destructive: true }),
  tool({
    name: "hello.greet",
    description: "greet someone",
    source: "plugin:hello",
    capability: "hello"
  }),
  tool({
    name: "hello.echo",
    description: "echo text",
    source: "plugin:hello",
    capability: "hello"
  })
];

describe("groupByCapability", () => {
  it("orders the operator's built-ins first, then the rest, then plugins alphabetically", () => {
    const groups = groupByCapability(TOOLS);
    expect(groups.map((group) => group.name)).toEqual(["terminal", "files", "runs", "host", "hello"]);
    expect(groups[0].description).toBe("Run commands in a real shell.");
    const last = groups[groups.length - 1];
    expect(last.plugin).toBe("hello");
    expect(last.description).toBe("Tools from the hello plugin.");
  });

  it("counts the tools that can change the machine on the group line", () => {
    const files = groupByCapability(TOOLS).find((group) => group.name === "files");
    expect(files?.tools.map((entry) => entry.name)).toEqual(["files.read", "files.write"]);
    expect(files?.destructive).toBe(1);
  });

  it("is empty for an empty catalogue", () => {
    expect(groupByCapability([])).toEqual([]);
  });
});

describe("filterTools", () => {
  it("matches name, description and capability, case-insensitively", () => {
    expect(filterTools(TOOLS, "GREET").map((entry) => entry.name)).toEqual(["hello.greet"]);
    expect(filterTools(TOOLS, "shell").map((entry) => entry.name)).toEqual(["terminal.create"]);
    expect(filterTools(TOOLS, "hello").map((entry) => entry.name)).toEqual([
      "hello.greet",
      "hello.echo"
    ]);
  });

  it("returns the whole catalogue for a blank query", () => {
    expect(filterTools(TOOLS, "   ")).toBe(TOOLS);
  });
});

describe("summariseSchema", () => {
  const schema = {
    type: "object",
    properties: {
      path: { type: "string", description: "Where to read" },
      depth: { type: "integer" },
      recursive: { type: "boolean" },
      mode: { type: "string", enum: ["read", "write"] }
    },
    required: ["path"]
  };

  it("reads a flat object into typed, required-marked fields", () => {
    const summary = summariseSchema(schema);
    expect(summary.fields).toEqual([
      {
        name: "path",
        type: "string",
        required: true,
        description: "Where to read"
      },
      {
        name: "depth",
        type: "integer",
        required: false,
        description: ""
      },
      {
        name: "recursive",
        type: "boolean",
        required: false,
        description: ""
      },
      {
        name: "mode",
        type: "enum",
        required: false,
        description: ""
      }
    ]);
  });

  it("names the type of a property that is not one scalar control", () => {
    const nested = summariseSchema({
      type: "object",
      properties: { filter: { type: "object", properties: { name: { type: "string" } } } }
    });
    expect(nested.fields).toEqual([
      { name: "filter", type: "object", required: false, description: "" }
    ]);

    const array = summariseSchema({
      type: "object",
      properties: { ids: { type: "array", items: { type: "string" } } }
    });
    expect(array.fields[0].type).toBe("array");

    const untyped = summariseSchema({
      type: "object",
      properties: { anything: { oneOf: [{ type: "string" }, { type: "number" }] } }
    });
    expect(untyped.fields[0].type).toBe("unknown");
  });

  it("treats an empty object as a tool with no arguments", () => {
    const summary = summariseSchema({ type: "object", properties: {} });
    expect(summary.fields).toEqual([]);
  });
});
