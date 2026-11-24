import { useCallback, useEffect, useRef, useState, type KeyboardEvent } from "react";
import { PageHeading } from "./PageHeading";
import { memoryFailure, readMemory, type MemoryEntry } from "./memoryApi";
import { useRequestGate } from "./requests";
import type { Grant } from "./terminal";
import "./styles/records.css";

/**
 * **THE MACHINE'S MEMORY, AND WHY THE CONSOLE OWNS A DOOR TO IT (the hundred-and-seventy-first list).**
 *
 * The device has carried `memory.list` and `memory.search` since the memory plugin landed — the AI writes what it
 * learned there, per machine — and the console had no way to look. Measured 2026-11-12: the device advertises **46
 * tools, the console wired 32**, and the memory family (six tools) was the largest group with no door at all. An
 * operator who wants to know what an agent learned on a machine should not have to open a terminal on it.
 *
 * **READ-ONLY, AND THAT IS THE SHAPE OF THE FIRST VERSION.** `memory.append`, `memory.update` and `memory.delete` are
 * writes with a long shadow (the record is the AI's own knowledge; a wrong edit is not undoable from here). They are
 * recorded in the backlog as the next door rather than half-built: a page that could delete but not restore would be
 * worse than a page that cannot delete.
 */
export function MemoryPage(props: { grant: Grant; deviceOnline?: boolean }) {
  const [entries, setEntries] = useState<MemoryEntry[] | null>(null);
  const [failed, setFailed] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [asked, setAsked] = useState("");
  const [reading, setReading] = useState(false);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const resultButtons = useRef(new Map<string, HTMLButtonElement>());
  const detailRef = useRef<HTMLElement | null>(null);

  const offline = props.deviceOnline === false;
  // Keep the last successful question with its rows while another read is pending.
  const wanted = useRef("");
  const grantRef = useRef<Grant | null>(null);
  const gate = useRequestGate();
  const read = useCallback(
    async (question: string) => {
      if (offline) return;
      const ticket = gate.begin();
      wanted.current = question;
      setReading(true);
      try {
        const answer = await readMemory(props.grant, question);
        if (gate.stale(ticket)) return;
        if (!answer.ok) {
          setFailed(answer.error ?? "The memory list could not be read.");
          setEntries(null);
        } else {
          setEntries(answer.entries);
          setSelectedId((current) => answer.entries.some((entry) => entry.id === current) ? current : null);
          setAsked(question);
          setFailed(null);
        }
      } catch (error) {
        if (gate.stale(ticket)) return;
        setFailed(memoryFailure(error).replace(/^The memory list could not be read — /, ""));
        setEntries(null);
      } finally {
        if (!gate.stale(ticket)) setReading(false);
      }
    },
    [props.grant, offline, gate]
  );

  useEffect(() => {
    gate.begin();
    setReading(false);
    if (grantRef.current !== props.grant) {
      grantRef.current = props.grant;
      wanted.current = "";
      setEntries(null);
      setFailed(null);
      setQuery("");
      setAsked("");
      setSelectedId(null);
    }
    if (!offline) void read(wanted.current);
  }, [read, gate, props.grant, offline]);

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    if (offline || reading) return;
    void read(query.trim());
  };

  const shown = entries ?? [];
  const count = shown.length;
  const selected = shown.find((entry) => entry.id === selectedId) ?? null;

  const focusEntry = useCallback(() => {
    const reduceMotion = window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;
    detailRef.current?.scrollIntoView?.({ block: "nearest", behavior: reduceMotion ? "auto" : "smooth" });
    detailRef.current?.focus();
  }, []);

  useEffect(() => {
    if (selectedId) focusEntry();
  }, [selectedId, focusEntry]);

  const closeEntry = () => {
    setSelectedId(null);
    if (selectedId) resultButtons.current.get(selectedId)?.focus();
  };
  const moveResult = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    let next: number;
    if (event.key === "ArrowDown") next = Math.min(index + 1, count - 1);
    else if (event.key === "ArrowUp") next = Math.max(index - 1, 0);
    else if (event.key === "Home") next = 0;
    else if (event.key === "End") next = count - 1;
    else return;
    event.preventDefault();
    resultButtons.current.get(shown[next].id)?.focus();
  };

  return (
    <section className="memory-panel records-workspace" aria-label="This machine's memory">
      <PageHeading
        level={2}
        title="Memory"
        description="Read what this machine has remembered. Search its text and tags."
        actions={<span className="chip">Read only</span>}
      />
      <form className="memory-toolbar memory-search" role="search" onSubmit={submit}>
        <div className="memory-query">
          <label htmlFor="memory-query">Search the machine's memory</label>
          <input
            id="memory-query"
            type="search"
            value={query}
            placeholder="Search titles, text and tags"
            onChange={(event) => setQuery(event.target.value)}
          />
        </div>
        <button type="submit" className="primary" disabled={reading || offline}>
          {reading ? "Reading…" : "Search"}
        </button>
        {asked !== "" && (
          <button
            type="button"
            disabled={reading || offline}
            onClick={() => {
              setQuery("");
              void read("");
            }}
          >
            Clear the search
          </button>
        )}
      </form>

      {failed !== null && (
        <div className="notice" role="status">
          <span>{failed}</span>
          <button type="button" disabled={reading || offline} onClick={() => void read(wanted.current)}>
            {reading ? "Retrying…" : "Retry"}
          </button>
        </div>
      )}
      {entries === null && failed === null && !offline && (
        <p className="hint" role="status">Reading the machine's memory…</p>
      )}
      {offline && (
        <p className="hint" role="status">
          {entries === null
            ? "Its memory cannot be read."
            : "The machine is offline — this is the last list the console could read."}
        </p>
      )}

      <div className="memory-workarea">
        <section className="memory-results" aria-label="Memory results" aria-busy={reading}>
          <header className="memory-results-head">
            <h3>Results</h3>
            {entries !== null && (
              <p className="memory-count">
                {count === 0
                  ? asked === ""
                    ? "This machine's memory is empty."
                    : `Nothing matches “${asked}”.`
                  : `${count} ${count === 1 ? "entry" : "entries"}${asked === "" ? "" : ` for “${asked}”`} · newest first`}
              </p>
            )}
          </header>
          {count > 0 && (
            <ol className="memory-list" aria-label="Memory entries">
              {shown.map((row, index) => (
                <li key={row.id} className={"memory-entry" + (selectedId === row.id ? " memory-entry-selected" : "")}>
                  <button
                    type="button"
                    className="memory-open"
                    aria-label={row.title}
                    aria-expanded={selectedId === row.id}
                    aria-controls="memory-inspector"
                    ref={(button) => {
                      if (button) resultButtons.current.set(row.id, button);
                      else resultButtons.current.delete(row.id);
                    }}
                    onClick={() => {
                      if (selectedId === row.id) focusEntry();
                      else setSelectedId(row.id);
                    }}
                    onKeyDown={(event) => moveResult(event, index)}
                  >
                    <span className="memory-title">{row.title}</span>
                    <span aria-hidden="true" className="memory-open-mark">→</span>
                  </button>
                  {(row.namespace || row.source || row.updatedAt !== null || row.deleted) && (
                    <p className="memory-meta">
                      {[row.namespace, row.source, row.updatedAt !== null ? stamp(row.updatedAt) : null]
                        .filter((part): part is string => part !== null && part !== "")
                        .join(" · ")}
                      {row.deleted ? " · deleted" : ""}
                    </p>
                  )}
                  {row.tags.length > 0 && (
                    <p className="memory-tags">
                      {row.tags.map((tag) => <span key={tag} className="chip">{tag}</span>)}
                    </p>
                  )}
                </li>
              ))}
            </ol>
          )}
        </section>

        {selected ? (
          <section
            id="memory-inspector"
            className="memory-detail"
            aria-label="Memory entry"
            tabIndex={-1}
            ref={detailRef}
            onKeyDown={(event) => {
              if (event.key !== "Escape") return;
              event.preventDefault();
              event.stopPropagation();
              closeEntry();
            }}
          >
            <header className="memory-detail-head">
              <div>
                <p className="records-eyebrow">Remembered entry</p>
                <h3>{selected.title}</h3>
              </div>
              <button type="button" onClick={closeEntry}>Close entry</button>
            </header>
            {selected.content !== "" && <p className="memory-body memory-text">{selected.content}</p>}
            <dl className="memory-detail-facts">
              <div><dt>Entry ID</dt><dd className="mono">{selected.id}</dd></div>
              {selected.namespace && <div><dt>Namespace</dt><dd>{selected.namespace}</dd></div>}
              {selected.source && <div><dt>Source</dt><dd>{selected.source}</dd></div>}
              {selected.updatedAt !== null && <div><dt>Updated</dt><dd>{stamp(selected.updatedAt)}</dd></div>}
              {selected.runId && <div><dt>Run ID</dt><dd className="mono">{selected.runId}</dd></div>}
              {selected.deleted && <div><dt>State</dt><dd>Deleted</dd></div>}
            </dl>
            <p className="hint memory-readonly-note">This entry is read only in the console.</p>
          </section>
        ) : (
          <div id="memory-inspector" className="memory-unselected">
            <span className="records-empty-mark" aria-hidden="true">↗</span>
            <h3>Read an entry</h3>
            <p className="hint">Choose a result to read its full text and the details this machine reports.</p>
          </div>
        )}
      </div>
    </section>
  );
}

/** The device writes SECOND-resolution stamps; the page prints a local time and its date, as the panel does. */
function stamp(seconds: number): string {
  if (seconds === 0) return "no date";
  const at = new Date(seconds * 1000);
  if (Number.isNaN(at.getTime())) return "no date";
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${at.getFullYear()}-${pad(at.getMonth() + 1)}-${pad(at.getDate())} ${pad(at.getHours())}:${pad(at.getMinutes())}`;
}
