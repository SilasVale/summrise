import { useCallback, useEffect, useMemo, useState } from "react";
import { message } from "./format";
import { copyText } from "./copy";
import { downloadFile } from "./download";
import { Modal } from "./Modal";
import { useRequestGate } from "./requests";
import { TagEditor } from "./TagEditor";
import { callTool, toolFailure, type Grant } from "./terminal";

/**
 * A memory entry on the wire: the id `memory.list`/`memory.search` return, the text, and its tags.
 */
type Entry = { id: string; text: string; tags: string[] };

/**
 * The answer to `memory.export`: the live entries as JSONL, the whole lines it holds, whether the
 * device's cap cut the store, and the size of the store it cut.
 */
type ExportAnswer = { text: string; entries: number; truncated: boolean; total_bytes: number };

/**
 * The answer to `memory.append`: the device accepted the fact and minted the id the next read shows.
 */
type AppendAnswer = { appended: boolean; id: string };

/// `memory.list` and `memory.search` refuse a larger limit; 100 is the capability's ceiling.
const LIMIT = 100;

/// `memory::MAX_TAGS` and `memory::MAX_TAG_BYTES`: the caps a new entry is subject to.
const MAX_TAGS = 32;
const MAX_TAG_BYTES = 128;

/// The sentence both forms show when their text box would send an empty entry.
const EMPTY_TEXT = "A memory entry needs some text.";

/// `memory::MAX_EXPORT_BYTES`: the device returns at most 2 MiB of JSONL per export.
const EXPORT_CAP = 2 * 1024 * 1024;

/// The cap as the sentence spells it; derived so the copy cannot disagree with the number.
const EXPORT_CAP_LABEL = `${EXPORT_CAP / (1024 * 1024)} MiB`;

