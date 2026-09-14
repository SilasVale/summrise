// ── a comment may not cite a file as the authority for the code when it is gone ──
//
// `server/entry.mjs` and `server/routing.mjs` both said "replicates vercel.json's
// rewrites" / "mirroring vercel.json" — four times between them — and **vercel.json
// does not exist**: it went with the Vercel retirement on 2026-09-08, which round 222
// completed. So every reader who wanted to check whether a routing change preserved
// the intended semantics was sent to a file that is not there.
//
// This is round 218's shape (a header that told every editor the opposite of the
// truth about the file they had open) with a sharper edge: the stale text names a
// SOURCE OF TRUTH rather than describing a copy. The behaviour itself was never
// unpinned — `routing.test.mjs` covers `pathFromRest` — so what broke was the
// reference, not the semantics, and that is what this asserts.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync, existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { join, dirname, relative } from "node:path";
import { readdirSync } from "node:fs";

const SERVER = join(dirname(fileURLToPath(import.meta.url)), "..");
const VERCEL_JSON = join(SERVER, "..", "vercel.json");

/** A mention is legitimate only if the same LINE says the file is gone. */
// NOTE: the stems are PREFIXES, so there is no trailing \b — the first version of
// this regex had one, and it therefore did not match "DELETED"/"deleted"/"retired",
// i.e. it rejected the very lines that were correct. It failed loudly on its first
// run, which is the property that makes a parser like this affordable at all.
const GONE = /\b(del|remov|retir|no longer|used to|gone|absent|was |were )/i;

test("no server comment cites vercel.json as the authority while the file is missing", () => {
  if (existsSync(VERCEL_JSON)) return; // present: citing it is fine, and this test is moot
  // Round 232 covered `server/*.mjs` only, and the SAME stale sentence was sitting in
  // `proxies/README.md` — the runbook an operator reads. So the scan covers the README
  // too. The `api/*` handlers mention "vercel" as LOG PREFIXES (`[vercel-zen]`), which
  // is an identifier rather than an authority claim, so they are out of scope by
  // construction: this test keys on `vercel.json`, which they never name.
  const files = [
    ...readdirSync(SERVER)
      .filter((f) => f.endsWith(".mjs"))
      .map((f) => join(SERVER, f)),
    join(SERVER, "..", "..", "README.md"),
  ];
  const offenders = [];
  for (const f of files) {
    readFileSync(f, "utf8")
      .split("\n")
      .forEach((line, i) => {
        if (/vercel\.json/i.test(line) && !GONE.test(line)) {
          const label = relative(join(SERVER, ".."), f);
          offenders.push(`${label}:${i + 1}: ${line.trim()}`);
        }
      });
  }
  assert.deepEqual(
    offenders,
    [],
    "vercel.json does not exist, so no line may present it as the source of the " +
      "routing semantics. Either state on that line that it was deleted, or point at " +
      "server/test/routing.test.mjs, which is where the semantics are actually pinned. " +
      `Offending lines:\n${offenders.join("\n")}`,
  );
});
