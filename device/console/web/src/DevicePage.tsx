import type { ReactNode } from "react";
import type { Device } from "./api";
import { DevicePicker } from "./DevicePicker";
import { GrantForm } from "./GrantForm";
import { shortId } from "./format";
import type { Grant } from "./terminal";

// The three states every device page repeats: pick a device, sign a grant for it, show the page. The
// page is rendered only for the grant that names the chosen device, and receives that grant, so it
// cannot call the device with another device's envelope.
export type DevicePageProps = {
  title: string;
  pickHint: string;
  /** Omitted by the pages whose grant panel has no hint to give (Browser). */
  grantHint?: string;
  devices: Device[];
  chosen: Device | null;
  grant: Grant | null;
  onChoose: (id: string) => void;
  onGrant: (deviceId: string, envelope: Grant["envelope"]) => void;
  children: (grant: Grant) => ReactNode;
};

export function DevicePage({
  title,
  pickHint,
  grantHint,
  devices,
  chosen,
  grant,
  onChoose,
  onGrant,
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
  if (!grant || grant.deviceId !== chosen.id) {
    return (
      <section className="panel">
        <div className="panel-head">
          <h2>Grant what the console may do on {chosen.name ?? shortId(chosen.id)}</h2>
        </div>
        {grantHint && <p className="hint">{grantHint}</p>}
        <GrantForm deviceId={chosen.id} onGranted={(envelope) => onGrant(chosen.id, envelope)} />
      </section>
    );
  }
  return <section className="panel">{children(grant)}</section>;
}
