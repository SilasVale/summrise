import { useMemo, useState } from "react";
import logoUrl from "./logo.svg";
import { DeviceCard, type Device } from "./DeviceCard";
import { AuditTable, type AuditLine } from "./AuditTable";

// A fixture UI while the page is rebuilt: the shapes here are exactly what /devices and /audit
// answer, so wiring it to the console is an import and a fetch, not a redesign.
const DEVICES: Device[] = [
  { id: "4c718d6e44c83388c7e07bd0d811fa0b7cf22e2fced85d35d0021dd26b11f687", name: "canary-one", online: true, version: "0.3.17", last_seen_ms: Date.now() - 12_000 },
  { id: "8780d0ee9a5a42bccd8b45590ad2af89ea3e9bbb841059a2e6aaba795fdf04a2", name: "DESKTOP-14RJCR8", online: true, version: "0.3.17", last_seen_ms: Date.now() - 20_000 }
];

const AUDIT: AuditLine[] = [
  { ts_ms: Date.now() - 60_000, kind: "mcp", device_id: DEVICES[0].id, tool: "terminal.create", status: 200 },
  { ts_ms: Date.now() - 120_000, kind: "grant", device_id: DEVICES[0].id, detail: "allow_destructive=true", status: 200 },
  { ts_ms: Date.now() - 180_000, kind: "enroll", status: 200 },
  { ts_ms: Date.now() - 240_000, kind: "denied", detail: "bad_operator_token", status: 401 }
];

export function App() {
  const [selected, setSelected] = useState<string | null>(null);
  const newest = useMemo(
    () => DEVICES.reduce<string | null>((best, device) => (best === null || compareVersions(device.version, best) > 0 ? device.version : best), null),
    []
  );

  return (
    <>
      <header className="topbar">
        <div className="topbar-inner">
          <div className="brand">
            <img src={logoUrl} alt="" width={26} height={26} />
            <h1>Summrise</h1>
            <span>console</span>
          </div>
          <p className="session">
            <span className="dot dot-ok" /> Signed in until 19:40
          </p>
        </div>
      </header>

      <main>
        <p className="lede">Pick a device below to work on it. Everything that touches a machine goes through one audited channel.</p>

        <div className="columns">
          <div className="col">
            <section className="panel">
              <div className="panel-head">
                <h2>Devices</h2>
                <div className="actions">
                  <button type="button">Refresh</button>
                  <button type="button">Sign out</button>
                </div>
              </div>
              <p className="summary">{DEVICES.length} devices, {DEVICES.filter((d) => d.online).length} online</p>
              <div className="cards">
                {DEVICES.map((device) => (
                  <DeviceCard
                    key={device.id}
                    device={device}
                    newest={newest}
                    selected={selected === device.id}
                    onSelect={() => setSelected(device.id)}
                  />
                ))}
              </div>
            </section>

            <section className="panel">
              <div className="panel-head">
                <h2>Add a device</h2>
                <div className="actions">
                  <button type="button">New enrollment code</button>
                </div>
              </div>
              <p className="hint">A code is single-use and expires in minutes. Run the command on the new machine.</p>
            </section>
          </div>

          <div className="col">
            {selected ? (
              <section className="panel">
                <div className="panel-head">
                  <h2>Working on {DEVICES.find((d) => d.id === selected)?.name}</h2>
                </div>
                <p className="hint">Grant what the console may do, then read its state, open a shell, or look at its browser.</p>
              </section>
            ) : (
              <section className="panel empty">
                <p>No device selected.</p>
                <p className="hint">Choose one on the left and its controls appear here.</p>
              </section>
            )}

            <section className="panel">
              <div className="panel-head">
                <h2>Audit</h2>
                <div className="actions">
                  <button type="button">Refresh</button>
                </div>
              </div>
              <AuditTable lines={AUDIT} />
            </section>
          </div>
        </div>
      </main>
    </>
  );
}

function compareVersions(left: string, right: string): number {
  const a = left.split(/[.+-]/).map((part) => Number.parseInt(part, 10) || 0);
  const b = right.split(/[.+-]/).map((part) => Number.parseInt(part, 10) || 0);
  for (let index = 0; index < Math.max(a.length, b.length); index += 1) {
    const difference = (a[index] ?? 0) - (b[index] ?? 0);
    if (difference !== 0) return difference;
  }
  return 0;
}
