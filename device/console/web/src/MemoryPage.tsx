import { useCallback, useEffect, useRef, useState } from "react";
import { memoryFailure, readMemory, type MemoryEntry } from "./memoryApi";
import type { Grant } from "./terminal";

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

  // A read that answers after the reader typed again is the OLD question's answer: the generation counts reads and
  // only the newest may write state.
  const generation = useRef(0);
  const read = useCallback(
    async (question: string) => {
      const mine = ++generation.current;
      setReading(true);
      try {
        const answer = await readMemory(props.grant, question);
        if (generation.current !== mine) return;
        if (!answer.ok) {
          setFailed(answer.error ?? "The memory list could not be read.");
          setEntries(null);
        } else {
          setEntries(answer.entries);
          setFailed(null);
        }
      } catch (error) {
        if (generation.current !== mine) return;
        setFailed(memoryFailure(error).replace(/^The memory list could not be read — /, ""));
        setEntries(null);
      } finally {
        if (generation.current === mine) setReading(false);
      }
    },
    [props.grant]
  );

  useEffect(() => {
    void read("");
  }, [read]);

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    setAsked(query.trim());
    void read(query.trim());
  };

  const shown = entries ?? [];
  const count = shown.length;

  return (
    <section className="panel memory-panel" aria-label="This machine's memory">
      <form className="memory-search" role="search" onSubmit={submit}>
        <label className="sr-only" htmlFor="memory-query">
          Search the machine's memory
        </label>
        <input
          id="memory-query"
          type="search"
          value={query}
          placeholder="Search titles, text and tags"
          onChange={(event) => setQuery(event.target.value)}
        />
        <button type="submit" className="btn" disabled={reading}>
          {reading ? "Reading…" : "Search"}
        </button>
        {asked !== "" && (
          <button
            type="button"
            className="link"
            onClick={() => {
              setQuery("");
              setAsked("");
              void read("");
            }}
          >
            Clear the search
          </button>
        )}
      </form>

      {/* The count is the page's one sentence about the store, and it states WHICH question it answers. */}
      {entries !== null && (
        <p className="memory-count">
          {count === 0
            ? asked === ""
              ? "This machine's memory is empty."
              : `Nothing matches “${asked}”.`
            : `${count} ${count === 1 ? "entry" : "entries"}${asked === "" ? "" : ` for “${asked}”`} · newest first`}
        </p>
      )}

      {failed !== null && (
        <div className="notice" role="status">
          <span>{failed}</span>
          <button type="button" disabled={reading} onClick={() => void read(asked)}>
            {reading ? "Retrying…" : "Retry"}
          </button>
        </div>
      )}

      {entries === null && failed === null && (
        <p className="hint" role="status">
          Reading the machine's memory…
        </p>
      )}

      {count > 0 && (
        <ol className="memory-list">
          {shown.map((row) => (
            <li key={row.id} className="memory-entry">
              <div className="memory-head">
                <h3 className="memory-title">{row.title}</h3>
                {/* **ONLY THE FACTS THIS BUILD REPORTS (the hundred-and-seventy-first list).** The shipped device's records are
                    `{id, tags, text}` — no namespace, no source, no timestamp — so the meta line joins what exists instead of
                    printing `shared · unknown · no date`, which would be three invented facts on one line. */}
                <span className="memory-meta">
                  {[row.namespace, row.source, row.updatedAt !== null ? stamp(row.updatedAt) : null]
                    .filter((part): part is string => part !== null && part !== "")
                    .join(" · ")}
                  {row.deleted ? " · deleted" : ""}
                </span>
              </div>
              {row.content !== "" && <p className="memory-body">{row.content}</p>}
              {row.tags.length > 0 && (
                <p className="memory-tags">
                  {row.tags.map((tag) => (
                    <span key={tag} className="chip">
                      {tag}
                    </span>
                  ))}
                </p>
              )}
            </li>
          ))}
        </ol>
      )}

      {props.deviceOnline === false && (
        <p className="hint">
          The machine is offline — this is the last list the console could read.
        </p>
      )}
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
