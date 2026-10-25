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
export type AuditLine = {
  ts_ms: number;
  kind: string;
  device_id?: string | null;
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
    })
};

export { HttpError };
