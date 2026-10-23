// A terminal through the same channel as everything else: the page calls /mcp, the console proxies to
// the device, the device runs the shell. Nothing here has a second credential, and every keystroke is
// a line in the device's audit.

export type Grant = { deviceId: string; envelope: unknown };

import { getToken } from "./api";

export async function callTool(grant: Grant, name: string, args: Record<string, unknown>): Promise<string> {
  const response = await fetch("/mcp", {
    method: "POST",
    headers: { "content-type": "application/json", authorization: `Bearer ${getToken()}` },
    body: JSON.stringify({
      device_id: grant.deviceId,
      grant: grant.envelope,
      request: { jsonrpc: "2.0", id: 1, method: "tools/call", params: { name, arguments: args } }
    })
  });
  const text = await response.text();
  // A proxy failure comes back as text, not JSON. Say what came back instead of "Unexpected token F".
  let body: any = null;
  try {
    body = text ? JSON.parse(text) : null;
  } catch {
    throw new Error(text.slice(0, 200) || `HTTP ${response.status}`);
  }
  const content = body?.result?.content;
  const first = content?.[0]?.text;
  if (body?.result?.isError) throw new Error(first ?? "the device refused");
  if (body?.error) {
    // A JSON-RPC error is an object; `new Error(object)` renders as "[object Object]" and hides the
    // one thing worth reading.
    const detail = typeof body.error === "string" ? body.error : JSON.stringify(body.error);
    throw new Error(detail);
  }
  if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
  return first ?? "";
}
