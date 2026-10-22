# The design sweeps' judge and driver are Rust — the split, and what stays JavaScript

## 0 · Where this sits

Landing **2** of `2026-10-08-every-decision-is-rust-design.md`, whose §4.1 says of it: *"2b is a decision
rather than a port — measured before it is started."* This is that measurement, and the shape it settles.

## 1 · What this changes, in one sentence

**The in-page COLLECTOR stays JavaScript, and the JUDGE and the DRIVER become Rust**, because
`browser_run_script` takes a JavaScript file and the collector only measures and returns rows, while everything
that reads those rows and decides what they mean is pure logic over JSON.

## 2 · The rule, in three parts

| part | language | why |
|---|---|---|
| the **collector** — runs in the page, walks the DOM, returns rows | **JS, and it stays** | `browser_run_script` takes a JS file; it decides nothing, it measures |
| the **judge** — rows in, verdict out | **Rust** | pure logic over JSON: thresholds, comparisons, the section ladder |
| the **driver's DECISIONS** — which section runs, what a failure means, the budgets | **Rust** | it is configuration and judgement, and it is data (`serde`), not generated code |
| **the driver's PLAYWRIGHT CALLS** — `page.goto`, `page.evaluate`, `page.mouse` | **JS, and it STAYS** | **RULING R1, MEASURED — AND THIS SPEC WAS WRONG ABOUT IT UNTIL 2026-10-10** |

### R1 · Why the Playwright driving cannot become Rust, in the ancestor's own measurement

**This spec first said "the DRIVER becomes Rust — the Playwright passes (`idlePass`, `ackPass`, …)", and the landing
refused it and went to the document this one descends from.** `2026-10-08-every-decision-is-rust-design.md` §4.1 had
already measured the question and settled it the other way:

> *"the repository has **NO Rust browser driver** … a grep for `webSocketDebuggerUrl`, `json/version` and
> `Input.dispatch` across every crate returns nothing, and **no CDP crate is in the lockfile**. So 'the Playwright
> driving becomes Rust' cannot be a port — **it would be a NEW browser dependency** … the driver stays JavaScript and
> becomes dumb … Rust builds the plan and judges the rows, and the plan is data (serde), not generated code."*

**So the split is not "driver in Rust" but "decisions in Rust, driving in JavaScript"** — and the passes keep their
`page.*` calls while every decision they were interleaved with moves. **A spec that contradicts its own ancestor is a
spec somebody re-derives; this paragraph is what stops that**, and it is here because a landing checked an instruction
against the document it was supposed to come from instead of following it.

## 3 · The inventory, MEASURED ON `main@556cd05a`

| file | lines |
|---|---|
| `agent/scripts/lib/design-sweep.mjs` | **1,765** |
| `agent/scripts/lib/sweep/console-run.cjs` | 736 |
| `agent/scripts/lib/sweep/panel-run.cjs` | 1,306 |
| `agent/scripts/lib/sweep/panel-stub.cjs` | 679 |
| `agent/scripts/lib/sweep/live-run.cjs` | 155 |
| `agent/scripts/lib/sweep/landing-run.cjs` | 140 |
| `agent/scripts/lib/sweep/plan-runtime.cjs` | 75 |
| **total** | **4,856** |

**AND THE NUMBERS THE OBJECTIVE CARRIES ARE STALE — 2,455 + 3,415 against this 1,765 + 3,091 — WHICH IS WHY THIS
TABLE IS MEASURED RATHER THAN QUOTED.** The objective's figures were true of some earlier tree; the rule this
repository learned the hard way is that a number without its tree is a number about somebody else's tree.

## 4 · The precedent, which is not a guess

**The e2e sweep was split exactly this way and it worked.** `agent/e2e-judge/` is the judge and the section
ladder (its own crate, 42 tests), **`agent/scripts/e2e/e2e.js` is the driver at 1,057 lines and decides
nothing**, and the plan reaches a device by `--bundle` — which replaced a hand-`cp`'d copy that no gate
compared. That landing's verification is the pattern to copy here: **one defect per check**, the CI pipeline
run against the real binary on loopback (`== 25/25 passed ==`), and the judge's refusals proven rather than
assumed.

## 5 · What verifies it

1. **A differential, on one tree, before the JavaScript is deleted.** The e2e landing's rule: the Rust judge
   and the JavaScript judge must agree on the same rows before either the old file or the old path goes.
2. **The inventory gate.** `LOGIC | agent/scripts/ |` is one of the five LOGIC rules left on `main`; when this
   landing ends, the sweeps' rule becomes BOUNDARY with the collector named as the reason, and `MAX_LOGIC`
   is **measured last** on the landed tree — force it to 0, read the refusal, write what it printed.
3. **The collector's own gate.** A JS file that stays needs a written reason in the manifest, and the reason
   is the one thing that cannot be argued away: **`browser_run_script` takes a JS file.**

## 6 · What is NOT in this landing

* **The React/TSX view layer** — out of scope with measured reasons in the parent spec (§6), unchanged here.
* **The Electron host** — same, and CDP `:9333` remains a product contract.
* **The other four LOGIC rules on `main`** — `agent/scripts/e2e/` (landing, in flight), `scripts/test/`
  (landing, in flight), `gateway/src/` and `gateway/test/` (landing 5, whose device-proxy slice is in flight
  and whose tail is the four surviving route families).

## 7 · The one risk this spec names rather than hides

**A collector that only measures still has to return the RIGHT rows**, and no Rust gate can see inside a page.
The e2e split answered this with a real device and a real browser; this landing has to answer it with the same
thing — `agent/resources/panel-react/scripts/local-browser.mjs` runs the real panel sweep **here**, against
what this checkout builds, which is the only way a collector's defect is visible before a device run.
