import { useEffect, useRef, useState, type ChangeEvent } from "react";
import { callApi, getToken } from "../lib/api";
import { useDeviceRead } from "../hooks/useDeviceRead";
import { ConnectCard } from "./ConnectCard";
import { DeviceLogsCard } from "./DeviceLogsCard";
import { RestartHistoryCard } from "./RestartHistoryCard";
import { DeviceHealthCard } from "./DeviceHealthCard";
import { UpdateCard, useUpdateStatus } from "./UpdateCard";
import { MonitorsCard } from "./MonitorsCard";
import { NotificationsCard } from "./NotificationsCard";
import type { Monitors } from "../hooks/useMonitors";
import type { AttentionItem } from "../lib/attention";
import type { NotifyPermission } from "../lib/notify";
import { EMPTY_MONITORS } from "../hooks/useMonitors";
import type { VitalsSeries } from "../hooks/useVitalsSeries";
import { EMPTY_BOOT_HISTORY, type BootHistory } from "../hooks/useBootHistory";
import { EMPTY_SERIES } from "../hooks/useVitalsSeries";
import { useAck } from "../lib/useAck";

// SettingsPage — device settings as a first-class page (both densities).
// Cards: Connect an AI client (onboarding — first, because nothing else on this
// page matters until a client is pointed here), Session buffer, Gateway
// (optional cloud config — register the device with a gateway console +
// optional free cloudflared tunnel), Memory, Terminal, Transport.
/** The update card owns its own poll (60 s, cached device-side): it is the only consumer of
 *  `/api/update`, and it must keep reading while a swap is in flight — exactly when the rest
 *  of the panel's connections are dropping. */
function UpdateSection({ runningRelease }: { runningRelease?: string }) {
  const update = useUpdateStatus();
  return (
    <UpdateCard
      status={update}
      failed={update.failed}
      refresh={update.refresh}
      runningRelease={runningRelease}
    />
  );
}

/** The spellings a loopback bind can have. Written out rather than imported, because what is being decided is which SENTENCE
 *  to print, and that sentence differs only for these. */
function isLoopback(host: string): boolean {
  return host === "127.0.0.1" || host === "localhost" || host === "::1" || host === "[::1]";
}

/** WHAT THE DEVICE'S SETTINGS READ PUTS INTO THIS PAGE'S FIELDS — the five editable values it seeds,
 *  as the inputs render them.
 *
 *  `gwStatus` IS THE ONE THAT IS NOT JUST A COPY OF THE BODY: the device has a gateway sentence to
 *  say only when a console is configured (`console_url`), so `null` here means "this reply did not
 *  mention a gateway" — which is NOT the same as "there is none". The card also carries the connect
 *  flow's own words ("connecting…", "registered", the re-read's warning), and a reply with nothing
 *  to say must not wipe one. */
interface SettingsSeed {
  bufferMb: string;
  gwUrl: string;
  gwStatus: string | null;
  memEntries: string;
  memBytesMb: string;
  memRetention: string;
}

/** THE VALUES THE FIELDS SHOW BEFORE THE DEVICE ANSWERS — byte-for-byte the defaults this page's
 *  `useState` calls carried, because they are also what an operator sees if the read fails. */
const SETTINGS_DEFAULTS: SettingsSeed = {
  bufferMb: "8",
  gwUrl: "",
  gwStatus: null,
  memEntries: "10000",
  memBytesMb: "64",
  memRetention: "",
};

/** THE DEVICE'S ANSWER AS THE FIELDS TAKE IT. Each field is written ONLY when the body carries it,
 *  and the value in hand (`previous`) is the floor — the rule the hand-rolled `.then` used to state
 *  one `if (typeof … === "number")` at a time, kept because a body that carries none of them (an
 *  older agent, a proxy's error page) has always left the fields alone rather than blanking them. */
function seedSettings(previous: SettingsSeed, body: unknown): SettingsSeed {
  const j = body as Record<string, unknown> | null | undefined;
  const next = { ...previous };
  if (j && typeof j.buffer_mb === "number") next.bufferMb = String(j.buffer_mb);
  if (j && typeof j.console_url === "string" && j.console_url) {
    next.gwUrl = j.console_url;
    // Persisted gateway state — show it, don't blank the card.
    const parts = ["connected"];
    if (j.tunnel_configured) parts.push(j.tunnel_running ? "tunnel: running" : "tunnel: configured");
    next.gwStatus = parts.join(" · ");
  }
  if (j && typeof j.memory_max_entries === "number") next.memEntries = String(j.memory_max_entries);
  if (j && typeof j.memory_max_bytes_mb === "number") next.memBytesMb = String(j.memory_max_bytes_mb);
  if (j && typeof j.memory_retention_days === "number") next.memRetention = String(j.memory_retention_days);
  return next;
}

