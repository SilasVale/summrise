import {
  Fragment,
  useCallback,
  useEffect,
  useRef,
  useState,
  type ChangeEvent,
  type FormEvent
} from "react";
import { useRequestGate } from "./requests";
import { callTool, type Grant } from "./terminal";

/** What a listed entry is: the three values `files.list`'s `kind` can take. */
type Kind = "file" | "directory" | "other";

/** One entry of a directory, as `files.list` returns it; `len` is 0 for anything but a file. */
type Entry = { name: string; kind: Kind; len: number };

/**
 * The file that is open.
 *
 * A text file is editable; a refused UTF-8 read is kept as the device's sentence, because a binary
 * file is a fact about the file rather than a failure of the page; and a base64 read is bytes to
 * download, never a textarea the operator could mistake for text.
 */
type Open =
  | { path: string; mode: "text"; text: string; saved: string }
  | { path: string; mode: "refused"; sentence: string }
  | { path: string; mode: "base64"; data: string };

/// The device's cap on one `files.read`/`files.write` payload; uploads are refused here first.
const MAX_FILE_BYTES = 1024 * 1024;

/**
 * The device's files root, as a page.
 *
 * Files are device state, so this goes through the same grant as the terminal, the browser, memory,
 * monitors and runs: `files.list` for a directory, `files.read`/`files.write` for one file and
 * `files.mkdir` for a new folder. There is no console route. The device has no delete or rename
 * tool, so the page offers neither and says so.
 *
 * A UTF-8 read that the device refuses is the signal that the bytes are not text: the page shows the
 * device's sentence and offers the same file as base64, which it downloads unchanged instead of
 * painting mojibake into an editor. Every read is gated so the newest listing or file wins; a failed
 * listing keeps the previous one where it is.
 */
