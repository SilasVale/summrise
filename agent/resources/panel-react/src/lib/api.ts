// Transport layer — migrated from the original panel.js callApi/callTool.
// Every device request carries Authorization: Bearer <token>; a 401 triggers
// the onUnauthorized callback (the UI drops back to the connection form).

let hostname = "";
let token = "";
let onUnauthorizedCb: (() => void) | null = null;

export function initTransport(host: string, tok: string, on401: () => void) {
  hostname = host;
  token = tok;
  onUnauthorizedCb = on401;
}
export function getHost() {
  return hostname;
}
export function getToken() {
  return token;
}

/**
 * DID THE DEVICE REFUSE THIS? — ONE predicate, because eleven call sites were asking it in two ways.
 *
 * The device answers a refusal as `200 + {ok:false,error}`, and that shape is a CONTRACT rather than an
 * oversight: `gateway/src/mcp.ts` records it in a comment and compensates with `!ok || data.ok === false`,
 * and the console re-checks it too. So the client is where the checking belongs — and it was being done
 * with `ok !== true` in nine places and `ok === false` in two, dialects that DISAGREE on a body carrying
 * no `ok` at all. The strict form is the correct one: the device always sends `ok: true` on success
 * (measured in round 69, when three fixtures that omitted it turned out to be the unrealistic part, not
 * the hook that refused them).
 *
 * AND A TEST THAT MOCKS THIS MODULE WHOLESALE WILL SILENTLY BREAK ON THE NEXT EXPORT ADDED HERE. Eleven
 * test files still write `vi.mock("../../lib/api", () => ({ callApi: vi.fn() }))`, which replaces every
 * export; adding this predicate turned three passing tests red with an EMPTY VALUE rather than an error,
 * because the hook caught the resulting TypeError as a failed read — including two whose fixtures carry
 * `ok: true`. Mock it by SPREADING the original (`async (importOriginal) => ({ ...(await
 * importOriginal()), callApi: vi.fn() })`, which eight files already do) so a new export cannot go
 * missing. `tsc` does not check a factory against the module.
 *
 * THIS IS NOT THE SAME QUESTION AS `res.ok` — that is the HTTP status, and `callApi` above already
 * handles it — nor as the `ok` of a NESTED object (a screenshot status inside a response), which asks
 * about a different thing again. Those distinctions are why this file is where the rule lives and why
 * the panel's readers are being migrated to it one dialect at a time.
 */
export function deviceRefused(j: any): boolean {
  return !(j !== null && typeof j === "object" && j.ok === true);
}

/** Fetch a path on the device with the bearer token. Returns parsed JSON. */
export async function callApi(
  path: string,
  init: RequestInit = {},
): Promise<any> {
  // round-107: the protocol was hardcoded https:// — the loopback on-device
  // panel (http://127.0.0.1:18080/panel) could never reach the agent.
  const proto = window.location.protocol === "http:" ? "http:" : "https:";
  const res = await fetch(`${proto}//${hostname}${path}`, {
    ...init,
    signal: AbortSignal.timeout(30_000),
    headers: {
      "content-type": "application/json",
      authorization: `Bearer ${token}`,
      ...(init.headers || {}),
    },
  });
  if (res.status === 401) {
    onUnauthorizedCb?.();
    throw new Error("unauthorized");
  }
  // round-86: a transient 502/500 must THROW — the old code returned the
  // error body as a successful result, so the poll mapped its characters
  // into junk sessions (sid: undefined) and tombstoned the list.
  if (!res.ok) {
    // round-86: a transient 502/500 must THROW — the old code returned the
    // error body as a successful result, so the poll mapped its characters
    // into junk sessions (sid: undefined) and tombstoned the list.
    // Agent error bodies ({ok:false,error}) carry the actionable message
    // (e.g. a playwright spawn failure) — surface it verbatim when the body
    // is parseable JSON; otherwise keep the HTTP status.
    let detail = "";
    try {
      const body = await res.text();
      if (body) {
        const j = JSON.parse(body);
        if (j && typeof j.error === "string") detail = j.error;
      }
    } catch {
      /* non-JSON error body — the HTTP status is the message */
    }
    throw new Error(detail || `HTTP ${res.status}`);
  }
  const text = await res.text();
  try {
    return JSON.parse(text);
  } catch {
    return text;
  }
}

/** Call a device tool (POST /api/tools/{name}). */
export async function callTool(
  name: string,
  body: Record<string, unknown> = {},
): Promise<any> {
  const res = await callApi(`/api/tools/${name}`, {
    method: "POST",
    body: JSON.stringify(body),
  });
  // The agent returns tool errors as HTTP 200 + {ok:false,error:...}.
  if (deviceRefused(res)) throw new Error(res?.error || `tool ${name} failed`);
  if (res && typeof res === "object" && "result" in res) return res.result;
  return res;
}
