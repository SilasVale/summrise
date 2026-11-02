import { callTool, ToolError, type Grant } from "./terminal";

/**
 * A plugin as `plugins.list` describes it. The JSON contract is fixed by the plan; every field is
 * read defensively because the page must survive a device that answers an older or partial shape.
 */
export type PluginState = "running" | "starting" | "stopped" | "failed";

export type PluginRecord = {
  id: string;
  namespace: string;
  name: string;
  version: string;
  description: string;
  state: PluginState;
  tools: number;
  path: string;
  last_error: string | null;
};

/** A `plugins.*` refusal: the product's `{code, message, action}` shape, parsed from the tool text. */
export type PluginFailure = { code: string | null; message: string; action: string | null };

/** Parse the JSON a tool returns as text; anything that is not JSON is not a plugin list. */
export function parseToolJson<T>(text: string): T | null {
  try {
    return JSON.parse(text) as T;
  } catch {
    return null;
  }
}

const STATES: readonly PluginState[] = ["running", "starting", "stopped", "failed"];

function record(value: unknown): PluginRecord | null {
  if (value === null || typeof value !== "object") return null;
  const raw = value as Record<string, unknown>;
  const state = String(raw.state ?? "stopped") as PluginState;
  return {
    id: String(raw.id ?? ""),
    namespace: String(raw.namespace ?? ""),
    name: String(raw.name ?? raw.namespace ?? ""),
    version: String(raw.version ?? ""),
    description: String(raw.description ?? ""),
    state: STATES.includes(state) ? state : "stopped",
    tools: Number.isFinite(Number(raw.tools)) ? Number(raw.tools) : 0,
    path: String(raw.path ?? ""),
    last_error: raw.last_error == null ? null : String(raw.last_error)
  };
}

/** The operator grant every plugin tool call uses: the page's own session on the chosen machine. */
function grantFor(deviceId: string): Grant {
  return { deviceId, envelope: null };
}

function pluginList(text: string): PluginRecord[] {
  const parsed = parseToolJson<unknown>(text);
  if (Array.isArray(parsed)) {
    return parsed.map(record).filter((entry): entry is PluginRecord => entry !== null);
  }
  // A device that wraps the list once is still readable; anything else is a broken answer.
  if (parsed !== null && typeof parsed === "object") {
    const wrapped = (parsed as { plugins?: unknown }).plugins;
    if (Array.isArray(wrapped)) {
      return wrapped.map(record).filter((entry): entry is PluginRecord => entry !== null);
    }
  }
  throw new Error("the machine answered plugins.list without a list");
}

export async function listPlugins(deviceId: string): Promise<PluginRecord[]> {
  return pluginList(await callTool(grantFor(deviceId), "plugins.list", {}));
}

/** What `plugins.add` answered: the new record and how many tools it contributed. */
export type AddedPlugin = { plugin: PluginRecord; tools: number };

export async function addPlugin(deviceId: string, path: string): Promise<AddedPlugin> {
  const text = await callTool(grantFor(deviceId), "plugins.add", { path });
  const parsed = parseToolJson<{ plugin?: unknown; tools?: unknown }>(text);
  const plugin = record(parsed?.plugin ?? null);
  if (!plugin) throw new Error("the machine added the plugin without returning its record");
  return { plugin, tools: Number(parsed?.tools) || plugin.tools };
}

export async function removePlugin(deviceId: string, namespace: string): Promise<void> {
  await callTool(grantFor(deviceId), "plugins.remove", { namespace });
}

export async function restartPlugin(deviceId: string, namespace: string): Promise<PluginRecord> {
  const text = await callTool(grantFor(deviceId), "plugins.restart", { namespace });
  const parsed = parseToolJson<{ plugin?: unknown }>(text);
  const plugin = record(parsed?.plugin ?? null);
  if (!plugin) throw new Error("the machine restarted the plugin without returning its record");
  return plugin;
}

/** What `plugins.inspect` answers: the manifest's identity, before anything is started. */
export type PluginSummary = {
  id: string;
  namespace: string;
  name: string;
  version: string;
  description: string;
};

/**
 * Read and validate a plugin directory without starting it. This is the add sheet's blur: `add` is a
 * destructive commit, so it must not be the thing that discovers a missing manifest.
 */
export async function inspectPlugin(deviceId: string, path: string): Promise<PluginSummary> {
  const text = await callTool(grantFor(deviceId), "plugins.inspect", { path });
  const parsed = parseToolJson<Record<string, unknown>>(text);
  if (!parsed || typeof parsed !== "object" || typeof parsed.namespace !== "string") {
    throw new Error("the machine answered plugins.inspect without a manifest");
  }
  return {
    id: String(parsed.id ?? ""),
    namespace: parsed.namespace,
    name: String(parsed.name ?? parsed.namespace),
    version: String(parsed.version ?? ""),
    description: String(parsed.description ?? "")
  };
}

export async function pluginLogs(
  deviceId: string,
  namespace: string,
  lines = 200
): Promise<string[]> {
  const text = await callTool(grantFor(deviceId), "plugins.logs", { namespace, lines });
  const parsed = parseToolJson<{ lines?: unknown }>(text);
  if (!parsed || !Array.isArray(parsed.lines)) {
    throw new Error("the machine answered plugins.logs without lines");
  }
  return parsed.lines.map((line) => String(line));
}

/**
 * The sentence and next action from a failed `plugins.*` call. The device sends
 * `{code, message, action}` as the tool text; a transport refusal is passed through as its sentence.
 */
export function pluginFailure(cause: unknown): PluginFailure {
  const raw = cause instanceof Error ? cause.message : String(cause);
  const parsed = parseToolJson<{ code?: unknown; message?: unknown; action?: unknown }>(raw);
  if (parsed && typeof parsed === "object") {
    const message = typeof parsed.message === "string" ? parsed.message : raw;
    return {
      code: typeof parsed.code === "string" ? parsed.code : null,
      message,
      action: typeof parsed.action === "string" ? parsed.action : null
    };
  }
  if (cause instanceof ToolError && !cause.fromDevice) {
    return { code: null, message: raw, action: null };
  }
  return { code: null, message: raw, action: null };
}
