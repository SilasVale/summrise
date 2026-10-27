import type { ReactNode } from "react";
import type { Device } from "./api";
import { DevicePicker } from "./DevicePicker";
import type { Grant } from "./terminal";

// The two states every device page repeats: pick a device, show the page. The page receives the
// chosen device as its scope; the console session is the permission, so there is no grant to sign.
export type DevicePageProps = {
  title: string;
  pickHint: string;
  devices: Device[];
  chosen: Device | null;
  onChoose: (id: string) => void;
  children: (grant: Grant) => ReactNode;
};

export function DevicePage({
  title,
  pickHint,
  devices,
  chosen,
  onChoose,
  children
}: DevicePageProps) {
  if (!chosen) {
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
  return <section className="panel">{children({ deviceId: chosen.id })}</section>;
}
