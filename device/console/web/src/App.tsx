import { useCallback, useEffect, useMemo, useState } from "react";
import logoUrl from "./logo.svg";
import {
  api,
  HttpError,
  NoAnswer,
  setToken,
  type AuditLine,
  type Device,
  type Session,
  type TokenMeta
} from "./api";
import { copyText } from "./copy";
import { shortId } from "./format";
import { DeviceCard, retrySentence } from "./DeviceCard";
import { recordVitals, type VitalsSample } from "./vitals";
import { AUDIT_WINDOW, AuditTable } from "./AuditTable";
import { EventsPage } from "./EventsPage";
import { PAGE_LABELS, Rail, type Page } from "./Rail";
import { Modal } from "./Modal";
import { GrantForm } from "./GrantForm";
import { ConnectCard } from "./ConnectCard";
import { DevicePage } from "./DevicePage";
import { DevicePicker } from "./DevicePicker";
import { TerminalWorkspace } from "./TerminalWorkspace";
import { BrowserPane } from "./BrowserPane";
import { FilesPage } from "./FilesPage";
import { MemoryPage } from "./MemoryPage";
import { MonitorChip } from "./MonitorChip";
import { MonitorsPage } from "./MonitorsPage";
import { RunsPage } from "./RunsPage";
import { ScreenPage } from "./ScreenPage";
import { SecurityPage } from "./SecurityPage";
import { SystemPage } from "./SystemPage";

