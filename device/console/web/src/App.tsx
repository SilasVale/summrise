import { useCallback, useEffect, useMemo, useState } from "react";
import logoUrl from "./logo.svg";
import { api, HttpError, setToken, type AuditLine, type Device, type Session } from "./api";
import { DeviceCard } from "./DeviceCard";
import { AuditTable } from "./AuditTable";
import { Rail, type Page } from "./Rail";
import { Modal } from "./Modal";
import { GrantForm } from "./GrantForm";
import { TerminalPane } from "./TerminalPane";
import { BrowserPane } from "./BrowserPane";

export function App() {
  const [session, setSession] = useState<Session | null>(null);
  const [devices, setDevices] = useState<Device[]>([]);
  const [audit, setAudit] = useState<AuditLine[]>([]);
  const [page, setPage] = useState<Page>("devices");
  const [selected, setSelected] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [tokenInput, setTokenInput] = useState("");
  const [renaming, setRenaming] = useState<Device | null>(null);
  const [removing, setRemoving] = useState<Device | null>(null);
  const [draftName, setDraftName] = useState("");
  const [notice, setNotice] = useState<string | null>(null);
  const [code, setCode] = useState<{ code: string; expires_at_ms: number } | null>(null);
  const [grant, setGrant] = useState<{ deviceId: string; envelope: unknown } | null>(null);

  const newest = useMemo(
    () =>
      devices.reduce<string | null>(
        (best, device) =>
          device.version && (best === null || compareVersions(device.version, best) > 0) ? device.version : best,
        null
      ),
    [devices]
  );

  const load = useCallback(async () => {
    const [state, list, lines] = await Promise.all([api.session(), api.devices(), api.audit()]);
    setSession(state);
    setDevices(list);
    setAudit(lines);
    setError(null);
  }, []);

  useEffect(() => {
    load().catch((cause) => {
      if (cause instanceof HttpError && cause.status === 401) {
        setSession({ authenticated: false });
        return;
      }
      setError(describe(cause));
    });
  }, [load]);

  // A signed-in session polls; a signed-out one does not. The device list is the thing an operator
  // watches, and a list that is a minute stale is the reason they refresh and lose their selection.
  useEffect(() => {
    if (!session?.authenticated) return;
    const timer = window.setInterval(() => {
      load().catch(() => {
        /* a poll that fails is retried by the next one; the sign-in state decides the rest */
      });
    }, 5000);
    return () => window.clearInterval(timer);
  }, [session?.authenticated, load]);

  const signOut = () => {
    setToken(null);
    setSession({ authenticated: false });
    setDevices([]);
    setAudit([]);
    setSelected(null);
    setGrant(null);
    setPage("devices");
  };

  const signIn = async () => {
    setBusy(true);
    try {
      const answer = await api.signIn({ token: tokenInput.trim() });
      setToken(answer.token);
      setTokenInput("");
      await load();
    } catch (cause) {
      setError(describe(cause));
    } finally {
      setBusy(false);
    }
  };

  const applyRename = async () => {
    if (!renaming) return;
    const name = draftName.trim();
    try {
      await api.rename(renaming.id, name === "" ? null : name);
      setRenaming(null);
      setNotice(name === "" ? "Name cleared; the device's own hostname applies." : `Renamed to ${name}.`);
      await load();
    } catch (cause) {
      setError(describe(cause));
    }
  };

  const applyRemove = async () => {
    if (!removing) return;
    try {
      await api.remove(removing.id);
      setNotice(`${removing.name ?? removing.id.slice(0, 12)} was removed; its channel closed.`);
      setRemoving(null);
      if (selected === removing.id) setSelected(null);
      await load();
    } catch (cause) {
      setError(describe(cause));
    }
  };

  const mintCode = async () => {
    try {
      setCode(await api.enroll());
    } catch (cause) {
      setError(describe(cause));
    }
  };

  if (session === null) return <p className="boot">Connecting…</p>;

  if (!session.authenticated) {
    return (
      <div className="login-wrap">
        <section className="panel login">
          <img src={logoUrl} alt="" width={40} height={40} />
          <h2>Sign in to the console</h2>
          <p className="hint">
            Cloudflare Access signs you in automatically. The operator token is the way in when it does
            not.
          </p>
          <label htmlFor="token">Operator token</label>
          <input
            id="token"
            type="password"
            autoComplete="off"
            spellCheck={false}
            value={tokenInput}
            onChange={(event) => setTokenInput(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter" && tokenInput.trim()) void signIn();
            }}
            placeholder="64 hex characters"
          />
          <button className="primary" type="button" disabled={busy || tokenInput.trim().length === 0} onClick={() => void signIn()}>
            {busy ? "Checking…" : "Sign in"}
          </button>
          {error && <p className="error">{error}</p>}
        </section>
      </div>
    );
  }

  const online = devices.filter((device) => device.online).length;
  const chosen = devices.find((device) => device.id === selected) ?? null;

  return (
    <div className="shell">
      <header className="topbar">
        <div className="topbar-inner">
          <div className="brand">
            <img src={logoUrl} alt="" width={26} height={26} />
            <h1>Summrise</h1>
            <span>console</span>
          </div>
          <p className="session">
            <span className={"dot " + (online > 0 ? "dot-ok" : "dot-off")} />
            {session.email ? session.email : `${online} of ${devices.length} devices online`}
            <button type="button" onClick={signOut}>Sign out</button>
          </p>
        </div>
      </header>

      <div className="body">
        <Rail page={page} onPage={setPage} devices={devices.length} online={online} />

        <main>
          {page === "devices" && (
            <div className="columns">
              <div className="col">
                <section className="panel">
                  <div className="panel-head">
                    <h2>Devices</h2>
                    <div className="actions">
                      <button type="button" onClick={() => void load()}>Refresh</button>
                    </div>
                  </div>
                  <p className="summary">
                    {devices.length} devices, {online} online
                  </p>
                  <div className="cards">
                    {devices.map((device) => (
                      <DeviceCard
                        key={device.id}
                        device={device}
                        newest={newest}
                        selected={selected === device.id}
                        onSelect={() => {
                          setSelected(device.id);
                          setPage("terminal");
                        }}
                        onRename={() => {
                          setRenaming(device);
                          setDraftName(device.name ?? "");
                        }}
                        onRemove={() => setRemoving(device)}
                      />
                    ))}
                  </div>
                </section>
              </div>

              <div className="col">
                {chosen ? (
                  <section className="panel">
                    <div className="panel-head">
                      <h2>Working on {chosen.name ?? chosen.id.slice(0, 12)}</h2>
                    </div>
                    <p className="hint">
                      Grant what the console may do, then read its state, open a shell, or look at its
                      browser.
                    </p>
                  </section>
                ) : (
                  <section className="panel empty">
                    <p>No device selected.</p>
                    <p className="hint">Choose one on the left and its controls appear here.</p>
                  </section>
                )}
                <section className="panel">
                  <div className="panel-head">
                    <h2>Add a device</h2>
                    <div className="actions">
                      <button type="button" className="primary" onClick={() => void mintCode()}>
                        New enrollment code
                      </button>
                    </div>
                  </div>
                  <p className="hint">
                    A code is single-use and expires in minutes. Run the command on the new machine.
                  </p>
                  {code && (
                    <pre className="code">
{`summrise-device --enroll ${location.origin} ${code.code}`}
{`expires ${new Date(code.expires_at_ms).toLocaleTimeString()}`}
                    </pre>
                  )}
                </section>

                <section className="panel">
                  <div className="panel-head">
                    <h2>Audit</h2>
                    <div className="actions">
                      <button type="button" onClick={() => void load()}>Refresh</button>
                    </div>
                  </div>
                  <AuditTable lines={audit} />
                </section>
              </div>
            </div>
          )}

          {page === "terminal" && (
            <>
              {!chosen && (
                <section className="panel empty">
                  <p>No device selected.</p>
                  <p className="hint">Pick one in Devices and the grant appears here.</p>
                </section>
              )}
              {chosen && (!grant || grant.deviceId !== chosen.id) && (
                <section className="panel">
                  <div className="panel-head">
                    <h2>Grant what the console may do on {chosen.name ?? chosen.id.slice(0, 12)}</h2>
                  </div>
                  <p className="hint">
                    A grant is signed by the console, names the tools and expires. The device checks it on
                    every call.
                  </p>
                  <GrantForm deviceId={chosen.id} onGranted={(envelope) => setGrant({ deviceId: chosen.id, envelope })} />
                </section>
              )}
              {chosen && grant && grant.deviceId === chosen.id && (
                <section className="panel">
                  <TerminalPane grant={grant} />
                </section>
              )}
            </>
          )}

          {page === "browser" && (
            <>
              {!chosen && (
                <section className="panel empty">
                  <p>No device selected.</p>
                  <p className="hint">Pick one in Devices and its browser appears here.</p>
                </section>
              )}
              {chosen && (!grant || grant.deviceId !== chosen.id) && (
                <section className="panel">
                  <div className="panel-head">
                    <h2>Grant what the console may do on {chosen.name ?? chosen.id.slice(0, 12)}</h2>
                  </div>
                  <GrantForm deviceId={chosen.id} onGranted={(envelope) => setGrant({ deviceId: chosen.id, envelope })} />
                </section>
              )}
              {chosen && grant && grant.deviceId === chosen.id && (
                <section className="panel">
                  <BrowserPane grant={grant} />
                </section>
              )}
            </>
          )}

          {page === "audit" && (
            <section className="panel">
              <div className="panel-head">
                <h2>Audit</h2>
                <div className="actions">
                  <button type="button" onClick={() => void load()}>Refresh</button>
                </div>
              </div>
              <AuditTable lines={audit} />
            </section>
          )}
        </main>
      </div>

      {notice && (
        <div className="toast" role="status">
          {notice}
          <button type="button" onClick={() => setNotice(null)}>Dismiss</button>
        </div>
      )}

      {renaming && (
        <Modal title={`Rename ${renaming.name ?? renaming.id.slice(0, 12)}`} onClose={() => setRenaming(null)}>
          <label htmlFor="rename">Name</label>
          <input
            id="rename"
            value={draftName}
            onChange={(event) => setDraftName(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") void applyRename();
            }}
            placeholder="leave empty to use the device's own hostname"
          />
          <div className="modal-actions">
            <button type="button" onClick={() => setRenaming(null)}>Cancel</button>
            <button type="button" className="primary" onClick={() => void applyRename()}>Save</button>
          </div>
        </Modal>
      )}

      {removing && (
        <Modal title={`Remove ${removing.name ?? removing.id.slice(0, 12)}?`} onClose={() => setRemoving(null)}>
          <p className="hint">
            Its channel closes now and it cannot reconnect. The device keeps running and can be enrolled
            again with a new code.
          </p>
          <div className="modal-actions">
            <button type="button" onClick={() => setRemoving(null)}>Cancel</button>
            <button type="button" className="danger" onClick={() => void applyRemove()}>Remove</button>
          </div>
        </Modal>
      )}

      <footer className="statusbar">
        <span>{devices.length} devices</span>
        <span className="sep" />
        <span>{online} online</span>
        <span className="sep" />
        <span>{session.via === "access" ? "signed in through Cloudflare Access" : "signed in with the operator token"}</span>
        <span className="grow" />
        <span>console 0.3.17</span>
      </footer>
    </div>
  );
}

function describe(cause: unknown): string {
  if (cause instanceof HttpError) return `${cause.status} ${cause.code}`;
  return cause instanceof Error ? cause.message : String(cause);
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