/**
 * What the device remembers, as a page.
 *
 * Memory is device state, so this reads it through the same grant as the terminal and the browser:
 * `memory.list` for the store and its tag filter, `memory.search` for a query,
 * `memory.update`/`memory.delete` for the two corrections, and `memory.export` for the device's own
 * JSONL. There is no console route for it — every call is signed by the console, checked by the
 * device, and audited like the other panes' calls.
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
  // The create dialog is its own draft: it exists before any entry does, so it shares nothing with
  // the edit form's `editing`/`draftText` state.
  const [creating, setCreating] = useState(false);
  const [createText, setCreateText] = useState("");
  const [createTags, setCreateTags] = useState<string[]>([]);
  const [createTag, setCreateTag] = useState("");
  const [appending, setAppending] = useState(false);
  const [createError, setCreateError] = useState<string | null>(null);
  const [exporting, setExporting] = useState(false);
  const [exported, setExported] = useState<ExportAnswer | null>(null);
  const [copied, setCopied] = useState(false);

  // Every read paints the same three things (the list, the note and the edit), so one ticket per read
  // is enough: a newer read supersedes the older one, whichever filter or button started it.
  const gate = useRequestGate();
  // An export is a read of a different shape, so it gets its own ticket: one that does not clear the
  // list's busy flag and does not let a stale device's JSONL open a dialog after the grant changed.
  const exportGate = useRequestGate();
  // A create is a write, so it gets a ticket too: an answer that belongs to the previous device must
  // not close this device's dialog or claim its status sentence.
  const createGate = useRequestGate();

  /// The six create states move together: a new grant, a new dialog, a close and a success all reset
  /// the same draft, so the reset is one function rather than four lists that can drift apart.
  const resetCreate = useCallback(() => {
    setCreating(false);
    setCreateText("");
    setCreateTags([]);
    setCreateTag("");
    setAppending(false);
    setCreateError(null);
  }, []);

  const read = useCallback(
    async (want: { query: string; tag: string }) => {
      const wanted = { query: want.query.trim(), tag: want.tag.trim() };
      const ticket = gate.begin();
      setBusy(true);
      setError(null);
      try {
        let found: Entry[];
        if (wanted.query) {
          found = JSON.parse(
            await callTool(grant, "memory.search", { query: wanted.query, limit: LIMIT })
          ) as Entry[];
          if (gate.stale(ticket)) return;
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
          if (gate.stale(ticket)) return;
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
        // A stale error describes a view that is gone, exactly like stale data does.
        if (gate.stale(ticket)) return;
        setError("Could not read memory: " + message(cause));
      } finally {
        // Only the newest read owns the busy flag: an older answer that lands late must not clear it
        // while the newer request is still on the wire.
        if (!gate.stale(ticket)) setBusy(false);
      }
    },
    [gate, grant]
  );

  // The store belongs to the device, so a different grant is a different list and a different draft.
  useEffect(() => {
    // An export still on the wire belongs to the previous device; its answer must not open a dialog
    // over this one's store, and a dialog left open is not this device's JSONL.
    exportGate.begin();
    createGate.begin();
    setExporting(false);
    setExported(null);
    setCopied(false);
    resetCreate();
    setQuery("");
    setTag("");
    setView({ query: "", tag: "" });
    setEntries([]);
    setEditing(null);
    setRemoving(null);
    setError(null);
    setStatus("Reading the device's memory…");
    void read({ query: "", tag: "" });
  }, [createGate, exportGate, read, resetCreate]);

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

  const startCreate = () => {
    resetCreate();
    setCreating(true);
  };

  // The Modal refocuses its first field whenever `onClose` changes; a handler that changed on every
  // keystroke would pull focus out of the tag box, so this one only changes with the submit state.
  const closeCreate = useCallback(() => {
    if (appending) return;
    resetCreate();
  }, [appending, resetCreate]);

  const addCreateTag = () => {
    const value = createTag.trim();
    if (!value) return;
    setCreateTags((current) => (current.includes(value) ? current : [...current, value]));
    setCreateTag("");
  };

  const removeCreateTag = (index: number) => {
    setCreateTags((current) => current.filter((_, at) => at !== index));
  };

  const remember = async () => {
    const text = createText.trim();
    // `memory.append` requires text; the button is disabled for this, and the guard keeps Enter or
    // a programmatic click from sending a blank fact.
    if (!text) return;
    // A tag still in the add box is part of the entry the operator wrote, exactly as in `save`.
    const pending = createTag.trim();
    const tags =
      pending && !createTags.includes(pending) ? [...createTags, pending] : [...createTags];
    const ticket = createGate.begin();
    setAppending(true);
    setCreateError(null);
    try {
      const answer = JSON.parse(
        await callTool(grant, "memory.append", { text, tags })
      ) as AppendAnswer;
      if (createGate.stale(ticket)) return;
      resetCreate();
      // Read the view that is showing, not the whole store: the new entry belongs to this filter.
      await read(view);
      if (createGate.stale(ticket)) return;
      setStatus(`Remembered ${answer.id}.`);
    } catch (cause) {
      // A refusal keeps the draft where the operator can fix it: the device's sentence is the fix.
      if (createGate.stale(ticket)) return;
      setCreateError("Could not remember: " + toolFailure(cause));
    } finally {
      if (!createGate.stale(ticket)) setAppending(false);
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

  const startExport = async () => {
    const ticket = exportGate.begin();
    setExporting(true);
    setError(null);
    try {
      const answer = JSON.parse(await callTool(grant, "memory.export", {})) as ExportAnswer;
      if (exportGate.stale(ticket)) return;
      setExported(answer);
      setCopied(false);
    } catch (cause) {
      // A refusal is the device's own sentence: show it where the page already reports reads, open no
      // dialog, and let the button come back so the page stays usable.
      if (exportGate.stale(ticket)) return;
      setError(exportFailure(cause));
    } finally {
      if (!exportGate.stale(ticket)) setExporting(false);
    }
  };

  const copyExport = async () => {
    if (!exported) return;
    // The whole answer, never the visible slice of the textarea: a clipped copy would read as a cut
    // store without saying so.
    if (await copyText(exported.text)) setCopied(true);
  };

  return (
    <div className="memory-page">
      <div className="panel-head">
        <h2>Memory</h2>
        <div className="actions">
          <button type="button" className="primary" onClick={startCreate}>
            New entry
          </button>
          <button type="button" disabled={exporting} onClick={() => void startExport()}>
            {exporting ? "Exporting…" : "Export"}
          </button>
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
                  {draftText.trim() === "" && <p className="hint">{EMPTY_TEXT}</p>}
                  <TagEditor
                    tags={draftTags}
                    draft={draftTag}
                    onDraft={setDraftTag}
                    onAdd={addTag}
                    onRemove={removeTag}
                    inputId={`memory-tags-${entry.id}`}
                    datalistId="memory-tag-options"
                  />
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
                      disabled={saving || draftText.trim() === ""}
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

      {creating && (
        <Modal title="Remember something" onClose={closeCreate}>
          <label htmlFor="memory-create-text">Text</label>
          <textarea
            id="memory-create-text"
            rows={4}
            value={createText}
            onChange={(event) => setCreateText(event.target.value)}
          />
          {createText.trim() === "" && <p className="hint">{EMPTY_TEXT}</p>}
          <TagEditor
            tags={createTags}
            draft={createTag}
            onDraft={setCreateTag}
            onAdd={addCreateTag}
            onRemove={removeCreateTag}
            inputId="memory-create-tags"
            datalistId="memory-tag-options"
          />
          <p className="hint">
            Tags are optional. The device caps one entry at {MAX_TAGS} tags of at most{" "}
            {MAX_TAG_BYTES} bytes each; a refusal leaves this draft here.
          </p>
          {createError && <p className="error">{createError}</p>}
          <div className="modal-actions">
            <button type="button" disabled={appending} onClick={closeCreate}>
              Cancel
            </button>
            <button
              type="button"
              className="primary"
              disabled={appending || createText.trim() === ""}
              onClick={() => void remember()}
            >
              {appending ? "Remembering…" : "Remember"}
            </button>
          </div>
        </Modal>
      )}

      {exported && (
        <Modal
          title="Device memory export"
          onClose={() => {
            setExported(null);
            setCopied(false);
          }}
        >
          <p className="hint">{exportCaption(exported)}</p>
          <textarea
            className="mono"
            aria-label="Exported JSONL"
            readOnly
            rows={12}
            value={exported.text}
          />
          {copied && (
            <p className="hint" role="status">
              Copied
            </p>
          )}
          <div className="modal-actions">
            <button type="button" onClick={() => void copyExport()}>
              Copy
            </button>
            <button
              type="button"
              onClick={() =>
                downloadFile(
                  `summrise-memory-${isoDay()}.jsonl`,
                  exported.text,
                  "application/x-ndjson"
                )
              }
            >
              Download
            </button>
            <button
              type="button"
              onClick={() => {
                setExported(null);
                setCopied(false);
              }}
            >
              Close
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

/**
 * What the export dialog says about its own size.
 *
 * A truncated answer is a prefix of the store, so the sentence names the cap, the cut and the store's
 * real size. The whole-store sentence says the opposite, because a prefix that reads as the store is
 * the failure this dialog exists to prevent.
 */
