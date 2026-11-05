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

/**
 * One tool as the device's live `tools/list` advertises it, plus the console's two computed fields.
 * `source` is `"builtin"` or `"plugin:<namespace>"`; `capability` is the built-in group or the
 * plugin's namespace. This is the page's source of truth: the static registry is a fallback only.
 */
export type LiveTool = {
  name: string;
  description: string;
  inputSchema: unknown;
  source: string;
  capability: string;
  destructive: boolean;
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
  /** Empty on an aggregate token; use `device_ids` for its scope. */
  device_id: string;
  /** The devices an aggregate token may reach; absent on a device token. */
  device_ids?: string[];
  tools: string[];
  allow_destructive: boolean;
  created_at_ms: number;
  /**
   * When this token last authenticated an MCP call, as the console recorded it; absent or `null` for
   * a token that has never been used. The console persists it at most once a minute, so a reload can
   * lag a use by under a minute — the fact is still live in memory on the device.
   */
  last_used_ms?: number | null;
  /**
   * When this token stops authenticating, in Unix milliseconds; absent or `null` for a token created
   * without a lifetime — the default, and how every record written before the field existed reads.
   */
  expires_at_ms?: number | null;
};
/**
 * What a create or a rotation answers with: the record's metadata plus the one plaintext the console
 * ever returns. It is the only shape that carries `token` — the list never does, and the console keeps
 * only the hash afterwards, so a caller has exactly one chance to show it.
 */
export type Created = TokenMeta & { token: string };

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
  /**
   * The console's human sentence for this line, when it wrote one. The live stream always carries
   * it; a fetched window carries it since the console started describing its own index.
   */
  sentence?: string;
  /** How long the device took, when the console measured it (live lines only). */
  ms?: number;
  /**
   * Whether the device's answer was a success, when the console said so. A device's JSON-RPC error
   * is HTTP 200, so the status alone cannot tell a refusal from a result.
   */
  ok?: boolean;
  /** Whether this line arrived on the live stream rather than being read back from the audit file. */
  live?: boolean;
  /**
   * Who made a call the console proxied: `console` for the operator page's own `/mcp` calls and the
   * terminal stream's reads, `client` for an AI client on `/mcp/:id` or `/mcp/all`. Absent on an
   * operator action (a grant, a token, a sign-in) and on every line written before the field —
   * neither is a client's call.
   */
  caller?: "console" | "client";
  /** The evidence for this call, when the live stream carried it. */
  evidence?: Record<string, unknown>;
};

/// The console did not answer with JSON: the edge answered instead — an Access login page after the
/// session lapsed, or a proxy's error page. The page reloads once through Access rather than telling
/// the operator to find a token.
export class NoAnswer extends Error {}

class HttpError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
    /// The console's additive recovery sentence beside the stable machine code. Falls back to the
    /// code, so a caller that only reads `message` still gets something true.
    message?: string
  ) {
    super(message ?? code);
    this.name = "HttpError";
  }
}

async function call<T>(path: string, init: RequestInit = {}): Promise<T> {
  // NO AUTHORIZATION HEADER. The session is the console's HttpOnly cookie, which the browser
  // attaches on its own; a bearer held in a JavaScript variable was lost on every reload and cost
  // the operator the 64-character token again.
  const headers: Record<string, string> = {};
  if (init.body !== undefined) headers["content-type"] = "application/json";
  const response = await fetch(path, { ...init, headers });
  const text = await response.text();
  let body: T | { error?: string } | null;
  try {
    body = text ? (JSON.parse(text) as T | { error?: string }) : null;
  } catch {
    throw new NoAnswer(path);
  }
  if (!response.ok) {
    const failure = body as { error?: string; message?: string } | null;
    const code = failure?.error ?? response.statusText;
    throw new HttpError(response.status, code, failure?.message);
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
  // Ends the presented session on the server and, behind Cloudflare Access, names the edge's own
  // logout: clearing only the console cookie is undone by the next request the edge authenticates,
  // so the page follows `access_logout` when it is there. Best effort as the page signs out; a
  // failure is not a reason to keep the in-memory session alive in the tab.
  logout: () => call<{ access_logout: string | null }>("/logout", { method: "POST" }),
  // `/fleet`, not `/devices`: the device namespace is behind Cloudflare Access's bypass (a
  // device has no email login), and a bypassed request carries no identity for the page to use.
  devices: () => call<Device[]>("/fleet"),
  // The device's live catalogue. An empty list is a valid answer (an older build advertises no
  // plugin tools but still has built-ins); 503 means the device is offline, and the body carries the
  // recovery sentence beside its code.
  liveTools: (id: string) =>
    call<{ tools: LiveTool[] }>(`/fleet/${encodeURIComponent(id)}/tools`),
  // `device` narrows the read on the server, during the scan: the newest `limit` lines for that
  // machine, not the fleet's newest `limit` filtered afterwards. Omitting it keeps the fleet window.
  audit: (limit = 20, device?: string) =>
    call<AuditLine[]>(
      `/audit?limit=${limit}${device ? `&device=${encodeURIComponent(device)}` : ""}`
    ),
  // The refusal log is a separate file with the audit line's shape: same route rules, same limit, and
  // no token id because the caller never authenticated.
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
  // There is no narrower `/tokens/<device>` read: the console serves the whole registry and a bare
  // DELETE by id. The one credential list splits the registry by scope, which is the same answer
  // without a request that 405s.
  createToken: (draft: {
    name: string;
    tools: string[];
    allow_destructive: boolean;
    /**
     * The lifetime the new token gets, in seconds: the server accepts 3600 through 31 536 000. Omit
     * it for the default — a token that never expires; the request body then carries no such key.
     */
    expires_in_s?: number;
  } & (
    | { device_id: string; device_ids?: never }
    | { device_ids: string[]; device_id?: never }
  )) =>
    call<Created>("/tokens", {
      method: "POST",
      body: JSON.stringify(draft)
    }),
  // Rotation replaces the record with one of the same scope: the request names only the id, and the
  // answer is the create shape because the new value is returned once, here and nowhere else.
  rotateToken: (id: string) => call<Created>(`/tokens/${id}/rotate`, { method: "POST" }),
  revokeToken: (id: string) => call<void>(`/tokens/${id}`, { method: "DELETE" })
};

export { HttpError };