export function FilesPage({ grant }: { grant: Grant }) {
  const [dir, setDir] = useState("");
  const [entries, setEntries] = useState<Entry[]>([]);
  const [note, setNote] = useState("Reading the device's files root…");
  const [loadingDir, setLoadingDir] = useState(false);
  const [listError, setListError] = useState<string | null>(null);

  const [open, setOpen] = useState<Open | null>(null);
  const [opening, setOpening] = useState<string | null>(null);
  const [fileError, setFileError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  const [folderName, setFolderName] = useState("");
  const [creating, setCreating] = useState(false);
  const [uploading, setUploading] = useState(false);
  const [actionNote, setActionNote] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);

  // Whether this grant has produced a listing yet. A failed first read has nothing to keep showing;
  // a failed refresh must leave the previous entries (and the directory they belong to) where they
  // are, so `dir` changes only when a listing succeeds.
  const listed = useRef(false);
  // The directory the page is *showing*, readable from an action that started in an earlier render.
  // A save or upload must not read a folder the operator has already left.
  const dirRef = useRef("");
  // The grant the page belongs to. A save or upload that resolves after a re-grant must not paint
  // its answer — or read its folder — into the new device's page.
  const grantRef = useRef(grant);
  // The listing and the open file are different views: a refresh must not discard a file read that
  // is still on the wire, and opening a file must not cancel a listing.
  const listGate = useRequestGate();
  const fileGate = useRequestGate();

  const read = useCallback(
    async (wanted: string) => {
      const ticket = listGate.begin();
      setLoadingDir(true);
      setListError(null);
      try {
        const rows = JSON.parse(
          await callTool(grant, "files.list", { path: wanted })
        ) as Entry[];
        if (listGate.stale(ticket)) return;
        listed.current = true;
        // The device sorts by name; the page's order is the file manager's: folders, files, then
        // the entries it cannot open.
        rows.sort(entryOrder);
        setEntries(rows);
        setDir(wanted);
        dirRef.current = wanted;
        setNote(listNote(rows));
      } catch (cause) {
        if (listGate.stale(ticket)) return;
        // The previous listing stays, and its breadcrumb still says where it is. Only a first read
        // has no note to keep.
        if (!listed.current) setNote("The device's files could not be read.");
        setListError("Could not list " + displayPath(wanted) + ": " + message(cause));
      } finally {
        if (!listGate.stale(ticket)) setLoadingDir(false);
      }
    },
    [grant, listGate]
  );

  // A different grant is a different device's tree: its listing, its open file and its draft.
  useEffect(() => {
    grantRef.current = grant;
    fileGate.begin();
    listed.current = false;
    dirRef.current = "";
    setDir("");
    setEntries([]);
    setNote("Reading the device's files root…");
    setListError(null);
    setOpen(null);
    setOpening(null);
    setFileError(null);
    setFolderName("");
    setActionNote(null);
    setActionError(null);
    void read("");
  }, [read, fileGate, grant]);

  const closeViewer = () => {
    // A read still on the wire must not reopen what the operator just closed.
    fileGate.begin();
    setOpening(null);
    setOpen(null);
    setFileError(null);
  };

  const openText = async (path: string) => {
    const ticket = fileGate.begin();
    setOpening(path);
    setFileError(null);
    try {
      const text = await callTool(grant, "files.read", { path });
      if (fileGate.stale(ticket)) return;
      // A textarea's value uses LF; normalising here keeps a CRLF file from looking dirty on the
      // first render, when nothing has been edited yet.
      const normalized = normalizeNewlines(text);
      setOpen({ path, mode: "text", text: normalized, saved: normalized });
    } catch (cause) {
      if (fileGate.stale(ticket)) return;
      const sentence = message(cause);
      if (isFileRefusal(sentence)) {
        // The device answered — this file is not UTF-8 text (or is over its cap). Show its sentence
        // and let the operator ask for the bytes instead. It is not an error page.
        setOpen({ path, mode: "refused", sentence });
      } else {
        setOpen(null);
        setFileError("Could not open " + displayFile(path) + ": " + sentence);
      }
    } finally {
      if (!fileGate.stale(ticket)) setOpening(null);
    }
  };

  const openBase64 = async (path: string) => {
    const ticket = fileGate.begin();
    setOpening(path);
    setFileError(null);
    try {
      const data = await callTool(grant, "files.read", { path, encoding: "base64" });
      if (fileGate.stale(ticket)) return;
      setOpen({ path, mode: "base64", data });
    } catch (cause) {
      if (fileGate.stale(ticket)) return;
      // The refusal card stays where it is: this second read failing is extra information, not a
      // reason to throw away the sentence the device gave for the first one.
      setFileError(
        "Could not read " + displayFile(path) + " as base64: " + message(cause)
      );
    } finally {
      if (!fileGate.stale(ticket)) setOpening(null);
    }
  };

  const save = async () => {
    if (!open || open.mode !== "text") return;
    const path = open.path;
    const content = open.text;
    setSaving(true);
    setFileError(null);
    setActionError(null);
    setActionNote(null);
    try {
      const answer = JSON.parse(
        await callTool(grant, "files.write", { path, content })
      ) as { path: string; bytes: number };
      if (grantRef.current !== grant) return;
      // The text may have moved on while the write was on the wire; only the bytes that were sent
      // are saved, and `text` is left as the operator now has it.
      setOpen((current) =>
        current && current.mode === "text" && current.path === path
          ? { ...current, saved: content }
          : current
      );
      setActionNote(`Saved ${displayFile(path)} (${bytes(answer.bytes)}).`);
      // The listing shows the old size; re-read it only when it is still the folder on screen.
      if (parentOf(path) === dirRef.current) await read(dirRef.current);
    } catch (cause) {
      if (grantRef.current !== grant) return;
      setFileError("Could not save " + displayFile(path) + ": " + message(cause));
    } finally {
      setSaving(false);
    }
  };

  const createFolder = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const name = folderName.trim();
    if (name === "") return;
    const folder = dir;
    const path = childPath(folder, name);
    setCreating(true);
    setActionError(null);
    setActionNote(null);
    try {
      await callTool(grant, "files.mkdir", { path });
      if (grantRef.current !== grant) return;
      setFolderName("");
      setActionNote(`Created ${displayFile(path)}.`);
      if (dirRef.current === folder) await read(folder);
    } catch (cause) {
      if (grantRef.current !== grant) return;
      setActionError("Could not create " + displayFile(path) + ": " + message(cause));
    } finally {
      setCreating(false);
    }
  };

  const upload = async (event: ChangeEvent<HTMLInputElement>) => {
    const input = event.currentTarget;
    const file = input.files?.[0] ?? null;
    // Clearing the selection is what lets the same name be chosen again: a real change event needs
    // a different value, and the next upload is usually the corrected file.
    input.value = "";
    if (!file) return;

    const name = file.name;
    if (file.size > MAX_FILE_BYTES) {
      // The device would refuse this too; saying it here saves the round trip and names the limit.
      setActionNote(null);
      setActionError(
        `${name} is ${bytes(file.size)}; an upload is capped at 1 MiB (1,048,576 bytes), so it was not sent to the device.`
      );
      return;
    }

    const folder = dir;
    const path = childPath(folder, name);
    setUploading(true);
    setActionError(null);
    setActionNote(null);
    try {
      const content = await readAsBase64(file);
      await callTool(grant, "files.write", { path, content, encoding: "base64" });
      if (grantRef.current !== grant) return;
      setActionNote(`Uploaded ${name} (${bytes(file.size)}) to ${displayFile(path)}.`);
      if (dirRef.current === folder) await read(folder);
    } catch (cause) {
      if (grantRef.current !== grant) return;
      setActionError("Could not upload " + name + ": " + message(cause));
    } finally {
      setUploading(false);
    }
  };

  const trail = crumbs(dir);
  const up = parentOf(dir);
  const dirty = open !== null && open.mode === "text" && open.text !== open.saved;
  const busy = opening !== null || saving;

  return (
    <div className="files-page">
      <div className="panel-head">
        <h2>Files</h2>
        <div className="actions">
          <button type="button" disabled={loadingDir} onClick={() => void read(dir)}>
            {loadingDir ? "Reading…" : "Refresh"}
          </button>
        </div>
      </div>
      <p className="hint">
        Paths are relative to the device's files root. A folder with more than 1000 entries is
        refused with the device's sentence, and a UTF-8 read that refuses can be opened as base64.
        Saving replaces the whole file; an upload replaces a file of the same name. Both are capped
        at 1 MiB. The device has no delete or rename tool, so this page offers neither.
      </p>

      <nav className="files-crumbs" aria-label="Breadcrumb">
        {trail.map((crumb, index) => {
          const current = index === trail.length - 1;
          return (
            <Fragment key={"crumb:" + crumb.path}>
              {index > 0 && (
                <span className="files-crumb-sep" aria-hidden="true">
                  /
                </span>
              )}
              {current ? (
                <span className="files-crumb-current" aria-current="location">
                  {crumb.name}
                </span>
              ) : (
                <button
                  type="button"
                  className="files-crumb"
                  disabled={loadingDir}
                  onClick={() => void read(crumb.path)}
                >
                  {crumb.name}
                </button>
              )}
            </Fragment>
          );
        })}
      </nav>

      <p className="hint">{note}</p>
      {listError && <p className="error">{listError}</p>}

      <div className="files-tools">
        <form className="files-new" onSubmit={(event) => void createFolder(event)}>
          <label htmlFor="files-new-folder">New folder</label>
          <div className="files-new-row">
            <input
              id="files-new-folder"
              value={folderName}
              onChange={(event) => setFolderName(event.target.value)}
              placeholder="name"
              autoComplete="off"
              spellCheck={false}
            />
            <button
              type="submit"
              className="mini"
              disabled={creating || folderName.trim() === ""}
            >
              {creating ? "Creating…" : "Create"}
            </button>
          </div>
        </form>

        <div className="files-upload">
          <label htmlFor="files-upload">Upload a file</label>
          <input
            id="files-upload"
            type="file"
            disabled={uploading}
            onChange={(event) => void upload(event)}
          />
          {uploading && <span className="hint">Uploading…</span>}
        </div>
      </div>

      {actionNote && <p className="hint">{actionNote}</p>}
      {actionError && <p className="error">{actionError}</p>}

      {(entries.length > 0 || up !== null) && (
        <ul className="files-list">
          {up !== null && (
            <li className="files-row files-up">
              <button
                type="button"
                className="files-name"
                disabled={loadingDir}
                onClick={() => void read(up)}
              >
                ..
              </button>
              <span className="files-kind">parent</span>
              <span className="files-size">—</span>
            </li>
          )}
          {entries.map((entry) => (
            <li className={"files-row files-" + entry.kind} key={entry.name}>
              {entry.kind === "other" ? (
                <span
                  className="files-name"
                  title="not a regular file or folder; the console cannot open it"
                >
                  {entry.name}
                </span>
              ) : (
                <button
                  type="button"
                  className="files-name"
                  onClick={() =>
                    entry.kind === "directory"
                      ? void read(childPath(dir, entry.name))
                      : void openText(childPath(dir, entry.name))
                  }
                >
                  {entry.name}
                  {entry.kind === "directory" ? "/" : ""}
                </button>
              )}
              <span className="files-kind">{entry.kind}</span>
              <span className="files-size">
                {entry.kind === "file" ? bytes(entry.len) : "—"}
              </span>
            </li>
          ))}
        </ul>
      )}

      {opening !== null && <p className="hint">Reading {displayFile(opening)}…</p>}
      {fileError && <p className="error">{fileError}</p>}

      {open && (
        <section className="card files-viewer">
          <header>
            <strong className="mono">{displayFile(open.path)}</strong>
            <div className="actions">
              <button
                type="button"
                className="mini"
                disabled={saving}
                onClick={closeViewer}
              >
                Close
              </button>
            </div>
          </header>

          {open.mode === "text" && (
            <>
              <label htmlFor="files-editor">Text</label>
              <textarea
                id="files-editor"
                className="files-editor"
                rows={18}
                value={open.text}
                spellCheck={false}
                disabled={saving}
                onChange={(event) => {
                  const text = event.target.value;
                  setOpen((current) =>
                    current && current.mode === "text" ? { ...current, text } : current
                  );
                }}
              />
              <p className="hint">
                Saving calls files.write and replaces the whole file with this text — the device
                has no partial write. The file is capped at 1 MiB.
              </p>
              <div className="files-actions">
                <span className="hint">
                  {dirty ? "Unsaved changes." : "No unsaved changes."}
                </span>
                <button
                  type="button"
                  className="mini primary"
                  disabled={saving || opening !== null || !dirty}
                  onClick={() => void save()}
                >
                  {saving ? "Saving…" : "Save"}
                </button>
              </div>
            </>
          )}

          {open.mode === "refused" && (
            <>
              <p className="hint">{open.sentence}</p>
              <p className="hint">
                The device refused a UTF-8 read. That is a fact about the file, not a failure of
                this page: opening it as base64 reads its bytes and offers them as a download
                instead of a textarea.
              </p>
              <div className="files-actions">
                <button
                  type="button"
                  className="mini primary"
                  disabled={busy}
                  onClick={() => void openBase64(open.path)}
                >
                  {opening === open.path ? "Reading…" : "Open as base64"}
                </button>
              </div>
            </>
          )}

          {open.mode === "base64" && (
            <>
              <p className="hint">
                The device returned {bytes(base64Bytes(open.data))} as base64. This page does not
                edit binary files; the download saves the exact bytes under the file's name.
              </p>
              <div className="files-actions">
                <button
                  type="button"
                  className="mini primary"
                  onClick={() => {
                    try {
                      download(open.path, open.data);
                    } catch (cause) {
                      setFileError(
                        "Could not decode " + displayFile(open.path) + " for download: " + message(cause)
                      );
                    }
                  }}
                >
                  Download
                </button>
              </div>
            </>
          )}
        </section>
      )}
    </div>
  );
}

