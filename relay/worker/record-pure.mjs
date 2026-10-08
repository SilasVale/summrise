// record-pure.mjs — RECORD WHAT THE SHIPPING FILE RELAY'S PURE DECISIONS ANSWER.
//
// ── WHY THIS FILE IS JAVASCRIPT, AND WHY IT IS TEMPORARY ────────────────────────────────────────
//
// The oracle IS JavaScript: `relay/src/index.js` and `relay/src/claim.js` are the shipping
// implementation, and the only way to know what they answer is to run them. This script runs them over
// a case list and writes `pure-corpus.json`; the Rust port is compared against that file by
// `tests/pure.rs`, which needs no Node at all.
//
// **IT IS THE SAME SHAPE AS THE SATELLITES' `verify.mjs` WAS, AND IT GOES THE SAME WAY**: when the port
// has been proven and the JavaScript is deleted (the plan's own order — prove, cut over, delete), the
// corpus is the record and this recorder retires with the file it drives. Until then it is how the
// corpus is RE-recorded after a deliberate change.
//
// Usage: node record-pure.mjs            (writes pure-corpus.json beside this file)
//        node record-pure.mjs --check    (records and diffs against the committed corpus; exit 1 on a
//                                         difference, which is how a drift in the shipping JS is seen)
import { writeFileSync, readFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const { buildContentDisposition, SHA256_RE, genToken } = await import("../src/index.js");
const { decideClaim, unavailableResponse } = await import("../src/claim.js");

// ── the cases ───────────────────────────────────────────────────────────────────────────────────
//
// **THE AWKWARD ONES ARE THE POINT.** A header value built from a filename has to survive quotes,
// backslashes, control bytes, non-ASCII, and the characters JavaScript's `trim()` treats as
// whitespace but Rust's does not (and the reverse): U+0085 is trimmed by Rust and NOT by JS, U+FEFF
// and U+00A0 are trimmed by JS and NOT by Rust. Those three code points are in the list below
// precisely because a hand-written port gets them wrong.
const NAMES = [
  "report.pdf",
  "SummriseAgent-Setup.exe",
  'a"b\\c.txt',
  "quote\"only\".bin",
  "\"\"\"",
  "\\",
  "a\u0000b\u001fc.txt",
  "bell\u0007.txt",
  "del\u007f.txt",
  "",
  "   ",
  "\u0000",
  "  spaced.txt  ",
  "\t\nnewlines\r\n.txt",
  "报告.pdf",
  "café.pdf",
  "🎉.bin",
  "  é  ",
  "mixed-报告-café.bin",
  "\u00a0nbsp.txt",
  "\ufeffbom.txt",
  "\u0085nel.txt",
  "a b&c=d.txt",
  "a-_.!~*'().txt",
  "plus+and%percent.txt",
  "semi;colon,comma.txt",
  "brackets[1]{2}.txt",
  "hash#question?mark.txt",
  "at@sign$dollar.txt",
  "日本語のファイル名.txt",
  "a".repeat(300) + ".bin",
];

// `decideClaim` is the one-time-claim rule: four branches, a numeric coercion and two fail-open /
// fail-closed choices that the comments in `claim.js` argue about at length. Every value shape
// `Number()` can meet is here, because that coercion IS the rule.
const CLAIMS = [];
for (const exists of [true, false]) {
  for (const raw of [
    undefined,
    null,
    "",
    "0",
    0,
    "1000",
    1000,
    "999",
    "1001",
    "abc",
    "NaN",
    "Infinity",
    "-5",
    "1.5",
    "1e3",
    " 42 ",
    "0x10",
    [],
    {},
    true,
    false,
    [1],
  ]) {
    for (const nowMs of [0, 999, 1000, 1001, 1e12]) {
      CLAIMS.push({ exists, expiresAtRaw: raw, nowMs, decision: decideClaim({ exists, expiresAtRaw: raw, nowMs }) });
    }
  }
}

const SHA256_CASES = [
  "0".repeat(64),
  "abcdef0123456789".repeat(4),
  "ABCDEF0123456789".repeat(4),
  "0".repeat(63),
  "0".repeat(65),
  "g".repeat(64),
  "",
  "  " + "0".repeat(64),
  "0".repeat(64) + " ",
  "sha256:" + "0".repeat(64),
];

const ENVELOPES = [["unavailableResponse", unavailableResponse()]];

// `genToken` is RANDOM, so its corpus is a SHAPE rather than bytes: what length, what alphabet, and
// whether two calls ever collide. A port that used `%62` without rejection sampling would still pass
// the shape checks — which is why the Rust test carries the distribution property itself, and why the
// recorded alphabet here is the whole 62 symbols.
const TOKENS = Array.from({ length: 2000 }, () => genToken());
const tokenShape = {
  lengths: [...new Set(TOKENS.map((t) => t.length))].sort(),
  alphabet: [...new Set(TOKENS.join(""))].sort().join(""),
  distinct: new Set(TOKENS).size,
  // A length other than the default, so the port's `len` argument is pinned too.
  short: Array.from({ length: 50 }, () => genToken(7)).map((t) => t.length),
};

async function envelope(r) {
  return {
    status: r.status,
    headers: [...r.headers.entries()].map(([k, v]) => `${k.toLowerCase()}: ${v}`).sort(),
    body: await r.text(),
  };
}

// The blobs this corpus was recorded from, so a reader can tell whether it still describes the files in
// front of them — `git hash-object` on the WORKING TREE, which is what was actually run.
const REPO = join(HERE, "..", "..");
const blob = (rel) => {
  try {
    return execFileSync("git", ["-C", REPO, "hash-object", rel], { encoding: "utf8" }).trim();
  } catch {
    return "(not a git checkout)";
  }
};

const corpus = {
  source: {
    "relay/src/index.js": blob("relay/src/index.js"),
    "relay/src/claim.js": blob("relay/src/claim.js"),
  },
  disposition: NAMES.map((raw) => ({ raw, header: buildContentDisposition(raw) })),
  claims: CLAIMS,
  sha256: SHA256_CASES.map((s) => ({ input: s, matches: SHA256_RE.test(s) })),
  envelopes: await Promise.all(
    ENVELOPES.map(async ([name, r]) => ({ name, ...(await envelope(r)) })),
  ),
  tokens: tokenShape,
};

// **THE RANDOM SECTION IS CHECKED BY ITS INVARIANTS, NOT BY EQUALITY** — `genToken` draws from
// `crypto.getRandomValues`, so a byte-for-byte comparison of the recorded shape would be a gate that
// fails at random. The invariants are what a port has to keep: the length, the whole 62-symbol alphabet,
// no collisions across 2000 draws, and the argument honoured.
function tokenInvariants(shape) {
  const bad = [];
  if (shape.lengths.join(",") !== "22") bad.push(`lengths ${shape.lengths.join(",")} (want 22)`);
  if (shape.alphabet.length !== 62) bad.push(`alphabet covers ${shape.alphabet.length} symbols (want 62)`);
  if (shape.distinct !== 2000) bad.push(`${shape.distinct} distinct of 2000`);
  if (new Set(shape.short).size !== 1 || shape.short[0] !== 7) bad.push(`genToken(7) gave ${[...new Set(shape.short)]}`);
  return bad;
}

const out = join(HERE, "pure-corpus.json");
const text = JSON.stringify(corpus, null, 2) + "\n";
if (process.argv.includes("--check")) {
  const before = JSON.parse(readFileSync(out, "utf8"));
  const differs = ["disposition", "claims", "sha256", "envelopes"].filter(
    (k) => JSON.stringify(before[k]) !== JSON.stringify(corpus[k]),
  );
  const bad = tokenInvariants(corpus.tokens);
  if (differs.length || bad.length) {
    console.error(
      `pure-corpus.json DIFFERS from what the shipping JavaScript answers now: ${differs.join(", ") || "—"}` +
        (bad.length ? `; token invariants: ${bad.join("; ")}` : ""),
    );
    process.exit(1);
  }
  console.log(
    `pure-corpus.json matches the shipping JavaScript (${corpus.disposition.length} disposition, ` +
      `${corpus.claims.length} claim, ${corpus.sha256.length} sha256, ${corpus.envelopes.length} envelope; ` +
      `token invariants hold)`,
  );
} else {
  writeFileSync(out, text);
  console.log(
    `recorded ${corpus.disposition.length} disposition case(s), ${corpus.claims.length} claim case(s), ` +
      `${corpus.sha256.length} sha256 case(s), ${corpus.envelopes.length} envelope(s), tokens: ` +
      `${tokenShape.lengths.join("/")} chars over ${tokenShape.alphabet.length} symbols`,
  );
}
