import type { LiveTool } from "./api";

/**
 * The pure decisions behind the Capabilities catalogue: grouping, filtering and the schema
 * summariser. No React, no fetch; the component tests exercise the states and this module is where
 * the rules are provable.
 */

/** The operator's order for the built-in groups; anything unnamed follows alphabetically. */
const BUILTIN_ORDER = ["terminal", "files", "screen", "browser", "runs", "host", "plugins"];

const JOBS: Record<string, string> = {
  terminal: "Run commands in a real shell.",
  files: "Read, write and create files.",
  screen: "See and capture the machine's screen.",
  browser: "Drive a browser page.",
  runs: "Run a command and keep its result.",
  host: "Read the machine's facts and restart the app.",
  plugins: "Install and supervise plugin tools.",
  state: "Read and set state the AI can resume.",
  memory: "Remember facts across calls.",
  monitor: "Watch a port or path for a change.",
  system: "List and end processes, test a port."
};

export type CapabilityGroup = {
  name: string;
  source: string;
  plugin: string | null;
  description: string;
  tools: LiveTool[];
  destructive: number;
};

export function groupByCapability(tools: LiveTool[]): CapabilityGroup[] {
  const groups = new Map<string, LiveTool[]>();
  for (const tool of tools) {
    const name = tool.capability || tool.name.split(".")[0] || "other";
    const list = groups.get(name);
    if (list) list.push(tool);
    else groups.set(name, [tool]);
  }

  const built: CapabilityGroup[] = [];
  const plugins: CapabilityGroup[] = [];
  for (const [name, list] of groups) {
    const sorted = [...list].sort((left, right) => left.name.localeCompare(right.name));
    const source = sorted[0]?.source ?? "builtin";
    const plugin = source.startsWith("plugin:") ? source.slice("plugin:".length) : null;
    const group: CapabilityGroup = {
      name,
      source,
      plugin,
      description: JOBS[name] ?? (plugin ? `Tools from the ${plugin} plugin.` : `Tools in ${name}.`),
      tools: sorted,
      destructive: sorted.filter((tool) => tool.destructive).length
    };
    if (plugin) plugins.push(group);
    else built.push(group);
  }

  const rank = (name: string) => {
    const at = BUILTIN_ORDER.indexOf(name);
    return at >= 0 ? at : BUILTIN_ORDER.length;
  };
  built.sort((left, right) => rank(left.name) - rank(right.name) || left.name.localeCompare(right.name));
  plugins.sort((left, right) => left.name.localeCompare(right.name));
  return [...built, ...plugins];
}

/** Filter on name, description or capability. The capability name is what the group line shows. */
export function filterTools(tools: LiveTool[], query: string): LiveTool[] {
  const needle = query.trim().toLowerCase();
  if (needle === "") return tools;
  return tools.filter((tool) =>
    `${tool.name} ${tool.description} ${tool.capability}`.toLowerCase().includes(needle)
  );
}

export type FieldType = "string" | "number" | "integer" | "boolean" | "enum" | "array" | "object" | "unknown";

export type SchemaField = {
  name: string;
  type: FieldType;
  required: boolean;
  description: string;
};

export type SchemaSummary = {
  fields: SchemaField[];
};

type SchemaObject = {
  type?: unknown;
  properties?: unknown;
  required?: unknown;
  enum?: unknown;
  description?: unknown;
};

function object(value: unknown): Record<string, unknown> | null {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

function fieldType(property: SchemaObject): FieldType {
  if (Array.isArray(property.enum)) return "enum";
  const type = typeof property.type === "string" ? property.type : null;
  if (
    type === "string" ||
    type === "number" ||
    type === "integer" ||
    type === "boolean" ||
    type === "array" ||
    type === "object"
  ) {
    return type;
  }
  if (object(property.properties)) return "object";
  // A `oneOf`/`anyOf`/`$ref` or a schema with no type at all is not one control.
  return "unknown";
}

/** Summarise a tool's input schema as the fields the catalogue lists. */
export function summariseSchema(schema: unknown): SchemaSummary {
  const root = object(schema);
  const properties = object(root?.properties) ?? {};
  const required = Array.isArray(root?.required)
    ? root.required.filter((name): name is string => typeof name === "string")
    : [];

  const fields: SchemaField[] = Object.entries(properties).map(([name, value]) => {
    const property = object(value) ?? {};
    return {
      name,
      type: fieldType(property),
      required: required.includes(name),
      description: typeof property.description === "string" ? property.description : ""
    };
  });

  return { fields };
}
