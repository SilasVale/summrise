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

/** HOW TO REACH THIS MACHINE — the sentence, and the one thing besides the bind that changes it.
 *
 *  `relayConfigured` IS THREE-VALUED, AND THE THREE VALUES ARE THREE DIFFERENT FACTS:
 *    - `false`     — the device ANSWERED, and its `server.relay_url` was empty. There is no relay, so offering one
 *                    is an instruction that cannot be followed.
 *    - `true`      — a relay is configured, so the list is true as written and the sentence is left alone.
 *    - `undefined` — the reply carried NO relay at all, or nobody has polled yet. **SILENCE IS NOT A "NO".**
 *                    `useAgentVitals` distinguishes "the body omitted `relay`" (it clears the field to null) from
 *                    "the body said `configured: false`" — and printing "no relay configured" off silence would be
 *                    the same lie in the other direction, about an agent that simply predates the field.
 *
 *  WHY THE CLAUSE NAMES `server.relay_url` AND WHEN IT WAS READ: the config file's path is printed two rows lower on
 *  this same card, so the sentence points at a file the reader can see. And `configured` is set ONCE at bind time
 *  (`agent/src/mcp/server.rs`, inside the relay-spawn block) with NOTHING watching the file afterwards — so an
 *  operator who hand-edits `config.yaml` and sees no change now has the reason on screen instead of a mystery.
 *
 *  AND IT DELIBERATELY DOES NOT SAY `unavailable`, `disconnected` OR `unreachable`: nothing was attempted and there is
 *  no relay to be down. Those words are reserved for `configured: true && connected: false`, which is a relay that
 *  exists and is failing — a different fact, and the one place they would be true.
 *
 *  NOT EXPORTED, and that is the gate's own rule rather than an oversight: `exports-check` refuses an export
 *  nobody outside the file uses, and every case here is covered through the RENDERED page (see the three relay
 *  states in `__tests__/SettingsPage.test.tsx`), which is the surface the operator actually reads. A second,
 *  string-level unit test would test the same three sentences twice. */
