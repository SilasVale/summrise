# ADR 0011 — two artifacts carrying one obligation get an assertion that reads both

Date: 2026-09-14 · Status: Adopted

## Context

This repository has a recurring defect, and by round 224 it had been found the same way
seven times: **a single fact is carried by two artifacts, nothing compares them, and one of
them drifts.**

| Found | The pair | How it was discovered | How far it had drifted |
|---|---|---|---|
| round 199 | `gateway/public/code/files/**` vs `manifest.json` | by hand, in a release pre-flight | the `store/byok.ts` entry sat uncommitted **nine rounds** |
| round 213 | the ledger's `Round log head` vs the journal's `Last updated` | by reading both | **91 rounds** |
| round 214 | `GATEWAY_DISPATCHED_CODES` (Rust) vs the literals `mcp.ts` matches (TS) | by extracting both mechanically | not yet — but one direction was uncovered |
| round 215 | `ALLOWED_ORIGINS` vs `CONSOLE_HOST`, `extension/manifest.json`, and zen-go's copy | by reading the comment's three claims | not yet — and `CONSOLE_HOST` is overridable from the dashboard |
| round 216 | `kvSeed` placement vs the header's enumeration of its own call sites | by reading the header | not yet — one word would reintroduce a quota vector |
| round 218 | the browser helper's header vs which of its two copies is generated | by reading the header | the header told every editor the opposite of the truth |
| round 221 | the convergence table's evidence cells vs the live CDN | by dialling the CDN | **two rows were done and read as open** |
| round 224 | the ADR index's numbers vs `docs/adr/`'s filenames | by counting both | two numbers each named two files, invisibly |

Every one of these was, on the day it was found, **documented on at least one side**. The
comment in `ratelimit.ts` names its call sites correctly. The ADR index lists every number
correctly. The browser helper's header describes a real generation step. `upstream.ts`
explains why a bare model passes through. **Documentation was never the missing thing — a
comparison was.** A statement about another file, written in this file, is a claim with the
epistemic status of a comment: it cannot fail.

## Decision

When one obligation is carried by two artifacts, it gets an assertion that **reads both as
data and compares them**. Neither side's value is restated in the assertion, so the
assertion cannot agree with itself.

Three rules follow from the instances above, each earned by a specific failure:

1. **The assertion reads the other artifact, not a copy of its content.** Round 214's
   `gateway_dispatched_codes_are_exactly_three` asserted a hardcoded list against itself
   while its message claimed "`gateway/src/mcp.ts` matches exactly these three" — a claim
   about a file it never opened. It was rewritten to open the file.
2. **A recorded exception carries the evidence that it is still an exception.** Round 224's
   ADR-collision allow-list asserts that each recorded collision still names exactly the
   files it names, because "a stale exception is how an exception becomes a licence" (round
   212's lesson about comments describing rules that no longer exist, applied to an
   allow-list).
3. **Where a rule is mechanical, the assertion DERIVES the artifact rather than checking it
   against a list.** Round 217's plugin-directory contract became an import-graph
   derivation; round 219 showed that an edit to `gateway/src` trips two independent
   detectors — the semantic test and the mirror obligation — so a mutation producing only
   ONE failure now means the mirror test did not fire, which is itself a signal.

## Rejected options

* **Document the agreement on each side.** Rejected because it is what was already being
  done, seven times, in seven files — and each of those documents was correct when written
  and wrong when read. A comment cannot fail, so it cannot warn.
* **Restate the value on both sides and assert each against its own copy.** This is the trap
  round 214 named: both sides' tests pass while the two diverge, and the mutation that
  matters — the OTHER side changing — turns nothing red. It is also the cheaper-looking
  option, which is why it needs naming.
* **A CI step that diffs the two files textually.** Rejected because most of these pairs are
  not textually comparable: a Rust `&[&str]` against TypeScript `data?.code === "…"`, a
  number against a filename, a boolean in a route table against six `slice` call sites. The
  comparison has to understand each side, and that is a test, not a diff.
* **Trust review to catch it.** Rejected on the record: rounds 199 and 213 were each found
  after the drift had already lasted nine and ninety-one rounds respectively, in artifacts a
  reviewer had looked at.

## Consequences and measurements

* Every instance above is mutation-proven, not merely green. The mutations are the point:
  removing the `byok.ts` manifest entry (round 211), editing the ledger head (213), adding a
  fourth dispatched code (214), deleting an origin from either CORS list (215), adding
  `kvSeed: true` to `auth.ts` (216), restoring the browser helper's old header (218),
  flipping `defaultRoute.stripPrefix` (220), creating a third colliding ADR number (224).
* **Measurable metric**: a mutation on EITHER side turns the assertion red. An assertion that
  only fails when its own file changes is not an instance of this ADR.
* **Failure criterion**: an assertion that passes while the two artifacts disagree. Round
  214's original test is the worked example — it passed for as long as nobody changed the
  gateway, and it would have kept passing after a gateway-side change.
* **Deletion criterion**: delete an instance when the two artifacts stop being two — for
  example if a manifest becomes build-generated rather than committed, there is no second
  copy to compare and the assertion loses its subject. Deleting it then is correct, and the
  deletion should say so rather than leaving a test asserting nothing.
* Real cost per instance: one test file, and a parser for each side's format. That parser is
  where every harness bug in rounds 214-219 occurred (five of them: a `find('[')` landing
  inside a type, two landed-checks matching strings that appear in comments, a header
  lookahead broken by a comment's continuation, and a prescribed grep that matched no line).
  **The parser is the fragile half, and it fails loudly on the FIRST run rather than
  silently on the day the artifacts diverge** — which is the property that makes the trade
  worth taking.
