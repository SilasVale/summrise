import { callTool, ToolError, type Grant } from "./terminal";
import type { HostFacts } from "./api";

/**
 * **A LIVE READ OF THE MACHINE'S FACTS (the hundred-and-seventy-third list).** Everything the Machine tab shows about the
 * hardware comes from the device's last FRAME — which can be hours old, and says so ("Last frame 5h ago … not a live read").
 * `host.facts` has been on the device all along and the console never called it; measured 2026-11-12 through the console's own
 * `/mcp`, it answers the same shape as a frame's `host` (`caps`, `cpu`, `disks`, `helper`, `machine`, `memory`, `os`,
 * `sessions`, `uptime_s`, …).
 *
 * The read is deliberately NOT automatic: a live read costs a round trip to the machine and the frame is usually fresh
 * enough. A button puts the choice — and the freshness — in the operator's hands, and the block says which of the two it is.
 */
export async function readHostFacts(grant: Grant): Promise<HostFacts> {
  const body = await callTool(grant, "host.facts", {});
  let parsed: unknown;
  try {
    parsed = JSON.parse(body);
  } catch {
    throw new Error("the machine answered with text, not its facts");
  }
  if (parsed === null || typeof parsed !== "object" || Array.isArray(parsed)) {
    throw new Error("the machine answered host.facts without an object");
  }
  const raw = parsed as Record<string, unknown>;
  if (raw.ok === false) {
    const refusal = typeof raw.error === "string" ? raw.error.trim() : "";
    throw new Error(refusal !== "" ? refusal : "the machine refused the read");
  }
  return raw as HostFacts;
}

/** The sentence a failed live read earns: the device's words when it gave them, this page's otherwise. */
export function hostFactsFailure(error: unknown): string {
  if (error instanceof ToolError) {
    return error.message.trim() !== ""
      ? `The machine's facts could not be read — ${error.message.trim()}`
      : "The machine's facts could not be read.";
  }
  return error instanceof Error && error.message.trim() !== ""
    ? `The machine's facts could not be read — ${error.message.trim()}`
    : "The machine's facts could not be read.";
}
