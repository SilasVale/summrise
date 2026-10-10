# Summrise Agent E2E suite

Repeatable, device-side verification that the AI-facing surface works
end-to-end. Created round-273 to fix the "tests were one-off scripts in
pwout" gap — every round-264..268 verification now runs from one file.

**IT IS TWO PROGRAMS NOW, IN TWO LANGUAGES, AND THE LINE BETWEEN THEM IS THE POINT.**

| half | where | what it is |
|---|---|---|
| the **DRIVER** | `agent/scripts/e2e/e2e.js` (JavaScript) | the platform half: `fetch` against the agent's own tool surface, a CDP WebSocket to 9333, the `browser_run_script` payloads, `require('node:fs')`, and the polling loops that wait for an answer. It writes down what it ASKED and what came back, and decides nothing |
| the **PLAN** and the **JUDGE** | `agent/e2e-judge/` (Rust, crate `summrise-e2e-judge`) | which sections run, in what order, what `--only` may name, every command and marker the suite types or requires, the policy caps and floors — and the 74 `check(...)` call sites that decide whether a device passed |

That is the same cut the design sweeps made: their in-page COLLECTOR stayed JavaScript because
`browser_run_script` takes a JS file, while the JUDGE and the PLAN became Rust. **This file used to hold
both halves**, and the reason that mattered is worth keeping: the checks *computed* their conditions
(`.some(...)`, `rs === 'OK'`, `text.includes('E2E-SESSION-OK')`, `state === 'partial'`), *derived* the
sentences a reader triages a failure from, and *decided* what ran. Those are decisions, and the boundary
manifest had cleared the file as a harness on a sentence that was true of its driving half only.

## What it checks

| Section | Round source | Verifies |
|---|---|---|
| `terminal` | 268 | session open → list contains it → resize → session-mode execute → run_in_background → terminal_read collects → terminal_write keystrokes → screen shows echo → unknown-session read evicted → close → history retains closed (12 checks) |
| `file` | 266 | stat → 2-page append upload (300KB) → single raw read download (1MiB cap) → list contains upload → missing-stat ok:false → text write+read (7) |
| `workflow` | 267 | process_list → local execute → file_write → stat → memory_save → memory_search → memory_list → memory_update → search-updated → memory_export → memory_delete → delete-verified (zero hits) (11) |
| `panel` | 274 | AI writes a unique marker into a terminal session → the SPA's VISIBLE xterm must show it (display verification) (2) |
| `mcp` | 281/285 | stdio + http connect auto-select the embedded view; first browser_navigate drives it. Click proof: snapshot → browser_click on an injected same-origin link (Learn more fallback) must drive the view (round-313) (6 per transport) |
| `evidence` | 277 | AI screenshot into pwout → GET /api/browser/pwshots (Evidence drawer data) lists it (2) |
| `browser` | 268 | browser_pw_info bundled → run_script fail path (exit≠0) → browser_run_script drives the embedded view via CDP 9333 → SPA address bar follows → focus-trap: focused-but-untyped bar still follows AI navigation (5) |
| `governance` | round-9 of the game-design work | goal set → the AI reads it off `terminal_list` → gate armed → an execute BLOCKS with its `intent`/`considered` → approve+grant → the granted family runs unasked → revoke → disarm → **the audit trail explains all of it** (16) |
| `runs` | round-13 | run identity: an id is minted → work wears it → an unattributed sibling is NOT absorbed → the boundary precedes what it names → close with its outcome → an unknown id is accepted as `known:false` (9) |

