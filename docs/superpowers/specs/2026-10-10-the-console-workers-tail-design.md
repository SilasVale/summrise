# The console worker's tail — the four route families, and what a worker entry is

## 0 · Where this sits

**Landing 5's tail**, of `2026-10-08-every-decision-is-rust-design.md`, whose §4 row 5 is the whole slice:

> `| 5 | the worker entries | gateway/src 4,913 · index/src 417 · proxies api 344 | byte-for-byte parity …`

**The device-proxy slice of landing 5 is landing now** (its own branch), and this spec is what remains after it.

## 1 · What this changes, in one sentence

**The console worker's remaining decisions become Rust** — the four route families that were never ported — so that
`LOGIC | gateway/src/ |` and `LOGIC | gateway/test/ |` can be reclassified with reasons rather than left as the last
LOGIC rules in the repository.

## 2 · The inventory, MEASURED ON `main@35bf4328`

| rule | files |
|---|---|
| `LOGIC | gateway/src/ |` | **38** |
| `LOGIC | gateway/test/ |` | **54** |
| **the gate's LOGIC count** | **91** — `gateway/src/` 37 + `gateway/test/` 54, in TWO rules |

**AND `agent/scripts/` IS ALREADY BOUNDARY** — every tracked JS file under it carries a longer, more specific rule, so
the directory rule is fully shadowed and matches zero files. **The sweeps' landing measured that and this spec repeats
it because a reader will otherwise expect the sweeps' slice to move the count. It does not.**

## 3 · The four families, and where each lives today

| family | its home now | why it was not ported |
|---|---|---|
| **`/api/admin/*`** | `plugins/admin.ts`, `store/admin.ts`, `store/models.ts` | the console's own administration surface; it reads the KV stores the worker's dispatch does not model |
| **`/api/plugins/status`** | **`plugins/status.ts` (NEW, 314 lines, landing in the device-proxy slice)** | it carries `routeStats(ctx)` — a registration index and a per-isolate hit counter for every route of every plugin, **a property of the TypeScript plugin table** that the Rust dispatches by family without |
| **the model-route selection** | `plugins/models-probe.ts`, `plugins/admin.ts`, `plugins/auth.ts` | the provider catalogue and the probe that decides which channel answers |
| **the key diagnostics** | `plugins/auth.ts`, `lib/ratelimit.ts` | `me/keys`, the reveal, the regenerate sweep, and the per-bucket limiters |

**AND `/api/plugins/status` IS THE PRECEDENT THIS SPEC ARGUES FROM.** It is the one route whose absence from the corpus
is *by design* — `gateway/wasm/src/mcp.rs:62` says *"deliberately NOT here"* and asserts it, and `index.ts` says
*"THE ONE EXCLUSION … The route therefore stays on the TypeScript path."* **A route that the oracle cannot cover is a
route whose test has no other witness**, and the device-proxy slice learned that the hard way: it deleted that test by
a name match, the suite's count FELL, and restoring it is what found a live 404.

## 4 · The rule, and the one thing this landing must not do

**A worker entry that replaces a shipping JavaScript one needs BYTE-FOR-BYTE PARITY EVIDENCE** — the ancestor's own
requirement in §4 row 5 — and the ported surface's oracle is the corpus (`gateway/wasm/…/expected.json`, 399 cases),
which pins **what the Rust answers**. So:

* a family that IS ported goes to the worker, and its tests go with a pointer naming the corpus cases that now cover
  them (`rename` 9 · the admin gate matrix 14 · `login` 9 · `panel-grant` 8 · `upload` 17 · `self-register` 9 ·
  `register` 9 · `/api/devices` 54 · `tunnel-token` 3 · `register-keys` 3 · `install-cmd` 1);
* **a family that is NOT ported STAYS, with a written reason** — and the reason is a fact about the Rust, not about
  effort: *the counters are a property of the TypeScript plugin table and the worker dispatches by family without it*;
* and **no test is deleted whose subject has no other witness.** Two categories of survivor, not one: **the subject
  survived a port that has not happened yet**, and **the subject has no other witness at all.**

## 5 · What verifies it

1. **The inventory gate**, with `MAX_LOGIC` measured LAST on the landed tree — force it to 0, read the refusal, write
   what it printed **and the tree**, because every number quoted in this migration has been true of some other one.
2. **The differential**, for any family that moves: the corpus is byte-compared, and it is the only oracle that can say
   the Rust answers what the TypeScript answered.
3. **`bash scripts/test/all-gates.bash`** — 49 ok / 0 failed on this repository's own bar — **and the CI run**, because
   a local bar that is not CI's check is not the same check. **That gap has cost this session four CI cycles.**

## 6 · What is NOT in this landing

* **The MCP tool table's single-sourcing.** The ancestor's §6.1 measured it and settled it: the remaining duplication is
  the *description text and the schema shapes* of 800-odd lines, **held together by a gate that reads the Rust side**,
  and *"the decision is unchanged in direction and smaller in scope — landing 5 removes the file by making the entry
  Rust, not by generating the table."*
* **The React/TSX view layer and the Electron host**, out of scope with measured reasons in the ancestor's §6.
* **`index/src` and `proxies api`** — the other two thirds of §4 row 5, each its own landing.

## 7 · The risk this spec names rather than hides

**A route family that stays TypeScript is a route the Rust worker does not answer, and the corpus cannot see it.** The
`/api/plugins/status` precedent shows both halves: the absence is *correct* (the counters are a TS property) and the
absence is *invisible* (the corpus has zero cases, so a deletion there is not a failure). **So every staying family
needs a test that runs, and the manifest rule that keeps it needs a reason a reader can check against the Rust.**
