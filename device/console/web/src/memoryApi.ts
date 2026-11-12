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
  namespace: string;
  source: string;
  runId: string | null;
  /** When the record was written, in Unix SECONDS (the device's unit). */
  createdAt: number;
  /** When it last changed, in Unix SECONDS. */
  updatedAt: number;
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

const entry = (value: unknown): MemoryEntry | null => {
  if (value === null || typeof value !== "object") return null;
  const raw = value as Record<string, unknown>;
  const id = text(raw.id);
  const title = text(raw.title);
  if (id === "" || title === "") return null;
  return {
    id,
    title,
    content: text(raw.content),
    tags: Array.isArray(raw.tags) ? raw.tags.filter((tag): tag is string => typeof tag === "string") : [],
    namespace: text(raw.namespace, "shared"),
    source: text(raw.source, "unknown"),
    runId: typeof raw.run_id === "string" && raw.run_id !== "" ? raw.run_id : null,
    createdAt: seconds(raw.created_at),
    updatedAt: seconds(raw.updated_at),
    deleted: raw.deleted === true
  };
};

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
  const rows = Array.isArray(answer.results) ? answer.results : null;
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