function reachabilityHint(host: string, relayConfigured?: boolean): string {
  const noRelay = relayConfigured === false;
  if (isLoopback(host)) {
    return noRelay
      ? "This machine only. To reach it from another one: a VPN or ssh -L. No relay configured — server.relay_url was empty when the agent started."
      : "This machine only. To reach it from another one: the relay, a VPN, or ssh -L.";
  }
  return noRelay
    ? "Reachable on the network; keep the device token secret. No relay configured — server.relay_url was empty when the agent started."
    : "Reachable on the network; keep the device token secret.";
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
    //
    // **AND SHOW WHAT WAS OBSERVED, NOT WHAT THE READER WANTS TO HEAR (round 359).** This used to push the LITERAL
    // `"connected"`, whose only input was `console_url` being a non-empty STRING — measured on a live device answering
    // `console_url=https://api.saisi.online`, where nothing on this path contacted the console, at all, ever. The two
    // facts the reply actually carries are STEPS: `tunnel_configured` is "tunnel.yml exists" and `tunnel_running` is a
    // `tasklist` SUBSTRING MATCH for `cloudflared.exe` (`agent/src/web/mod.rs`). So the line names each as the step it
    // is, and says outright that the one thing a reader would assume — that something verified reachability — did not
    // happen. A tunnel that is configured, running and reachable from nowhere is the ordinary case this hides.
    //
    // THE HONEST VOCABULARY ALREADY EXISTS AND IS NOT REACHABLE FROM HERE: `agent/src/tunnel.rs` reports
    // "verified via the API" / "provisioned … but NOT REACHABLE — the API reports status '…' with 0 live
    // connection(s); remote clients will get 530" / "reachability NOT VERIFIED (the API did not answer)". **NONE OF IT
    // IS PERSISTED**, so a page load cannot show it and this line is what remains. Persisting that verdict is the
    // larger fix; it is named here rather than attempted, because it is a change to the agent's stored state and not
    // to one sentence.
    //
    // `null` IS UNTOUCHED ABOVE: a reply that mentions no gateway still leaves `gwStatus` alone, because "this reply
    // said nothing about a gateway" is not "there is none" — the one place this card already behaved honestly, and an
    // empty `console_url` still renders no status line at all.
    const parts = ["console configured"];
    if (j.tunnel_configured)
      parts.push(j.tunnel_running ? "cloudflared.exe running" : "cloudflared.exe not running");
    next.gwStatus = `${parts.join(" · ")} (reachability not checked)`;
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
  relayConfigured,
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
  /** WHETHER A RELAY IS CONFIGURED, out of the same `/api/status` sample that carries `config` — and **THREE-VALUED ON
   *  PURPOSE**: `false` means the device ANSWERED that its `server.relay_url` was empty, while `undefined` means it said
   *  nothing about a relay at all (an older agent, or no poll yet). The reachability sentence prints "No relay
   *  configured" for the first and must not for the second — printing it off silence is the same lie in the other
   *  direction. See `reachabilityHint`. */
  relayConfigured?: boolean;
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
      {/* ── THE PAGE OPENED WITH FOUR FLAT PARAGRAPHS OF DIM PROSE, AND THEY ARE ALL ONE SUBJECT ────────────────
          Round 138 turned the settings into cards and round 141 followed the folds; this header kept the old shape, so
          the page still BEGAN with a wall of `muted` text before the first container — the operator's *"这个页面太过混乱"*
          surviving at the top after being fixed below. **Four lines about one machine are a card about one machine.**
          The reachability sentence is NOT deleted: it is genuinely useful and it is REFERENCE, so it becomes the card's
          quiet footnote instead of its opening sentence. */}
      <div className="settings-section">
        <h2>This device</h2>
      {/* **TWO LINES DELETED, AND NEITHER LOSES ANYTHING.**
          `Local agent on {location.host}` printed the address the pinned status strip already prints on every page
          (`127.0.0.1:18080`), so the reader met the same string twice, twenty pixels apart.
          And `Bound to …` was rendered unconditionally, so for a local browser — the normal case — it repeated the
          line above it VERBATIM. It is a different fact only when the two disagree, and that is now the only time
          it is shown. Both deletions are the class this page kept failing at: prose that restates something the
          screen is already saying. */}
      {config?.host &&
      `${config.host}${typeof config.port === "number" ? `:${config.port}` : ""}` !== location.host ? (
        <p className="muted">
          Bound to {config.host}
          {typeof config.port === "number" ? `:${config.port}` : ""}
        </p>
      ) : null}
      {config?.host ? (
        <p className="hint">{reachabilityHint(config.host, relayConfigured)}</p>
      ) : null}

      {/* Onboarding FIRST. Until an AI client is pointed here, none of the rest
          of this page matters — the measured gap this card closes was that a
          new user's first screen was a terminal and the product's promise was
          invisible. See docs/adr/proposal-game-design.md §4. */}
      {/* THE TWO THINGS THE OPERATOR NAMED, ON THE SURFACE (1.2.448). He asked for the device token and the config file by
          name and said the rest of this page was clutter. The token is this device's credential and the panel already holds
          it; the config file is where every value below comes from. Each used to require knowing where to look — one inside a
          client snippet, the other inside a YAML file on disk. */}
      {/* **AND THE TOKEN WAS THE THIRD LINE OF A GREY PARAGRAPH.** An independent review measured it: the credential the
          operator asked for BY NAME sat in `p.muted` — `--muted` ink, with `<code>` rendering one size SMALLER than the
          sentence holding it — while two `<button>`s floated on the prose baseline, positioned by literal `{" "}` text
          nodes. The most important fact on the card was typographically the least important thing in it. It has its own
          row now, built from `settings-row-bar` (the same bar the Session buffer and Memory fields use), which already
          wraps — so a long token pushes the buttons down instead of overflowing. */}
      <div className="settings-row-bar">
        <span className="muted">Device token:</span>
        <code>{revealed ? token : "••••••••••••"}</code>
        <button className="btn" onClick={() => setRevealed((v) => !v)}>{revealed ? "Hide" : "Reveal"}</button>
        <button className="btn" onClick={() => void navigator.clipboard?.writeText(token)}>Copy</button>
      </div>
      {config?.path ? (
        <p className="muted">Config file: <code>{config.path}</code></p>
      ) : null}
      </div>

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
                <button className="btn btn-primary btn-mini" onClick={connectGateway} disabled={gwBusy} {...gwAck("connect")}>
                  {gwBusyOn === "connect" ? "Connecting…" : "Connect"}
                </button>
                <button className="btn btn-ghost btn-mini" onClick={() => setGwConfirm(false)} disabled={gwBusy} {...gwAck("cancel")}>Cancel</button>
              </>
            ) : (
              <button className="btn btn-primary btn-mini" onClick={() => setGwConfirm(true)} disabled={gwBusy} {...gwAck("arm")}>
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

      {/* **A CARD WHOSE ONLY CONTROL CANNOT BE REACHED BY THE READER LOOKING AT IT.** In a plain browser
          `window.summriseDesktop` is absent, so this card rendered a 17px heading, a sentence, and a
          checkbox that was **DISABLED** — a control permanently out of reach, sitting beside live ones.
          It renders only where the bridge exists now, which is the Electron shell, where it works.

          **AND THE SENTENCE ABOVE THE CHECKBOX WENT WITH IT**, for the reason the first review of this
          page gave: "Start Summrise Desktop automatically when you log in to this machine" sat 16px above
          a checkbox whose own label is `Start on login`. **Prose restating the control underneath it** —
          the same class as the `Local agent on …` line and the `Bound to …` echo deleted in 1.2.489.
          **What is lost: nothing. The label says it, and it is the label of the thing it describes.** */}
      {hasDesktopBridge && (
        <div className="settings-section">
          <h2>Desktop app</h2>
          <label className="settings-check">
            <input
              type="checkbox"
              checked={autoLaunch}
              disabled={autoLaunchBusy}
              {...autoLaunchAck("autolaunch")}
              onChange={(e) => setAutoLaunch(e.target.checked)}
            />
            <span>Start on login</span>
          </label>
          {autoLaunchStatus && <p className="hint">{autoLaunchStatus}</p>}
        </div>
      )}

      <div className="settings-section">
        <h2>Memory</h2>
        <p className="muted">
          Memory entries live in <code>&lt;install&gt;/memory/memory.jsonl</code>.
        </p>
        {/* **THREE IDENTICAL BOXES AND NOT ONE OF THEM SAID WHAT IT WAS.** Measured by an independent
            review of this page: `grep '<label'` found only the two that wrap checkboxes, so every
            number field here carried an `aria-label` — invisible — and nothing else. A reader could
            not tell entries from MiB from days; the only hint was one placeholder on the third. The
            paragraph above made it worse by ending with "retention empty = keep forever", which is
            the placeholder restated. `settings-check` is reused rather than a new class invented:
            it is already `flex` + `gap: 8px`, which is exactly a label beside its control. */}
        <div className="settings-row-bar">
          <label className="settings-check">
            <span>Entries</span>
            <input
              className="settings-input-narrow"
              type="number"
              min={1}
              step={1}
              value={memEntries}
              onChange={edited("memEntries", setMemEntries)}
              aria-label="Memory max entries"
            />
          </label>
          <label className="settings-check">
            <span>MiB</span>
            <input
              className="settings-input-narrow"
              type="number"
              min={1}
              step={1}
              value={memBytesMb}
              onChange={edited("memBytesMb", setMemBytesMb)}
              aria-label="Memory max MiB"
            />
          </label>
          <label className="settings-check">
            <span>Retention days</span>
            <input
              className="settings-input-narrow"
              type="number"
              min={1}
              step={1}
              value={memRetention}
              onChange={edited("memRetention", setMemRetention)}
              placeholder="forever"
              aria-label="Memory retention days"
            />
          </label>
          <button className="btn btn-ghost btn-mini" onClick={saveMemory} disabled={memBusy} {...memAck("memory")} aria-label="Save memory capacity">Save</button>
        </div>
        {memStatus && <p className="hint">{memStatus}</p>}
      </div>
    </div>
  );
}
