/**
 * API client — thin wrapper around fetch with credentials: 'same-origin'
 * and automatic JSON parsing. 401 responses trigger a global auth reset
 * (via notifyUnauthorized, which lives in lib/unauthorized.ts) so the UI
 * switches to the login page.
 *
 * THIS FILE DOES NOT IMPORT A CONTEXT MODULE ANY MORE. It used to import
 * `notifyUnauthorized` from ../contexts/AuthContext.tsx while that file imported
 * `api` from here — a cycle, and underneath it a layering inversion: the data
 * layer reaching into the React layer to find a callback. Both sides depend on
 * the leaf module now, which is where the seam belongs.
 */

import { notifyUnauthorized } from "../lib/unauthorized.ts";

export class ApiError extends Error {
  status: number;
  data: unknown;
  constructor(status: number, message: string, data?: unknown) {
    super(message);
    this.name = "ApiError";
    this.status = status;
    this.data = data;
  }
}



/** A custom provider as the console sees it (server-reduced: never the raw key). */
export interface ProviderView {
  prefix: string;
  label: string;
  baseURL: string;
  api: string;
  models: {
    id: string;
    name?: string;
    contextWindow?: number;
    maxTokens?: number;
    reasoningEffort?: string;
    input?: string[];
  }[];
  /** Fully-qualified ids this provider contributes to /v1/models. */
  advertised: string[];
  /** Named Worker secret the record points at ("" when the key is inline). */
  keyEnv: string;
  keyMasked: string;
  /** The credential dot: true when a key resolves in THIS deployment. */
  keyReady: boolean;
}

/** The display/discovery facets a model may declare (never routing semantics). */
export interface ModelFacets {
  name?: string;
  contextWindow?: number;
  maxTokens?: number;
  /** Default when the client sends none; the client's own value always wins. */
  reasoningEffort?: string;
}

/** What a probe found. `notOffered` is a CHECK, never a verdict: the router
 *  normalises names (wire remaps, `[1m]` markers), so a raw diff reports false drift. */
export interface ProbeResult {
  checked: boolean;
  reason?: string;
  endpoint?: string;
  offered?: string[];
  advertised?: string[];
  notAdvertised?: string[];
  notOffered?: string[];
}

/** One model inside a provider record. */
export interface ProviderModelDraft {
  id: string;
  name?: string;
  contextWindow?: number;
  maxTokens?: number;
  reasoningEffort?: string;
  input?: string[];
}

/** What the add/edit form submits. Mirrors the server's parseProviderSpec. */
interface ProviderDraft {
  prefix: string;
  label?: string;
  baseURL: string;
  api: string;
  apiKey?: string;
  apiKeyEnv?: string;
  models: {
    id: string;
    name?: string;
    contextWindow?: number;
    maxTokens?: number;
    reasoningEffort?: string;
    input?: string[];
  }[];
}

async function request<T>(path: string, options?: RequestInit): Promise<T> {
  const res = await fetch(path, {
    headers: { "Content-Type": "application/json" },
    credentials: "same-origin",
    ...options,
  });
  let data: unknown = null;
  try {
    data = await res.json();
  } catch {
    /* noop */
  }
  if (res.status === 401) {
    notifyUnauthorized();
    throw new ApiError(401, "Unauthorized", data);
  }
  if (!res.ok) {
    const errData = data as { error?: { message: string } } | undefined;
    throw new ApiError(res.status, errData?.error?.message || res.statusText, data);
  }
  return data as T;
}

// ── Types ──────────────────────────────────────────────────────

export interface Me {
  username: string;
  role: "admin" | "user";
  token: string;
  keys: Record<string, { configured: boolean; masked: string } | undefined>;
}

export interface RouteInfo {
  prefix: string;
  backend: string;
  desc: string;
  models: string[];
}

