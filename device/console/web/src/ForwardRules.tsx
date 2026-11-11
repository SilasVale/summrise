import { useCallback, useEffect, useRef, useState, type FormEvent } from "react";
import {
  addForward,
  forwardFailure,
  listForwards,
  removeForward,
  type ForwardRow,
  type NewForward
} from "./forwardApi";
import { spanText } from "./age";
import { Sheet } from "./Sheet";
import type { Grant } from "./terminal";

/**
 * The machine's forward rules: what is listening, what it carries, and the two acts that change it.
 *
 * **WHY THIS IS ON THE MACHINE TAB AND NOT IN THE TOOL CATALOGUE.** The catalogue's own sheet says
 * it "does not invoke anything … so the catalogue cannot become a REPL with the operator's
 * authority". A rule is also not a thing a client calls: it is a listening port and an SSH hop that
 * runs with NOBODY PRESENT, which is a fact about the machine standing here, not a capability a
 * caller reaches for. So it is a section of the machine's page, with the machine's own sections'
 * shape, and the two writes live where a person reads them.
 *
 * **THE CREDENTIAL IS TYPED HERE AND NEVER SHOWN AGAIN.** The device stores it with the row (a rule
 * outlives its caller, so its executor must reach `via` at boot with nobody present), and
 * `forward.list` reports the row WITHOUT it. That is the whole reason this page can draw a list at
 * all: two copies of a secret — one on the machine and one in a browser — is one more than the
 * product means to keep. The form's state is dropped the moment it closes.
 *
 * **A RULE'S STATE IS OBSERVED, NOT HOPED FOR.** `listening` is bind() having returned, `since_ms` is
 * when that began, `connections` counts connections CARRIED (both directions ended without an error),
 * and `attempts` is how many times the bind ladder has tried. One rule can therefore read
 * `not listening · 3 bind attempts · last error: …` while another reads `listening for 2 h · 41 carried`
 * — and the first is the one that needs a person.
 */

/** How a rule's state is labelled. A chip carries a WORD, never a colour alone. */
function stateOf(rule: ForwardRow): { label: string; className: string } {
  if (rule.listening) return { label: "Listening", className: "chip chip-ok" };
  if (rule.last_error !== null) return { label: "Not listening", className: "chip chip-bad" };
  return { label: "Starting", className: "chip chip-warn" };
}

function listenSentence(rule: ForwardRow): string {
  if (!rule.listening || rule.since_ms === null) {
    return rule.attempts === 0 ? "binding now" : `${rule.attempts} bind attempts`;
  }
  return `bound ${spanText(Math.max(0, Date.now() - rule.since_ms))}`;
}

function carriedSentence(count: number): string {
  // **"CARRIED", NOT "CONNECTIONS".** The device's own doc says the count is both directions ended
  // without an error — a connection that was accepted and refused is a log line, not a count. A
  // page that said "connections" would make the number mean "arrived" while the device means "worked".
  return `${count} ${count === 1 ? "connection" : "connections"} carried`;
}

/**
 * The five values, as a form.
 *
 * `CONNECT` is not one of them: `via` names the connection AND `auth` is how it authenticates, and
 * the device refuses a rule whose credential it cannot use. The form therefore asks for both, and
 * the key path is the only credential a person can re-open later — a password is typed once and
 * stored inside the rule.
 */

/**
 * The form's own shape: the five values plus the credential it is still typing.
 *
 * `host_key` is a REQUIRED string here while it is optional on the wire: the form has a field, and
 * a field's value is "" until somebody types in it. `ruleOf` is where "" becomes "absent", because
 * the device's schema refuses an empty string and means "the device's known_hosts decides" by the
 * field being gone.
 */
type Draft = {
  name: string;
  listen: string;
  target: string;
  via: string;
  auth: NewForward["auth"];
  authKind: "password" | "key";
  password: string;
  keyPath: string;
  host_key: string;
};

/** What the form holds when it opens: every field empty, the credential a password. */
const EMPTY: Draft = {
  name: "",
  listen: "",
  target: "",
  via: "",
  auth: { kind: "password", password: "" },
  authKind: "password",
  password: "",
  keyPath: "",
  host_key: ""
};

