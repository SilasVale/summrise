// The console's HTTP surface, typed. Every call carries the session token once there is one; the
// Cloudflare Access path needs no token, and /session says which of the two answered.

export type Session = { authenticated: boolean; email?: string; via?: string };
export type Device = {
  id: string;
  name: string | null;
  online: boolean;
  version: string | null;
  last_seen_ms: number | null;
  registered_at_ms?: number | null;
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
  const body = text ? (JSON.parse(text) as T | { error?: string }) : null;
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
  devices: () => call<Device[]>("/devices"),
  audit: (limit = 20) => call<AuditLine[]>(`/audit?limit=${limit}`),
  health: () => call<{ version: string; devices: number }>("/health"),
  rename: (id: string, name: string | null) =>
    call<{ id: string; name: string | null }>(`/devices/${id}/name`, {
      method: "POST",
      body: JSON.stringify({ name })
    }),
  remove: (id: string) => call<void>(`/devices/${id}`, { method: "DELETE" }),
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