/// The child path the plan specifies: a name from a listing under the directory being shown. No
/// `..` is ever built here; the parent row computes its target from the current string instead.
function childPath(parent: string, name: string): string {
  return parent === "" ? name : parent + "/" + name;
}

/// The parent directory of a path, or null at the root — computed, never sent as a `..` component.
function parentOf(path: string): string | null {
  if (path === "") return null;
  const cut = path.lastIndexOf("/");
  return cut < 0 ? "" : path.slice(0, cut);
}

/// The last component of a path; a download uses it as the file's name.
function baseName(path: string): string {
  const cut = path.lastIndexOf("/");
  return cut < 0 ? path : path.slice(cut + 1);
}

/// The breadcrumb's segments, each with the absolute path clicking it lists. The root's label is
/// the page's word for `""`, which is what `files.list` calls the root.
function crumbs(path: string): { name: string; path: string }[] {
  const trail = [{ name: "root", path: "" }];
  if (path === "") return trail;
  let prefix = "";
  for (const part of path.split("/")) {
    prefix = prefix === "" ? part : prefix + "/" + part;
    trail.push({ name: part, path: prefix });
  }
  return trail;
}

/// The two `files.read` refusals that are facts about the bytes rather than failures of the page: a
/// file that is not UTF-8 text (the device names the encoding to ask for) and one over the 1 MiB
/// cap (the device names the size and the cap). A missing file is neither.
function isFileRefusal(sentence: string): boolean {
  return sentence.includes("not UTF-8 text") || sentence.includes("the cap is");
}