function ruleOf(draft: Draft): NewForward {
  return {
    name: draft.name.trim(),
    listen: draft.listen.trim(),
    target: draft.target.trim(),
    via: draft.via.trim(),
    auth:
      draft.authKind === "password"
        ? { kind: "password", password: draft.password }
        : { kind: "key", path: draft.keyPath.trim() },
    host_key: draft.host_key.trim()
  };
}

/** What the form refuses before it asks the device. The device refuses these too; both is on purpose. */
function draftProblem(draft: Draft): string | null {
  for (const [field, value] of [
    ["Name", draft.name],
    ["Listen", draft.listen],
    ["Target", draft.target],
    ["Via", draft.via]
  ] as const) {
    if (value.trim() === "") return `${field} is required.`;
  }
  for (const [field, value] of [
    ["Listen", draft.listen],
    ["Target", draft.target]
  ] as const) {
    // The device splits on the LAST colon, so a bare host is a rule with no port, and a wildcard is
    // refused outright. Naming both here means the form does not round-trip to say so.
    const host = value.trim().slice(0, value.trim().lastIndexOf(":"));
    if (host === "" || host === value.trim()) return `${field} must be host:port.`;
    if (field === "Listen" && [ "0.0.0.0", "::", "*" ].includes(host.trim())) {
      return "A wildcard listen is refused: name the address whose audience you chose.";
    }
  }
  const via = draft.via.trim();
  if (!via.includes("@")) return "Via must be user@host:port.";
  if (draft.authKind === "password") {
    if (draft.password === "") return "Type the password the `via` connection uses, or choose a key.";
  } else {
    const path = draft.keyPath.trim();
    if (path === "") return "Give the absolute path of the key, or choose a password.";
    // The device requires an absolute path: a relative one is resolved against whatever the service
    // is running from, which is not a place a key was put.
    if (!path.startsWith("/") && !/^[A-Za-z]:[\\/]/.test(path)) {
      return "The key path must be absolute: the rule outlives this page, so it cannot depend on where a path was typed.";
    }
  }
  return null;
}

