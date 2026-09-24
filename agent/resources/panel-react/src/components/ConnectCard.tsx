// ConnectCard — the panel's AI-client onboarding (game-design proposal §4).
//
// WHY THIS EXISTS. Measured: the panel had ZERO onboarding for AI clients. A
// new user installs Summrise, opens the panel, and sees a terminal. The product's
// promise — "AI can drive this machine" — is invisible until an external client
// is configured by hand, and nothing in the UI helped configure one. In game
// terms: no tutorial, and the game does not start until you edit a config file.
//
// The card does three things, in the order a new user needs them:
//   1. says WHAT this machine can be asked to do (the tool surface, grouped)
//   2. hands over the exact config for the user's AI client (generated from
//      the LIVE host and token, so it cannot be stale)
//   3. proves the connection actually works, on the spot
//
// SECURITY NOTE. The token is real and the panel already holds it (it IS the
// transport credential — see lib/boot.ts). Showing it here is the same
// deliberate, explicit-intent trade the gateway console already makes for BYOK
// keys: it is masked by default, revealed only on request, and the whole point
// of the card is to put it in the user's own client config. It is never put in
// a URL (ADR 0004) and never logged.

import { releaseVersionLabel } from "../lib/agentVersion";
import { useCallback, useMemo, useState } from "react";
import { callApi, getHost, getToken } from "../lib/api";
import { useDeviceRead } from "../hooks/useDeviceRead";
import { copyText } from "../lib/clipboard";

/** One client the user might be connecting. `json` builds the snippet; the
 *  field paths differ per client, which is the only reason this is a table. */
interface ClientSpec {
  id: string;
  label: string;
  /** Where the snippet goes — the single most common source of "it didn't
   *  work" for a hand-rolled config, so it is stated rather than assumed. */
  where: string;
  build: (url: string, token: string) => string;
}

const CLIENTS: ClientSpec[] = [
  {
    id: "dsh",
    label: "DSH",
    where: "~/.dsh/mcp.json (or the Harness MCP settings)",
    build: (url, token) =>
      JSON.stringify(
        {
          mcpServers: {
            summrise: {
              type: "http",
              url,
              headers: { Authorization: `Bearer ${token}` },
            },
          },
        },
        null,
        2,
      ),
  },
  {
    id: "claude",
    label: "Claude Code",
    where:
      'claude mcp add --transport http summrise <url> --header "Authorization: Bearer <token>"',
    build: (url, token) =>
      `claude mcp add --transport http summrise ${url} \\\n  --header "Authorization: Bearer ${token}"`,
  },
  {
    id: "curl",
    label: "curl (a quick check)",
    where: "any shell — this is the raw protocol, no client needed",
    build: (url, token) =>
      `curl -sS ${url} \\\n  -H "Authorization: Bearer ${token}" \\\n  -H "Content-Type: application/json" \\\n  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list"}'`,
  },
];

type Probe = { state: "idle" | "running" | "ok" | "fail"; detail?: string };

