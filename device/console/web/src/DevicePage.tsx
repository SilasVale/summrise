import { useMemo, type ReactNode } from "react";
import type { Device } from "./api";
import { DevicePicker } from "./DevicePicker";
import type { Grant } from "./terminal";

// Pick a device, then render its page.
//
// **THE GRANT FORM IS GONE, AND DELETING IT IS THE POINT.** The operator's session is the authority:
// the console's `/mcp` treats an absent envelope as the operator's full authority on the named
// device, so a workspace page never asks for a second consent. An AI client's token is still minted
// deliberately in ConnectCard — that is the consent an operator gives, once, per client — but a page
// the operator opened themselves does not sign a grant to read a directory or take a screenshot. This
// shell used to interpose that form, so the first visit to Files, Screen, Runs or Machine stopped on a
// consent screen the product statement says does not exist.
export type DevicePageProps = {
  title: string;
  pickHint: string;
  devices: Device[];
  chosen: Device | null;
  onChoose: (id: string) => void;
  children: (grant: Grant) => ReactNode;
};

export function DevicePage({ title, pickHint, devices, chosen, onChoose, children }: DevicePageProps) {
  // ONE GRANT PER DEVICE, not one per render. Pages use it as an effect dependency; a fresh object on
  // every parent render re-ran their reads and invalidated in-flight ones (Activity → Runs lost its
  // detail to exactly that).
  const grant = useMemo<Grant | null>(
    () => (chosen ? { deviceId: chosen.id, envelope: null } : null),
    [chosen?.id]
  );
  if (!chosen || !grant) {
    return (
      <section className="panel">
        <div className="panel-head">
          <h2>{title}</h2>
        </div>
        <p className="hint">{pickHint}</p>
        <DevicePicker devices={devices} onChoose={onChoose} />
      </section>
    );
  }
  return <section className="panel">{children(grant)}</section>;
}