export function ForwardRules({
  grant,
  deviceOnline
}: {
  grant: Grant;
  /// The fleet's reachability fact; only `false` is a fact, `undefined` is "no opinion".
  deviceOnline?: boolean;
}) {
  const [rows, setRows] = useState<ForwardRow[] | null>(null);
  const [reading, setReading] = useState(false);
  const [readError, setReadError] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
  const [removing, setRemoving] = useState<ForwardRow | null>(null);
  const offline = deviceOnline === false;

  // One read in flight at a time, and its answer is applied only if this page is still the one that
  // asked: a section that unmounts mid-read must not set state.
  const live = useRef(true);
  useEffect(() => {
    live.current = true;
    return () => {
      live.current = false;
    };
  }, []);

  const read = useCallback(async () => {
    if (offline) return;
    setReading(true);
    setReadError(null);
    try {
      const next = await listForwards(grant);
      if (live.current) setRows(next);
    } catch (cause) {
      if (live.current) setReadError(forwardFailure(cause));
    } finally {
      if (live.current) setReading(false);
    }
  }, [grant, offline]);

  useEffect(() => {
    void read();
  }, [read]);

  return (
    <section className="system-section" aria-labelledby="system-forwards">
      <h3 id="system-forwards">Forward rules</h3>
      <p className="hint">
        A rule binds a port on this machine and carries it through one SSH connection, so the far
        host gains no listening port. It starts with the device and keeps running with nobody
        present — which is why its own values say who can reach what, and why the credential
        belongs to the rule rather than to whoever created it.
      </p>

      {offline ? (
        <p className="hint">
          The machine is offline; the rules it carries are read when it answers.
        </p>
      ) : readError !== null ? (
        <div className="notice" role="status">
          <span>{readError}</span>
          <button type="button" disabled={reading} onClick={() => void read()}>
            {reading ? "Reading…" : "Retry"}
          </button>
        </div>
      ) : rows === null ? (
        <div className="skeleton-list" aria-label="Reading the forward rules" role="status">
          {[0, 1].map((row) => (
            <div className="skeleton-row" key={row}>
              <span className="skeleton skeleton-name" />
              <span className="skeleton skeleton-line" />
            </div>
          ))}
        </div>
      ) : rows.length === 0 ? (
        <section className="empty-state">
          <p className="hint">
            No forward rules. A rule takes a port on this machine and carries it through one SSH
            connection to a host that opens nothing of its own.
          </p>
          <button type="button" className="primary" onClick={() => setAdding(true)}>
            Add a rule
          </button>
        </section>
      ) : (
        <div className="forward-list">
          {rows.map((rule) => (
            <article className="forward-row" key={rule.name}>
              <div className="forward-main">
                <div className="forward-head">
                  <span className="forward-name">{rule.name}</span>
                  <span className={stateOf(rule).className}>{stateOf(rule).label}</span>
                </div>
                {/* THE ARROW IS THE RULE, READ LEFT TO RIGHT: what is bound here, what it reaches,
                    and the connection that carries it. */}
                <p className="forward-route mono">
                  {rule.listen} → {rule.target} via {rule.via}
                </p>
                <p className="forward-meta">
                  {listenSentence(rule)} · {carriedSentence(rule.connections)}
                </p>
                {rule.last_error !== null && <p className="error">{rule.last_error}</p>}
              </div>
              <div className="forward-actions">
                <button
                  type="button"
                  className="mini"
                  onClick={() => setRemoving(rule)}
                  aria-label={`Remove the forward rule ${rule.name}`}
                >
                  Remove
                </button>
              </div>
            </article>
          ))}
        </div>
      )}

      {rows !== null && rows.length > 0 && (
        <div className="system-toolbar">
          <button
            type="button"
            className="mini"
            disabled={reading}
            onClick={() => void read()}
            title="Re-read every rule's live state"
          >
            {reading ? "Reading…" : "Refresh"}
          </button>
          <button type="button" className="mini" onClick={() => setAdding(true)}>
            Add a rule
          </button>
        </div>
      )}

      {adding && (
        <AddForwardSheet
          grant={grant}
          onClose={() => setAdding(false)}
          onAdded={() => {
            setAdding(false);
            void read();
          }}
        />
      )}

      {removing !== null && (
        <RemoveForwardSheet
          grant={grant}
          rule={removing}
          onClose={() => setRemoving(null)}
          onRemoved={() => {
            setRemoving(null);
            void read();
          }}
        />
      )}
    </section>
  );
}

