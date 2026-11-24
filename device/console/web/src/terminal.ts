// A terminal through the same channel as everything else: the page calls /mcp, the console proxies to
// the device, the device runs the shell. Nothing here has a second credential, and every keystroke is
// a line in the device's audit.

/**
 * What a page needs to call a device.
 *
 * `envelope` is the legacy signed grant, or null for the operator's own session: the console's
 * `/mcp` route already treats a missing grant as the operator's full authority on the named device.
 * The page no longer mints a one-hour grant just to open a shell.
 */
export type Grant = { deviceId: string; envelope: unknown | null };

/** Whether the Console proved this call was not sent, or its outcome is still unknown. */
export type ToolDelivery = "not_sent" | "outcome_unknown" | "answered";

/** A failed exchange keeps HTTP status, delivery certainty and device result facts separate. */
export class ToolError extends Error {
  constructor(
    message: string,
    /** The HTTP status, or 0 when the browser did not receive an answer. */
    readonly status: number,
    /** The JSON-RPC code, when the console or the device answered with one. */
    readonly code: number | null = null,
    /** True when the sentence is the device's own tool refusal. */
    readonly fromDevice = false,
    readonly delivery: ToolDelivery = fromDevice || (status >= 400 && status < 500) ? "answered" : "outcome_unknown",
    /** Only a domain reader may validate and consume these structured device facts. */
    readonly resultData: unknown = undefined
  ) {
    super(message);
    this.name = "ToolError";
  }
}

/** A missing response never proves that a side effect did not run. */
export function toolFailure(cause: unknown): string {
  const sentence = cause instanceof Error ? cause.message : String(cause);
  if (cause instanceof ToolError && !cause.fromDevice) {
    if (cause.delivery === "not_sent") {
      return "the console did not send this call to the device: " + sentence;
    }
    if (cause.delivery === "outcome_unknown") {
      return "the call's outcome is unknown; the device may still be working. Check the record before trying again. " + sentence;
    }
  }
  return sentence;
}

const PRE_SEND_CODES = new Set(["device not connected", "device reconnecting", "device busy"]);

/** Safe recovery sentences are owned here; an arbitrary additive message is never echoed. */
function consoleFailure(code: string): string {
  switch (code) {
    case "device not connected": return "this machine is not connected; connect it, then try again";
    case "device reconnecting": return "this machine is reconnecting; try again in a moment";
    case "device busy": return "this machine is busy; try again in a moment";
    case "device disconnected": return "the device disconnected before the console heard the result";
    case "device timeout": return "the console stopped waiting for the device";
    case "device did not answer": return "the device accepted the request but did not answer before the console stopped waiting";
    default: return code;
  }
}

/**
 * What a call is besides its tool and arguments.
 *
 * `poll` marks the console's own automatic read — the Session card's list refresh, the terminal's
 * fallback read — so the console can leave it out of the audit and the live stream instead of
 * recording the console redrawing what it already shows. A person's action never sets it: the same
 * tool at a click is the page's own call and stays in the record. Only the operator route reads the
 * marker; an AI client's `/mcp/:id` body is its own JSON-RPC request and never carries it.
 */
export type CallOptions = { poll?: boolean };

/**
 * **THE EDGE'S ANSWER IS NOT A SENTENCE WE PRINT (the hundred-and-thirty-fifth list).** A non-JSON
 * answer used to be sliced to 200 characters and put straight into the operator's sentence: an
 * Access login page (or any proxy error page) made the Terminal read
 * `Could not read this device's sessions: <!doctype html> <html lang="en"> <head> <meta charset="utf-8"> …`.
 * The sixty-third list settled this for the console's own routes — a failure's sentence names the
 * subject, not the transport — and `api.call` already answers a non-JSON 200 with `NoAnswer`. This
 * route kept the body. Three shapes, three sentences:
 *
 *   - a web page (starts with `<`): who answered, and the status. The page is NOT quoted; nobody
 *     reads markup as a sentence, and the console's job is not to show the proxy's HTML.
 *   - one short line of text (<= 80 characters, no second line): the proxy's own sentence, kept —
 *     it is the only text that names what the proxy objects to.
 *   - anything else: the status, and that the answer was not JSON.
 */
function notJson(text: string, status: number): string {
  const trimmed = text.trim();
  if (trimmed.startsWith("<")) {
    return `the edge answered this call with a web page, not the console (HTTP ${status})`;
  }
  const firstLine = trimmed.split("\n")[0].trim();
  if (firstLine !== "" && trimmed.length <= 80) return firstLine;
  return `the answer was not JSON (HTTP ${status})`;
}

export async function callTool(
  grant: Grant,
  name: string,
  args: Record<string, unknown>,
  options: CallOptions = {}
): Promise<string> {
  let response: Response;
  try {
    response = await fetch("/mcp", {
      method: "POST",
      // The session cookie rides along; nothing here holds a credential.
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        device_id: grant.deviceId,
        // Omit the field entirely for a session grant: `/mcp` distinguishes absent (operator
        // authority) from present (a legacy grant it must verify).
        ...(grant.envelope === null ? {} : { grant: grant.envelope }),
        ...(options.poll ? { poll: true } : {}),
        request: { jsonrpc: "2.0", id: 1, method: "tools/call", params: { name, arguments: args } }
      })
    });
  } catch (cause) {
    // Losing the response does not prove that the request failed to reach the device.
    throw new ToolError(cause instanceof Error ? cause.message : String(cause), 0);
  }
  const text = await response.text();
  // A proxy failure comes back as text, not JSON.
  let body: any = null;
  try {
    body = text ? JSON.parse(text) : null;
  } catch {
    throw new ToolError(notJson(text, response.status), response.status);
  }
  const content = body?.result?.content;
  const first = typeof content?.[0]?.text === "string" ? content[0].text : undefined;
  if (body?.result?.isError) {
    throw new ToolError(first ?? "the device refused", response.status, null, true, "answered", body.result.structuredContent);
  }
  if (body?.error) {
    // String errors are the Console's stable codes. Delivery needs an explicit pre-send fact as
    // well: a status (or a familiar sentence from an edge) cannot establish whether a call ran.
    if (typeof body.error === "string") {
      const delivery: ToolDelivery = response.status === 503 && body.delivery === "not_sent" && PRE_SEND_CODES.has(body.error)
        ? "not_sent"
        : response.status >= 400 && response.status < 500 ? "answered" : "outcome_unknown";
      throw new ToolError(consoleFailure(body.error), response.status, null, false, delivery);
    }
    const code = typeof body.error.code === "number" ? body.error.code : null;
    const message = typeof body.error.message === "string" ? body.error.message : JSON.stringify(body.error);
    throw new ToolError(message, response.status, code, false, response.status < 500 ? "answered" : "outcome_unknown");
  }
  if (!response.ok) throw new ToolError(`${response.status} ${response.statusText}`, response.status);
  return first ?? "";
}
