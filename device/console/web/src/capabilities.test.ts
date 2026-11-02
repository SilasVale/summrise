import { describe, expect, it } from "vitest";
import type { LiveTool } from "./api";
import {
  buildArguments,
  exampleArguments,
  filterTools,
  formatResult,
  formArguments,
  groupByCapability,
  initialForm,
  summariseSchema,
  validateForm
} from "./capabilities";

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
    expect(summary.rawOnly).toBe(false);
    expect(summary.fields).toEqual([
      {
        name: "path",
        type: "string",
        required: true,
        description: "Where to read",
        enumValues: [],
        nested: false,
        example: undefined
      },
      {
        name: "depth",
        type: "integer",
        required: false,
        description: "",
        enumValues: [],
        nested: false,
        example: undefined
      },
      {
        name: "recursive",
        type: "boolean",
        required: false,
        description: "",
        enumValues: [],
        nested: false,
        example: undefined
      },
      {
        name: "mode",
        type: "enum",
        required: false,
        description: "",
        enumValues: ["read", "write"],
        nested: false,
        example: undefined
      }
    ]);
  });

  it("forces Raw JSON for a nested property or a schema with no flat fields", () => {
    const nested = summariseSchema({
      type: "object",
      properties: { filter: { type: "object", properties: { name: { type: "string" } } } }
    });
    expect(nested.rawOnly).toBe(true);
    expect(nested.reason).toBe("This tool takes a nested object; edit the JSON.");

    const array = summariseSchema({
      type: "object",
      properties: { ids: { type: "array", items: { type: "string" } } }
    });
    expect(array.rawOnly).toBe(true);

    const untyped = summariseSchema({
      type: "object",
      properties: { anything: { oneOf: [{ type: "string" }, { type: "number" }] } }
    });
    expect(untyped.rawOnly).toBe(true);
  });

  it("treats an empty object as a tool with no arguments", () => {
    const summary = summariseSchema({ type: "object", properties: {} });
    expect(summary.fields).toEqual([]);
    expect(summary.rawOnly).toBe(false);
  });
});

describe("initialForm, validation and arguments", () => {
  const summary = summariseSchema({
    type: "object",
    properties: {
      path: { type: "string" },
      limit: { type: "integer" },
      recursive: { type: "boolean" },
      mode: { type: "string", enum: ["read", "write"] }
    },
    required: ["path", "recursive"]
  });

  it("starts strings blank, booleans Not sent and raw mode off", () => {
    expect(initialForm(summary)).toEqual({
      values: { path: "", limit: "", mode: "" },
      booleans: { recursive: "omit" },
      raw: "{}",
      rawMode: false
    });
  });

  it("names the missing required field and focuses it", () => {
    const form = initialForm(summary);
    const errors = validateForm(summary, form);
    expect(errors.fields).toEqual({
      path: 'Add a value for "path".',
      recursive: 'Choose true or false for "recursive".'
    });
    expect(errors.first).toBe("path");

    const built = buildArguments(summary, form);
    expect(built).toEqual({ ok: false, message: 'Add a value for "path".' });
  });

  it("omits blank optional fields and sends only what was filled", () => {
    const form = initialForm(summary);
    form.values.path = "notes.txt";
    form.values.limit = "5";
    form.booleans.recursive = "false";

    expect(buildArguments(summary, form)).toEqual({
      ok: true,
      value: { path: "notes.txt", limit: 5, recursive: false }
    });
  });

  it("refuses a number that is not one, by field name", () => {
    const form = initialForm(summary);
    form.values.path = "notes.txt";
    form.booleans.recursive = "true";
    form.values.limit = "five";
    expect(formArguments(summary, form)).toEqual({
      ok: false,
      message: 'Enter a number for "limit".'
    });
  });

  it("edits raw JSON and refuses to run what does not parse", () => {
    const form = initialForm(summary);
    form.rawMode = true;
    form.raw = "{ nope";
    expect(validateForm(summary, form)).toEqual({
      fields: {},
      raw: "Fix the JSON before running.",
      first: "$raw"
    });

    form.raw = JSON.stringify({ path: "notes.txt", recursive: true });
    expect(buildArguments(summary, form)).toEqual({
      ok: true,
      value: { path: "notes.txt", recursive: true }
    });

    form.raw = "[]";
    expect(formArguments(summary, form)).toEqual({
      ok: false,
      message: "The arguments must be a JSON object."
    });
  });

  it("starts a nested schema in raw mode", () => {
    const nested = summariseSchema({
      type: "object",
      properties: { filter: { type: "object", properties: { q: { type: "string" } } } }
    });
    const form = initialForm(nested);
    expect(form.rawMode).toBe(true);
    expect(form.values).toEqual({});
  });
});

describe("exampleArguments", () => {
  it("promises only required fields, with the schema's own example when supplied", () => {
    expect(
      exampleArguments({
        type: "object",
        properties: {
          name: { type: "string", example: "Ada" },
          mode: { type: "string", enum: ["a", "b"] },
          quiet: { type: "boolean" },
          count: { type: "integer" },
          tags: { type: "array" }
        },
        required: ["name", "mode", "quiet", "count"]
      })
    ).toEqual({ name: "Ada", mode: "a", quiet: true, count: 0 });
  });
});

describe("formatResult", () => {
  it("formats JSON with two-space indentation and counts its lines", () => {
    expect(formatResult('{"a":1}')).toEqual({
      parsed: true,
      pretty: '{\n  "a": 1\n}',
      lines: 3
    });
  });

  it("passes a non-JSON answer through verbatim", () => {
    expect(formatResult("hello from the plugin")).toEqual({
      parsed: false,
      pretty: "hello from the plugin",
      lines: 1
    });
  });
});
