import { useCallback, useEffect, useRef, useState } from "react";
import { forwardFailure, listForwards, type ForwardRow } from "./forwardApi";
import { spanText } from "./age";
import { useLiveTools } from "./useLiveTools";
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

/**
 * How long this rule has been bound, measured AGAINST THE MOMENT THE LIST WAS READ.
 *
 * **`since_ms` IS THE DEVICE'S CLOCK AND `Date.now()` IS THE BROWSER'S.** Subtracting one from the
 * other is a number that looks like an age and is not one: a device two minutes ahead makes every
 * rule read "bound 2 m" the instant it binds, and one two minutes behind makes it read "bound 0 s"
 * for two minutes. The read's own moment — taken when the console asked, on the console's clock —
 * removes the skew for every row of one read at once.
 */
function listenSentence(rule: ForwardRow, readAtMs: number): string {
  if (!rule.listening || rule.since_ms === null) {
    return rule.attempts === 0 ? "binding now" : `${rule.attempts} bind attempts`;
  }
  // A device clock AHEAD of the console's yields a negative age, and a negative span is not a
  // measurement: it is the two clocks disagreeing, so the sentence says the one thing it knows.
  const span = readAtMs - rule.since_ms;
  if (span < 0) return "bound (this machine's clock is ahead of the console's)";
  return `bound ${spanText(span)}`;
}

function carriedSentence(count: number): string {
  // **"CARRIED", NOT "CONNECTIONS".** The device's own doc says the count is both directions ended
  // without an error — a connection that was accepted and refused is a log line, not a count. A
  // page that said "connections" would make the number mean "arrived" while the device means "worked".
  return `${count} ${count === 1 ? "connection" : "connections"} carried`;
}


export function ForwardRules({
  grant,
  deviceOnline,
  deviceId
}: {
  grant: Grant;
  /// The fleet's reachability fact; only `false` is a fact, `undefined` is "no opinion".
  deviceOnline?: boolean;
  /// The machine whose tool list decides whether these three tools exist on it.
  deviceId: string | null;
}) {
  /**
   * **A MACHINE RUNNING AN OLDER BUILD ANSWERS `unknown tool: forward.list` TO EVERY CALL.** The
   * section's own read would then show that refusal as a failure with a Retry button, and a Retry
   * that cannot succeed is worse than a sentence — the operator reads it as `the console is broken`
   * while the machine is fine and merely older. `Plugins` learned this first (`PluginList.tsx`'s
   * `Older build` state); this is the same fact, checked BEFORE the read instead of after it.
   *
   * The hook is the one `ToolsTab` calls, against a module-level cache: two tabs on one device
   * asking inside the same 30 s window is one request, which is what that cache is for.
   */
  const live = useLiveTools(deviceId);
  const toolListKnown = !live.loading && live.error === null;
  // **THE DEVICE'S OWN TOOL NAME (the hundred-and-fortieth list).** This gate looked for
  // `forward.list` — a name no device registers — so every real machine took the "Older build" branch
  // and the list was never even asked for. `agent/src/vocabulary.rs` names the two read tools
  // `forward_list` and `forward_probe`.
  const supported = live.tools.some((tool) => tool.name === "forward.list");
  const [rows, setRows] = useState<ForwardRow[] | null>(null);
  // **THE CLOCK EVERY ROW ON THIS PAGE IS MEASURED AGAINST.** Set by the read that brought the rows,
  // so the ages cannot be computed from the browser's `now` drifting forward between renders — a
  // section that re-rendered a minute later would otherwise report every rule a minute older.
  const [readAtMs, setReadAtMs] = useState(0);
  const [reading, setReading] = useState(false);
  const [readError, setReadError] = useState<string | null>(null);
  const offline = deviceOnline === false;
  // **AN OLDER BUILD IS NEITHER A FAILURE NOR AN EMPTY TABLE.** Only a finished read makes
  // `supported` a fact; before that it is unknown, and a call made on a guess would be a call the
  // device refuses for a reason that has nothing to do with this section.
  const canRead = toolListKnown && supported;

  // One read in flight at a time, and its answer is applied only if this page is still the one that
  // asked: a section that unmounts mid-read must not set state.
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  const read = useCallback(async () => {
    if (offline || !canRead) return;
    setReading(true);
    setReadError(null);
    try {
      const next = await listForwards(grant);
      if (mounted.current) {
        setRows(next.rows);
        setReadAtMs(next.readAtMs);
      }
    } catch (cause) {
      if (mounted.current) setReadError(forwardFailure(cause));
    } finally {
      if (mounted.current) setReading(false);
    }
  }, [grant, offline, canRead]);

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

      {!toolListKnown ? (
        <p className="hint">The rules are read once this machine's tool list arrives.</p>
      ) : !supported ? (
        // **AN OLDER BUILD IS A SENTENCE, NOT A RETRY LOOP.** The machine is not broken and the
        // console is not broken; it simply cannot answer these three tools yet, and a Retry button
        // would offer an act that cannot work.
        //
        // **AND IT IS THE ATOM'S `inCard` SHAPE, NOT ITS STANDALONE ONE (the hundred-and-twenty-fourth list).**
        // Measured 2026-11-11 by the card-level pass: this `.empty-state` sits inside `.system-section` and measured
        // **1.000** — a card painted the card's own colour, 962x147 of hairline around nothing. The atom records two
        // shapes; a block inside a card takes the one with no surface of its own.
        <section className="empty-state in-card">
          <h3>Older build</h3>
          <p className="hint">
            This machine does not advertise the forward tools yet. It needs a newer Summrise to
            carry rules.
          </p>
        </section>
      ) : offline ? (
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
          {/* **THIS PAGE READS; IT DOES NOT CREATE (the hundred-and-fortieth list).** The device
              registers `forward_list` and `forward_probe` and deliberately no `forward_add` (design
              0015 D6): `via` reaches a host that never agreed to be reached, and `listen` decides who
              can use the address, so creating a rule is the operator's act on the machine. The button
              that used to sit here called `forward.add`, a tool the device does not have and must not. */}
          <p className="hint">
            A rule is written on the machine itself — <code>forwards.json</code> in its data
            directory — and appears here once the machine carries it.
          </p>
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
                  {listenSentence(rule, readAtMs)} · {carriedSentence(rule.connections)}
                </p>
                {rule.last_error !== null && <p className="error">{rule.last_error}</p>}
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
        </div>
      )}

    </section>
  );
}
