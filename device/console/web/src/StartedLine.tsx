import type { DeviceUpdate, HostFacts } from "./api";
import { processStartMs, startedStory } from "./updateStory";

/**
 * THE DEVICE'S ACCOUNT OF ITS LAST START, DRAWN WHERE THE START IS THE ANSWER.
 *
 * The shell used to print this line under the h1 on every Device tab (audit-44 F1): Files and
 * Terminal opened with an age before their own answer, and Overview put it above the machine's
 * name. The two surfaces whose subject is the machine's own start draw it now — Overview's machine
 * head, beside the name it describes, and the Machine tab's Device app section, beside the update
 * card — and no other tab draws it.
 *
 * `updateShown` is the Machine tab's rule: the card there states the running version and the
 * updater's last answer, so the line keeps only the moment of the start. Overview has no card, so
 * its line also ages an answer that belongs to this start.
 */
export type StartedDevice = {
  version: string | null;
  update?: DeviceUpdate | null;
  host?: Pick<HostFacts, "restarts"> | null;
};

export function StartedLine({
  device,
  updateShown = false
}: {
  device: StartedDevice;
  /** True when the page draws the update card beside this line (the Machine tab). */
  updateShown?: boolean;
}) {
  const started = startedStory(device.update, {
    startedAtMs: processStartMs(device),
    shownVersion: device.version,
    updateShown
  });
  if (!started) return null;
  return (
    <p className="started-line" title={started.title ?? undefined}>
      {started.text}
    </p>
  );
}