export function App() {
  const [session, setSession] = useState<Session | null>(null);
  const [devices, setDevices] = useState<Device[]>([]);
  // The browser-local history sampled from the same fleet the page polls, one entry per device id.
  // It lives in localStorage; this state is the copy the cards draw.
  const [vitals, setVitals] = useState<Record<string, VitalsSample[]>>({});
  const [audit, setAudit] = useState<AuditLine[]>([]);
  // Failed authentications, from their own log. Fetched with the audit because both are bounded
  // windows the operator reads, and a poll that refreshed one and not the other would be half a story.
  const [denials, setDenials] = useState<AuditLine[]>([]);
  // The operator's permanent tokens, fetched so the audit can name the token behind a call. Empty when
  // the console would not answer: the table falls back to record ids.
  const [tokens, setTokens] = useState<TokenMeta[]>([]);
  const [page, setPage] = useState<Page>("devices");
  const [selected, setSelected] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [tokenInput, setTokenInput] = useState("");
  const [renaming, setRenaming] = useState<Device | null>(null);
  const [removing, setRemoving] = useState<Device | null>(null);
  const [connecting, setConnecting] = useState<Device | null>(null);
  const [draftName, setDraftName] = useState("");
  const [notice, setNotice] = useState<string | null>(null);
  const [code, setCode] = useState<{ code: string; expires_at_ms: number } | null>(null);
  const [grant, setGrant] = useState<{ deviceId: string; envelope: unknown } | null>(null);
  /** True when the grant in force was refused, so the form is offered again above the live tabs. */
  const [grantStale, setGrantStale] = useState(false);
  const [consoleVersion, setConsoleVersion] = useState<string | null>(null);
  const [accessNote, setAccessNote] = useState<string | null>(null);

  const newest = useMemo(
    () =>
      devices.reduce<string | null>(
        (best, device) =>
          device.version && (best === null || compareVersions(device.version, best) > 0) ? device.version : best,
        null
      ),
    [devices]
  );

  // The versions the fleet is actually running, oldest first. A mixed fleet is a real limitation:
  // the console's tool registry is one list, while each device advertises its own — so a tool the
  // page offers (or an aggregate token names) may not exist on the older build.
  const versions = useMemo(
    () =>
      Array.from(
        new Set(
          devices
            .map((device) => device.version)
            .filter((version): version is string => Boolean(version))
        )
      ).sort(compareVersions),
    [devices]
  );

  // A device whose channel hello predates the version field is not "the same build": it is a build
  // the page cannot name, and a tool surface it may not share. Count it separately from the known
  // versions rather than dropping it from the warning.
  const unreported = devices.filter((device) => !device.version).length;

  // Ask who we are first, then ask for data. Fetching the device list before knowing whether there is
  // a session earns two 401s on every cold load — visible in the browser console as failures that are
  // not failures, and exactly the noise that hides a real one later.
  const refreshData = useCallback(async () => {
    const [list, lines, refusals, tokenList] = await Promise.all([
      api.devices(),
      api.audit(AUDIT_WINDOW),
      api.denials(AUDIT_WINDOW),
      tokensOrEmpty()
    ]);
    setDevices(list);
    // The poll is the only sampler; the stored last timestamp enforces the 30 s cadence. Sampling
    // after a successful fleet fetch is what keeps an offline device from adding a stale point.
    setVitals(recordVitals(list, Date.now()));
    setAudit(lines);
    setDenials(refusals);
    setTokens(tokenList);
  }, []);

  const probe = useCallback(async () => {
    const [state, health] = await Promise.all([api.session(), api.health()]);
    setConsoleVersion(health.version);
    setSession(state);
    if (!state.authenticated) return;
    await refreshData();
    setError(null);
  }, [refreshData]);

  const load = useCallback(async () => {
    if (!session?.authenticated) {
      await probe();
      return;
    }
    await refreshData();
  }, [probe, refreshData, session?.authenticated]);

  useEffect(() => {
    probe().catch((cause) => {
      if (cause instanceof NoAnswer || cause instanceof TypeError) {
        // The edge answered instead of the console. On this host that is an Access login page after
        // the session lapsed, and a reload is what re-enters Access; the guard is what keeps a broken
        // edge from reloading forever.
        if (reloadOnce()) {
          window.location.reload();
          return;
        }
        setSession({ authenticated: false });
        setAccessNote(
          "The console did not answer: the request was intercepted before it arrived. An expired Cloudflare Access session looks exactly like this — reload to sign in through Access."
        );
        return;
      }
      if (cause instanceof HttpError && cause.status === 401) {
        setSession({ authenticated: false });
        return;
      }
      setError(describe(cause));
    });
  }, [probe]);

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
    // Best effort, and deliberately not awaited: the request goes out with the token still in hand,
    // and the page clears whether it answers, fails or never arrives. A slow console must not leave
    // a live session in this tab.
    api.logout().catch(() => {});
    setToken(null);
    setSession({ authenticated: false });
    setDevices([]);
    setVitals({});
    setAudit([]);
    setDenials([]);
    setTokens([]);
    setSelected(null);
    setGrant(null);
    setGrantStale(false);
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
      setNotice(`${removing.name ?? shortId(removing.id)} was removed; its channel closed.`);
      setRemoving(null);
      if (selected === removing.id) setSelected(null);
      await load();
    } catch (cause) {
      setError(describe(cause));
    }
  };

  // Remote troubleshooting has the same problem as the panel's: a screenshot is not a bug report. The
  // console can assemble the same facts it can see, and say which of them it can see.
  const copyDiagnostics = async (device: Device | null) => {
    if (!device) return;
    const lines = [
      "Summrise device diagnostics (from the console)",
      `device: ${device.name ?? "(unnamed)"} (${device.id})`,
      `online: ${device.online ? "yes" : "no"}  version: ${device.version ?? "unknown"}  last seen: ${device.last_seen_ms ? new Date(device.last_seen_ms).toISOString() : "never"}`,
      device.last_attempt && device.last_attempt.attempts > 0
        ? `console link: ${retrySentence(device.last_attempt.attempts, device.last_attempt.reason)}`
        : "console link: no failure reported since it last connected",
      "recent audit for this device:"
    ];
    const mine = audit.filter((line) => line.device_id === device.id).slice(0, 8);
    if (mine.length === 0) lines.push(`  (none in the last ${AUDIT_WINDOW} lines)`);
    for (const line of mine) {
      lines.push(
        `  ${new Date(line.ts_ms).toISOString()}  ${line.kind} ${line.tool ?? line.detail ?? ""} ${line.status}`.trimEnd()
      );
    }
    const text = lines.join("\n");
    if (!(await copyText(text))) {
      setNotice("Could not copy the diagnostics: the browser refused the clipboard.");
      return;
    }
    setNotice("Diagnostics copied for " + (device.name ?? shortId(device.id)) + ".");
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
          {accessNote && <p className="error">{accessNote}</p>}
          {!accessNote && session.access?.header && session.access.error && (
            <p className="error">
              Cloudflare Access signed you in, but the console refused the token: {session.access.error}
            </p>
          )}
          {!accessNote && !session.access?.header && session.access?.configured && (
            <p className="hint">
              This request carried no Access identity — usually a page restored after the session
              expired. <a href="/">Reload through Access</a>.
            </p>
          )}
          {!accessNote && session.access && !session.access.configured && (
            <p className="hint">
              Access trust is off on this console, so the operator token is the only way in.
            </p>
          )}
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
            <span className="crumb">{PAGE_LABELS[page]}</span>
          </div>
          <p className="session">
            {chosen && grant && grant.deviceId === chosen.id && (
              <MonitorChip grant={grant} onOpen={() => setPage("monitors")} />
            )}
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
                  {(versions.length > 1 || unreported > 0) && (
                    <p className="hint" role="status">
                      {versions.length > 1 &&
                        `The fleet is running ${versions.length} builds (${versions.join(", ")}). `}
                      {unreported > 0 &&
                        `${unreported} device${unreported === 1 ? "" : "s"} did not report a build. `}
                      Tools can differ by device; upgrade the older builds before relying on one
                      shared tool surface.
                    </p>
                  )}
                  <div className="cards">
                    {devices.map((device) => (
                      <DeviceCard
                        key={device.id}
                        device={device}
                        vitals={vitals[device.id] ?? []}
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
                        onDiagnostics={() => void copyDiagnostics(device)}
                        onConnect={() => setConnecting(device)}
                      />
                    ))}
                  </div>
                </section>
              </div>

              <div className="col">
                {chosen ? (
                  <section className="panel">
                    <div className="panel-head">
                      <h2>Working on {chosen.name ?? shortId(chosen.id)}</h2>
                      <div className="actions">
                        <button type="button" onClick={() => void copyDiagnostics(chosen)}>Copy diagnostics</button>
                        <button type="button" onClick={() => setConnecting(chosen)}>Connect an AI client</button>
                      </div>
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
                  <AuditTable lines={audit} devices={devices} tokens={tokens} />
                </section>
              </div>
            </div>
          )}

          {page === "files" && (
            <DevicePage
              title="Files"
              pickHint="The files live on the device, so pick one. The console signs a grant for the file tools, and the device checks that grant on every list, read, write and mkdir."
              grantHint="A grant is signed by the console, names the tools and expires. The device checks it on every call. Everything is selected by default: press the button and the files appear."
              devices={devices}
              chosen={chosen}
              grant={grant}
              onChoose={setSelected}
              onGrant={(deviceId, envelope) => setGrant({ deviceId, envelope })}
            >
              {(active) => <FilesPage grant={active} />}
            </DevicePage>
          )}

          {page === "terminal" && !chosen && (
            <section className="panel">
              <div className="panel-head">
                <h2>Terminal</h2>
              </div>
              <p className="hint">
                A terminal runs on a device, so pick one. The console signs a grant for the tools it
                needs, and the device checks that grant on every call.
              </p>
              <DevicePicker devices={devices} onChoose={setSelected} />
            </section>
          )}
          {page === "terminal" && chosen && (
            <section className="panel">
              <div className="panel-head">
                <h2>Terminal on {chosen.name ?? shortId(chosen.id)}</h2>
              </div>
              <p className="hint">
                Your console session is the permission: the operator may use every tool on this
                device, destructive calls included. No grant to sign.
              </p>
            </section>
          )}
          {/*
            The workspace is rendered whichever page is showing, and hidden rather than unmounted:
            a rail click used to destroy the tabs, their cursors and their xterms. The section is a
            direct child of `main` so the page's grid keeps its gap; `hidden` takes it out of the
            layout (and out of the modal's way) on every other page.
          */}
          {chosen && (
            <section className="panel" hidden={page !== "terminal"}>
              <TerminalWorkspace
                grant={{ deviceId: chosen.id }}
                visible={page === "terminal"}
                onGrantRefused={() => undefined}
              />
            </section>
          )}

          {page === "browser" && (
            <DevicePage
              title="Browser"
              pickHint="The browser is one the device runs for you — in the user's session when the helper is there. Pick a device to grant the console its browser tools."
              devices={devices}
              chosen={chosen}
              grant={grant}
              onChoose={setSelected}
              onGrant={(deviceId, envelope) => setGrant({ deviceId, envelope })}
            >
              {(active) => <BrowserPane grant={active} />}
            </DevicePage>
          )}

          {page === "screen" && (
            <DevicePage
              title="Screen"
              pickHint="The screen belongs to the device's user session, so pick the device whose screen you want to see. The console signs a grant for the capture tool, and the device checks that grant on every call."
              grantHint="A grant is signed by the console, names the tools and expires. The device checks it on every call. Everything is selected by default: press the button and the screen can be captured."
              devices={devices}
              chosen={chosen}
              grant={grant}
              onChoose={setSelected}
              onGrant={(deviceId, envelope) => setGrant({ deviceId, envelope })}
            >
              {(active) => <ScreenPage grant={active} />}
            </DevicePage>
          )}

          {page === "memory" && (
            <DevicePage
              title="Memory"
              pickHint="Memory is kept on the device, so pick one. The console signs a grant for the memory tools, and the device checks that grant on every read and every edit."
              grantHint="A grant is signed by the console, names the tools and expires. The device checks it on every call. Everything is selected by default: press the button and the entries appear."
              devices={devices}
              chosen={chosen}
              grant={grant}
              onChoose={setSelected}
              onGrant={(deviceId, envelope) => setGrant({ deviceId, envelope })}
            >
              {(active) => <MemoryPage grant={active} />}
            </DevicePage>
          )}

          {page === "monitors" && (
            <DevicePage
              title="Monitors"
              pickHint="Monitors run on the device, so pick one. The console signs a grant for the monitor tools, and the device checks that grant on every add, probe and removal."
              grantHint="A grant is signed by the console, names the tools and expires. The device checks it on every call. Everything is selected by default: press the button and the targets appear."
              devices={devices}
              chosen={chosen}
              grant={grant}
              onChoose={setSelected}
              onGrant={(deviceId, envelope) => setGrant({ deviceId, envelope })}
            >
              {(active) => <MonitorsPage grant={active} />}
            </DevicePage>
          )}

          {page === "runs" && (
            <DevicePage
              title="Runs"
              pickHint="A run happens on the device, so pick one. runs.exec is destructive and audited: the console signs a grant for it, and the device checks that grant on every call."
              grantHint="A grant is signed by the console, names the tools and expires. The device checks it on every call. Everything is selected by default: press the button and the run form appears."
              devices={devices}
              chosen={chosen}
              grant={grant}
              onChoose={setSelected}
              onGrant={(deviceId, envelope) => setGrant({ deviceId, envelope })}
            >
              {(active) => <RunsPage grant={active} />}
            </DevicePage>
          )}

          {page === "system" && (
            <DevicePage
              title="System"
              pickHint="Processes and reachability live on the device, so pick one. The console signs a grant for the system tools, and the device checks that grant on every read, every kill and every network test."
              grantHint="A grant is signed by the console, names the tools and expires. The device checks it on every call. Everything is selected by default: press the button and the process list appears."
              devices={devices}
              chosen={chosen}
              grant={grant}
              onChoose={setSelected}
              onGrant={(deviceId, envelope) => setGrant({ deviceId, envelope })}
            >
              {(active) => <SystemPage grant={active} />}
            </DevicePage>
          )}

          {page === "audit" && (
            <section className="panel">
              <div className="panel-head">
                <h2>Audit</h2>
                <div className="actions">
                  <button type="button" onClick={() => void load()}>Refresh</button>
                </div>
              </div>
              <AuditTable lines={audit} devices={devices} tokens={tokens} />
            </section>
          )}

          {page === "security" && (
            <SecurityPage
              lines={denials}
              devices={devices}
              tokens={tokens}
              onTokensChanged={refreshData}
            />
          )}

          {page === "events" && (
            <DevicePage
              title="Events"
              pickHint="The events are the device's own, so pick one. The console signs a grant for the state tool, and the device checks that grant on every read."
              grantHint="A grant is signed by the console, names the tools and expires. The device checks it on every call. Everything is selected by default: press the button and the device's health and its last ten events appear."
              devices={devices}
              chosen={chosen}
              grant={grant}
              onChoose={setSelected}
              onGrant={(deviceId, envelope) => setGrant({ deviceId, envelope })}
            >
              {(active) => <EventsPage grant={active} />}
            </DevicePage>
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
        <Modal title={`Rename ${renaming.name ?? shortId(renaming.id)}`} onClose={() => setRenaming(null)}>
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

      {connecting && (
        <ConnectCard device={connecting} onClose={() => setConnecting(null)} />
      )}

      {removing && (
        <Modal title={`Remove ${removing.name ?? shortId(removing.id)}?`} onClose={() => setRemoving(null)}>
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
        <span className="identity">
          {session.via === "access" ? "signed in through Cloudflare Access" : "signed in with the operator token"}
        </span>
        <span className="grow" />
        {/* The build's own number, from /health: a label typed into the page drifts exactly like the
            one that was typed into the launcher script. */}
        <span>console {consoleVersion ?? "…"}</span>
      </footer>
    </div>
  );
}

/**
 * The operator's token list, asked for beside the devices and the audit.
 *
 * The audit resolves each line's `token_id` through it, so it is fetched with the data the page draws —
 * but it is a convenience, not the load. A console that refuses it (an older build without the route,
 * a session that may not read it, a poll that catches a restart) answers with an empty list: the table
 * keeps the ids, which are the fact, and the page keeps working.
 */
function tokensOrEmpty(): Promise<TokenMeta[]> {
  return api.tokens().catch(() => []);
}

function describe(cause: unknown): string {
  if (cause instanceof HttpError) return `${cause.status} ${cause.code}`;
  return cause instanceof Error ? cause.message : String(cause);
}

/// Whether this page may reload itself once to re-enter Cloudflare Access.
///
/// The guard is a timestamp rather than a flag: a session that expires an hour later in the same tab
/// still gets its reload, while an edge that keeps answering with a login page cannot put the page in
/// a reload loop.
function reloadOnce(): boolean {
  const key = "summrise.access.reload";
  try {
    const now = Date.now();
    const last = Number(window.sessionStorage.getItem(key) ?? "0");
    if (now - last < 20_000) return false;
    window.sessionStorage.setItem(key, String(now));
    return true;
  } catch {
    return false;
  }
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
