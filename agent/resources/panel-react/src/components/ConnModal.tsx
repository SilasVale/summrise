import { useEffect, useRef, useState } from "react";
import { callApi } from "../lib/api";
import { useAck } from "../lib/useAck";
import { useDeviceRead } from "../hooks/useDeviceRead";

/** "No saved connections yet" — the value before the first successful read. The modal works without
 *  them (the picker is hidden while the list is empty), so this is also what a device that keeps no
 *  connection memory shows. */
const NO_SAVED: any[] = [];

// SSH / Serial connection modal (migrated from vanilla panel.js showModal/
// connectModal). Includes the round-70 "Saved connections" dropdown that
// pre-fills the fields from the device's connection memory.
//
// THE LIST IS A `useDeviceRead` READ (that module's header names its readers), and this is the caller
// that needed `read`: the device's connection memory is a TOOL, so the request is a POST carrying a
// JSON body to `/api/tools/terminal_saved_connections` — a request no `path` can state. The
// UNWRAP stays in the fold, where it has always been, because the module's refusal guard reads the
// envelope of whatever the read resolves (`{ok, result}`); `read`'s own doc has why `callTool`
// cannot be handed over instead.
export function ConnModal({ kind, onClose, onConnect }: {
  kind: "ssh" | "serial";
  onClose: () => void;
  onConnect: (target: string, extra: Record<string, unknown>) => Promise<unknown>;
}) {
  const [status, setStatus] = useState("");
  // THE EARLY RETURNS USED TO CLEAR THE FLAG BY HAND, twice, and a third added later would have been a stuck
  // button. `run` clears it on every path (lib/useAck.ts).
  const { busy, ack, run } = useAck();
  // ssh fields
  const [host, setHost] = useState("");
  const [port, setPort] = useState("22");
  const [user, setUser] = useState("");
  const [pass, setPass] = useState("");
  const [keyPath, setKeyPath] = useState("");
  // serial fields
  const [sport, setSport] = useState("");
  const [baud, setBaud] = useState("115200");
  // P4b: serial auto-reconnect (unplug / device reboot → re-open same port)
  const [autoReconnect, setAutoReconnect] = useState(false);

  // Load saved connections (best-effort — the modal works without them). THE REQUEST IS THE
  // CALLER'S (`read`): the same bytes this modal has always sent — `callTool(name)` would build
  // exactly these — but stated as the POST itself, because the module's refusal guard reads the
  // envelope of what a read resolves and `callTool` has already unwrapped it (see `read`'s doc).
  //
  // AND THE FOLD IS WHAT THE HAND-WRITTEN `.then` DID: keep only the connections of THIS modal's
  // kind. A body this modal cannot use is a FAILED read rather than an empty list — the module then
  // keeps the last good list and reports it, where an empty picker would be this modal claiming the
  // device remembers no connections (the module's oldest rule).
  const { data: saved, refresh: refreshSaved } = useDeviceRead<any[]>({
    read: () =>
      callApi("/api/tools/terminal_saved_connections", {
        method: "POST",
        body: "{}",
      }),
    reduce: (_previous, body) => {
      const list = (
        body as { result?: { connections?: unknown } } | null | undefined
      )?.result?.connections;
      if (!Array.isArray(list)) {
        throw new Error(
          "terminal_saved_connections answered without a connections list",
        );
      }
      return list.filter((c: any) => (c.kind || "").startsWith(kind));
    },
    initial: NO_SAVED,
  });

  // AND A KIND SWITCH RE-READS, WHICH IS WHAT THIS MODAL HAS ALWAYS DONE (the effect that used to
  // hold the fetch was keyed on `kind`). It is deliberately NOT `resetKey`: that puts the value back
  // to `initial`, so the dropdown would VANISH for one round trip and every field under it would
  // jump — while what changed here is a FILTER over one device resource, not the subject of the
  // read. `refresh` is the module's own "read now", so the previous list stays on screen until the
  // new one lands, exactly as it did before the migration.
  //
  // THE FIRST RUN IS SKIPPED because `useDeviceRead` already reads once when it is rendered; without
  // that, mounting the modal would make the same request twice.
  const mountReadDone = useRef(false);
  useEffect(() => {
    if (!mountReadDone.current) {
      mountReadDone.current = true;
      return;
    }
    void refreshSaved();
  }, [kind, refreshSaved]);

  // round-102: serial framing params (parity/data/stop) ride in the saved
  // connection's params — remember them on pick and merge into the connect.
  const savedParams = useRef<Record<string, unknown>>({});

  function pickSaved(id: string) {
    const c = saved.find((s) => s.id === id);
    if (!c || !c.target) return;
    savedParams.current = (c.params && typeof c.params === "object" ? c.params : {}) as Record<string, unknown>;
    if (kind === "ssh") {
      // round-102: portless targets ('user@host') never matched the
      // port-required regex and the pick failed silently. Port defaults to 22.
      const m = /^(.*)@(.*?)(?::(\d+))?$/.exec(c.target);
      if (m) { setHost(m[2]); setPort(m[3] || "22"); setUser(m[1]); }
      // key_path rides in the saved params (a path, not a secret — the
      // password is stripped server-side before persisting).
      const kp = savedParams.current.key_path;
      setKeyPath(typeof kp === "string" ? kp : "");
    } else {
      setSport(c.target.split("?")[0]);
      const b = /baud=(\d+)/.exec(c.target || "");
      if (b) setBaud(b[1]);
    }
  }

  async function connect() {
    if (busy) return;
    await run("connect", async () => {
      try {
        if (kind === "ssh") {
          if (!host || !user) { setStatus("host + username required"); return; }
          const target = `${user}@${host}:${port}`;
          // key_path set → public-key auth server-side; the password field
          // doubles as the key passphrase.
          const extra: Record<string, unknown> = { password: pass };
          if (keyPath.trim()) extra.key_path = keyPath.trim();
          await onConnect(target, extra);
        } else {
          if (!sport) { setStatus("port required"); return; }
          // round-102: replay the saved framing params (parity/data/stop) so a
          // reconnect preserves the link config.
          const extra: Record<string, unknown> = { ...savedParams.current };
          delete extra.password; // never send credentials through here
          if (autoReconnect) extra.auto_reconnect = true;
          await onConnect(`${sport}?baud=${baud}`, extra);
        }
        // P2-2: the password/passphrase must not linger in the form state
        // after a successful connect (screen share / shoulder surf).
        setPass("");
        onClose();
      } catch (e: any) {
        setStatus(e.message || "connect failed");
      }
    });
  }

  // P2-2: dismissing the modal also wipes the password field — a cancelled
  // attempt leaves no secret in the (possibly re-opened) form state.
  function close() {
    setPass("");
    onClose();
  }

  const mkField = (label: string, value: string, set: (v: string) => void, placeholder: string, type = "text") => (
    <>
      {/* panel audit #3: label association via aria-label (ids would collide
          across the several mkField calls in one modal). */}
      <label>{label}</label>
      <input aria-label={label} value={value} onChange={(e) => set(e.target.value)} placeholder={placeholder} type={type} autoComplete="off" />
    </>
  );

  return (
    <div id="conn-modal" onClick={(e) => { if (e.target === e.currentTarget) close(); }}>
      <div className="modal-card">
        <h2>{kind === "ssh" ? "New SSH" : "New Serial"}</h2>
        {saved.length > 0 && (
          <>
            <label>Saved connections</label>
            <select id="saved-conn" onChange={(e) => pickSaved(e.target.value)} defaultValue="">
              <option value="">— pick a saved connection —</option>
              {saved.map((c) => (
                <option key={c.id} value={c.id}>{c.label || c.target} ({c.id})</option>
              ))}
            </select>
          </>
        )}
        {kind === "ssh" ? (
          <>
            {mkField("Host", host, setHost, "host.example.com")}
            {mkField("Port", port, setPort, "22")}
            {mkField("Username", user, setUser, "user")}
            {mkField("Private key path (optional)", keyPath, setKeyPath, "C:\\Users\\me\\.ssh\\id_ed25519")}
            {mkField(keyPath.trim() ? "Key passphrase (optional)" : "Password (optional)", pass, setPass, keyPath.trim() ? "leave empty for unencrypted key" : "leave empty for keychain", "password")}
          </>
        ) : (
          <>
            {mkField("Port", sport, setSport, "COM3 or /dev/ttyUSB0")}
            {mkField("Baud rate", baud, setBaud, "115200")}
            <label className="settings-check">
              <input type="checkbox" checked={autoReconnect} onChange={(e) => setAutoReconnect(e.target.checked)} />
              <span>Auto-reconnect (reconnect after cable unplug or device restart)</span>
            </label>
          </>
        )}
        <div className="modal-actions">
          <button onClick={close}>Cancel</button>
          <button className="primary" onClick={connect} disabled={busy} {...ack("connect")}>Connect</button>
        </div>
        <div id="modal-status" className={status.startsWith("host") || status.startsWith("port") ? "error" : ""}>{status}</div>
      </div>
    </div>
  );
}