function exportCaption(answer: ExportAnswer): string {
  const noun = answer.entries === 1 ? "entry" : "entries";
  const here = `${answer.entries} ${noun} and ${byteCount(utf8Bytes(answer.text))} of JSONL`;
  if (answer.truncated) {
    return (
      `The device caps one export at ${EXPORT_CAP_LABEL} (${byteCount(EXPORT_CAP)}), and this answer was cut there: ` +
      `it holds ${here}, while the store holds ${byteCount(answer.total_bytes)}, newest first.`
    );
  }
  return (
    `The whole store: ${here}, newest first. ` +
    `The device caps one export at ${EXPORT_CAP_LABEL} (${byteCount(EXPORT_CAP)}); this answer fits.`
  );
}

/// The exported text as the device counts it: UTF-8 bytes, not JavaScript code units.
function utf8Bytes(text: string): number {
  return new TextEncoder().encode(text).length;
}

/**
 * What the page says when an export fails.
 *
 * The shared classifier keeps the device's refusal verbatim and says when the console never reached
 * it; the prefix is this page's, because the operator asked this page for the export.
 */
function exportFailure(cause: unknown): string {
  return "Could not export memory: " + toolFailure(cause);
}

/// "9,000,000 bytes" — exact and grouped, rather than a rounded size that hides which store is meant.
function byteCount(count: number): string {
  return count.toString().replace(/\B(?=(\d{3})+(?!\d))/g, ",") + " bytes";
}

/// The operator's calendar day, for the download's filename.
function isoDay(): string {
  const now = new Date();
  const month = String(now.getMonth() + 1).padStart(2, "0");
  const day = String(now.getDate()).padStart(2, "0");
  return `${now.getFullYear()}-${month}-${day}`;
}