/// A path as the page shows it: the root is a word, everything else is rooted with `/`.
function displayPath(path: string): string {
  return path === "" ? "the files root" : displayFile(path);
}

function displayFile(path: string): string {
  return "/" + path;
}

/// The order a directory is read in: folders, then files, then the entries with no opener.
const KIND_ORDER: Record<Kind, number> = { directory: 0, file: 1, other: 2 };

function entryOrder(left: Entry, right: Entry): number {
  return KIND_ORDER[left.kind] - KIND_ORDER[right.kind] || left.name.localeCompare(right.name);
}

function listNote(rows: Entry[]): string {
  if (rows.length === 0) {
    return "This folder is empty; create one above or upload a file into it.";
  }
  const folders = rows.filter((entry) => entry.kind === "directory").length;
  const files = rows.filter((entry) => entry.kind === "file").length;
  const others = rows.length - folders - files;
  const counts: string[] = [];
  if (folders > 0) counts.push(folders === 1 ? "1 folder" : `${folders} folders`);
  if (files > 0) counts.push(files === 1 ? "1 file" : `${files} files`);
  if (others > 0) {
    counts.push(others === 1 ? "1 other entry" : `${others} other entries`);
  }
  const noun = rows.length === 1 ? "entry" : "entries";
  return `${rows.length} ${noun}: ${counts.join(", ")}.`;
}

