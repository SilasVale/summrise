// The console's HTTP surface, typed. Every call carries the session token once there is one; the
// Cloudflare Access path needs no token, and /session says which of the two answered.

export type Session = {
  authenticated: boolean;
  email?: string;
  via?: string;
  /// What the console made of the Cloudflare Access identity on this request: whether the trust is
  /// configured, whether the header arrived, and why it was refused. The page draws a login box when
  /// none of it is an identity, and these three facts are the sentence it can show instead of one.
  access?: { configured: boolean; header: boolean; error?: string };
};
/**
 * One baseline generation the launcher wrote: the version, and the channel it came from.
 *
 * The device reads it from `<install>/baseline/<generation>/baseline.json` for display — what the
 * launcher wrote, not a signature check the console makes. A missing or half-written sidecar arrives
 * as `null`, never as a partial object.
 */
export type UpdateGeneration = { version?: string; channel?: string };

/**
 * The launcher's last swap, from `<install>/swap.result.json`.
 *
 * `applied` is a pair that took over, `rolled_back` a pair that failed and returned to `previous`,
 * and `refused` a swap the launcher declined before it touched anything — a refusal is not an
 * update. The device reports `null` for a missing, torn or unrecognised result, and leaves the key
 * out on a device too old to report it.
 */
export type UpdateSwap = {
  outcome: "applied" | "rolled_back" | "refused";
  version?: string;
  at_ms: number;
  reason?: string;
};

/**
 * One restore the launcher recorded in `<install>/repair.log`: the app copy no longer matched the
 * generation it came from, so the launcher rewrote it. `from` is the generation the bytes came from;
 * `sha256` is the launcher's hash of them, kept out of the visible text.
 */
export type UpdateRepair = { at_ms: number; from: "baseline" | "previous"; sha256: string };

/** What the device sent about its machine on the channel: the console shows it without a grant. */
export type HostFacts = {
  os?: { name?: string; version?: string; arch?: string };
  machine?: string;
  uptime_s?: number;
  cpu?: { cores?: number };
  memory?: { total_bytes?: number; available_bytes?: number };
  disks?: { name: string; total_bytes: number; free_bytes: number }[];
  sessions?: { id: number; user?: string | null; state: string }[];
  helper?: { registered?: boolean; pid?: number | null; session?: number | null };
  /// The launcher's three baseline generations: `current` is the pair it trusts, `pending` is a pair
  /// staged and not yet proven, `previous` is the pair a failed update returns to. Each generation is
  /// `null` when its sidecar is missing or unreadable, so a device with no baselines reports three
  /// nulls; the whole key is absent on a device too old to report it.
  update?: {
    current?: UpdateGeneration | null;
    previous?: UpdateGeneration | null;
    pending?: UpdateGeneration | null;
    /// The launcher's last swap result. `null` when the file is missing, torn or names an outcome
    /// this console does not know — never an inferred restart.
    last_swap?: UpdateSwap | null;
    /// The launcher's newest restores, newest first and at most five; empty when it never repaired
    /// anything (or the log is missing), and absent on a device too old to report it.
    repairs?: UpdateRepair[];
  } | null;
};

/** A saved terminal preset: the shape of a connection, never its credential. */
export type SavedConnection = {
  id: string;
  name: string;
  kind: string;
  host?: string;
  port?: number;
  user?: string;
  port_path?: string;
  baud?: number;
};

export type Device = {
  id: string;
  name: string | null;
  online: boolean;
  version: string | null;
  last_seen_ms: number | null;
  registered_at_ms?: number | null;
  last_attempt?: { attempts: number; reason: string } | null;
  host?: HostFacts | null;
};
/**
 * One permanent token's metadata, as the console lists it: never the token itself.
 *
 * The plaintext `srm_…` is returned once by `createToken` and exists nowhere else — the console keeps
 * only its hash, and this list is what the page can show afterwards.
 */
export type TokenMeta = {
  id: string;
  name: string;
  device_id: string;
  tools: string[];
  allow_destructive: boolean;
  created_at_ms: number;
  /**
   * When this token last authenticated an MCP call, as the console recorded it; absent or `null` for
   * a token that has never been used. The console persists it at most once a minute, so a reload can
   * lag a use by under a minute — the fact is still live in memory on the device.
   */
  last_used_ms?: number | null;
};
export type AuditLine = {
  ts_ms: number;
  kind: string;
  device_id?: string | null;
  /**
   * The record id (64 lowercase hex) of the permanent token that authenticated this call. Absent or
   * `null` for a grant-authenticated call and for every action that is not a token's MCP call; the
   * console never writes the `srm_` value here.
   */
  token_id?: string | null;
  tool?: string | null;
  detail?: string | null;
  status: number;
};