**76 checks on a full run**, from 74 `check(...)` call sites: the six `mcp` checks run once per transport,
and four of the call sites are alternative arms a clean run does not take (the two "no desktop SPA target"
early returns, the `panel` one, and `runs`' `prerequisites missing` branch).

## Usage

**On a box with cargo** — the plan comes from the judge, so `--only` and `--no-browser` are validated in
Rust rather than by a list the driver keeps:

```bash
summrise-e2e-judge --plan --only governance,runs --no-browser > plan.json   # what will run
node agent/scripts/e2e/e2e.js --plan plan.json --token <token> --rows rows.json
summrise-e2e-judge --rows rows.json --expect governance,runs                # 0 passed · 1 failed · 2 did not run
```

(or `cargo run --quiet --manifest-path agent/Cargo.toml -p summrise-e2e-judge -- …` when it is not on PATH.)

**On a device** — one file, and it already carries the plan:

```powershell
node e2e.js --token <agent-token>                  # writes e2e-rows.json
node e2e.js --token <token> --rows D:\Summrise\e2e-rows.json
```

Env: `SUMMRISE_AGENT_TOKEN` also works; `--base` overrides the agent URL; `SUMMRISE_EVIDENCE_DIR`
(default `C:\ProgramData\Summrise\pwout`) and `SUMMRISE_PW_DIR` (default
`D:\Summrise\components\playwright`) override the two paths the plan carries.

**`--only` AND `--no-browser` ARE REFUSED BY THE DRIVER, ON PURPOSE.** Both are decisions about which
sections run, and the vocabulary they are checked against lives in Rust — the guard that refuses a name
selecting nothing exists because `--only governance-typo` used to be indistinguishable from success.
Re-implementing the filter in JavaScript would put that decision back where it came from. Ask the judge
for a plan instead, as above; the bundled copy a device runs carries the full nine-section plan.

## The exit codes, and where the verdict lives

**THE DRIVER'S EXIT CODE SAYS WHETHER IT COLLECTED, NOT WHETHER THE DEVICE PASSED.** `0` = the rows were
written, `1` = no token, `2` = it could not run at all (no plan, or a section it does not implement). A run
that aborted half-way still writes its rows, and those rows carry `driver.abort`, which the judge reports
as a `suite` failure. **The judge's exit code is the verdict**: 0 ran and all passed, 1 ran and something
failed, 2 DID NOT RUN — a bad `--only`, an unreadable rows document, or `--expect` naming sections the
recording did not run.

**SO THE ROWS HAVE TO LEAVE THE DEVICE, AND THAT IS NOT FREE YET.** Everything a reader acts on is
computed from the rows, and on a device there is no toolchain to compute it with. The rows are a file and
a file can be carried; a full run's is about 1 MB, most of it the 300 KB transfer's base64 read back
twice, and the relay route is the one the recorded corpus already failed to take (below). **What is
therefore NOT verified today: a device run's own PASS/FAIL lines.** They come from the judge, and the
judge is not on the device. Three ways out, in the order they should be tried: the relay (fixes the corpus
and this together), a plan-only run of the two platform-neutral sections whose rows are small enough to
come back through a terminal read, or a `summrise-e2e-judge.exe` staged onto the device beside
`summrise-launch.exe` — which is a release-path change and the operator's call, not a night's work.

## The equivalence oracle, and why it is a NAMED follow-up rather than done

The thing that proves this port reproduces the JavaScript verdicts is a **RECORDED RUN judged by both**:
`e2e-rec.json`, **906 KB of raw tool answers** recorded on 2026-10-09 through the `--record` mode that
existed for exactly this port. It is **stranded on the device**, because the device cannot reach the relay
— so it is not wired, and `agent/e2e-judge/tests/checks.rs` says so in its own header rather than letting
a reader assume otherwise.

What IS verified without it:

* a synthetic run that is green in every section (76 checks, `exit 0`), and **one planted defect per
  check** that must turn that check red — the table in `tests/checks.rs` is the suite's real spec;
* the two directions of the contract between the halves: every observation id the judge reads is one the
  driver records, and every plan key is read by one half and published by the plan (`tests/contract.rs`);
* the emitted bundle is byte-for-byte what the emitter produces today (`tests/bundle.rs`);
* the `agent-e2e` CI job drives the REAL binary on loopback with `--only governance,runs` and judges the
  rows it recorded — the 25 checks of the two platform-neutral sections, end to end, on every push.

**And one check is pass-or-skip BY CONSTRUCTION**: `mcp http drives embedded view`. An `http` arm whose
navigation did not reach the marker is reported as a SKIP, because only that transport can be on
playwright-mcp's private-headless fork, where "the embedded view followed" asserts a precondition the
device does not have (round 100, measured — eleven rounds were spent asserting it anyway). The mutation
table names that rather than quietly excusing it.

## Which sections run where

`governance` and `runs` are deliberately **platform-neutral** — tool calls and HTTP only, no PowerShell
and no path joining — so they also run against a Linux agent on loopback, which is what CI does:

```bash
cd agent
cargo build --features terminal --bin summrise-agent
# config.yaml: server.host 127.0.0.1, a free port, a device_token
./target/debug/summrise-agent /tmp/summrise-e2e/config.yaml &
cargo run --quiet -p summrise-e2e-judge -- --plan --only governance,runs > /tmp/summrise-e2e/plan.json
node scripts/e2e/e2e.js --plan /tmp/summrise-e2e/plan.json --token <token> \
  --base http://127.0.0.1:<port> --rows /tmp/summrise-e2e/rows.json
cargo run --quiet -p summrise-e2e-judge -- --rows /tmp/summrise-e2e/rows.json --expect governance,runs
```

Both ran in CI before this port too, and the point was the same: the `governance` section sat RED for a
full round without anyone noticing, because nothing executed it. What the port adds is that the two
sections not in CI are at least JUDGEABLE without a device — a recording is a file.

**There is NO `SUMMRISE_DATA_DIR` override** — `paths.rs` resolves the data dir
registry-first (`registry_value("DataDir")`, else `install_dir()`), and on Linux
`registry_value` is always `None`, so every runtime directory (sessions, memory,
pwout, runs, logs) lands BESIDE THE EXE, i.e. under `target/debug/`. Earlier
revisions of this recipe exported `SUMMRISE_DATA_DIR=/tmp/summrise-e2e/data`, which
silently did nothing: the agent ran happily against `target/debug/` while the
reader believed it was isolated. If you need a clean data dir, point the
config's `server` at a scratch install and remove `target/debug/{sessions,runs,pwout,memory,logs}`
between runs.

The other seven sections are DEVICE-targeted by design and will partially fail
elsewhere: `terminal` runs `Write-Output` (PowerShell), and `file`/`evidence` join
paths with `\` under a hardcoded `C:\ProgramData\Summrise\pwout`. Those failures
are the environment, not the agent — verified by pointing `SUMMRISE_EVIDENCE_DIR` at a
Linux directory, which moves `file` from an ENOENT abort to 6/7 with only the
backslash join failing.

## Rendering the REAL panel (`../panel-render-audit.mjs`)

`e2e.js` drives the agent's HTTP surface. It says nothing about what the operator
SEES. `scripts/panel-render-audit.mjs` covers that gap:

```bash
node agent/scripts/panel-render-audit.mjs --out /tmp/panel-audit
```

It loads `agent/resources/panel/panel.js` + `panel.css` — the exact bytes the agent
embeds via `include_str!` — into a page served at a real `/panel/` origin (by
Playwright route interception, so NO listener is opened anywhere), with
`window.fetch` stubbed to a fixed device state. The app then boots through its own
production path and renders its own component tree. It:

1. measures **every visible text node**, alpha-compositing background alpha AND the
   ancestor `opacity` chain;
2. asserts each governance element is **present**, so a clean sweep over a page
   that failed to render cannot pass;
3. checks the top bar for overflow and reports page errors.

### Why it exists

Every earlier visual check was a hand-built HTML gallery: markup I wrote,
stylesheet I linked, elements I had just created. That verifies the CSS you are
thinking about and nothing else — and it is how FIVE chrome contrast defects
survived several rounds of claiming to "audit" contrast. `#session-count` (2.33
light), `.side-time` (2.29), `.side-count` (4.40), `.tab.active` (3.83) and
`.view-switch-btn.active` (3.65) were wrong the whole time and no
feature-by-feature gallery could see them, because I only ever measured what I was
working on. Auditing the real running app found all five in one pass.

