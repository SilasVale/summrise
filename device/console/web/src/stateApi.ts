import { callTool, ToolError, type Grant } from "./terminal";

/**
 * **THE DEVICE'S OWN VIEW (the hundred-and-seventy-seventh list).** `state.get` answers the device's state document plus its
 * health — `status` (`ok`/`degraded`), the release and pid, whether the document is POISONED (unreadable), and the last ten
 * events the device itself recorded (which tool, allow or deny, when, and whether it ended well).
 *
 * That is a different claim from everything else on the Machine tab: the page shows what the CONSOLE knows about the machine, and
 * this shows what the MACHINE knows about itself. When the two disagree — a live process with a wedged channel is the measured
 * example — this is the half that settles it. A poisoned document answers with `state: null` and `status: "degraded"` rather than
 * a failed call, so the page draws the degradation instead of an error.
 */
export type DeviceEvent = {
  event: string;
  id: number | null;
  tool: string | null;
  decision: string | null;
  ok: boolean | null;
  transport: string | null;
  tsMs: number | null;
};

export type DeviceSelf = {
  status: string;
  release: string | null;
  pid: number | null;
  poisoned: boolean;
  events: DeviceEvent[];
};

const str = (value: unknown): string | null =>
  typeof value === "string" && value.trim() !== "" ? value.trim() : null;
const num = (value: unknown): number | null =>
  typeof value === "number" && Number.isFinite(value) ? value : null;

export async function readDeviceState(grant: Grant): Promise<DeviceSelf> {
  const body = await callTool(grant, "state.get", {});
  let answer: Record<string, unknown>;
  try {
    const parsed: unknown = JSON.parse(body);
    if (parsed === null || typeof parsed !== "object" || Array.isArray(parsed)) {
      throw new Error("the machine answered state.get without an object");
    }
    answer = parsed as Record<string, unknown>;
  } catch (error) {
    if (error instanceof Error && error.message.startsWith("the machine answered")) throw error;
    throw new Error("the machine answered with text, not its state");
  }
  // A REFUSAL CARRIES THE DEVICE'S SENTENCE. `state.get` answers `{ok:false, error}` when the read itself failed, and that
  // sentence is the one a person needs — the page must not draw an empty record as if the machine had nothing to say.
  if (answer.ok === false) {
    const refusal = typeof answer.error === "string" ? answer.error.trim() : "";
    throw new Error(refusal !== "" ? refusal : "the machine refused the read");
  }
  const events = Array.isArray(answer.events) ? answer.events : [];
  return {
    status: str(answer.status) ?? "unknown",
    release: str(answer.release) ?? str(answer.version),
    pid: num(answer.pid),
    // A document that could not be read is reported as `state: null` with degraded health; both are read.
    poisoned: answer.state === null || answer.poisoned === true,
    events: events.flatMap((value): DeviceEvent[] => {
      if (value === null || typeof value !== "object") return [];
      const raw = value as Record<string, unknown>;
      const event = str(raw.event);
      if (event === null) return [];
      return [
        {
          event,
          id: num(raw.id),
          tool: str(raw.tool),
          decision: str(raw.decision),
          ok: typeof raw.ok === "boolean" ? raw.ok : null,
          transport: str(raw.transport),
          tsMs: num(raw.ts_ms)
        }
      ];
    })
  };
}

/** The sentence a failed read earns: the device's words when it gave them, this page's otherwise. */
export function deviceStateFailure(error: unknown): string {
  if (error instanceof ToolError) {
    return error.message.trim() !== ""
      ? `The machine's own record could not be read — ${error.message.trim()}`
      : "The machine's own record could not be read.";
  }
  return error instanceof Error && error.message.trim() !== ""
    ? `The machine's own record could not be read — ${error.message.trim()}`
    : "The machine's own record could not be read.";
}