function AddForwardSheet({
  grant,
  onClose,
  onAdded
}: {
  grant: Grant;
  onClose: () => void;
  onAdded: () => void;
}) {
  const [draft, setDraft] = useState<Draft>(EMPTY);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const set = (patch: Partial<Draft>) => setDraft((was) => ({ ...was, ...patch }));

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    const problem = draftProblem(draft);
    if (problem !== null) {
      setError(problem);
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await addForward(grant, ruleOf(draft));
      // **THE CREDENTIAL DIES WITH THIS FORM.** `setDraft(EMPTY)` runs on the way out, but the sheet
      // unmounts, which is what actually drops it — this call is the belt to that braces.
      setDraft(EMPTY);
      onAdded();
    } catch (cause) {
      setError(forwardFailure(cause));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Sheet
      title={draft.name.trim() === "" ? "Add a forward rule" : `Forward as ${draft.name.trim()}`}
      onClose={onClose}
      busy={busy}
      footer={
        <>
          <button type="button" disabled={busy} onClick={onClose}>
            Cancel
          </button>
          <button type="submit" form="add-forward" className="primary" disabled={busy}>
            {busy ? "Adding…" : "Add rule"}
          </button>
        </>
      }
    >
      <form id="add-forward" className="forward-form" onSubmit={(event) => void submit(event)}>
        <p className="hint">
          An edit is the same act: a rule whose name already exists is replaced, and its listener
          moves to the new address.
        </p>

        <div className="system-field">
          <label htmlFor="forward-name">Name</label>
          <input
            id="forward-name"
            value={draft.name}
            onChange={(event) => set({ name: event.target.value })}
            placeholder="console"
            autoComplete="off"
            spellCheck={false}
            required
          />
        </div>

        <div className="system-field">
          <label htmlFor="forward-listen">Listen (this machine)</label>
          <input
            id="forward-listen"
            value={draft.listen}
            onChange={(event) => set({ listen: event.target.value })}
            placeholder="100.85.163.159:8080"
            autoComplete="off"
            spellCheck={false}
            required
          />
        </div>

        <div className="system-field">
          <label htmlFor="forward-target">Target (the far side dials)</label>
          <input
            id="forward-target"
            value={draft.target}
            onChange={(event) => set({ target: event.target.value })}
            placeholder="10.10.61.83:7738"
            autoComplete="off"
            spellCheck={false}
            required
          />
        </div>

        <div className="system-field">
          <label htmlFor="forward-via">Via (the SSH connection)</label>
          <input
            id="forward-via"
            value={draft.via}
            onChange={(event) => set({ via: event.target.value })}
            placeholder="user@host:22"
            autoComplete="off"
            spellCheck={false}
            required
          />
        </div>

        <fieldset className="forward-auth">
          <legend>How the connection authenticates</legend>
          <label>
            <input
              type="radio"
              name="forward-auth"
              checked={draft.authKind === "password"}
              onChange={() => set({ authKind: "password" })}
            />
            Password
          </label>
          <label>
            <input
              type="radio"
              name="forward-auth"
              checked={draft.authKind === "key"}
              onChange={() => set({ authKind: "key" })}
            />
            Key file
          </label>
        </fieldset>

        {draft.authKind === "password" ? (
          <div className="system-field">
            <label htmlFor="forward-password">Password the connection uses</label>
            <input
              id="forward-password"
              type="password"
              value={draft.password}
              onChange={(event) => set({ password: event.target.value })}
              autoComplete="off"
              required
            />
            <p className="hint">
              Stored inside this rule on the machine, because the rule connects at boot with nobody
              present. It is never shown again, here or anywhere.
            </p>
          </div>
        ) : (
          <div className="system-field">
            <label htmlFor="forward-key">Key path (on this machine)</label>
            <input
              id="forward-key"
              value={draft.keyPath}
              onChange={(event) => set({ keyPath: event.target.value })}
              placeholder="/home/user/.ssh/id_ed25519"
              autoComplete="off"
              spellCheck={false}
              required
            />
            <p className="hint">
              An absolute path on the machine, not a key's contents: the device reads the file when
              it connects.
            </p>
          </div>
        )}

        <div className="system-field">
          <label htmlFor="forward-hostkey">Host key fingerprint (optional)</label>
          <input
            id="forward-hostkey"
            value={draft.host_key}
            onChange={(event) => set({ host_key: event.target.value })}
            placeholder="SHA256:…"
            autoComplete="off"
            spellCheck={false}
          />
          <p className="hint">
            Without it the device's own known-hosts decides. With it, the rule checks the far side's
            key before it carries anything — the one object here that connects with nobody watching.
          </p>
        </div>

        {error !== null && <p className="error">{error}</p>}
      </form>
    </Sheet>
  );
}

function RemoveForwardSheet({
  grant,
  rule,
  onClose,
  onRemoved
}: {
  grant: Grant;
  rule: ForwardRow;
  onClose: () => void;
  onRemoved: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const remove = async () => {
    setBusy(true);
    setError(null);
    try {
      await removeForward(grant, rule.name);
      onRemoved();
    } catch (cause) {
      setError(forwardFailure(cause));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Sheet
      title={`Remove ${rule.name}?`}
      onClose={onClose}
      busy={busy}
      footer={
        <>
          <button type="button" disabled={busy} onClick={onClose}>
            Cancel
          </button>
          <button type="button" className="danger" disabled={busy} onClick={() => void remove()}>
            {busy ? "Removing…" : "Remove"}
          </button>
        </>
      }
    >
      <div className="remove-plugin">
        <p>
          Connections through {rule.listen} stop being carried, and the address becomes bindable
          again. The credential stored with this rule is deleted with it.
        </p>
        {rule.connections > 0 && (
          <p className="hint">
            This rule has carried {rule.connections}{" "}
            {rule.connections === 1 ? "connection" : "connections"} since the device started.
          </p>
        )}
        {error !== null && <p className="error">{error}</p>}
      </div>
    </Sheet>
  );
}