interface PublicRouteInfo {
  routes: RouteInfo[];
  /**
   * THE AUTHORITATIVE CATALOGUE, and the only list safe to SET A ROUTE FROM.
   *
   * `routes[].models` holds BARE names per channel ("mimo-v2.5"), while routing is
   * by PREFIX — an unprefixed name goes to Command Code, not to the channel it was
   * listed under. Setting a route from `routes[].models` therefore silently
   * switched to a DIFFERENT channel than the one the user clicked in.
   *
   * This list is prefixed ids and is identical to what `/v1/models` serves.
   */
  models: string[];
  apiHost: string;
}

export interface HealthChannel {
  id: string;
  model: string;
  ok: boolean;
  reason?: string;
}

interface HealthResponse {
  channels: HealthChannel[];
}

export interface User {
  id: string;
  username: string;
  role: string;
  token: string;
  enabled: boolean;
}

export interface Device {
  name: string;
  hostname: string;
  token: string;
  registeredAt?: number;
  lastSeenAt?: number;
  lastVersion?: string;
}

export interface DeviceStatus {
  agent_up?: boolean;
  tunnel_up?: boolean;
  version?: string;
  /** Set only when the device's last run CRASHED (`/api/plugins/status`, round 256).
   *  The gateway drops the normal verdicts on purpose — the fleet marks exceptions,
   *  not every restart. Absent means "nothing to report", never "fine". */
  last_boot_kind?: string;
  /** The device's own sentence about that crash, for the row's tooltip. */
  last_boot?: string;
  /** THE DEVICE'S OWN UPDATE VERDICT (round 29 of the standing goal), forwarded by the gateway's probe instead of
   *  re-derived here. This page used to decide `lastVersion !== install.version` from the KV copy — which can be an
   *  hour old and knows nothing about a rollback pin — while the device answers the same question live, pin-aware.
   *  Absent for an agent older than the route; then the page falls back to the comparison. */
  update?: { current?: string; latest?: string; update_available: boolean; pinned_to: string | null };
  checked_at?: number;
}

export interface RegKeyInfo {
  code: string;
  expiresAt: number;
}

// ── API endpoints ──────────────────────────────────────────────

