// A terminal through the same channel as everything else: the page calls /mcp, the console proxies to
// the device, the device runs the shell. Nothing here has a second credential, and every keystroke is
// a line in the device's audit.

export type Grant = { deviceId: string; envelope?: unknown };

import { getToken } from "./api";

/**
 * A failed tool call, with the parts a page needs to tell its refusals apart.
 *
 * Three failures look identical if only the sentence is kept: a grant the console or the device
 * refused (`403` with a code, or a JSON-RPC error with code `-32001`/`-32002`), a transport that never
 * reached the device (`503`, `504`, a `fetch` rejection), and a tool-level refusal the device itself
 * sent (`isError: true` in a 200). `status`, `code` and `fromDevice` are what a page reads instead of
 * matching English; `message` stays the sentence a person reads.
 */
export class ToolError extends Error {
  constructor(
    message: string,
    /** The HTTP status, or 0 when the request never got an answer. */
    readonly status: number,
    /** The JSON-RPC code, when the console or the device answered with one. */
    readonly code: number | null = null,
    /** True when the sentence is the device's own tool refusal. */
    readonly fromDevice = false
  ) {
    super(message);
    this.name = "ToolError";
  }
}

/**
 * The sentence a page shows for a failed tool call.
 *
 * The device's own refusal is passed through verbatim: it names the fix (a grant, a missing helper, a
 * tab that is gone). A request that never reached the device gets a sentence that says so, because
 * `Failed to fetch` on its own reads like the device answered and refused, and the two ask the
 * operator for different things. A console refusal (a `403` with a code) is passed through as well.
 */
export function toolFailure(cause: unknown): string {
  const sentence = cause instanceof Error ? cause.message : String(cause);
  if (
    cause instanceof ToolError &&
    !cause.fromDevice &&
    (cause.status === 0 || cause.status === 503 || cause.status === 504)
  ) {
    return "the console never reached the device: " + sentence;
  }
  return sentence;
}

export async function callTool(grant: Grant, name: string, args: Record<string, unknown>): Promise<string> {
  let response: Response;
  try {
    response = await fetch("/mcp", {
      method: "POST",
      headers: { "content-type": "application/json", authorization: `Bearer ${getToken()}` },
      body: JSON.stringify({
        device_id: grant.deviceId,
        request: { jsonrpc: "2.0", id: 1, method: "tools/call", params: { name, arguments: args } }
      })
    });
  } catch (cause) {
    // The browser could not deliver the request at all: no status, no body.
    throw new ToolError(cause instanceof Error ? cause.message : String(cause), 0);
  }
  const text = await response.text();
  // A proxy failure comes back as text, not JSON. Say what came back instead of "Unexpected token F".
  let body: any = null;
  try {
    body = text ? JSON.parse(text) : null;
  } catch {
    throw new ToolError(text.slice(0, 200) || `HTTP ${response.status}`, response.status);
  }
  const content = body?.result?.content;
  const first = content?.[0]?.text;
  if (body?.result?.isError) {
    throw new ToolError(first ?? "the device refused", response.status, null, true);
  }
  if (body?.error) {
    // A JSON-RPC error is an object; `new Error(object)` renders as "[object Object]" and hides the
    // one thing worth reading. An ApiError body (`{error: "grant_expired"}`) is a string.
    if (typeof body.error === "string") throw new ToolError(body.error, response.status);
    const code = typeof body.error.code === "number" ? body.error.code : null;
    const message = typeof body.error.message === "string" ? body.error.message : JSON.stringify(body.error);
    throw new ToolError(message, response.status, code);
  }
  if (!response.ok) throw new ToolError(`${response.status} ${response.statusText}`, response.status);
  return first ?? "";
}