export function ConnectCard() {
  const [client, setClient] = useState<string>(CLIENTS[0].id);
  const [revealed, setRevealed] = useState(false);
  const [copied, setCopied] = useState<string | null>(null);
  const [probe, setProbe] = useState<Probe>({ state: "idle" });

  // The device's MCP endpoint. Built from the LIVE location rather than a
  // configured value: whatever host the operator reached this panel on is, by
  // definition, a host their AI client can reach too. A hardcoded value here
  // would be wrong for every remote/tunnel user.
  //
  // AND IT IS RECOMPUTED EVERY RENDER, which is what "live" has to mean: this was
  // `useMemo(…, [])` — computed ONCE at mount — so an operator who reached the panel on one host and
  // later on another kept a copy-ready URL pointing at the first. The memo saved a string concatenation
  // and cost the property the sentence above claims. Found 2026-09-24 by the panel exploration; this
  // card lives in Settings, which stays mounted across a reconnect.
  const mcpUrl = `${location.protocol}//${getHost()}/mcp`;
  const token = getToken();

  // THE ONE-SHOT READ IS `useDeviceRead`'s (see its header): the mount read, the refusal guard,
  // the unmount guard. No `everyMs` — the registry is static per agent process.
  //
  // AND THE FAILURE IS NOW THE READ STATE, NOT AN EMPTY ARRAY. This used to `setTools([])` in the
  // catch, so `tools.length === 0` meant both "read failed" and "the agent runs no tools" — two
  // different claims drawn as one sentence. `read` tells them apart, and the render below asks it.
  const { data: tools, read } = useDeviceRead<string[]>({
    path: "/api/spec",
    // A body this build cannot use yields NO names; the read still counts as answered, and the
    // card shows the count it actually got rather than claiming the surface was unreadable.
    reduce: (_previous, body) => {
      const spec = body as {
        plugins?: Array<{ tools?: Array<{ name?: unknown }> }>;
      } | null;
      const names: string[] = [];
      for (const p of spec?.plugins ?? []) {
        for (const t of p?.tools ?? []) if (t?.name) names.push(t.name as string);
      }
      return names;
    },
    initial: [],
  });

  const snippet = useMemo(() => {
    const spec = CLIENTS.find((c) => c.id === client) ?? CLIENTS[0];
    // Masked by default. A placeholder (not a redaction) so the snippet stays
    // copy-pasteable once revealed, and so a user who copies without revealing
    // gets an obviously-incomplete config rather than a silently broken one.
    return spec.build(mcpUrl, revealed ? token : "<your-device-token>");
  }, [client, mcpUrl, token, revealed]);

  const where = (CLIENTS.find((c) => c.id === client) ?? CLIENTS[0]).where;

  const doCopy = useCallback((what: string, text: string) => {
    copyText(text).then(() => {
      setCopied(what);
      window.setTimeout(() => setCopied((c) => (c === what ? null : c)), 1600);
    });
  }, []);

  /** Self-test: hit the authenticated status endpoint with the SAME token the
   *  snippet carries. Proves the credential works, which is the part users get
   *  wrong — a config can be syntactically perfect and still 401. */
  const runProbe = useCallback(() => {
    setProbe({ state: "running" });
    callApi("/api/status")
      .then((s) =>
        setProbe({
          state: "ok",
          // The RELEASE, not the frozen Cargo protocol version — see
          // lib/agentVersion.ts. This probe reported v1.0.145 from Settings while the
          // status strip said v1.2.354 for the same device.
          detail: `${releaseVersionLabel(s)} · ${s?.live_sessions ?? 0} live session(s)`,
        }),
      )
      .catch((e) =>
        setProbe({ state: "fail", detail: String(e?.message ?? e) }),
      );
  }, []);

  return (
    <>
      <div className="settings-section">
        <h2>Connect an AI client</h2>
        <p className="connect-lede">
          This panel is the <b>human</b> view of the machine. AI clients drive
          it over MCP — point one here and it can operate this device with the
          tools below.
        </p>

        {read === "reading" ? (
          <p className="connect-muted">Reading the tool surface…</p>
        ) : read === "unreadable" ? (
          // The sentence is unchanged; what changed is what it is drawn FROM. It used to be
          // `tools.length === 0` — the same empty array a failure wrote — so it must now be the
          // READ's own verdict.
          <p className="connect-muted">
            Could not read the tool surface from this agent.
          </p>
        ) : (
          <>
            <div className="connect-total">
              <b>{tools.length}</b> tools available on this device
            </div>
          </>
        )}
      </div>

      <div className="settings-section">
        <h2>Give it to your client</h2>
        <div className="connect-tabs">
          {CLIENTS.map((c) => (
            <button
              key={c.id}
              type="button"
              className={`connect-tab${c.id === client ? " on" : ""}`}
              onClick={() => setClient(c.id)}
            >
              {c.label}
            </button>
          ))}
        </div>
        <p className="connect-where">{where}</p>
        <pre className="connect-snippet">{snippet}</pre>
        <div className="settings-actions connect-actions">
          <button type="button" onClick={() => setRevealed((r) => !r)}>
            {revealed ? "Hide token" : "Reveal token"}
          </button>
          <button type="button" onClick={() => doCopy("snippet", snippet)}>
            {copied === "snippet" ? "Copied" : "Copy config"}
          </button>
          <button
            type="button"
            onClick={runProbe}
            disabled={probe.state === "running"}
          >
            {probe.state === "running" ? "Testing…" : "Test this credential"}
          </button>
        </div>
        {probe.state === "ok" && (
          <p className="connect-probe ok">Connected — {probe.detail}</p>
        )}
        {probe.state === "fail" && (
          <p className="connect-probe fail">Failed — {probe.detail}</p>
        )}
        <p className="connect-muted connect-foot">
          The token is this device's own credential. Anyone holding it can drive
          the machine, so treat the config like a password.
        </p>
      </div>
    </>
  );
}