Two measurement traps are baked into the probe, both paid for:

* **`opacity` is in neither `getComputedStyle(color)` nor `backgroundColor`.** A
  probe that composites backgrounds alone reports a dimmed element at its FULL
  colour. Six text sites were dimmed that way and every one measured "ok".
* **xterm paints from its own palette**, not from the panel tokens. Measuring it
  here mixes two colour systems; `themeContrast.test.ts` pins the same exclusion
  with the same reason.

## Why the trail assertions exist

Every check in `governance` passed on an early build while **arming the approval
gate left no trace in the audit trail at all** — the hold was recorded, the goal
was recorded, and the switch that decides whether commands run unasked was
invisible. Found only by driving a real agent end to end: each piece was
individually correct and only the joined-up history was missing. The trail
assertion is what turns that from a discovery into a failure.

(mcp covers stdio+http auto-select AND AI click interaction — snapshot →
browser_click {target} on the injected same-origin link (Learn more fallback) must
drive the embedded view, proving interactions beyond navigation reach the page the
user watches).

## Measured on the device (round 9 of the standing goal)

All nine sections, `node e2e.js --token <device_token>` against d1 at release 1.2.474, judged by the
JavaScript that this port replaced:

```
== 73/74 passed == (2 skipped)
```

The two skips are the `mcp http` arm's view checks: that arm launched its own private headless browser, so
there is no embedded view for a click to drive. The suite said so rather than passing quietly, which is why
they are counted separately.