export function SettingsPage({
  onOpenMemory,
  restarts,
  restartsFailed,
  vitals,
  vitalsFailed,
  runningRelease,
  config,
  monitors,
  monitorsFailed,
  onMonitorAdd,
  onMonitorRemove,
  onMonitorProbe,
  notifyPermission,
  onRequestNotify,
  onTestNotify,
  attention,
}: {
  onOpenMemory?: () => void;
  /** The device's restart history, polled ONCE by the shell that renders this page —
   *  optional so every existing caller keeps compiling, and a caller without it gets a
   *  card that says it has nothing to show rather than a second poller. */
  restarts?: BootHistory;
  restartsFailed?: boolean;
  /** The vitals series, polled once by the shell (see `useVitalsSeries`) — optional so
   *  every existing caller keeps compiling. */
  vitals?: VitalsSeries;
  vitalsFailed?: boolean;
  /** The reachability monitors, polled once by the shell (see `useMonitors`) — optional so
   *  every existing caller keeps compiling. The card itself is pure. */
  monitors?: Monitors;
  monitorsFailed?: boolean;
  onMonitorAdd?: (host: string, port: number) => Promise<{ ok: boolean; error?: string }>;
  onMonitorRemove?: (id: string) => void;
  onMonitorProbe?: (id: string) => void;
  /** Getting-your-attention (round 264): the browser's answer, the way to ask, and the test send. */
  notifyPermission?: NotifyPermission;
  onRequestNotify?: () => Promise<NotifyPermission>;
  onTestNotify?: () => void;
  attention?: AttentionItem[];
  /** The release the shell sees the device RUNNING (`/api/status`). The update card watches
   *  it change to recognise a swap that happened while the operator was looking. */
  runningRelease?: string;
  /** The CONFIGURED bind, from /api/status. Optional: a caller that does not pass it renders no bind line rather than a
   *  guessed one — the rule the device line below already follows for `location.host`. */
  config?: { host?: string; port?: number; path?: string } | null;
}) {
  const [revealed, setRevealed] = useState(false);
  const token = getToken();
  const [bufferMb, setBufferMb] = useState(SETTINGS_DEFAULTS.bufferMb);
  const [status, setStatus] = useState("");

  // Memory capacity card (round-358: server-wired since round-357, editable
  // here — previously config.yaml-only). Retention "" = keep forever.
  const [memEntries, setMemEntries] = useState(SETTINGS_DEFAULTS.memEntries);
  const [memBytesMb, setMemBytesMb] = useState(SETTINGS_DEFAULTS.memBytesMb);
  const [memRetention, setMemRetention] = useState(SETTINGS_DEFAULTS.memRetention);
  const [memStatus, setMemStatus] = useState("");
  const { busy: memBusy, ack: memAck, run: runMem } = useAck();

  // Desktop-app card (Electron shell only): auto-launch on login.
  const desktopBridge = (window as any).summriseDesktop;
  const [hasDesktopBridge] = useState(!!desktopBridge?.getAutoLaunch);
  const [autoLaunch, setAutoLaunchState] = useState(false);
  const { busy: autoLaunchBusy, ack: autoLaunchAck, run: runAutoLaunch } = useAck();
  const [autoLaunchStatus, setAutoLaunchStatus] = useState("");

  useEffect(() => {
    if (!desktopBridge?.getAutoLaunch) return;
    desktopBridge.getAutoLaunch().then((j: any) => {
      if (j?.ok) setAutoLaunchState(!!j.enabled);
    }).catch(() => setAutoLaunchStatus("desktop bridge unavailable"));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function setAutoLaunch(enabled: boolean) {
    if (!desktopBridge?.setAutoLaunch) return;
    await runAutoLaunch("autolaunch", async () => {
      setAutoLaunchStatus("");
      try {
        const j = await desktopBridge.setAutoLaunch(enabled);
        if (j?.ok) { setAutoLaunchState(!!j.enabled); setAutoLaunchStatus(j.enabled ? "enabled — Summrise Desktop starts at login" : "disabled"); }
        else setAutoLaunchStatus(j?.error || "failed");
      } catch (e: any) { setAutoLaunchStatus(e?.message || "failed"); }
    });
  }

  // Gateway card state
  const [gwUrl, setGwUrl] = useState(SETTINGS_DEFAULTS.gwUrl);
  const [gwKey, setGwKey] = useState("");
  const [gwTunnel, setGwTunnel] = useState(false);
  const [gwStatus, setGwStatus] = useState(SETTINGS_DEFAULTS.gwStatus ?? "");
  // FOUR CONTROLS SHARE THIS ONE (the confirm, its Cancel, the trigger, and the Connect that owns the label).
  // Unlike GoalBar and PathView, the label was never WRONG here: only `connectGateway` sets this flag, so
  // "Connecting…" could only appear while a connect was actually in flight. What the hook adds is that the flag
  // clears on EVERY exit — a throw used to skip the `finally`'s counterpart in the sibling handlers, which is
  // the defect MonitorsCard demonstrably had.
  const { busy: gwBusy, busyOn: gwBusyOn, ack: gwAck, run: runGw } = useAck();
  // P1-5: connecting spends a one-time reg-key + may provision a tunnel —
  // inline two-step confirm, copied from the memory_delete pattern
  // (MemoryPage): first click arms, second executes. Cancel disarms.
  const [gwConfirm, setGwConfirm] = useState(false);

  /** WHAT THE OPERATOR HAS TYPED INTO THE SEEDED FIELDS, where the read can see it. Every one of
   *  the five fields below records its edits here, and `keepEdits` puts them back over the device's
   *  answer at settle time — which is the difference between this read and the hand-rolled one it
   *  replaces (see the read's own comment). The registration key and the tunnel checkbox are NOT
   *  here: the answer never writes them, so there is nothing for it to clobber. */
  const editsRef = useRef<Partial<SettingsSeed>>({});
  /** RECORD AN EDIT AND SHOW IT — one helper, because a field that forgot to record would be a
   *  field the device's answer may silently replace, and five hand-written handlers is how one of
   *  them gets forgotten. */
  const edited =
    (field: keyof SettingsSeed, set: (v: string) => void) =>
    (e: ChangeEvent<HTMLInputElement>) => {
      editsRef.current[field] = e.target.value;
      set(e.target.value);
    };

  // THE MOUNT READ IS `useDeviceRead`'s (see its header): one GET of `/api/settings`, the refusal
  // guard, the unmount guard and the ordering guard. No `everyMs` — these fields are editable, so a
  // cadence would be a poll nobody asked for writing into a form.
  //
  // AND THE ANSWER COMES BACK OVER THE EDITS, which is why the read belongs on this seam. The
  // fields are editable from the first frame (carrying the defaults above), this route can take
  // seconds to answer (it shells out to `tasklist` for the tunnel state, and a relay adds more),
  // and the hand-rolled `.then` wrote every field it carried — so a value typed in the meantime was
  // silently replaced by the device's. `keepEdits` carries what was typed back OVER the answer,
  // which is also why the fields nobody has touched still take the device's values: the settle is
  // merged, never withheld.
  const { data: seed, read } = useDeviceRead<SettingsSeed>({
    path: "/api/settings",
    initial: SETTINGS_DEFAULTS,
    reduce: seedSettings,
    keepEdits: () => editsRef.current,
  });
  // THE SEED LANDS IN THE FIVE FIELDS — all of them, unconditionally, because the value ALREADY
  // carries the operator's edits: assigning a field they typed in writes their own text back. The
  // gateway sentence is the one exception, `null` meaning the reply had nothing to say about it.
  useEffect(() => {
    setBufferMb(seed.bufferMb);
    setGwUrl(seed.gwUrl);
    if (seed.gwStatus !== null) setGwStatus(seed.gwStatus);
    setMemEntries(seed.memEntries);
    setMemBytesMb(seed.memBytesMb);
    setMemRetention(seed.memRetention);
  }, [seed]);
  // A FAILED READ SAYS SO, in the sentence this page has always used for it. The module keeps the
  // fields on a failure (that rule is its own), so the sentence is all that is left to state here.
  useEffect(() => {
    if (read === "unreadable") setStatus("read failed");
  }, [read]);

  const { busy: saveBusy, ack: saveAck, run: runSave } = useAck();
  async function save() {
    const mb = Number(bufferMb);
    if (!Number.isFinite(mb) || mb < 1 || mb > 64) { setStatus("enter 1-64"); return; }
    // SPA audit LOW-4: unguarded Save double-clicked duplicate PUTs with
    // racing status.
    await runSave("save", async () => {
      try {
        const j = await callApi("/api/settings", { method: "PUT", body: JSON.stringify({ buffer_mb: mb }) });
        if (j && j.ok) setStatus("saved");
        else setStatus("save failed");
      } catch { setStatus("save failed"); }
    });
  }

  // Save memory capacity: entries + MiB required (>= 1); retention empty =
  // keep forever, otherwise >= 1 day. Applies immediately server-side.
  async function saveMemory() {
    const entries = Number(memEntries);
    const mb = Number(memBytesMb);
    const ret = memRetention.trim() === "" ? null : Number(memRetention);
    if (!Number.isInteger(entries) || entries < 1) { setMemStatus("entries must be >= 1"); return; }
    if (!Number.isInteger(mb) || mb < 1) { setMemStatus("MiB must be >= 1"); return; }
    if (ret !== null && (!Number.isInteger(ret) || ret < 1)) { setMemStatus("retention must be empty or >= 1 day"); return; }
    await runMem("memory", async () => {
      try {
        const j = await callApi("/api/settings", {
          method: "PUT",
          body: JSON.stringify({
            memory_max_entries: entries,
            memory_max_bytes_mb: mb,
            memory_retention_days: ret,
          }),
        });
        if (j && j.ok) setMemStatus("saved — applies immediately");
        else setMemStatus("save failed");
      } catch { setMemStatus("save failed"); }
    });
  }

  // Save gateway config + register + optional tunnel, one click.
  async function connectGateway() {
    if (!gwUrl.trim()) { setGwStatus("gateway URL required"); return; }
    setGwConfirm(false);
    await runGw("connect", async () => {
    setGwStatus("connecting…");
    try {
      const j = await callApi("/api/gateway/connect", {
        method: "POST",
        body: JSON.stringify({
          console_url: gwUrl.trim(),
          reg_key: gwKey.trim(),
          tunnel: gwTunnel,
        }),
      });
      if (j && j.ok) {
        const parts = [
          j.registered ? "registered" : "not registered",
          `tunnel: ${j.tunnel || "skipped"}`,
        ];
        setGwStatus(parts.join(" · "));
        // SPA audit MED-1: a used one-time reg-key must NOT linger in the
        // visible form (screen share / shoulder surf) — wipe it on success.
        setGwKey("");
      } else {
        await gwFailure("connect failed", String(j?.error || "connect failed"));
      }
    } catch (e: any) {
      await gwFailure("connect failed", String(e?.message || "connect failed"));
    }
    });
  }

  // SPA audit MED-2: the key is SPENT server-side before the slow tunnel
  // step — a client-side timeout (30s abort) makes the panel say "failed"
  // while the device is actually bound; re-sending burns a dead key. On ANY
  // failure with a key typed, re-read settings: a bound console_url means
  // "do not retry" — clear the field and say so.
  //
  // AND THIS READ STAYS A DIRECT `callApi`, deliberately — it is NOT a seed. Its answer decides a
  // WRITE-side question (whether to wipe a spent key) and it writes its two fields by hand, so
  // folding it into the module's `refresh` would run it through `keepEdits`, where the key the
  // operator typed is an edit that wins — and the wipe this function exists to perform could then
  // never happen. The module owns the read that SEEDS this page's form, not this one.
  async function gwFailure(prefix: string, msg: string) {
    if (!gwKey.trim()) { setGwStatus(msg || prefix); return; }
    try {
      const st = await callApi("/api/settings");
      if (st && st.console_url) {
        setGwKey("");
        setGwStatus(`${msg || prefix} — but the gateway IS bound now: do NOT re-send the same key`);
        return;
      }
    } catch { /* fall through to the plain error */ }
    setGwStatus(msg || prefix);
  }

  return (
    <div className="desktop-settings">
      {/* ONE h1, FOR THE OUTLINE ONLY (round 199). The shell renders the page's VISIBLE title in the card header, and
          `DesktopShell` mounts a hidden h1 for the terminal page for exactly this reason — its own comment records that the
          desktop measured ZERO headings while every other page had one. A visible h1 here made Settings the only page that
          said its own name twice on screen. */}
      <h1 className="sr-only">Settings</h1>
      {/* AND THE ADDRESS IS THE DEVICE'S, NOT A LITERAL (round 199). It was 127.0.0.1:18080, which is the local agent's port
          and a lie for every remote or tunnelled user — the rule `ConnectCard` states two files away: "A hardcoded value here
          would be wrong for every remote/tunnel user." `location.host` is the same string the browser itself is talking to. */}
      <p className="muted">Device: local agent on {location.host}</p>
      {/* AND WHAT IT IS BOUND TO, WHICH IS A DIFFERENT FACT. The line above is the address this browser reached the agent
          on; this is the address the agent was CONFIGURED to listen on. They agree for a local browser and differ for a
          relayed or tunnelled caller — and until 1.2.448 the configured bind existed only inside config.yaml on the device,
          so "where is that set?" had no answer anywhere in the interface. The second half is printed ONLY for a loopback
          bind: there it is both true and useful, and for a network-bound device it would be a lie. */}
      {config?.host ? (
        <p className="muted">
          Bound to {config.host}
          {typeof config.port === "number" ? `:${config.port}` : ""}
          {isLoopback(config.host)
            ? " — this machine only. To reach it from another one: the relay, a VPN, or ssh -L."
            : " — reachable on the network; keep the device token secret."}
        </p>
      ) : null}

      {/* Onboarding FIRST. Until an AI client is pointed here, none of the rest
          of this page matters — the measured gap this card closes was that a
          new user's first screen was a terminal and the product's promise was
          invisible. See docs/adr/proposal-game-design.md §4. */}
      {/* THE TWO THINGS THE OPERATOR NAMED, ON THE SURFACE (1.2.448). He asked for the device token and the config file by
          name and said the rest of this page was clutter. The token is this device's credential and the panel already holds
          it; the config file is where every value below comes from. Each used to require knowing where to look — one inside a
          client snippet, the other inside a YAML file on disk. */}
      <p className="muted">
        Device token: <code>{revealed ? token : "••••••••••••"}</code>{" "}
        <button className="btn" onClick={() => setRevealed((v) => !v)}>{revealed ? "Hide" : "Reveal"}</button>{" "}
        <button className="btn" onClick={() => void navigator.clipboard?.writeText(token)}>Copy</button>
      </p>
      {config?.path ? (
        <p className="muted">Config file: <code>{config.path}</code></p>
      ) : null}

      {/* FOLDED, NOT DELETED. The client snippets are what a new client needs and the diagnostics are what a BROKEN device
          needs; neither is a setting, and both were competing with the twenty controls that are. A `details` keeps them one
          click away and out of the page's reading order. */}
      <details className="settings-fold">
        <summary>Connect an AI client</summary>
        <ConnectCard />
      </details>

      <details className="settings-fold">
        <summary>Diagnostics</summary>
      <DeviceHealthCard series={vitals ?? EMPTY_SERIES} failed={vitalsFailed} />

      <UpdateSection runningRelease={runningRelease} />

      <NotificationsCard
        permission={notifyPermission ?? "unsupported"}
        onRequest={onRequestNotify ?? (async () => "unsupported" as const)}
        onTest={onTestNotify ?? (() => {})}
        attention={attention ?? []}
      />

      <MonitorsCard
        monitors={monitors ?? EMPTY_MONITORS}
        failed={monitorsFailed}
        onAdd={onMonitorAdd ?? (async () => ({ ok: false, error: "not wired" }))}
        onRemove={onMonitorRemove ?? (() => {})}
        onProbe={onMonitorProbe ?? (() => {})}
      />

      <DeviceLogsCard />

      <RestartHistoryCard history={restarts ?? EMPTY_BOOT_HISTORY} failed={restartsFailed} />
      </details>

      <div className="settings-section">
        <h2>Gateway</h2>
        <p className="muted">
          Optional — connect this device to a Summrise gateway console so remote clients can use its
          terminal / browser / memory. Pure local mode needs none of this.
        </p>
        <div className="settings-gw-form">
          <input
            /* no class: `input:not([type])` and `input[type=password]` are styled by the sheet's generic input
               rule, and a class that matches no rule is a name a reader has to check (round 250). */
            placeholder="Gateway URL (e.g. https://gateway.example.com)"
            value={gwUrl}
            onChange={edited("gwUrl", setGwUrl)}
            aria-label="Gateway URL"
          />
          <input
            /* no class: `input:not([type])` and `input[type=password]` are styled by the sheet's generic input
               rule, and a class that matches no rule is a name a reader has to check (round 250). */
            type="password"
            placeholder="Registration key (optional — generate at the console)"
            value={gwKey}
            onChange={(e) => setGwKey(e.target.value)}
            aria-label="Registration key"
            autoComplete="off"
          />
          <label className="settings-check">
            <input type="checkbox" checked={gwTunnel} onChange={(e) => setGwTunnel(e.target.checked)} />
            <span>Public access (free cloudflared tunnel)</span>
          </label>
          <div className="settings-actions">
            {gwConfirm ? (
              <>
                <span className="mem-confirm-hint">save & connect?</span>
                <button className="btn btn-danger btn-mini" onClick={connectGateway} disabled={gwBusy} {...gwAck("connect")}>
                  {gwBusyOn === "connect" ? "Connecting…" : "Connect"}
                </button>
                <button className="btn btn-ghost btn-mini" onClick={() => setGwConfirm(false)} disabled={gwBusy} {...gwAck("cancel")}>Cancel</button>
              </>
            ) : (
              <button className="btn btn-ghost btn-mini" onClick={() => setGwConfirm(true)} disabled={gwBusy} {...gwAck("arm")}>
                Save & connect
              </button>
            )}
          </div>
        </div>
        {gwStatus && <p className="hint settings-status">{gwStatus}</p>}
      </div>

      <div className="settings-section">
        <h2>Session buffer</h2>
        <p className="muted">
          Output recall per terminal session (memory + spill file, ~2x this). 1-64.
          Applies to new output; persisted across restarts.
        </p>
        <div className="settings-row-bar">
          <input
            className="settings-input-narrow"
            type="number"
            min={1}
            max={64}
            step={1}
            value={bufferMb}
            onChange={edited("bufferMb", setBufferMb)}
            aria-label="Session buffer MiB"
          />
          <button className="btn btn-ghost btn-mini" onClick={save} disabled={saveBusy} {...saveAck("save")}>Save</button>
        </div>
        {status && <p className="hint">{status}</p>}
      </div>

      {/* Desktop-app card — only in the Electron shell (window.summriseDesktop bridge). */}
      <div className="settings-section">
        <h2>Desktop app</h2>
        <p className="muted">
          Start Summrise Desktop automatically when you log in to this machine.
        </p>
        <label className="settings-check">
          <input
            type="checkbox"
            checked={autoLaunch}
            disabled={autoLaunchBusy || !hasDesktopBridge}
            {...autoLaunchAck("autolaunch")}
            onChange={(e) => setAutoLaunch(e.target.checked)}
          />
          <span>Start on login{!hasDesktopBridge ? " (desktop app only)" : ""}</span>
        </label>
        {autoLaunchStatus && <p className="hint">{autoLaunchStatus}</p>}
      </div>

      <div className="settings-section">
        <h2>Memory</h2>
        <p className="muted">
          Memory entries live in <code>&lt;install&gt;/memory/memory.jsonl</code>, shared across
          AI clients (Claude Code / DSH / this desktop). Capacity applies
          immediately and persists across restarts; retention empty = keep
          forever. AI clients save knowledge via <code>memory_save</code> and
          query via <code> memory_search</code>.
        </p>
        <div className="settings-row-bar">
          <input
            className="settings-input-narrow"
            type="number"
            min={1}
            step={1}
            value={memEntries}
            onChange={edited("memEntries", setMemEntries)}
            aria-label="Memory max entries"
          />
          <input
            className="settings-input-narrow"
            type="number"
            min={1}
            step={1}
            value={memBytesMb}
            onChange={edited("memBytesMb", setMemBytesMb)}
            aria-label="Memory max MiB"
          />
          <input
            className="settings-input-narrow"
            type="number"
            min={1}
            step={1}
            value={memRetention}
            onChange={edited("memRetention", setMemRetention)}
            placeholder="retention days (empty = forever)"
            aria-label="Memory retention days"
          />
          <button className="btn btn-ghost btn-mini" onClick={saveMemory} disabled={memBusy} {...memAck("memory")} aria-label="Save memory capacity">Save</button>
        </div>
        {memStatus && <p className="hint">{memStatus}</p>}
        {onOpenMemory && <button className="btn btn-ghost btn-mini" onClick={onOpenMemory}>Open Memory</button>}
      </div>

      <div className="settings-section">
        <h2>Terminal</h2>
        <p className="muted">
          Sessions (PTY/SSH/serial) are held by the agent service — closing this
          window or refreshing never kills a running session. Reconnect via the
          + buttons or <code>terminal_connect_saved</code>.
        </p>
      </div>

      <div className="settings-section">
        <h2>Transport</h2>
        <p className="muted">
          The desktop shell talks to the agent over loopback HTTP/WS with the
          device token. No cloud dependency — gateway endpoints are optional.
        </p>
      </div>
    </div>
  );
}