export const api = {
  // Auth
  me: () => request<Me>("/api/me"),
  login: (username: string, password: string) =>
    request<unknown>("/api/auth/login", {
      method: "POST",
      body: JSON.stringify({ username, password }),
    }),
  register: (username: string, password: string, inviteCode: string) =>
    request<unknown>("/api/auth/register", {
      method: "POST",
      body: JSON.stringify({ username, password, inviteCode }),
    }),
  resetPassword: (adminKey: string, newPassword: string) =>
    request<unknown>("/api/auth/reset-password", {
      method: "POST",
      body: JSON.stringify({ adminKey, newPassword }),
    }),
  logout: () => request<unknown>("/api/auth/logout", { method: "POST" }),

  // Route
  getRoute: () => request<{ model?: string; effective?: string }>("/api/me/route"),
  // ── Model catalogue (admin) — the catalogue is DATA now, not compiled code ──
  getModelState: () =>
    request<{
      custom: string[];
      disabled: string[];
      /** What each CONSOLE-OWNED model declares, by id. */
      facets?: Record<string, ModelFacets>;
      /** Ids the CONFIG FILE declares. The file wins for these, so a panel edit is reverted
       *  by the next deploy — the controls stay off, exactly as they do for `filePrefixes`. */
      fileModels?: string[];
      fileOverrides?: string[];
    }>("/api/admin/models"),
  addModel: (spec: {
    id: string;
    ownedBy?: string;
    wire?: string;
    usEgress?: boolean;
    search?: boolean;
    responsesOnly?: boolean;
    name?: string;
    contextWindow?: number;
    maxTokens?: number;
    /** Default when the client sends none; the client's own value always wins. */
    reasoningEffort?: "low" | "medium" | "high" | "max";
  }) =>
    request<{ ok: boolean }>("/api/admin/models", { method: "POST", body: JSON.stringify(spec) }),
  /** Built-ins are DISABLED, not deleted — their six facets cannot be re-derived. */
  deleteModel: (id: string) =>
    request<{ ok: boolean; removed?: string; disabled?: string }>(
      `/api/admin/models/${encodeURIComponent(id)}`,
      { method: "DELETE" },
    ),
  enableModel: (id: string) =>
    request<{ ok: boolean }>(`/api/admin/models/${encodeURIComponent(id)}/enabled`, {
      method: "PUT",
    }),

  /* ── custom PROVIDERS (admin) ──────────────────────────────────────────────
   * A provider is the layer UNDER a model: a prefix, an endpoint, a protocol and
   * a credential. Until these three methods existed the records were reachable
   * only by hand-editing KV — the panel is the writer now, so every guard the
   * server enforces (reserved prefix, non-https/private baseURL, unsupported
   * protocol, missing key) arrives as a 4xx FROM HERE rather than as a 502 on
   * the first request that happens to use the prefix.
   * `keyReady` is the signal the row's credential dot renders; the inline key is
   * never returned (the server reduces it to a mask). */
  /* Ask the upstream what it serves. `checked:false` is NOT an empty catalogue —
   * the server refuses to conflate "could not look" with "offers nothing", and the
   * reason travels with it. `notAdvertised` comes back PREFIXED, ready to adopt. */
  probeModels: (prefix: string) =>
    request<ProbeResult>("/api/admin/models/probe", {
      method: "POST",
      body: JSON.stringify({ prefix }),
    }),
  /** THE ESCAPE HATCH: the effective catalogue as the document `config/models.ts` takes.
   *  The server renders the text, so the console never assembles the shape itself. */
  getCatalogue: () =>
    request<{
      document: { providers: unknown[]; models: unknown[]; overrides: unknown[] };
      text: string;
      file: { providers: string[]; models: string[]; overrides: string[] };
    }>("/api/admin/catalogue"),
  getProviders: () =>
    request<{
      providers: ProviderView[];
      /** Wire protocols THIS build serves. */
      apis: string[];
      /** Prefixes the FILE declares — their panel controls are disabled. */
      filePrefixes: string[];
    }>(
      "/api/admin/providers",
    ),
  addProvider: (spec: ProviderDraft) =>
    request<{ ok: boolean; provider: ProviderView; providers: string[] }>(
      "/api/admin/providers",
      { method: "POST", body: JSON.stringify(spec) },
    ),
  deleteProvider: (prefix: string) =>
    request<{ ok: boolean; removed: string }>(
      `/api/admin/providers/${encodeURIComponent(prefix)}`,
      { method: "DELETE" },
    ),

  setRoute: (model: string | null) =>
    request<unknown>("/api/me/route", {
      method: "PUT",
      body: JSON.stringify({ model }),
    }),

  // US Proxy
  getUsProxy: () => request<{ enabled: boolean }>("/api/me/usproxy"),
  setUsProxy: (enabled: boolean) =>
    request<unknown>("/api/me/usproxy", {
      method: "PUT",
      body: JSON.stringify({ enabled }),
    }),

  // Keys
  saveKey: (name: string, value: string) =>
    request<{ masked: string }>("/api/me/keys", {
      method: "PUT",
      body: JSON.stringify({ name, value }),
    }),
  // Session-gated full-key read for the Keys page copy button (the /api/me
  // list only carries masked values — copying the mask was the bug).
  revealKey: (name: string) =>
    request<{ ok: boolean; name: string; value: string }>("/api/me/keys/reveal", {
      method: "POST",
      body: JSON.stringify({ name }),
    }),
  deleteKey: (name: string) =>
    request<unknown>(`/api/me/keys?name=${encodeURIComponent(name)}`, {
      method: "DELETE",
    }),
  testKey: (name: string) =>
    request<{ ok: boolean; status?: number; detail?: string }>("/api/me/keys/test", {
      method: "POST",
      body: JSON.stringify({ name }),
    }),
  usageKey: (name: string) =>
    request<{
      ok: boolean;
      label?: string;
      usage?: number;
      limit?: number | null;
      balance?: number;
      windows?: Record<
        string,
        { used?: number; limit?: number | null; remaining?: number; resetAt?: string }
      >;
      rateLimit?: { limit?: number; interval?: string };
      detail?: string;
    }>("/api/me/keys/usage", {
      method: "POST",
      body: JSON.stringify({ name }),
    }),

  // Public routes
  getPublicRoutes: () => request<PublicRouteInfo>("/api/admin/public"),

  // Health
  getHealth: () => request<HealthResponse>("/api/health"),

  // Token regen
  regenerateToken: () => request<{ token: string }>("/api/me/token/regenerate", { method: "POST" }),

  // Admin: password
  getAdminPassword: () => request<{ set: boolean }>("/api/admin/password"),
  setAdminPassword: (password: string) =>
    request<unknown>("/api/admin/password", {
      method: "PUT",
      body: JSON.stringify({ password }),
    }),

  // Admin: invite
  generateInvite: () => request<{ code: string }>("/api/admin/invite", { method: "POST" }),

  // Admin: users
  getUsers: () => request<{ users: User[] }>("/api/admin/users"),
  setEnabled: (id: string, enabled: boolean) =>
    request<unknown>(`/api/admin/users/${encodeURIComponent(id)}/enabled`, {
      method: "PUT",
      body: JSON.stringify({ enabled }),
    }),

  // Admin: Cloudflare token
  getCfToken: () =>
    request<{ configured: boolean; masked?: string }>("/api/admin/cloudflare-token"),
  setCfToken: (token: string) =>
    request<unknown>("/api/admin/cloudflare-token", {
      method: "PUT",
      body: JSON.stringify({ token }),
    }),

  // Devices
  getDevices: () => request<{ devices: Device[] }>("/api/devices"),
  saveDevice: (name: string, hostname: string, token: string) =>
    request<unknown>("/api/devices", {
      method: "POST",
      body: JSON.stringify({ name, hostname, token }),
    }),
  renameDevice: (oldName: string, newName: string, hostname?: string) =>
    request<unknown>(`/api/devices/${encodeURIComponent(oldName)}/rename`, {
      method: "POST",
      body: JSON.stringify({ name: newName, ...(hostname ? { hostname } : {}) }),
    }),
  deleteDevice: (name: string) =>
    request<unknown>(`/api/devices/${encodeURIComponent(name)}`, { method: "DELETE" }),
  getDeviceMcp: (name: string) =>
    request<{ mcp: { json: string } }>(`/api/devices/${encodeURIComponent(name)}/mcp`),

  // Devices: one-time panel grant (admin) — mints a 120s single-use code the
  // agent redeems with its own token, so the PERMANENT device token never
  // rides in a panel URL (history/address bar/logs/referer).
  openDevicePanel: (name: string) =>
    request<{ ok: boolean; url: string }>(`/api/devices/${encodeURIComponent(name)}/panel-grant`, {
      method: "POST",
    }),

  // Devices: registration key
  generateRegKey: () => request<{ key: string }>("/api/devices/register-key", { method: "POST" }),
  listRegKeys: () => request<{ keys: RegKeyInfo[] }>("/api/devices/register-keys"),
  revokeRegKey: (code: string) =>
    request<unknown>(`/api/devices/register-keys/${encodeURIComponent(code)}`, {
      method: "DELETE",
    }),

  // Devices: current install version (null → UI falls back to its constant)
  getInstallCmd: () =>
    request<{ ok: boolean; version: string | null; download: string | null }>(
      "/api/devices/install-cmd",
    ),

  // Plugins: status (fresh=1 bypasses the gateway's 30s probe cache)
  getPluginStatus: (fresh?: boolean) =>
    request<{ devices: Record<string, DeviceStatus> }>(
      fresh ? "/api/plugins/status?fresh=1" : "/api/plugins/status",
    ),
};
