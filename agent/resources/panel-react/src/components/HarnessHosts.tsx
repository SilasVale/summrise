// HarnessHosts — the LEFT pane of the harness page: the hosts this device can work on, each with its own
// harness on the right.
//
// WHAT THIS COLUMN IS NOT. It is not a file browser, an editor, a command line or a terminal. Those belong
// to the harness on the far side, which is the arrangement the operator chose: hosts on the left, the
// SELECTED host's own sessions and files on the right. The earlier version of this page drew all of them
// in the panel, and that was a second implementation of something the host already has.
//
// WHAT IT DOES, in three jobs and no more:
//   1. ask the agent where each host's harness is (`/api/workspace/harnesses`);
//   2. say what the agent last OBSERVED there, and how long ago — never a bare verdict, because a row that
//      reads "answering" with the probe three hours old is a claim the reader cannot act on;
//   3. switch the view, THROUGH the main process, which is what checks the address against the doors the
//      agent configured. The panel never navigates the native view itself, and it cannot point it
//      somewhere the agent did not name.

import { useCallback, useEffect, useState } from "react";
import { callApi } from "../lib/api";
import { dshBridge } from "../lib/embeddedBridge";

/** One row as the agent sends it. `state` is the agent's word for what it observed, not this file's. */
type Host = {
  connection_id: string;
  local_port: number;
  remote_port: number;
  url: string;
  state: string;
  checked_ms: number | null;
  probe: string | null;
};

/** `zhengsaisi@10.10.61.83:22122` → `zhengsaisi@10.10.61.83`, the part a person recognises. */
function nameOf(id: string): string {
  const m = /^ssh:(.+?)@([^:]+)/.exec(id);
  if (!m) return id;
  return m[1].includes("@") || !m[2] ? id : `${m[1]}@${m[2]}`;
}

/**
 * How old a claim is, in the words a reader can act on. A number here would be the unit doing the
 * explaining; this is the same idea the panel's own `checked 12s ago` uses, and the reason a stale
 * `answering` cannot pass for a current one.
 */
function ageOf(checkedMs: number | null, now: number): string | null {
  if (checkedMs === null) return null;
  const s = Math.max(0, Math.round((now - checkedMs) / 1000));
  if (s < 60) return `${s}s ago`;
  const m = Math.round(s / 60);
  if (m < 60) return `${m}m ago`;
  const h = Math.round(m / 60);
  if (h < 24) return `${h}h ago`;
  return `${Math.round(h / 24)}d ago`;
}

export function HarnessHosts({ selected, onSelect }: { selected?: string | null; onSelect?: (id: string) => void } = {}) {
  const [hosts, setHosts] = useState<Host[] | null>(null);
  const [refused, setRefused] = useState<string | null>(null);
  const [now, setNow] = useState(() => Date.now());

  // `callApi`, NOT a bare fetch: it carries the device token, resolves the host and the protocol, and
  // bounds the wait. A raw `fetch("/api/…")` from the panel reaches the agent with no `authorization`
  // header at all, and every workspace door is auth-gated — so it answers 401 on a device and nowhere
  // else. A test that mocks `fetch` cannot see that, which is the reason this note exists.
  const read = useCallback(async () => {
    try {
      const j = await callApi("/api/workspace/harnesses", {
        method: "POST",
        body: JSON.stringify({ action: "list" }),
      });
      if (j?.ok === false) {
        setHosts(null);
        setRefused(`the agent did not answer (${j?.error ?? j?.code ?? "no reason given"})`);
        return;
      }
      setHosts(Array.isArray(j?.harnesses) ? (j.harnesses as Host[]) : []);
      setRefused(null);
    } catch (e) {
      setHosts(null);
      setRefused(`the agent did not answer (${e instanceof Error ? e.message : String(e)})`);
    }
  }, []);

  useEffect(() => {
    void read();
    // The column's ages move while it is open, so a claim does not sit there reading as current for an
    // hour. A minute is the same interval the panel's own relative times use.
    const t = setInterval(() => setNow(Date.now()), 60_000);
    return () => clearInterval(t);
  }, [read]);

  const select = useCallback(async (host: Host) => {
    onSelect?.(host.connection_id);
    const b = dshBridge();
    if (!b) return;
    // The main process answers with the address it ACTUALLY loaded, which for a refusal is
    // `about:blank` — so the switch is reported as it happened rather than as a click.
    const answer = (await b.go(host.url)) as { url?: string } | undefined;
    if (answer?.url && answer.url !== host.url) {
      setRefused(`that host's harness is not on ${host.url} — the view stayed at ${answer.url}`);
    } else {
      setRefused(null);
    }
  }, [onSelect]);

  return (
    <div className="hosts-pane">
      <div className="hosts-pane-head">
        <span className="hosts-pane-title">Hosts</span>
        <button className="btn btn-mini" onClick={() => void read()}>
          Refresh
        </button>
      </div>

      {refused ? (
        <p className="hosts-pane-refusal" role="status">
          {refused}
        </p>
      ) : null}

      {hosts === null ? (
        <p className="hosts-pane-note">reading the agent…</p>
      ) : hosts.length === 0 ? (
        <p className="hosts-pane-note">No host yet — add one and point a forward at its harness.</p>
      ) : (
        <ul className="hosts-list">
          {hosts.map((h) => {
            const isOpen = selected === h.connection_id;
            const age = ageOf(h.checked_ms, now);
            return (
              <li key={h.connection_id} className="hosts-row" data-open={isOpen ? "yes" : "no"}>
                <button className="hosts-row-name" onClick={() => void select(h)} title={h.connection_id}>
                  {nameOf(h.connection_id)}
                </button>
                {/* THE STATE AND THE SENTENCE BEHIND IT, plus how old it is. A row that printed only
                    "answering" would be a claim with no date on it, which is the failure this repository
                    measured twice before writing the rule it now keeps. */}
                <span className="hosts-row-state" data-state={h.state}>
                  {h.state}
                  {age ? ` · ${age}` : ""}
                </span>
                {h.probe ? <span className="hosts-row-probe">{h.probe}</span> : null}
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