let token: string | null = null;
export function setToken(next: string | null) {
  token = next;
}
export function getToken(): string {
  return token ?? "";
}

/// The console did not answer with JSON: the edge answered instead — an Access login page after the
/// session lapsed, or a proxy's error page. The page reloads once through Access rather than telling
/// the operator to find a token.
export class NoAnswer extends Error {}

class HttpError extends Error {
  constructor(
    readonly status: number,
    readonly code: string
  ) {
    super(code);
  }
}

async function call<T>(path: string, init: RequestInit = {}): Promise<T> {
  const headers: Record<string, string> = {};
  if (init.body !== undefined) headers["content-type"] = "application/json";
  if (token) headers.authorization = `Bearer ${token}`;
  const response = await fetch(path, { ...init, headers });
  const text = await response.text();
  let body: T | { error?: string } | null;
  try {
    body = text ? (JSON.parse(text) as T | { error?: string }) : null;
  } catch {
    throw new NoAnswer(path);
  }
  if (!response.ok) {
    const code = (body as { error?: string } | null)?.error ?? response.statusText;
    throw new HttpError(response.status, code);
  }
  return body as T;
}

export const api = {
  session: () => call<Session>("/session"),
  signIn: (credential: { token: string } | { code: string }) =>
    call<{ token: string; expires_at_ms: number }>("/auth", {
      method: "POST",
      body: JSON.stringify(credential)
    }),
  // `/fleet`, not `/devices`: the device namespace is behind Cloudflare Access's bypass (a
  // device has no email login), and a bypassed request carries no identity for the page to use.
  devices: () => call<Device[]>("/fleet"),
  audit: (limit = 20) => call<AuditLine[]>(`/audit?limit=${limit}`),
  health: () => call<{ version: string; devices: number }>("/health"),
  tools: () => call<{ name: string }[]>("/tools"),
  rename: (id: string, name: string | null) =>
    call<{ id: string; name: string | null }>(`/fleet/${id}/name`, {
      method: "POST",
      body: JSON.stringify({ name })
    }),
  remove: (id: string) => call<void>(`/fleet/${id}`, { method: "DELETE" }),
  connections: (id: string) => call<SavedConnection[]>(`/fleet/${id}/connections`),
  saveConnection: (id: string, draft: Omit<SavedConnection, "id">) =>
    call<SavedConnection>(`/fleet/${id}/connections`, {
      method: "POST",
      body: JSON.stringify(draft)
    }),
  removeConnection: (id: string, connection: string) =>
    call<void>(`/fleet/${id}/connections/${connection}`, { method: "DELETE" }),
  enroll: () => call<{ code: string; expires_at_ms: number }>("/enroll", { method: "POST" }),
  grant: (
    device_id: string,
    tools: string[],
    expires_in_s: number,
    allow_destructive: boolean
  ) =>
    call<{ grant: unknown; signature: string }>("/grants", {
      method: "POST",
      body: JSON.stringify({ device_id, tools, expires_in_s, allow_destructive })
    }),
  // Permanent tokens: the create answer is the only one that carries the plaintext, so it is the one
  // shape that carries `token`. Every list entry is metadata, and a revoke is a bare DELETE.
  tokens: () => call<TokenMeta[]>("/tokens"),
  createToken: (draft: {
    name: string;
    device_id: string;
    tools: string[];
    allow_destructive: boolean;
  }) =>
    call<TokenMeta & { token: string }>("/tokens", {
      method: "POST",
      body: JSON.stringify(draft)
    }),
  revokeToken: (id: string) => call<void>(`/tokens/${id}`, { method: "DELETE" })
};

export { HttpError };


/** Whether the console can ask a mesh broker about devices, and which broker it would ask. */
export type MeshStatus = { configured: boolean; broker: string | null };

/**
 * What the console learned about one device's mesh registration: no broker configured, a broker answer,
 * or a typed refusal. The readiness route contacts the broker, never the device.
 */
export type MeshReadiness =
  | { configured: true; reachable: boolean; candidates: number }
  | { configured: false }
  | { error: string };

export function meshStatus(): Promise<MeshStatus> {
  return call<MeshStatus>("/mesh");
}

export async function meshReadiness(deviceId: string): Promise<MeshReadiness> {
  try {
    return await call<{ configured: true; reachable: boolean; candidates: number }>(
      `/fleet/${encodeURIComponent(deviceId)}/mesh`
    );
  } catch (error) {
    const code = error instanceof HttpError ? error.code : "mesh_unavailable";
    if (code === "no_mesh_broker") return { configured: false };
    return { error: code };
  }
}
