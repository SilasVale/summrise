import { useCallback, useEffect, useMemo, useState } from "react";
import { Modal } from "./Modal";
import { callTool, type Grant } from "./terminal";

/**
 * A memory entry on the wire: the id `memory.list`/`memory.search` return, the text, and its tags.
 */
type Entry = { id: string; text: string; tags: string[] };

/// `memory.list` and `memory.search` refuse a larger limit; 100 is the capability's ceiling.
const LIMIT = 100;

/**
 * What the device remembers, as a page.
 *
 * Memory is device state, so this reads it through the same grant as the terminal and the browser:
 * `memory.list` for the store and its tag filter, `memory.search` for a query, and
 * `memory.update`/`memory.delete` for the two corrections. There is no console route for it — every
 * call is signed by the console, checked by the device, and audited like the other panes' calls.
 */
export function MemoryPage({ grant }: { grant: Grant }) {
  const [entries, setEntries] = useState<Entry[]>([]);
  const [query, setQuery] = useState("");
  const [tag, setTag] = useState("");
  const [view, setView] = useState({ query: "", tag: "" });
  const [status, setStatus] = useState("Reading the device's memory…");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [editing, setEditing] = useState<string | null>(null);
  const [draftText, setDraftText] = useState("");
  const [draftTags, setDraftTags] = useState<string[]>([]);
  const [draftTag, setDraftTag] = useState("");
  const [saving, setSaving] = useState(false);
  const [removing, setRemoving] = useState<Entry | null>(null);
  const [deleting, setDeleting] = useState(false);

  const read = useCallback(
    async (want: { query: string; tag: string }) => {
      const wanted = { query: want.query.trim(), tag: want.tag.trim() };
      setBusy(true);
      setError(null);
      try {
        let found: Entry[];
        if (wanted.query) {
          found = JSON.parse(
            await callTool(grant, "memory.search", { query: wanted.query, limit: LIMIT })
          ) as Entry[];
          // A search matches text and tags, but the device has no query that searches and filters at
          // once: the tag narrows the matches a search returned.
          if (wanted.tag) {
            const wantedTag = wanted.tag.toLowerCase();
            found = found.filter((entry) =>
              entry.tags.some((value) => value.toLowerCase() === wantedTag)
            );
          }
          const noun = found.length === 1 ? "match" : "matches";
          const capped = found.length >= LIMIT ? ` (at most ${LIMIT} are returned)` : "";
          setStatus(
            `${found.length} ${noun} for “${wanted.query}”${wanted.tag ? ` tagged ${wanted.tag}` : ""}${capped}.`
          );
        } else {
          const args: Record<string, unknown> = { limit: LIMIT };
          if (wanted.tag) args.tag = wanted.tag;
          found = JSON.parse(await callTool(grant, "memory.list", args)) as Entry[];
          if (found.length === 0) {
            setStatus(wanted.tag ? `Nothing is tagged ${wanted.tag}.` : "The device remembers nothing yet.");
          } else {
            const noun = found.length === 1 ? "entry" : "entries";
            const capped = found.length >= LIMIT ? ` (at most ${LIMIT} are returned)` : "";
            setStatus(
              `${found.length} ${noun}${wanted.tag ? ` tagged ${wanted.tag}` : ""}, newest first${capped}.`
            );
          }
        }
        setEntries(found);
        setView(wanted);
        // A filter can be why an entry vanished from the list; an edit of an entry that is still
        // showing survives the re-read.
        setEditing((current) => (found.some((entry) => entry.id === current) ? current : null));
      } catch (cause) {
        setError("Could not read memory: " + message(cause));
      } finally {
        setBusy(false);
      }
    },
    [grant]
  );

  // The store belongs to the device, so a different grant is a different list and a different draft.
  useEffect(() => {
    setQuery("");
    setTag("");
    setView({ query: "", tag: "" });
    setEntries([]);
    setEditing(null);
    setRemoving(null);
    setError(null);
    setStatus("Reading the device's memory…");
    void read({ query: "", tag: "" });
  }, [read]);

  const knownTags = useMemo(
    () =>
      [...new Set(entries.flatMap((entry) => entry.tags))].sort((left, right) =>
        left.localeCompare(right)
      ),
    [entries]
  );

  const startEdit = (entry: Entry) => {
    setEditing(entry.id);
    setDraftText(entry.text);
    setDraftTags([...entry.tags]);
    setDraftTag("");
    setError(null);
  };

  const addTag = () => {
    const value = draftTag.trim();
    if (!value) return;
    setDraftTags((current) => (current.includes(value) ? current : [...current, value]));
    setDraftTag("");
  };

  const removeTag = (index: number) => {
    setDraftTags((current) => current.filter((_, at) => at !== index));
  };

  const save = async (entry: Entry) => {
    // `memory.update` takes the complete tag list: the editor's chips are the replacement, and an
    // empty list clears every tag. A tag still in the add box is part of the edit the operator made.
    const pending = draftTag.trim();
    const tags = pending && !draftTags.includes(pending) ? [...draftTags, pending] : [...draftTags];
    setSaving(true);
    setError(null);
    try {
      const updated = JSON.parse(
        await callTool(grant, "memory.update", { id: entry.id, text: draftText, tags })
      ) as Entry;
      // The device answers the entry as written. Show it before the re-read fixes its position:
      // an update moves the entry to the newest end of the list, and a re-read that fails should
      // not make a successful save look like the old text.
      setEntries((current) => current.map((item) => (item.id === updated.id ? updated : item)));
      setEditing(null);
      await read(view);
    } catch (cause) {
      setError("Could not update " + entry.id + ": " + message(cause));
    } finally {
      setSaving(false);
    }
  };

  const remove = async () => {
    if (!removing) return;
    setDeleting(true);
    setError(null);
    try {
      await callTool(grant, "memory.delete", { id: removing.id });
      setEntries((current) => current.filter((entry) => entry.id !== removing.id));
      setEditing((current) => (current === removing.id ? null : current));
      setRemoving(null);
      await read(view);
    } catch (cause) {
      setError("Could not delete " + removing.id + ": " + message(cause));
    } finally {
      setDeleting(false);
    }
  };

  const filterByTag = (value: string) => {
    setTag(value);
    void read({ query, tag: value });
  };

  const clear = () => {
    setQuery("");
    setTag("");
    void read({ query: "", tag: "" });
  };

  return (
    <div className="memory-page">
      <div className="panel-head">
        <h2>Memory</h2>
        <div className="actions">
          <button type="button" disabled={busy} onClick={() => void read(view)}>
            Refresh
          </button>
        </div>
      </div>
      <p className="hint">
        Newest first, at most {LIMIT}. A search matches text and tags; a tag asks the device for the
        entries carrying it.
      </p>

      <form
        className="memory-toolbar"
        onSubmit={(event) => {
          event.preventDefault();
          void read({ query, tag });
        }}
      >
        <input
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          placeholder="search text and tags — leave empty to list everything"
          aria-label="Search memory"
        />
        <input
          value={tag}
          onChange={(event) => setTag(event.target.value)}
          list="memory-tag-options"
          placeholder="tag"
          aria-label="Filter by tag"
        />
        <datalist id="memory-tag-options">
          {knownTags.map((value) => (
            <option key={value} value={value} />
          ))}
        </datalist>
        <button type="submit" className="primary" disabled={busy}>
          {busy ? "Reading…" : "Search"}
        </button>
        {(query !== "" || tag !== "") && (
          <button type="button" onClick={clear}>
            Clear
          </button>
        )}
      </form>

      <p className="hint">{status}</p>
      {error && !removing && <p className="error">{error}</p>}

      {entries.length > 0 && (
        <div className="cards">
          {entries.map((entry) => (
            <article className="card memory-entry" key={entry.id}>
              <header>
                <span className="mono muted">{entry.id}</span>
                {editing !== entry.id && (
                  <div className="actions">
                    <button type="button" className="mini" onClick={() => startEdit(entry)}>
                      Edit
                    </button>
                    <button
                      type="button"
                      className="mini danger"
                      onClick={() => {
                        setError(null);
                        setRemoving(entry);
                      }}
                    >
                      Delete
                    </button>
                  </div>
                )}
              </header>

              {editing === entry.id ? (
                <div className="memory-edit">
                  <label htmlFor={`memory-text-${entry.id}`}>Text</label>
                  <textarea
                    id={`memory-text-${entry.id}`}
                    rows={4}
                    value={draftText}
                    onChange={(event) => setDraftText(event.target.value)}
                  />
                  <label htmlFor={`memory-tags-${entry.id}`}>Tags</label>
                  {draftTags.length === 0 ? (
                    <p className="hint">No tags.</p>
                  ) : (
                    <div className="memory-tags">
                      {draftTags.map((value, index) => (
                        <button
                          type="button"
                          className="chip chip-tag"
                          key={`${entry.id}-${index}`}
                          title={`Remove ${value}`}
                          onClick={() => removeTag(index)}
                        >
                          {value} ×
                        </button>
                      ))}
                    </div>
                  )}
                  <div className="memory-add-tag">
                    <input
                      id={`memory-tags-${entry.id}`}
                      value={draftTag}
                      onChange={(event) => setDraftTag(event.target.value)}
                      onKeyDown={(event) => {
                        if (event.key === "Enter") {
                          event.preventDefault();
                          addTag();
                        }
                      }}
                      list="memory-tag-options"
                      placeholder="a tag; Enter adds it"
                    />
                    <button type="button" className="mini" disabled={draftTag.trim() === ""} onClick={addTag}>
                      Add tag
                    </button>
                  </div>
                  <p className="hint">
                    Click a tag to remove it. Saving replaces the text and the whole tag list; an
                    empty list clears every tag.
                  </p>
                  <div className="actions">
                    <button
                      type="button"
                      className="mini"
                      disabled={saving}
                      onClick={() => setEditing(null)}
                    >
                      Cancel
                    </button>
                    <button
                      type="button"
                      className="mini primary"
                      disabled={saving}
                      onClick={() => void save(entry)}
                    >
                      {saving ? "Saving…" : "Save"}
                    </button>
                  </div>
                </div>
              ) : (
                <>
                  <p className="memory-text">{entry.text}</p>
                  <div className="memory-tags">
                    {entry.tags.length === 0 && <span className="hint">No tags</span>}
                    {entry.tags.map((value, index) => (
                      <button
                        type="button"
                        className="chip chip-tag"
                        key={`${entry.id}-${index}`}
                        title={`Filter by ${value}`}
                        onClick={() => filterByTag(value)}
                      >
                        {value}
                      </button>
                    ))}
                  </div>
                </>
              )}
            </article>
          ))}
        </div>
      )}

      {removing && (
        <Modal
          title={`Delete ${removing.id}?`}
          onClose={() => {
            if (deleting) return;
            setRemoving(null);
            setError(null);
          }}
        >
          <p className="hint">
            The entry is retired: every later read omits it. The journal keeps the bytes, so the
            console cannot undo this.
          </p>
          <pre className="code">{preview(removing.text)}</pre>
          {removing.tags.length > 0 && <p className="hint">Tags: {removing.tags.join(", ")}</p>}
          {error && <p className="error">{error}</p>}
          <div className="modal-actions">
            <button
              type="button"
              disabled={deleting}
              onClick={() => {
                setRemoving(null);
                setError(null);
              }}
            >
              Cancel
            </button>
            <button type="button" className="danger" disabled={deleting} onClick={() => void remove()}>
              {deleting ? "Deleting…" : "Delete"}
            </button>
          </div>
        </Modal>
      )}
    </div>
  );
}

/// A confirmation dialog shows enough of the entry to recognise it, not a megabyte of it.
function preview(text: string): string {
  return text.length > 280 ? text.slice(0, 280) + "…" : text;
}

function message(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}
