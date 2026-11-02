import type { LiveTool } from "./api";

/**
 * The pure decisions behind Capabilities and Try it: grouping, filtering, the schema summariser, the
 * form's values and the arguments it builds. No React, no fetch; the component tests exercise the
 * states and this module is where the rules are provable.
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
  enumValues: string[];
  /// A property the form cannot render as one control: the sheet goes straight to Raw JSON.
  nested: boolean;
  example: unknown;
};

export type SchemaSummary = {
  fields: SchemaField[];
  rawOnly: boolean;
  /// The sentence shown when the schema forces Raw JSON; null for a flat object.
  reason: string | null;
};

type SchemaObject = {
  type?: unknown;
  properties?: unknown;
  required?: unknown;
  enum?: unknown;
  description?: unknown;
  example?: unknown;
  default?: unknown;
  format?: unknown;
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

function sample(field: SchemaField): unknown {
  switch (field.type) {
    case "string":
      return "";
    case "number":
    case "integer":
      return 0;
    case "boolean":
      return true;
    case "enum":
      return field.enumValues[0] ?? "";
    case "array":
      return [];
    case "object":
    case "unknown":
      return {};
  }
}

/** Summarise a tool's input schema as the fields the form shows, and whether it can show a form. */
export function summariseSchema(schema: unknown): SchemaSummary {
  const root = object(schema);
  const properties = object(root?.properties) ?? {};
  const required = Array.isArray(root?.required)
    ? root.required.filter((name): name is string => typeof name === "string")
    : [];

  const fields: SchemaField[] = Object.entries(properties).map(([name, value]) => {
    const property = object(value) ?? {};
    const type = fieldType(property);
    const enumValues = Array.isArray(property.enum)
      ? property.enum.map((entry) => String(entry))
      : [];
    const example = property.example ?? property.default;
    return {
      name,
      type,
      required: required.includes(name),
      description: typeof property.description === "string" ? property.description : "",
      enumValues,
      nested: type === "array" || type === "object" || type === "unknown",
      example
    };
  });

  if (fields.length === 0) {
    const rootType = typeof root?.type === "string" ? root.type : "object";
    const rawOnly = rootType !== "object" && rootType !== "unknown";
    return {
      fields,
      rawOnly,
      reason: rawOnly ? "This tool takes a nested object; edit the JSON." : null
    };
  }
  const rawOnly = fields.some((field) => field.nested);
  return {
    fields,
    rawOnly,
    reason: rawOnly ? "This tool takes a nested object; edit the JSON." : null
  };
}

export type BoolChoice = "omit" | "true" | "false";

export type FormState = {
  values: Record<string, string>;
  booleans: Record<string, BoolChoice>;
  raw: string;
  rawMode: boolean;
};

/** Blank for every scalar (omitted when optional), booleans Not sent, raw `{}` for a nested schema. */
export function initialForm(summary: SchemaSummary): FormState {
  // A schema the form cannot render has one editor, not empty controls beside it.
  if (summary.rawOnly) return { values: {}, booleans: {}, raw: "{}", rawMode: true };
  const values: Record<string, string> = {};
  const booleans: Record<string, BoolChoice> = {};
  for (const field of summary.fields) {
    if (field.type === "boolean") booleans[field.name] = "omit";
    else values[field.name] = "";
  }
  return { values, booleans, raw: "{}", rawMode: false };
}

/// The fields' values as arguments, without enforcing required: this is also the Raw JSON serialise.
export type ArgumentsResult =
  | { ok: true; value: Record<string, unknown> }
  | { ok: false; message: string };

export function formArguments(summary: SchemaSummary, form: FormState): ArgumentsResult {
  if (form.rawMode) {
    let parsed: unknown;
    try {
      parsed = JSON.parse(form.raw);
    } catch {
      return { ok: false, message: "Fix the JSON before running." };
    }
    const value = object(parsed);
    if (!value) return { ok: false, message: "The arguments must be a JSON object." };
    return { ok: true, value };
  }

  const value: Record<string, unknown> = {};
  for (const field of summary.fields) {
    if (field.type === "boolean") {
      const choice = form.booleans[field.name] ?? "omit";
      if (choice !== "omit") value[field.name] = choice === "true";
      continue;
    }
    const raw = (form.values[field.name] ?? "").trim();
    if (raw === "") continue;
    if (field.type === "number" || field.type === "integer") {
      const number = Number(raw);
      if (!Number.isFinite(number)) {
        return { ok: false, message: `Enter a number for "${field.name}".` };
      }
      value[field.name] = field.type === "integer" ? Math.trunc(number) : number;
    } else {
      value[field.name] = form.values[field.name];
    }
  }
  return { ok: true, value };
}

export type FormErrors = {
  /// One sentence per field, keyed by the property name.
  fields: Record<string, string>;
  /// The Raw JSON editor's one error, when it is the thing that must be fixed.
  raw: string | null;
  /// The first thing to focus: a property name, or `"$raw"` for the editor.
  first: string | null;
};

export function validateForm(summary: SchemaSummary, form: FormState): FormErrors {
  const fields: Record<string, string> = {};
  if (form.rawMode) {
    const result = formArguments(summary, form);
    return result.ok
      ? { fields, raw: null, first: null }
      : { fields, raw: result.message, first: "$raw" };
  }
  for (const field of summary.fields) {
    if (!field.required) continue;
    if (field.type === "boolean") {
      if ((form.booleans[field.name] ?? "omit") === "omit") {
        fields[field.name] = `Choose true or false for "${field.name}".`;
      }
    } else if ((form.values[field.name] ?? "").trim() === "") {
      fields[field.name] = `Add a value for "${field.name}".`;
    }
  }
  const first = Object.keys(fields)[0] ?? null;
  return { fields, raw: null, first };
}

/** Validate, then build the arguments the call will send. */
export function buildArguments(summary: SchemaSummary, form: FormState): ArgumentsResult {
  const errors = validateForm(summary, form);
  if (errors.first === "$raw") return { ok: false, message: errors.raw ?? "Fix the JSON before running." };
  if (errors.first) return { ok: false, message: errors.fields[errors.first] };
  return formArguments(summary, form);
}

/** An example object for the raw editor and for documentation; only required fields are promised. */
export function exampleArguments(schema: unknown): Record<string, unknown> {
  const summary = summariseSchema(schema);
  const value: Record<string, unknown> = {};
  for (const field of summary.fields) {
    if (!field.required) continue;
    value[field.name] = field.example !== undefined ? field.example : sample(field);
  }
  return value;
}

export type FormattedResult = { parsed: boolean; pretty: string; lines: number };

/** Format a tool's returned text as JSON when it parses; otherwise show it verbatim. */
export function formatResult(text: string): FormattedResult {
  try {
    const value = JSON.parse(text);
    const pretty = JSON.stringify(value, null, 2);
    return { parsed: true, pretty, lines: pretty.split("\n").length };
  } catch {
    return { parsed: false, pretty: text, lines: text.split("\n").length };
  }
}

/// The client-side guard on one Try it call: the device itself allows 120 s, and the page says what
/// that means rather than freezing the sheet.
export const CALL_TIMEOUT_MS = 120_000;
export const TIMEOUT_SENTENCE =
  "No answer after 2 minutes. The call may still be running; check Activity.";