/// A textarea's API value uses LF; a CRLF file must not start out looking edited.
function normalizeNewlines(text: string): string {
  return text.replace(/\r\n?/g, "\n");
}

/// "0 B", "812 B", "4.2 KB", "1.0 MB" — the device reports bytes, so the row does too.
function bytes(count: number): string {
  if (count < 1024) return `${count} B`;
  if (count < 1024 * 1024) return `${(count / 1024).toFixed(1)} KB`;
  return `${(count / (1024 * 1024)).toFixed(1)} MB`;
}

/// The length of base64 without decoding it: four characters carry three bytes, minus padding.
function base64Bytes(data: string): number {
  const padding = data.endsWith("==") ? 2 : data.endsWith("=") ? 1 : 0;
  return Math.max(0, Math.floor((data.length * 3) / 4) - padding);
}

/// Read a chosen file as a data URL and return only its base64 payload, which is what `files.write`
/// takes with `encoding: "base64"` — the browser never has to build a binary string for `btoa`.
function readAsBase64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onerror = () =>
      reject(reader.error ?? new Error("the browser could not read the file"));
    reader.onload = () => {
      const result = reader.result;
      if (typeof result !== "string") {
        reject(new Error("the browser returned no data URL"));
        return;
      }
      const comma = result.indexOf(",");
      if (comma < 0) {
        reject(new Error("the browser returned no base64 payload"));
        return;
      }
      resolve(result.slice(comma + 1));
    };
    reader.readAsDataURL(file);
  });
}

/// Save the base64 bytes as a file: decode to a typed array, wrap it in a Blob and click a temporary
/// link. The object URL is revoked on the next task, after the browser has started the download.
function download(path: string, data: string) {
  const binary = atob(data);
  const decoded = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) {
    decoded[index] = binary.charCodeAt(index);
  }
  const url = URL.createObjectURL(new Blob([decoded]));
  const link = document.createElement("a");
  link.href = url;
  link.download = baseName(path);
  link.rel = "noopener";
  document.body.append(link);
  link.click();
  link.remove();
  // Give the browser a moment to start the download before the URL is revoked.
  window.setTimeout(() => URL.revokeObjectURL(url), 1000);
}

function message(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}