**AND REREADING IT AFTER THE PORT FOUND THE COUNT IS NOT THE ONE THE OLD COMMENT CLAIMED.** The JS file
says "the 75 `check(...)` calls below" in a comment; the file carries **74 call sites** (the 75th `check(`
in the file is the function's own definition, and a 76th is that comment). A full green run registers
**76 checks** — the six `mcp` ones run per transport, and four sites are alternative arms a clean run does
not take. The ported judge reproduces 76 for the same input, and `tests/checks.rs` pins it.

**THE ONE FAILURE WAS A FLAKE, AND IT IS THE CHECK ROUND 313 CALLED DETERMINISTIC.**
`mcp stdio click drives embedded view` missed during the full run — the view ended at `https://example.com/` instead of
the injected link's target — and PASSED when the section was re-run alone (`--only mcp` →
`https://example.com/inner-click-test`). So the same-origin click proof is still load-sensitive, and a full-suite run is
the load that finds it.

**AND ITS TRIAGE PRINTED `geometry: null`, WHICH WAS TWO CAUSES WEARING ONE STRING.** `clickGeom` returns the CDP
expression's value and that expression is `JSON.stringify(...)`, so a page with no `Learn more` link yields the
four-character STRING `"null"` — truthy, and printed verbatim. "The link is not in the embedded view" and "the probe
answered nothing" read identically and need OPPOSITE responses: the first is the click path losing the page, which is
what the triage exists to show; the second is the instrument being broken. It now says which, and the line is a
`[triage]` NOTE rather than a check — the decision to print it (the click did not drive) is the judge's.

**AND THE COPY THE DEVICE FETCHES IS A BUILD PRODUCT NOW.** `index/public/summrise-agent/e2e.js` is the driver
source with the plan embedded, written by `summrise-e2e-judge --bundle`, and
`agent/e2e-judge/tests/bundle.rs` fails when the committed bytes are not what the emitter produces today — with
the command that fixes it in the message. Until this landing nothing copied or checked that file and no gate read
it, so a fix landed here changed nothing on the device until somebody remembered to `cp` it.

## Known device quirks (handled by the suite)

- Right after a playwright-driven navigation, the desktop SPA target may
  answer `Runtime.evaluate` without `result.value` for a few seconds
  (Electron CDP quirk, observed ~3s → fine at ~8s). The browser section
  polls generously and tolerates empty responses.
- Closing a CDP WebSocket immediately after `send` can trip Node's
  `UV_HANDLE_CLOSING` assert on Windows — the suite waits for the close
  handshake.
- Pulling the suite from `raw.githubusercontent.com` can be flaky from d1
  (remote closed connections); retry, or pin the commit SHA in the URL.

## Prereqs

- Agent running on the device (port 18080 default).
- Sections `panel`, `mcp`, `evidence`, `browser`: Electron desktop up (CDP 9333) + bundled playwright.
- Test artifacts land in `D:\Summrise\pwout\` and are cleaned up by the suite.

## Maintenance notes

- Tool calls go through `POST /api/tools/{name}` with body = args object
  (no `{tool,args}` wrapper) — the shape external AI clients use.
- `terminal_open` returns the sid as a string in `.result`.
- Do NOT run the terminal section from inside the session you want to test
  (busy-lock deadlock — round-70 lesson).
- **EDITING THE DRIVER MEANS RE-EMITTING THE BUNDLE**, in the same commit:
  `cargo run --quiet --manifest-path agent/Cargo.toml -p summrise-e2e-judge -- --bundle -o index/public/summrise-agent/e2e.js`.
  A cargo test fails until you do, which is the point.
- A RENAMED OBSERVATION ID IS A CHECK THAT STOPS EXISTING rather than one that fails. Both ends are pinned by
  `tests/contract.rs`, and a section that recorded observations the judge could not read is reported as
  `<section>: the judge produced no checks` rather than going quiet.
