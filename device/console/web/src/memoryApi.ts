import { callTool, ToolError, type Grant } from "./terminal";

/**
 * **THE MACHINE'S MEMORY, READ THE WAY THE DEVICE WRITES IT.** `memory.list` and `memory.search` answer
 * `{ ok: true, results: [MemoryRecord…] }` (`agent/src/plugins/memory/tools.rs`), and this reader takes that
 * shape rather than a guess: a title, a body, tags, a namespace, the `source` that wrote it, an optional
 * `run_id`, and two SECOND-resolution stamps (`created_at`, `updated_at` — the panel's own `stampSeconds`
 * multiplies by 1000 for the same reason).
 *
 * A record this reader cannot identify is dropped, not invented: an entry without an id cannot be named in a
 * refusal, and one without a title would draw an empty row that looks like a bug.
 */
export type MemoryEntry = {
  id: string;
  title: string;
  content: string;
  tags: string[];
  /** Absent on the build the fleet runs today (`{id, tags, text}`); the page then draws only what it has. */
  namespace: string | null;
  source: string | null;
  runId: string | null;
  /** When the record was written, in Unix SECONDS, or null when the build does not report one. */
  createdAt: number | null;
  updatedAt: number | null;
  deleted: boolean;
};

const text = (value: unknown, fallback = ""): string =>
  typeof value === "string" ? value : fallback;

const seconds = (value: unknown): number => {
  // The store writes seconds; a build that ever wrote milliseconds would be off by 1000, so the reader refuses a
  // number that cannot be seconds (anything past the year 3000 is somebody's millis).
  const n = typeof value === "number" && Number.isFinite(value) ? value : 0;
  return n > 0 && n < 32_503_680_000 ? Math.round(n) : 0;
};

/**
 * One record, in EITHER shape. Measured on the LIVE fleet 2026-11-12: the shipped device (0.3.292) answers
 * `[{id, tags, text}]` — no title, no content, no timestamps — while the source in this tree answers
 * `{ok, results:[{id, title, content, tags, namespace, source, created_at, updated_at, deleted}]`. The page read only the
 * newer one, so the real machine's memory drew as `the machine answered without a list`.
 */
const entry = (value: unknown): MemoryEntry | null => {
  if (value === null || typeof value !== "object") return null;
  const raw = value as Record<string, unknown>;
  const id = text(raw.id);
  if (id === "") return null;
  const body = text(raw.content) !== "" ? text(raw.content) : text(raw.text);
  const titled = text(raw.title);
  // A build without a title gets one from its own text: the first line, cut at a word boundary. It is the record's opening
  // sentence, not a summary the console invented.
  const title = titled !== "" ? titled : firstLine(body);
  const content = titled !== "" ? body : body.slice(Math.min(body.length, firstLine(body).length)).trim();
  if (title === "") return null;
  return {
    id,
    title,
    content,
    tags: Array.isArray(raw.tags) ? raw.tags.filter((tag): tag is string => typeof tag === "string") : [],
    namespace: text(raw.namespace) !== "" ? text(raw.namespace) : null,
    source: text(raw.source) !== "" ? text(raw.source) : null,
    runId: typeof raw.run_id === "string" && raw.run_id !== "" ? raw.run_id : null,
    createdAt: seconds(raw.created_at),
    updatedAt: seconds(raw.updated_at),
    deleted: raw.deleted === true
  };
};

/** The first line of a record's text, cut to 80 characters at a space when it is longer. */
function firstLine(body: string): string {
  const line = body.split("\n")[0].trim();
  if (line.length <= 80) return line;
  const cut = line.slice(0, 80);
  const space = cut.lastIndexOf(" ");
  return (space > 40 ? cut.slice(0, space) : cut).trim() + "…";
}

/**
 * The list, newest first. `query` is the difference between the two tools: `memory.search` matches a substring
 * over title, content and tags, `memory.list` enumerates. Passing one call for both would make the page's
 * search box a lie whenever the device had a search tool and the page skipped it.
 */
export async function readMemory(
  grant: Grant,
  query: string
): Promise<{ entries: MemoryEntry[]; ok: boolean; error: string | null }> {
  const trimmed = query.trim();
  const body = trimmed === ""
    ? await callTool(grant, "memory.list", { limit: 50 })
    : await callTool(grant, "memory.search", { query: trimmed, limit: 50 });
  let answer: Record<string, unknown>;
  try {
    const parsed: unknown = JSON.parse(body);
    answer = (parsed !== null && typeof parsed === "object" ? parsed : {}) as Record<string, unknown>;
  } catch {
    // A body that is not JSON is not a list; the page says so rather than drawing an empty store.
    return { entries: [], ok: false, error: "the machine answered with text, not a list" };
  }
  if (answer.ok === false) {
    return { entries: [], ok: false, error: text(answer.error, "the machine refused the read") };
  }
  // **TWO SHAPES, AND THE DEVICE SAYS WHICH BY ANSWERING.** Measured on the LIVE fleet 2026-11-12: the shipped device
  // (0.3.292) answers `memory.list` with a **bare array** of `{id, tags, text}` records, while the source in this tree answers
  // `{ok, results:[…]}` with the richer record. The page read only the newer one, so the real machine's memory drew as
  // `the machine answered without a list`.
  const rows: unknown[] | null = Array.isArray(answer)
    ? (answer as unknown[])
    : Array.isArray(answer.results)
      ? answer.results
      : null;
  if (rows === null) {
    return { entries: [], ok: false, error: "the machine answered without a list" };
  }
  return {
    entries: rows.map(entry).filter((row): row is MemoryEntry => row !== null),
    ok: true,
    error: null
  };
}

/** The one sentence a failed read earns: the device's own words when it gave them, this page's otherwise. */
export function memoryFailure(error: unknown): string {
  if (error instanceof ToolError) {
    return error.message.trim() !== ""
      ? `The memory list could not be read — ${error.message.trim()}`
      : "The memory list could not be read.";
  }
  return "The memory list could not be read.";
}
