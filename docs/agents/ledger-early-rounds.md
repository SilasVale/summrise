# Ledger archive — the early round-numbered entries (round 54 to round 162)

These are the round narratives written while this loop still NUMBERED its rounds, from **round 162 down to round 54** —
kept in the order they were written, which is descending. They were MOVED here in round 273, not deleted, and
`ledger-budget-check.mjs` enforces both halves of that: a FLOOR, so a prune that deleted instead of moving fails, and a
CEILING, so this file cannot become the next record nobody can read.

**WHY THEY MOVED, AND IT IS NOT A JUDGEMENT ABOUT THEIR VALUE.** `docs/agents/design-ledger.md` was **457,929 bytes
against its own 400,000-byte ceiling** — and the check that applies that ceiling sat BELOW the only place its `failures`
array is read, so it could never fail. The gate printed `ok — … (457929 B of 400000)` and exited **0**. Two rounds (188
and 189) then recorded the conclusion that the ledger "has no byte ceiling", which was true of the BEHAVIOUR and false of
the CODE; the difference is three lines of control flow. The measurement, and the fix, are in `design-ledger.md` under
the round-273 note.

**HOW TO FIND A SECTION HERE.** The index at the top of `design-ledger.md` is the entry point, and it resolves a title
across ALL FOUR archives — `design-ledger.md` (the recent rounds and the exploration series), this file (the early
round-numbered ones), `ledger-appendix.md` (the lookup tables) and `ledger-mutations.md` (the mutation table).
`ledger-budget-check.mjs` checks that every title the index names still exists somewhere, so a rename here fails a gate
rather than silently breaking a pointer.

---

## round 162 — round 161's four candidates dissolve under the rules themselves, and the extractor was wrong three ways

Round 161 left four "rules nothing can apply" as a candidate and refused to delete them. That refusal was right, and **the count itself was
wrong — in three different ways, all visible by reading the RULES rather than the names**:

```
components.css:274   console's `.brand-img` so both rails present it the same way. */
                     ^^ A CLASS NAMED INSIDE A COMMENT. My extractor counted a mention as a definition.
components.css:1647  .notify-state.is-granted { color: var(--state-ok); ... }
components.css:1648  .notify-state.is-denied  { color: var(--danger-on-soft); ... }
                     ^^ COMPOUND SELECTORS. `is-granted` is not a class anything applies on its own;
                        the question is whether `notify-state` plus a state class is built — and
                        `notify-state` IS used in the panel, with the state half built from a stem
                        (`is-` alone is too short for the stem search that dissolved the other six).
desktop.css:411      .side-nav-btn,
                     ^^ ONE ENTRY IN A SELECTOR LIST (.side-nav-btn, .side-action, .details-close, …)
```

**AND THE HISTORY SAYS THE SAME THING ABOUT THE ONE THAT LOOKED CLEANEST**: `is-denied` came from `a34f391f` ("attention that follows you
out of the tab") and was touched again by `c3e775f0` — **"audit the state tokens used as graphics — one marginal border hardened, and a
state restored"**. A past round LOOKED AT THAT RULE AND RESTORED IT DELIBERATELY. **Deleting it would undo a recorded decision, which is
exactly what the repository's "read the ledger first" rule exists to prevent.**

**SO THE CANDIDATE IS WITHDRAWN AND THE LESSON IS THE SESSION'S OLDEST ONE, NOW FROM THE OTHER SIDE**: every phantom finding in this thread
(rounds 128, 137, 146, 147, 161) came from an extractor that could not see a form the file uses — a header’s prose, a mirror’s other
direction, a route table’s third comparison shape, a comment, a compound selector, a stem too short to search. **AND ROUND 161 ALREADY KNEW
THIS**: it dissolved eight of its own twelve in a second pass and then shipped the remaining four anyway. **The pause before deleting was
right; the number attached to it was not, and a number is what the next reader acts on.**

## round 161 — four stylesheet rules nothing can apply, found by a second pass that dissolved eight of twelve

`agent/resources/panel-react/src/styles/` is 5,950 lines and defines **618 classes**; the built sheet is what **five gates** read (round 153),
so a rule nothing can apply costs every one of them. The first pass — every `.class` in the sheets, grepped literally across the panel's
`.ts`/`.tsx` — returned **12 unreferenced**. **THE SECOND PASS DISSOLVED EIGHT OF THEM**, which is the whole reason this round is worth
recording:

```
BY STEM, NOT BY LITERAL:
  device-logs-verdict-ok / -quiet / -warn   DeviceLogsCard.tsx builds the name from a stem
  s-bg / s-muted / s-warn                   useSSE.ts builds the name from a stem
BY OWNERSHIP:
  xterm-cursor / xterm-cursor-blink         they are in node_modules/@xterm/xterm/css/xterm.css —
                                            a library's classes, applied at runtime, not ours to call dead
```

**AND THE REMAINING FOUR ARE CONFIRMED BY THE BUNDLE RATHER THAN BY THE SOURCE**:

```
brand-img        in built panel.js: 0   in built panel.css: 1
is-denied        in built panel.js: 0   in built panel.css: 1
is-granted       in built panel.js: 0   in built panel.css: 1
side-nav-btn     in built panel.js: 0   in built panel.css: 1
```

`panel.js` contains **all of the panel's JavaScript**, so a class that appears zero times there and once in the sheet is a rule no code
path can apply. **AND NOTHING GATES THIS**: `stylesheet-hygiene` checks that a COMMENT in a sheet does not look like code (its own header,
round 87) — unused rules are unmeasured, which is why four have survived.

**THEY ARE NOT DELETED HERE, AND THE REASON IS THE SAME ONE THAT MADE THIS ROUND CAREFUL**: a class can be applied by something a bundle
does not contain — a browser extension, an operator's bookmarklet, a runtime string this grep cannot see — and **round 161 has already had
one extractor answer dissolve under a better question.** Four rules in 5,950 lines are not a defect worth a guess; they are a CANDIDATE
with three measurements behind it and one decision left, and the decision is not mine to make at the edge of a budget.


**Round 160, one line**: the whole suite re-run after the seven self-audit rounds (153-159), which changed only records and
`AGENTS.md` — **and `AGENTS.md` grew by ~3 KB in that span**, which is the one file among them a gate measures directly:

```
bash scripts/test/all-gates.bash
  -> 58 ok, 0 failed, 1 not runnable here (of 59 gate command(s)), AGENTS.md 28.9 KB of its 48 KB ceiling
```

**UNCHANGED FROM ROUND 152, AND THAT IS THE POINT OF RUNNING IT**: the rounds in between were corrections to WHAT THIS REPOSITORY
SAYS rather than to what it does, so a green suite is the evidence that saying it differently did not change anything — and the
instruction-file ceiling is the one gate that could have objected, because `AGENTS.md` is where most of those corrections landed.


**Round 159, one line — "EVERYTHING IS PUSHED" IS TRUE, AND THE CLONE IS STILL NOT A MIRROR OF THE REMOTE**: round 155 found the local
TAGS eighteen behind (API-created, never fetched). The same question asked of the BRANCH answers differently, and both were measured
from outside this box:

```
local HEAD                 a80ff573c959a818809fdee2b8653de40c22e571
local main                 a80ff573…
origin/main (tracking ref) a80ff573…
git ls-remote origin       a80ff573…  refs/heads/main          <- the git protocol
GitHub API /commits/main   a80ff573…  "docs(ledger): all five panel.css gates run in CI…"   <- the API
```

**SO THE TWO CLAIMS ARE DIFFERENT CLAIMS, AND THIS SESSION MAKES BOTH**: *"my work is on the remote"* — verified, twice, against
authorities rather than the tracking ref — and *"my clone knows what the remote has"* — **FALSE for tags, and that is the direction that
hides finished work.** A tracking ref is a local cache of a remote fact; it agrees here because everything recent was pushed FROM here,
and it would have agreed just as happily in round 155 while eighteen tags sat unfetched.

**THE GENERAL FORM, WHICH IS THE SESSION'S OLDEST LESSON WEARING ITS LAST COSTUME**: `origin/main` and `git tag` are BOTH summaries of
the remote, and neither is the remote. **`git ls-remote` and the API are.**


**Round 158, one line — ALL FIVE `panel.css` GATES RUN IN CI, CHECKED RATHER THAN ASSUMED**: round 153 found that five gates read the
committed panel sheet (`chrome-stillness`, `feedback`, `motion`, `state-colour`, `stylesheet-hygiene`). **A gate that reads a file but never
runs is a gate whose verdict nobody has.** Each was checked for a `ci.yml` reference and a file in `scripts/test/`:

```
chrome-stillness        ci.yml:1   test-file:1
feedback                ci.yml:3   test-file:1
motion                  ci.yml:6   test-file:1
state-colour            ci.yml:1   test-file:1
stylesheet-hygiene      ci.yml:1   test-file:1
panel-sheet-freshness   ci.yml:1   test-file:1     (the sixth, which watches the sheet itself)
```

**NO GAP.** The counts above one are substring hits (`motion` inside other words), not extra gates — the check was for ABSENCE, and there
is none. **This is the shape a verification round should have: a question with two possible answers, asked with one command, and the
answer written down either way.** The alternative — assuming five gates that mention a file also run — is exactly the class rounds
146-151 spent six rounds on: a claim about a tool made without opening it.


**Round 157, one line**: the three "convenient command vs authority" pairs from rounds 153-155 are now in AGENTS.md, because
they pass its own test — **they change what a reader DOES on their next count.** `grep -l` counts files that mention a thing, `grep -c`
counts headers with rows, and `git tag` counts a clone that has not fetched the API-made tags. **All three were wrong in the direction
of undercounting**, and the block names the command to use beside each.

## round 156 — every number in the standard summary now has an authority and a round

Rounds 153, 154 and 155 each corrected or re-measured a number this session had been carrying, and this round finished the sweep by
checking the last one — **`device current`, which had not been read since round 80, seventy-six rounds earlier**:

```
summrise status   (on d1)
  status: RUNNING · install dir: D:\Summrise · panel: http://127.0.0.1:18080/panel/
  release: 1.2.474 · this CLI: 1.2.474 · update: none in flight
  latest: 1.2.474 (this device is current)
```

**AND THAT CLOSES AN AUDIT OF THE WHOLE SUMMARY LINE — five claims, five authorities, five rounds**:

| the claim | its authority | measured in |
|---|---|---|
| 10 releases (1.2.465 → 1.2.474) | `GET /repos/…/git/refs/tags` — NOT `git tag`, which stops at 1.2.456 here | round 155 |
| 59 gate commands, 58 green | `bash scripts/test/all-gates.bash` | round 152 |
| the mutation table has 44 rows | `awk '/^\| /{n++} END{print n-2}'` — NOT `grep -c`, which counts the header | round 154 |
| the device is current, 1.2.474 | `summrise status` on d1 | round 156 |
| the gateway is deployed at 5ee39ab2 | `./scripts/build.sh gateway` output | round 145 |

**THREE OF THE FIVE WERE WRONG OR UNVERIFIABLE WHEN FIRST CARRIED, AND ALL THREE IN THE SAME WAY**: a convenient command was treated as
the authority — `grep -l` for a filenames count, `grep -c` for a row count, `git tag` for a release count. **Each convenience was one
step from the tool that actually produces the number, and each error was in the direction of undercounting**: two filenames where five
gates read the file, 46 lines where 44 are rows, and zero releases where ten were shipped. **A summary line is a claim set, and this
session now knows what each claim rests on** — which is the only state in which the line is worth writing at the top of every round.

## round 155 — the release count was right, and it could not have been checked locally

The standard summary of this session carries "10 releases (1.2.465 → 1.2.474)". Rounds 153 and 154 had each corrected a carried count, so
this one was measured too — **against `git tag` first, which is the obvious tool and the wrong one**:

```
git tag --list "v1.2.*" --sort=-v:refname | head -3     ->  v1.2.456, v1.2.455, v1.2.454
git tag --list "v1.2.*" | awk … >=465 | wc -l           ->  0            <- NOTHING
```

**THE LOCAL CLONE HAS NO TAG ABOVE 1.2.456, AND THE REMOTE HAS 94 UP TO 1.2.474.** The cause is written in this repository's own
release procedure and is not a defect: tags are created through the **GitHub API** (a `git push` of tags times out here), and the git
mirror **refuses `git fetch --tags`** with HTTP 400. So the tags exist on the remote, are correct, and have simply never been fetched
into this clone.

**MEASURED AGAINST THE AUTHORITY INSTEAD**:

```
GET /repos/SilasVale/summrise/git/refs/tags?per_page=100
  -> 94 tags on the remote, highest v1.2.474
  -> releases from 1.2.465 up: 10 — 1.2.465 … 1.2.474
```

**SO THE NUMBER WAS RIGHT AND THE METHOD WAS THE HAZARD.** Anyone auditing "how many releases has this loop shipped" reaches for
`git tag`, and on this clone that answers **zero for every release this session made** — an undercount of eighteen, in the direction
that makes finished work look like it never happened. **The authority is the API, and the reason is that the tags were made there.**


**Round 154, one line — THE MUTATION TABLE HAS 44 ROWS, AND MY SUMMARIES SAID 46 FOR TEN ROUNDS**: the number came from
`grep -c '^| '`, which counts the HEADER and the SEPARATOR along with the rows:

```
grep -c '^| '                                        -> 46   <- what was carried
awk '/^\| /{n++} END{print n-2}' docs/agents/ledger-mutations.md  -> 44   <- the mutation rows
```

**AND IT IS THE SAME MISTAKE AS ROUND 153, ONE FILE OVER**: there a `grep -l` counted FILENAMES where the tool's own message said
FIVE; here a `grep -c` counted LINES where the table has 44 ROWS. **Both were caught by reading the thing instead of a summary of it,
and neither number was wrong in a way that mattered — which is exactly why they are worth recording: a carried count that is off by two
teaches the reader to stop checking, and this ledger has spent the session insisting that a count is a summary and the tool is the
measurement.** The command now sits at the top of the table it counts, in an HTML comment, so the next reader does not have to
re-derive it.


**Round 153, one line — THE PANEL BUILD IS TWO ARTIFACTS, AND THREE GUARDS ALREADY KNEW**: round 139's inventory row named
`agent/resources/panel/panel.js` as the guarded build output. `panel.css` sits beside it — 261,630 B, rebuilt in the SAME SECOND as the
bundle, generated from `panel-react/src/styles/*.css` — and **FIVE checks read it** — `chrome-stillness`, `feedback`, `motion`, `state-colour`,
`stylesheet-hygiene` — **which `panel-sheet-freshness-check.mjs:75` states in its own message.** The first version of this line said
"three" because a `grep -l` found three FILENAMES; the count was in the file all along, one line of its output away. **So nothing was missing and no guard is owed**; what was wrong was the row's phrasing, and
it now names both files.

**AND THAT IS THE RIGHT WAY FOR A RECHECK TO END**: the question "did round 139 miss a fifth artifact?" was asked because 202 commits on
a file NOT in the inventory is exactly the shape of an omission — and the answer, measured, is that the guard covers a build of two files
and the row named one. **A row that is right but incomplete is a smaller error than a missing row, and both are worth the one command
that tells them apart.**


**Round 152, one line — THE GATE COUNT IS NOW 59, MEASURED RATHER THAN INFERRED**: round 151 added a gate to `ci.yml`, so the
number the suite prints changed, and this round ran the whole thing to read it rather than adding one to the old one:

```
bash scripts/test/all-gates.bash
  -> 58 ok, 0 failed, 1 not runnable here (of 59 gate command(s) in .github/workflows/ci.yml)
```

**58 ok / 0 failed, one more than the 57/58 before it, and the new check is among the passing.** The count comes from
`ci-command-table-check`'s own extraction of the workflow, which is why the inventory's scripts row says in so many words that "the
gate count is `gate-mutations-check`'s own line, never a number here" — **a carried count is a number that drifts, and this one was
read from the tool that produces it.**

**AND THE THREE-PLACE WIRING IS CONFIRMED END TO END BY THIS RUN**: the file alone would have failed `ci-command-table-check`
(round 149 measured that enforcement), and the `ci.yml` line alone would have been a command with no check behind it. **Both, plus the
declaration, is what makes the count go up by exactly one and nothing else change.**

## round 151 — the route-header gate is BUILT, wired in three places, and proven in both directions

Rounds 146-150 specified this gate and measured what adding one costs. This round paid it:

```
1. scripts/test/http-route-header-check.mjs      the check — 11 header routes vs 33 Exact + 5 Prefix
2. .github/workflows/ci.yml                      the invocation, beside production-host-check
3. ci-command-table-check                        declared WITH ITS REASON, which is where a gate with no AGENTS.md row belongs
```

**AND BOTH DIRECTIONS ARE PROVEN, WHICH IS WHAT ROUND 124 REVERTED A GATE FOR LACKING**:

```
mutated (a header route no Pattern row resolves): exit 1
  "names 1 route(s) no Pattern row resolves — a reader is sent looking for something the device does not serve: /api/does-not-exist"
restored (clean tree):                            exit 0   (11 routes resolve)
```

**AND THE EXTRACTION IS NOT A SINGLE REGEX, BECAUSE THE HEADER WRITES PROSE**: `GET /panel, /panel/` and
`POST /api/plugins/playwright/start|stop` are one line each, and rounds 146 AND 147 each recorded a phantom finding from matching them
literally. The check splits on `,`/`|`/whitespace and matches a templated route (`{name}`) by prefix — **the two mistakes that produced
those phantoms are the two things it handles explicitly**, which is the only way a check written after them can claim to have learned.

**AND IT REFUSES TO PASS VACUOUSLY**: no routes parsed from the header, or no `Pattern` rows found, is a FAILURE rather than an empty
success — the shape round 95 found in a check whose predicate a stale value already satisfied, and the shape this ledger has spent the
session insisting on.


**Round 149, one line — THE GATE FOR ROUND 146'S CHECK IS THREE EDITS, AND THAT IS NOW MEASURED RATHER THAN ASSUMED**: the
one-directional assertion is "every route the HTTP header NAMES must exist in the `Pattern` table" (the reverse is deliberately not
asserted — round 147 made the header say `A SELECTION, NOT THE INVENTORY`). Adding it is NOT one file:

```
1. scripts/test/http-route-header-check.mjs      the check itself
2. .github/workflows/ci.yml                      a line invoking it — because
   scripts/test/all-gates.bash:50 extracts the gate list FROM ci.yml
   (grep -ohE '(node|bash|python3) +scripts/test/…'), it does not discover files
3. ci-command-table-check                        enforces that EVERY scripts/test file IS named in
   ci.yml (its own comment at :53), so a file dropped in without the line fails that gate
```

**AND THE SIZES ARE WORTH THE LINE**: `ci.yml` names **52** gate commands and `scripts/test/` holds **48** `.mjs` files (the rest are
`.bash` and `.py`). Those two numbers are why "just add a gate" is a three-place change in this repository — and why a round that
dropped the file in without the other two would leave `ci-command-table-check` red, which is the mechanism working, not a nuisance.

**IT IS SPECIFIED AND NOT BUILT**, deliberately: a gate needs BOTH directions proven (round 124 reverted one that failed on a clean
tree), and this round has the mechanism and the sizes rather than the budget. **Two rounds, one candidate, and the second one starts
from a measurement instead of a hypothesis** — which is the difference rounds 146-148 were about.


**Round 148, one line — ROUND 147 IS VERIFIED AGAINST ALL SEVEN OF CI'S CARGO CHECKS, BY EXIT CODE**: a doc-comment change in
`agent/src/web/mod.rs` still owes the whole set, because that file is compiled by every one of them. Round 147 ran three of the seven
(`fmt --check`, `clippy --features terminal,keyring`, `xwin check`); this round ran the rest:

```
clippy -p summrise-agent-core --all-targets -- -D warnings   exit=0
test   -p summrise-agent                                     exit=0   690 passed; 0 failed
test   -p summrise-agent --features terminal,keyring          exit=0   753 passed; 0 failed
test   -p summrise-agent-core                                exit=0    29 passed; 0 failed
```

**THE SUITE SIZES ARE THIS ROUND'S MEASUREMENT AND NOT A CELL'S**: the inventory's tests row says in so many words "EVERY SUITE PRINTS
ITS OWN COUNT — run it. No counts are carried here on purpose", because a carried count drifts. **690 / 753 / 29 is what those three
commands printed today**, and the command is the record.

**AND THE ORDER OF THE TWO ROUNDS IS THE PATTERN, NOT AN ACCIDENT**: 147 ran the checks it could name from the change it had made, and
this round ran the ones it had not. **Seven of seven, each by exit code, none inferred from output** — the shape AGENTS.md has carried
since round 129 and which rounds 117 and 128 each paid for.

## round 147 — the header listed ten of thirty-eight routes, and the fix is to point at the table

Round 146 specified this check and named its mechanism; this round ran it correctly. **The `Pattern` table holds 33 `Exact` rows and 5
`Prefix` rows. The header's `Routes:` list names TEN, and its framing sentence carries no qualifier — it reads as THE list.**
**TWENTY-THREE TABLE ROUTES ARE ABSENT FROM IT**, and they are not trivia: `/api/settings`, `/api/monitors` and its three verbs,
`/api/logs`, `/api/boots`, `/api/sessions`, `/api/vitals/history`, `/api/update`, `/api/run/mark-exit` — **most of what the panel
calls.**

**Two of my three candidate "missing" findings were AGAIN extraction artifacts** (the header writes `GET /panel, /panel/` and
`POST …/start|stop`, and my regex took the comma and the pipe literally), so the documented→table direction is clean. **The
table→documented direction is not**, and it is the same class as round 128: a header read as the truth sends a reader looking
elsewhere for a route that exists.

**THE FIX IS NOT TO COMPLETE THE LIST** — a hand-maintained 38-line comment drifts again, and this file has 8,302 lines that move. It
says what the list IS (`A SELECTION, NOT THE INVENTORY`), names the omission with examples, and points at the authority: the
`Pattern` rows `route_of` resolves and a checker can enumerate. **A comment that says what it is cannot be wrong about what it is**,
which is the cheapest fix available to a document that cannot afford to be exhaustive.

## round 146 — I hypothesised an if-cascade with no route table, and the file already had one

`agent/src/web/mod.rs` is **8,302 lines** and the largest file in the agent, and its dispatch begins with `if path == "/api/browser/actions"`,
`if path == "/api/operation"`, … so the first read looks like a cascade. The header also LISTS ten routes, which makes a
documented-vs-actual diff look like the obvious check. It was run, and it reported six routes "documented but not dispatched".

**ALL SIX ARE FALSE POSITIVES OF MY OWN EXTRACTION, AND THE TRUTH IS BETTER THAN THE HYPOTHESIS**:

```
/api/status          ->  pattern: Pattern::Exact("/api/status")            line 933
/api/spec            ->  pattern: Pattern::Exact("/api/spec")              line 927
/api/plugins/status  ->  pattern: Pattern::Exact("/api/plugins/status")    line 1060
/api/tools/          ->  Prefix "/api/tools/"  … and a comment at 802:
                          "Move a row and `route_of` answers …"           <- ARM ORDER IS DOCUMENTED
/panel               ->  path == "/panel" || path == "/panel/"             line 679
```

**There is a route TABLE — `Pattern::Exact` / `Pattern::Prefix`, with a `route_of` resolver and a comment warning that arm order
matters.** My regex knew two comparison shapes (`path ==`, `path.starts_with`) and the file uses three, so it reported the routes it
could not see as missing. **THAT IS THE THIRD PHANTOM FINDING THIS SESSION FROM A NAIVE EXTRACTOR** — round 128 read a header as a dead
endpoint, round 137 checked one direction of a mirror, this one diffed two lists with the wrong vocabulary — and each was caught by
opening the thing rather than by reasoning about it. **The rule the three of them share: an extractor is a hypothesis about a file's
shape, and it fails by reporting what it cannot parse as absent.**

**WHAT SURVIVES IS A BETTER CANDIDATE THAN THE ONE I STARTED WITH**: the header documents ten routes and the code has a `Pattern` table,
so the two CAN be compared — the same documented-vs-actual assertion `ci-command-table-check` makes for CI, applied to the device's own
surface. **It is specified rather than built**, and the mechanism is now known (`Pattern::Exact`/`Prefix` rows plus the header's `//!`
list), which is the difference between this round and the one that would have started from my wrong hypothesis.


**Round 145, one line — THE OWED DEPLOY IS PAID, WITH ITS VERIFICATION IN THE SAME COMMAND**: round 144 recorded that round 128's
comment fix had left the live Source Viewer serving the sentence that reads as a dead endpoint, and refused to start a production
deploy without budget to check it. This round ran it with the check chained:

```
token loaded (53 chars)          from ~/.cloudflare-token
./scripts/build.sh gateway       exit=0 · Current Version ID: 5ee39ab2 · checked 45 files, 0 drifted
curl …/code/files/summrise-gate/src/plugins/translate.ts   http=200
  → "THE PARENTHESIS THAT USED TO BE HERE SAID"     THE CORRECTED SENTENCE IS LIVE
```

**AND THE CHAINING IS THE POINT, NOT THE DEPLOY**: round 144's refusal was "a production deploy begun with no budget to verify it is
worse than a recorded debt" — and the answer to that is not more budget, it is keeping the verification INSIDE the same command, so the
deploy cannot land unverified. The `curl` looks for the NEW text by name, which is what makes its absence a failure rather than a
page that merely answered 200. **A deploy and its check in one command are one action; split across two rounds they are a gamble.**


**Round 144, one line — A GATEWAY DEPLOY IS OWED, RECORDED RATHER THAN STARTED**: round 128 corrected a comment in
`gateway/src/plugins/translate.ts` and re-synced its mirror in `gateway/public/code/files/summrise-gate/`, and **the gateway is a
DEPLOYED worker** — so the live Source Viewer still serves the OLD sentence, the one that reads as though `/v1/chat/completions` were a
dead endpoint. The change is a comment, so no behaviour differs; what differs is **what an operator reading the viewer is told**, which
is the whole reason that mirror exists.

The scope is exactly two files and the command is the one AGENTS.md carries (`./scripts/build.sh gateway`, credential in
`~/.cloudflare-token` — checked, it exists; `CLOUDFLARE_API_TOKEN` is not in the environment). **IT IS NOT STARTED HERE, AND THAT IS
THE POINT OF THE LINE**: a production deploy begun with no budget left to verify it is worse than a recorded debt, because the record
is what makes the next round run `curl` against the deployed viewer and read what it says. **This is the same pattern that got rounds
54+56 and 65 and 73 shipped**: write down what is owed, with its command, where a reader will meet it.


**Round 143, one line — THE POSITIONAL PROMISES SURVIVED THE REORDER, AND THE CHECK IS NOW A COMMAND**: round 142 moved 42 lines
inside the Committing section, and moving text is exactly what breaks a sentence that points at its neighbours. So every positional
reference in both instruction files was listed and read:

```
grep -nE "below|above|the paragraph after|the next section|paragraph below|section below|table above|row above" AGENTS.md agent/AGENTS.md
```

**TEN REFERENCES, AND NINE ARE UNAMBIGUOUS** ("the rule above is do not push while CI is running", "the Release and layout sections
below", "use the npm flow above", "see below for why not alpha"). **The tenth was the one at risk** — `AGENTS.md:167`, inside the
hook paragraph, promising that "**the paragraph after this one records what is installed and how it was proven**" — and it HOLDS:
the paragraph after it is "TWO THINGS ABOUT INSTALLING IT", which carries both the `.githooks/` command and the empty-commit proof.

**SO THE METHOD IS THE DELIVERABLE, AND IT IS ONE LINE**: after moving text in a document whose sentences point at each other, list the
pointers and read the ones the move crossed. Round 142 learned that an insertion is a claim about its narrative; this round turns that
into a check that costs a grep — **which is the same progression as rounds 129->130 and 141->142, where a lesson became a habit one
round after it was learned.**


**Round 142, one line**: the two shell rules added in rounds 129 and 141 had both landed INSIDE the pre-commit hook's
story — between "A pre-commit hook runs the emitters" and "IT IS INSTALLED NOW" — so a reader met two shell hazards before
learning whether the hook runs, and the installation paragraph still said "the paragraph after this one records what is
installed". The 42 lines moved to the end of the section: **rationale → installed → proven → THEN the rules for running
commands.** No wording changed, only position — and the lesson is that an insertion into a narrative is a claim about the
narrative, made by someone who was looking at the paragraph and not at the section.


**Round 140, one line**: round 139's inventory moved to `docs/agents/inventory.md` §12 — the checkpoint table whose stated
purpose is that every clause names the INSTRUMENT and its VERDICT — as seven rows, two of which record guards that are NOT tests
(`setup`'s pin refusal on the device, the release audit's byte comparison). **A finding belongs in the file that exists to hold it**,
and a reader adding a generated file can now tell in one look whether it has a home.

**AND THIS LINE WAS MANGLED WHEN IT WAS FIRST WRITTEN, WHICH IS WHY IT IS WORTH A SENTENCE**: the command that wrote it was
`python3 -c "…"` — DOUBLE-quoted — and the backticks in the text were therefore executed as COMMAND SUBSTITUTION, so the line
committed as "moved to  §12" and "('s pin refusal on the device". The stderr said so (`docs/agents/inventory.md: Permission
denied`, `setup: command not found`) and the commit went out anyway. **Two failures in one command, both already in AGENTS.md**: a
command whose output was not read (the round-117/128 class) and a shell quoting hazard — and the second is the specific form
**BACKTICKS INSIDE DOUBLE QUOTES ARE NOT LITERAL** that this repository's own text is full of, so it will happen again to whoever
writes prose about code from a shell. The fix shape is the one already recorded: **a QUOTED heredoc (`<<'EOF'`) puts the text through
verbatim**, which is what all four of this round's file edits used except this one.

## round 139 — the class closes: four generated artifacts, four guards, and no fifth gap

Rounds 133-138 closed one shape — **generated, committed, unguarded** — by finding it three times. This round asked the question that
ends a class rather than extending it: **what else in this repository is generated and committed?**

```
agent/resources/panel/panel.js               663,487 B   guarded by panel-sheet-freshness-check.mjs
agent/summrise-agent-npm/bin/summrise.js     198,635 B   guarded by the pack chain (cmp against a fresh compile)
index/components.json                          2,185 B   guarded by SETUP, at install time: a pin mismatch is REFUSED
index/public/summrise-agent/version.json         669 B   guarded by the RELEASE AUDIT: CDN vs the GitHub asset, byte for byte
```

**NO FIFTH GAP.** The two JSONs are worth naming because their guards are NOT tests: one is enforced on the device by `setup`
refusing a sha256 mismatch, the other by the release audit comparing the two channels. **A guard does not have to be a gate to be a
guard, and this round is where that distinction stopped being a loophole and became an inventory.**

**AND ONE WORRY WAS CHECKED RATHER THAN ASSUMED**: `git ls-files | grep -E "\.(exe|tgz|zip)$"` returns **nothing**, so the built exe
the release flow copies into the package is a LOCAL artifact, not a committed mirror — which is why no guard for it is missing.
**The inventory is the deliverable here**: after six rounds of finding this shape one instance at a time, the answer to "what else"
is now a list with four entries and four mechanisms, and a reader can tell in one look whether a new generated file has a home.

## round 138 — the third unguarded copy, and this one I created myself three rounds into the e2e thread

Round 137 closed the instruments mirror in both directions. The same question, asked once more, found a third copy of the same
shape: **`index/public/summrise-agent/e2e.js` is the e2e suite the DEVICE fetches**, published to the CDN in round 85 so the cadence
could hand a section to a real panel. It is a copy of `agent/scripts/e2e/e2e.js` — **and rounds 103, 113 and 114 each re-copied it
BY HAND, with nothing asserting the two matched.**

```
currently: in sync
1. clean:                  npm test exit=0   pass 922 / fail 0
2. drifted published copy: npm test exit=1   "is not the suite in agent/scripts"
3. restored:               npm test exit=0   pair byte-identical again
```

**AND THE PRECEDENT WAS ALREADY IN THE REPOSITORY**: the panel's build output (`resources/panel/panel.js`) has exactly this shape —
generated, committed, embedded — and it IS gated, by `panel-sheet-freshness-check.mjs`. **The e2e pair was the same kind of artifact with
none of the protection, and it was mine.**

**THE THREAD'S SHAPE, IN FOUR ROUNDS, IS NOW COMPLETE AND WORTH STATING PLAINLY**: 133 found a mirror nobody guarded; 134 guarded one
direction of it; 137 found the guard itself had the asymmetry; 138 found a THIRD copy that the first three rounds had not looked at
because it does not live under `public/code/files/`. **Each round asked "what else has this shape" and each answer was one step further
from where the last one stopped** — which is what a class looks like when it is being closed honestly rather than sampled.

## round 137 — the guard I wrote last round had the hole it was written to close, one direction over

Round 134 wrote `instruments-mirror.test.mjs` and proved it both ways. **It checked only mirrored → source.** The gateway mirror's
test has checked THREE directions since it was written — `missing`, `extra`, `differing` — and this round asked why mine had two.

**BECAUSE `sync-code-viewer.sh` COPIES BY GLOB** (`cp agent/scripts/*.mjs` plus `lib/*.mjs`, line 49-57). So the case that matters is
not "a mirrored file drifted" — the script re-copies every match — but **"a NEW instrument was added and nobody re-synced"**. On the
old test that case passed: every file that WAS in the mirror matched its source, so the assertion was satisfied **while the Source
Viewer was quietly missing an instrument a reader is told to run.** The manifest cannot notice either: it walks the DESTINATION.

```
1. clean tree:                 npm test exit=0   pass 922 / fail 0
2. unmirrored new instrument:  npm test exit=1   "holds instruments that the Source Viewer does not publish"
3. restored:                   npm test exit=0
```

**AND THE SHAPE OF THIS MISTAKE IS THE ONE THIS THREAD KEEPS FINDING, NOW COMMITTED BY ME IN THE ROUND THAT WAS FIXING IT**: a guard
that looks only at what exists cannot see what is GONE. Round 133 was about a mirror nobody guarded; round 134 guarded one direction
of it; round 137 found that the guard itself had the asymmetry. **The gateway test had the answer in it the whole time** — three
directions, written before the mistake — which is the argument for reading the neighbours of anything you are about to write.


**Round 136, one line**: the whole suite re-run after the rounds that found the SECOND mirror unguarded (133), wrote its guard
and proved it both ways (134), and checked the new file against all four of gateway's CI checks before being asked (135) —
`bash scripts/test/all-gates.bash` → **57 ok, 0 failed, 1 not runnable here (of 58)**, unchanged. The count of gate COMMANDS is the
same because the new guard lives inside `gateway npm test`, which CI already runs: **a test added to a suite is invisible in this
number by design**, and the number that moved is the suite's own — gateway went from 921 to **922** tests.


**Round 135, one line — THE CHECK WAS MADE BEFORE IT WAS DEMANDED, WHICH IS THE FIRST TIME IN THIS THREAD**: round 134 added
a NEW file, `gateway/test/instruments-mirror.test.mjs`, and verified it with `npm test` alone — while CI runs FOUR checks in that
directory and two of them (`lint`, `format:check`) are the ones that object to a new `.mjs`. Rounds 128 and 130 both learned this
AFTER a red result; this round ran all four first:

```
typecheck      exit=0  ok
test           exit=0  ok
lint           exit=0  ok
format:check   exit=0  ok
```

**ALL FOUR PASS, SO ROUND 134 IS FULLY VERIFIED AND NOTHING NEEDS AMENDING.** The value here is not the result — it is that the
question "which checks are aimed at what I changed" was asked at the moment of the change rather than one round later, which is the
difference between a rule this ledger has written down and a rule it has absorbed.

## round 134 — the unguarded mirror gets its guard, and both directions are proven by exit code

Round 133 found nine mirrored instruments under `gateway/public/code/files/instruments/` with **no test referencing them at all**,
while the gateway mirror beside them had one. This round wrote that test — `gateway/test/instruments-mirror.test.mjs` — and proved it
the way rounds 124-125 insisted a gate must be proved:

```
1. clean tree:          npm test exit=0   pass 922 · fail 0     (921 -> 922: one new test)
2. mutated instrument:  npm test exit=1   "…serves instrument code that nobody runs…"
3. restored:            npm test exit=0
```

**AND THE SHAPE WAS DE-RISKED BEFORE IT WAS WRITTEN, WHICH IS WHAT ROUND 124 COST.** The gateway mirror needs the sync script's
redaction rules — three of them, keyed to `src/...` paths — so a naive copy of that test would have had to re-implement them and might
have failed on a clean tree (the mistake that made round 124 revert its own gate). The measurement first: every mirrored instrument is
**byte-identical to its source**, including the two that carry a production host (`console-design-sweep.mjs` 1, `landing-design-sweep.mjs`
3). **No redaction applies to instruments, so the guard is a plain byte comparison** and the test needs none of that machinery.

**AND THE EXIT CODES WERE READ THIS TIME, NOT THE OUTPUT** — three runs, three codes, which is the shape round 129 wrote into
AGENTS.md after rounds 117 and 128 each lost a red suite to a discarded status. **A lesson from three rounds ago is now just how the
command is written**, which is the only kind of learning this ledger counts.

## round 133 — an unguarded mirror, found by asking the question round 128 taught

Round 128 learned that `gateway/src` has a TRACKED MIRROR in the Source Viewer, kept in sync by `sync-code-viewer.sh` and guarded by
`code-viewer-mirror.test.mjs` — the gate that caught an un-synced comment edit. This round asked the obvious follow-up: **what else is
mirrored, and is any of it unguarded?**

**TWO MIRRORS EXIST**: `gateway/public/code/files/summrise-gate/` (the gateway source, 54 files) and
`gateway/public/code/files/instruments/` (nine agent scripts). Both were checked against their sources:

```
panel-design-sweep.mjs in sync   console-design-sweep.mjs in sync   lib/contrast-probe.mjs in sync
lib/sweep-bundle.mjs   in sync   lib/design-sweep.mjs     in sync   panel-render-audit.mjs  in sync
harness-boot-check.mjs in sync   live-panel-probe.mjs    in sync   landing-design-sweep.mjs in sync
```

**AND `index/public/summrise-agent/e2e.js` — the CDN copy created in round 85 — matches `agent/scripts/e2e/e2e.js` byte for byte**, so the
pair this session created is in sync too. Nothing is stale.

**BUT THE SECOND MIRROR IS GUARDED BY NOTHING.** `grep -rln "files/instruments" gateway/test/` returns no file: the `summrise-gate`
mirror has a test, the `instruments` mirror has only the script that writes it. **A drift there would be silent — and it holds the
instruments AGENTS.md tells a reader to run against the device**, `live-panel-probe.mjs` among them, whose entire hand-over story is
that the DEVICE fetches it from a URL. **An unguarded mirror of a published instrument is a viewer that can serve a script nobody
ran, under a name everybody trusts.**

**THE REPAIR IS THE ONE THAT ALREADY EXISTS, EXTENDED BY ONE FILE**: `code-viewer-mirror.test.mjs` asserts the gateway mirror matches;
the same assertion over `files/instruments/` — comparing each mirrored file with `agent/scripts/<same path>` — is the whole change, and
its mutation is a one-byte edit to a mirrored instrument. **It is specified rather than shipped because a gate needs both directions
proven (rounds 124-125), and this round is out of budget before the proof could be run.**


**Round 131, one line**: the whole suite re-run after the rounds that touched gateway SOURCE and its tracked mirror (128),
added the pipe rule to AGENTS.md (129) and ran gateway's full four-check CI set (130) — `bash scripts/test/all-gates.bash` →
**57 ok, 0 failed, 1 not runnable here (of 58)**, unchanged. Round 130 was the targeted check and this is the whole one; **the two are
not substitutes**: a directory's four checks answer "did I run what was aimed at my change", and the suite answers "does anything else
disagree with it" — which is the question a mirror re-sync can raise, because the mirror is a second copy of the same source.


**Round 130, one line — THE TWO CHECKS I SKIPPED WERE THE TWO AIMED AT MY CHANGE**: round 128 edited
`gateway/src/plugins/translate.ts` and verified it with `npx tsc --noEmit` and `npm test`. CI runs FOUR checks in that directory
(`npm run typecheck · test · lint · format:check`), and this round ran all four by exit code:

```
typecheck      exit=0  ok
test           exit=0  ok
lint           exit=0  ok
format:check   exit=0  ok
```

**ALL FOUR PASS, SO ROUND 128 IS FULLY VERIFIED — AND THE TWO THAT WERE SKIPPED ARE NOT ARBITRARY.** A COMMENT change is precisely
what `format:check` (prettier) and `lint` exist to notice: the first reformats prose in code, the second can object to it. Running
half of a directory's check set and reporting the directory as verified is the same failure as rounds 122 and 125, one turn
earlier in the chain: **the question is not "did I run a check" but "did I run the checks that were aimed at what I changed".**


**Round 129, one line**: round 128's lesson is now in AGENTS.md's Committing section, where a committer acts, and in the
narrower form the two payments earned: **redirect, check, THEN filter.** Both failures preserved the exit code correctly and then
destroyed it one step later (117 with `>/dev/null 2>&1`, 128 with a pipe), so the rule as a slogan was never the missing piece —
**the missing piece was a shape to follow instead**, and it is written down with both incidents named.

## round 128 (cont.) — I PUSHED A RED GATEWAY SUITE, AND THE MASK WAS A PIPE AGAIN

The comment correction above changed `gateway/src/plugins/translate.ts`, and `gateway npm test` went to **exit 1** — which I did not see,
because the command I ran was:

```
cd gateway && npx tsc --noEmit | tail -2; npm test 2>&1 | grep -E "^. (pass|fail)" | head -2
```

**A PIPELINE EXITS WITH ITS LAST COMMAND'S STATUS**, so `set -e` saw `grep`/`head` succeeding and the failing suite passed by. That is
round 117's failure (a check run, its answer discarded) in its second costume: there, `>/dev/null 2>&1` threw the exit code away; here,
a pipe replaced it. **Both times the suite was red and the commit went out.**

**AND THE GATE THAT CAUGHT IT IS THE ONE BUILT FOR THIS EXACT KIND OF EDIT.** `code-viewer-mirror.test.mjs` keeps a TRACKED mirror of
`gateway/src/` in `gateway/public/code/files/summrise-gate/src`, re-synced by `gateway/scripts/sync-code-viewer.sh`, and its failure
message names the stakes better than this ledger could: the viewer "serves code the worker does not run — **including descriptions that
may state facts the device has since disproved**". A round that corrects a DESCRIPTION is precisely the round that must re-sync, and the
gate said so while I was not listening.

**FIXED AND VERIFIED BY EXIT CODE, NOT BY OUTPUT**: `sync-code-viewer.sh` exit 0 (54 files, 3 production hosts redacted), then
`gateway npm test` **exit 0 — pass 921, fail 0**, then the four doc gates and the hook, each gated with `&&`.

**THE HABIT TO KEEP IS NARROWER THAN THE RULE**: "read the exit code, not the output" has been in AGENTS.md all along, and the two
failures here were not failures to read it — they were failures to PRESERVE it. So the rule earns a clause: **do not put a command
whose status you need on the left of a pipe.**

## round 127 — a candidate LOOKED like duplication and the measurement says it is not, which is worth a round

`index/src/index.js` is the second-highest untouched source file in the corrected hotspot list (200 commits), the CDN worker that
serves every release, and it holds three component routes in a row — `:211` `cloudflared.exe` (68 lines), `:279`
`electron-win32-x64.zip` (39), `:318` `summrise-playwright.zip` (40). Three blocks of the same SYNTAX shape in one file is the
textbook deepening candidate, and this repository has made exactly that move before (round 272: all five sweep payloads into
modules assembled by one bundler).

**SO THE SHAPES WERE COMPARED, BY NORMALISING AWAY STRING LITERALS AND IDENTIFIERS — AND THE NORMALISATION DELETED THE ANSWER.**
What the three blocks actually carry is three DIFFERENT failure modes, each recorded in the comment that the comparison stripped:

- `:211` — "PINNED, NOT `latest`": this id is a versioned GitHub artifact fetched on demand through the installer, and the reason
  is in eight lines above the code;
- `:279` — "A ZERO-BYTE OBJECT IS NOT A BUNDLE. Measured …: an absent id gave 502 …, an EMPTY one gave 200 with …": a guard that
  exists because an empty object once shipped as a valid download;
- `:318` — "IT IS AN EXECUTED ARTIFACT AT A MUTABLE KEY (round-99 F2)": a cache header that was deliberately set to `max-age=86400`
  with NO etag, with the paragraph explaining what that costs.

**THREE BLOCKS THAT LOOK ALIKE AND RECORD THREE UNRELATED INVARIANTS ARE NOT DUPLICATION.** Collapsing them would put a pinned-
artifact rule, a zero-byte guard and a cache-policy decision behind one code path — and this repository's whole method is that
each of those rules was paid for by a measured defect. **A refactor that merges them saves forty lines and makes three rules
unfindable**, which is the trade the delete-the-comment test refuses.

**AND THE METHOD LESSON IS THE SESSION'S OLDEST ONE, IN A NEW COSTUME**: to compare the three blocks I removed their comments,
and the comments were the content. **Normalising away the prose is how a codebase looks like it has duplication it does not
have** — and the same move in the other direction (reading the shapes WITHOUT the prose) is how a rule gets deleted by someone
who never saw it.


**Round 126, one line**: the whole suite re-run after `ci-command-table-check` gained its A2 direction (the scoped
instruction file) — `bash scripts/test/all-gates.bash` → **57 ok, 0 failed, 1 not runnable here (of 58)**, unchanged. The gate
that grew is ONE OF THE FIFTY-EIGHT, so a four-gate spot check could not have said whether its new assertion contradicts some
other instrument; the suite is what answers that, and the answer is no. **A gate added to a suite is a change to the suite**, which
is why the round after adding one is spent running the thing it joined.

## round 125 — the scoped-file gate ships, with the half of the proof round 124 was missing

Round 123 found that `agent/AGENTS.md` carried commands that are not CI's, and noticed why nothing caught it: **a scoped
instruction file inherits none of its parent's gates.** Round 124 built the gate, watched it bite, and REVERTED it because it also
refused a clean tree — its extractor kept each line's trailing `#` comment, so `cargo fmt --all -- --check   # note` never matched
the string CI contains. This round ships the repaired version and proves BOTH directions:

```
1. clean tree (must pass): ok
2. mutated  (`cargo test --workspace --all-features`): exit non-zero, naming exactly that command
3. restored (must pass):   ok
```

`ci-command-table-check.mjs` direction A2 now reads the scoped file, strips comments and trailing whitespace before matching, and
checks each of its check-shaped commands against the workflow — the same assertion the root table has had since round 151, applied
to the file that had none. **The mutation row records both halves**, because the interesting failure here was not the bite: it was
a gate that would have blocked every commit until somebody weakened it, which is how gates get weakened.

**AND THE CLASS IS NOW CLOSED RATHER THAN SAMPLED**: round 122 repaired the file, round 123 proved the set is exactly two files,
rounds 124-125 gave the second one the gate the first one already had. Four rounds for a file nobody had read, which is what
"the hottest file in the repository" should have cost somebody a hundred rounds ago.


**Round 124, one line — I BUILT THE GATE ROUND 123 ASKED FOR AND REVERTED IT, BECAUSE IT FAILED ON A CLEAN TREE**: the change was
ten lines in `ci-command-table-check.mjs`, extending its DOC → CI direction to `agent/AGENTS.md`, and it BITES exactly as intended
(mutating one command into `cargo test --workspace --all-features` produced "agent/AGENTS.md names `…` and NO step in ci.yml runs
it"). **BUT IT ALSO REFUSED THE UNMUTATED TREE**, naming `cargo fmt --all -- --check                       # --check, because CI never
runs the MUTATING form` — because my extractor keeps each line's TRAILING COMMENT, and a command with an inline `#` note is not the
string CI contains. **A gate that fails on a clean tree is worse than no gate**: it would block every commit until somebody weakened
it, which is how gates get weakened. So it is reverted rather than shipped with a fix I could not verify in the same step, and the
specified repair is TWO clauses: strip `#`-comments (and trailing whitespace) before matching, and then prove BOTH directions — the
mutation must fail AND the clean tree must pass. **The bite alone is half a proof**; this round is the half that was missing.


**Round 123, one line**: round 122 raised the obvious follow-up — are there OTHER scoped instruction files with the same
gap? — and the audit answers it in one command: `git ls-files | grep -E "(^|/)(AGENTS|CLAUDE|CONTEXT)\.md$"` finds **exactly
TWO**, `AGENTS.md` (24,834 B) and `agent/AGENTS.md` (5,658 B), and both now carry the rules round 122 added (WAIT 2/2, do-not-push
2/1, `-- --check` 1/2). **So the fix covered the whole set rather than one sample of it**, which is the difference between repairing a
file and closing a class. Note also what makes the root table trustworthy where the agent file was not: `ci-command-table-check`
already held the root's per-directory commands against what CI actually runs, so the root was gated and the agent file was not —
**a scoped instruction file inherits none of its parent's gates.**


**Round 121, one line**: the whole suite re-run after NINE rounds of change (107-120: the mutation table moved to its own
archive, a ceiling was added for it and proven, five pointers corrected, the inventory cell re-measured, the e2e script's terminal
predicate fixed, the pre-commit hook installed and proven to refuse) — `bash scripts/test/all-gates.bash` → **57 ok, 0 failed, 1 not
runnable here (of 58)**, the same totals as round 111 and as every run before them. The last full-suite run predated the hook
install, so this is also the first time the suite has run WITH a hook that fires on commit.

## round 120 — the hook is PROVEN TO REFUSE, which is what round 118 owed it

Round 118 installed the pre-commit hook and proved it RUNS (an empty commit, exit 0). Its own paragraph carries the rule that
demands more — **"PROVE THE MUTATION, NOT THE HOOK"** — because a hook that runs is not a hook that stops anything. So: a syntax
error appended to a payload module, and the hook run by hand:

```
mutated  (agent/scripts/lib/sweep/panel-run.cjs):  hook exit=1
   "The emitted scripts are how the device runs every design check. Fix the emitter, then commit —
    or use --no-verify if you know the failure is unrelated, and say so in the commit."
restored:                                          hook exit=0   (tree clean)
```

**THE FIRST ATTEMPT PROVED NOTHING, AND THAT IS THE ROUND'S SECOND LESSON.** It planted the error into
`agent/scripts/lib/sweep/*.mjs` — and the payloads are **`.cjs`**. The glob matched no file, `F` was empty, and both the "mutated"
and the "restored" run therefore executed against an UNMUTATED tree and both exited 0. **A test whose mutation never landed looks
exactly like a test that passed** — the same shape as round 109 (a padding that did not cross the ceiling) and round 113 (a flake
that was deterministic). It is recorded here rather than quietly re-run, and the row in the mutation table carries it too.

**SO THE MECHANISM CHAIN IS NOW COMPLETE AND EACH LINK IS PROVEN**: round 117 pushed through a red gate because the hook was run by
hand and its answer discarded; round 118 installed it so it runs itself, and proved only that it RUNS; round 119 corrected a false
objection to the global install by opening the file; round 120 proves it REFUSES. **The habit is now a mechanism, and the mechanism
is a measured one.**


**Round 117 (cont.) — AND I PUSHED A COMMIT WHOSE GATE HAD FAILED, IN THREE SEPARATE WAYS.** `production-host-check` refused the
new §5.8 cell because it carried a production URL into `docs/agents/inventory.md`, a file not on that gate's declared list — and the
commit went out anyway, because: (1) my command chained the gates and the commit with `;` rather than `&&`, so a red gate did not stop
it; (2) I ran `bash scripts/hooks/pre-commit >/dev/null 2>&1` and **threw away its exit code**, which is the one habit AGENTS.md asks
for by hand — and the hook would have refused; (3) the cell itself was written without asking what that gate allows, in a repository
where every round for a hundred rounds has asked exactly that. **All three are the same mistake at three layers: a check was RUN and
its ANSWER was discarded** — which is this session's oldest lesson, here committed by the round that was busy citing it. The fix is
threefold and only the first is in the file: the URL is out of the inventory (the ledger and AGENTS.md may name it; the inventory may
not), the gate is green again, and the record says plainly that the push happened through a red gate rather than quietly rewriting it.


**Round 117, one line**: the inventory's §5.8 cell still read "**nothing else runs them either — no schedule, no documented
manual cadence**", which stopped being true in round 82 and was disproved by every round since: it now records the cadence, the CDN
transport, the **73/74 (2 skipped)** baseline across all seven sections, the **two real defects** the runs found and fixed, the vacuous
check and the SKIP state the suite gained — and the one remaining failure as **bounded to full-suite load** rather than "sometimes".
**A cell that is stale in the flattering direction is still stale**, and this one was hiding a dozen rounds of work.


**Round 116, one line — THE SUITE'S CURRENT BASELINE, AFTER ALL THREE FIXES**: the whole device suite gives
**73/74 passed (2 skipped)** — up from round 104's 72/74 — with the terminal check passing (`state=partial exit=null` reported in
its line), the panel's two checks passing, and the http arm's two visibility checks SKIPPED by name of arm. **The one remaining
failure is `FAIL mcp stdio click drives embedded view -- https://example.com/`, and its character is now specific rather than
vague: it appears in FULL-SUITE runs and not in section-only ones** (round 104 full: failed; rounds 103/115 mcp-and-terminal only:
passed) — which is exactly what that check's own comment predicts, "under contention (parallel drivers on one box) a click can land
while the view is mid-navigation and silently do nothing (device-caught)". **So the honest baseline is 73/74 with one
load-dependent check, not 74/74 and not a mystery**, and the difference from round 104 is that both of ITS failures have been
chased to an end: one was a wrong predicate (fixed, 12/12 twice) and one was this flake, now bounded to the load case.


**Round 114, one line**: round 113 named the fix, this round applied it — the terminal check now asserts the MARKER and
REPORTS the state (`state=partial exit=null` is what this device produces for a PTY execute, measured twice in a row), instead of
demanding a state name the device does not emit. **AND THE OWED VERIFICATION IS PAID (round 115): `12/12 passed`, TWICE**, with the check reporting
`PASS terminal session execute -- state=partial exit=null`:

```
run 1: == 12/12 passed ==   PASS terminal session execute  -- state=partial exit=null
run 2: == 12/12 passed ==   PASS terminal session execute  -- state=partial exit=null
```

**So the predicate was the whole failure, and the fix is confirmed by the same instrument that found it.** Note what the passing
line now CARRIES: `state=partial exit=null` is visible to whoever reads the run, where before it was the reason for a FAIL with no
way to tell a wrong check from a wrong device. **A check that reports the shape it accepted is worth more than one that only says
yes** — and that is the difference between the three rounds this took (104 read it as flake, 113 built machinery that fixed nothing
and said so, 114 corrected the predicate, 115 paid for it).

## round 113 — I diagnosed a flake and built machinery, and the re-run says the PREDICATE is wrong

Round 104 saw `FAIL terminal session execute -- state=partial exit=null` once, called it the documented flake class, and this
round acted on that reading: the check's `await sleep(2500)` — commented "let the shell boot (first-prompt gate)" — was a sleep
wearing a gate's name, so it became a real poll for the shell's first output, and the execute got a second attempt and a 30-second
timeout. Deployed and run TWICE on the device:

```
run 1: == 11/12 passed ==   FAIL terminal session execute  -- state=partial exit=null
run 2: == 11/12 passed ==   FAIL terminal session execute  -- state=partial exit=null
```

**TWO FOR TWO, IDENTICAL — so it was never a flake, and the machinery did not touch it.** The hypothesis was wrong; the run is
what said so.

**AND THE DISCRIMINATOR HAS BEEN IN THIS LEDGER SINCE ROUND 87**: the PANEL section opens a PTY and runs
`Write-Output "PANEL-VIS-…"` through the same `terminal_execute`, and its check reads
`PASS panel ai write -- state=partial`. **It passes BECAUSE it only asks for the marker in the text**: `!!(ex && (ex.text ||'').includes(marker))`.
The terminal section asks for `ex.state === 'done'` **as well** — and on this device a PTY execute of `Write-Output` returns
`state=partial` with `exit_code=null` EVERY time, marker present and correct.

**SO THE CHECK ASSERTS A STATE THE DEVICE DOES NOT PRODUCE, WHILE THE THING IT ACTUALLY WANTS IS ALREADY IN ITS HANDS** — the same
shape as the panel selector (round 90) and the mcp arm (rounds 100-103), in a third section. The fix is not machinery: it is to ask
whether the marker came back, and to treat `partial` as the normal shape of a PTY execute on this device — or to record why a
`done` is expected, if some caller genuinely needs it.

**THE MACHINERY STAYS ANYWAY, ON ITS OWN MERITS**: a poll for the shell's first output is a better gate than a fixed 2.5 s sleep,
and a second attempt with a longer timeout is what the click check already does for the same class of timing. Neither is proven to
fix anything — they are proven NOT to have made this worse (11/12 both runs, the same total as before). **A change that does not
fix the thing it was aimed at should say so in its own record**, which is what this section is for.


**Round 112, one line**: the inventory's docs cell said the ledger "is over 4,600" lines, and `wc -l` now reads **4,286**
— because round 107 moved 91 KB of table out of it. **The cell was true when written and false the moment I split the file**, which is
the drift that cell exists to warn about, so it now names all THREE archives with their counts and the command that produces them.
A size in prose is a measurement with a date, not a property of the file.


**Round 111, one line**: the WHOLE suite re-run after the rounds that moved 91 KB of table between archives and added a
ceiling for the third one — `bash scripts/test/all-gates.bash` → **57 ok, 0 failed, 1 not runnable here (of 58)**, the same totals
as before them. Rounds 107-110 had only been checked against the three DOC gates, which is the subset next to the change; the
whole suite is what a round that edits a gate and rewrites three archives owes.


**Round 109, one line — THE NEW CEILING IS PROVEN TO BITE, ON ITS SECOND TRY**: round 107 gave `ledger-mutations.md` its own
ceiling, and this round padded it to **461,649 B** — the gate exits 1 with "is 461649 bytes and the ceiling is 400000 — a table that
outgrows its own file is the round-49 problem again"; restored to **92,048 B** it is ok again. **The first attempt was inconclusive and
is worth keeping**: padding to 303,249 B produced exit 0, because 303 KB is UNDER the 400 KB ceiling — a mutation that does not cross
the boundary proves nothing, and reading its exit 0 as "the gate is broken" would have been the same mistake as reading any other
summary as the measurement. **AND THE ROW IS IN THE TABLE NOW (round 110)**, appended with the format the file actually uses — it is THREE columns
(`| gate | mutation that must fail it | result |`), and round 109's script had counted two because the line it grabbed sat inside a
CODE BLOCK rather than in the table. So the owed row is paid, and the reason it was owed is itself the entry's value: **the file's
format was read off a code block, and refusing to write was the right outcome of getting that wrong.**


**Round 107, one line — THE SPLIT IS DONE, AND THE GATE CAUGHT MY OWN MISTAKE WHILE I DID IT**: the mutation table moved to
`docs/agents/ledger-mutations.md` (**91,532 B**), taking its host from **396,139 → 304,606 B** and leaving both halves with real
headroom; `ledger-budget-check.mjs` gained a third archive constant, a ceiling for it, and — because the first run FAILED with
"the ledger index names 1 section(s) that do not exist: THE INSTRUCTION FILE WAS 68% EVIDENCE, AND THE MOVE BROKE IT FIRST" — the
index resolver at `:80` now reads all three files instead of two. **That failure is the round's best evidence**: the gate refused a
commit whose index pointed at a section that had just moved, which is precisely the "pruning must mean MOVED, not deleted" rule it
was written for, and it fired on the round that was doing the moving.


**Round 106, one line**: the appendix split is now MECHANICAL rather than exploratory — `ledger-budget-check.mjs` declares its
archives as plain constants at the top (`const ARCHIVE`, `const APPENDIX`, `ARCHIVE_CEILING = 400_000`, `ARCHIVE_FLOOR_APX =
100_000`) and then applies the ceiling and the index-marker rules to each, so the third archive is: a
`docs/agents/ledger-mutations.md` holding the `## Which mutation must fail which gate` section, a `const MUTATIONS` beside the
other two, one more line in whatever list applies the ceiling, and the same for `numbered-claims-check.mjs`'s `docs` array
(which is where "the number in the ledger is the number a command produced" is enforced). **Naming the constants here is the point
of the line**: the next round executes the split instead of re-reading the gate to discover its shape, which is what this round
spent itself doing.


**Round 105, one line — A BUDGET ABOUT TO BITE, WITH ITS NUMBERS**: measured rather than assumed,
`docs/agents/design-ledger.md` is **341,144 B of 400,000** (~11 rounds of headroom at the current ~5 KB/round), but
`docs/agents/ledger-appendix.md` is **396,139 B of 400,000 — 3,861 B of headroom**, and its "Which mutation must fail which
gate" table is the thing rounds APPEND to whenever a gate earns a mutation. So the next few gate additions will trip
`ledger-budget-check` on an otherwise-good commit, and the fix is the one round 49 already established rather than a raised
ceiling: **move the mutations table into its own file** (a third archive, with its own ceiling and its own delimited index
markers) and teach the gate to look there. **Relaxing the ceiling is not on the table** — a budget that is raised whenever it
binds is not a budget, and this ledger has spent the whole session insisting on that difference. Recorded here because the
number, not the intention, is what makes it actionable: 3,861 bytes is fewer than two mutation rows.


**Round 104, one line**: the WHOLE device-targeted suite re-run with round 103's script — a regression check on `check()`
and the summary line, which every section uses — gives **72/74 passed (2 skipped)**: `FAIL terminal session execute -- state=partial
exit=null` and `FAIL mcp stdio click drives embedded view -- https://example.com/`, both of them the DOCUMENTED flake class (a
slow command; "a click can land while the view is mid-navigation and silently do nothing", device-caught per the check's own
comment) and neither of them in the code this round touched — the stdio click PASSED in the mcp-only run minutes earlier. So the
third state and the new counting rule are regression-clean across all seven sections, and **74+2 is the honest size of the suite
that had never once been executed**: 74 assertions that apply on this device, 2 that name the arm they cannot apply on.

## round 103 — the arm-aware check ships, and the mcp section reports 10/10 with two honest skips

Rounds 100-102 established that the `mcp` section was asserting a CONTRACT THAT CANNOT HOLD on this device (http on the
private-headless fork) and specified the fix. This round implemented and verified it:

```
PASS mcp stdio connect · navigate ok · drives embedded view (/mcp-autoselect-…) · SPA intact
PASS mcp stdio click learn-more (ref=f2e7) · click drives embedded view (/inner-click-test)
PASS mcp http  connect · navigate ok · SPA intact · click learn-more (ref=f1e7)
SKIP mcp http  drives embedded view  -- private-headless arm (9229 launched without a desktop view): the tool drove its own browser
SKIP mcp http  click drives embedded view  -- private-headless arm: the click drove the browser the tool owns, which no embedded view shows

== 10/10 passed == (2 skipped)
```

**THREE CHANGES, EACH ONE THE ANSWER TO A ROUND OF THIS THREAD**: `skip(name, reason)` gives the suite the third state it never
had, so an arm that cannot apply here no longer has to choose between lying (PASS) and inventing a defect (FAIL) — which is what
it did for eleven rounds; the summary EXCLUDES skips from both numbers (`pass: null` is neither, and counting it as either is the
failure mode this thread kept finding); and the arm is decided ONCE per transport, because only http can be on the private fork —
stdio spawns and owns its own child, which is why its two visibility checks still assert hard and still pass.

**THE SUITE STILL TESTS VISIBILITY — IT JUST NO LONGER PRETENDS TO.** The embedded view is asserted, hard, through the transport
that can reach it; the private arm asserts the contract that is actually true of it. **And the number is now honest in a way it
never was: `10/10 (2 skipped)` says exactly how much of the section ran on this device and how much did not apply.**

**WHAT THIS THREAD COST AND BOUGHT, IN ONE PLACE**: twelve rounds to turn `1/2` into `2/2` (the panel selector, round 90) and
`11/12` into `10/10 (2 skipped)` (the arm, round 103) — from a suite in which NOTHING had ever been executed. Two defects fixed
(a selector that clicked a navigation tab, a contract asserted without its precondition), one diagnostic rescued from a filter
(round 93), one log finally read (round 98), and one habit named in four successive forms: **an instrument nobody runs does not
merely fail to find defects — it accumulates its own.**


**Round 102, one line**: the fix round 101 named is now specified rather than described — `check()` at `e2e.js:115` takes
`(name, cond, detail)` and has **no skip concept**, while the summary line is computed from the results array; so the change is:
(1) let a result carry `pass: null` with a reason and print `SKIP <name> -- <reason>`; (2) exclude `null` results from BOTH numbers
on the `== N/M passed ==` line, so a skipped arm cannot silently become a pass; (3) in the `mcp` section, decide the arm ONCE per
transport — the private-headless arm is the one where the tool drives its own browser, and the observed tab list (`about:blank`, no
embedded-view tab) is the in-band signal for it — and assert the arm's real contract: on ATTACH, "the embedded view followed"; on
PRIVATE, that the call drove the browser it owns, with the visibility check SKIPPED and its reason naming the arm. **The counting
rule matters more than the printing rule**: this suite's numbers are read by humans as a baseline, and a skip that counts as a pass
would be the third instrument in this thread to report success for something that did not happen.

## round 91 — the six sections that had never run: 45/49, and the failures cluster in ONE place

Round 90 proved the panel section had been broken for its entire life by a selector nobody had ever executed. So this round ran
THE OTHER SIX that have never run — `terminal`, `file`, `workflow`, `mcp`, `evidence`, `browser` — on the device, over the same
transport:

```
PASS terminal read unknown evicted · terminal history retains closed
PASS file upload 2 pages · stat · single-read download · list contains upload · stat missing ok:false · text write · text read
PASS workflow process_list · local execute · file_write · file_stat · memory_save · memory_search · memory_list · memory_update
     · memory_search updated · memory_export · memory_delete verified
PASS mcp stdio connect · navigate ok · drives embedded view (https://example.com/mcp-autoselect-…)
PASS mcp stdio SPA intact (targets=2) · click learn-more (ref=f3e2 want=inner-click-test)
FAIL mcp stdio click drives embedded view  -- https://example.com/
PASS mcp http connect · navigate ok · SPA intact · click learn-more
FAIL mcp http drives embedded view         -- https://example.com/
FAIL mcp http click drives embedded view   -- https://example.com/
PASS evidence screenshot saved · pwshots lists it
PASS browser pwinfo bundled · run_script fail path · drives view · SPA bar sync · focus-trap bar follows

== 45/49 passed ==
```

**FORTY-FIVE OF FORTY-NINE PASS, AND THE FOUR FAILURES (three of them inside the captured window) ARE ALL IN ONE SECTION.** What
passes around them is the informative part: the MCP transport CONNECTS, `navigate` WORKS and the embedded view really does move
to `https://example.com/mcp-autoselect-…`, the SPA stays intact, and the CLICK finds a ref and reports it — but the check that asks
whether the CLICK DROVE THE VIEW reports **`https://example.com/`**, the un-navigated URL, over BOTH transports (stdio and http).
**So the break is not the transport and not the SPA: it is the path from "a click was performed" to "the view followed it".**

**AND THE CHECK'S OWN CODE SPLITS THE FOUR INTO TWO DIFFERENT FAILURES (round 92)**, which is what makes the next step a
diagnosis rather than a guess. Line 372 asks one question — `embedded.url.includes(marker)`, i.e. DID THE VIEW MOVE TO THE URL WE
NAVIGATED TO — and it is the check that distinguishes the transports: **stdio passes it** (the view really did reach
`/mcp-autoselect-…`) and **http fails it** (the view stayed at `https://example.com/`, the PREVIOUS url). Line 438 asks a
different one — did the CLICK drive the view — and it fails for BOTH.

**SO THE TWO FAILURES ARE NOT THE SAME FAILURE**: one transport does not move the view on NAVIGATE, and neither transport moves
it on CLICK. And in both cases the click itself SUCCEEDED — `click learn-more` passes with `ref=f3e2 want=inner-click-test`, so a
ref was found and handed to the tool. **What is unproven is only the last link: "a click was performed" → "the view followed
it".** A check that finds its target and then sees nothing move is a very different object from a check that cannot find its
target, and the suite already says which one this is.

## round 93 (cont.) — THE RE-RUN CORRECTS ROUNDS 91 AND 92: three of the four were FLAKE, and the survivor is a RACE

The same `mcp` section, run again with nothing changed in the code:

```
PASS mcp stdio connect · navigate ok · drives embedded view (/mcp-autoselect-…) · SPA intact
PASS mcp stdio click learn-more (ref=f2e7) · click drives embedded view (/inner-click-test)
PASS mcp http  connect · navigate ok · SPA intact
FAIL mcp http  drives embedded view  -- https://example.com/inner-click-test
PASS mcp http  click learn-more (ref=f1e7) · click drives embedded view (/inner-click-test)

== 11/12 passed ==
```

**11/12, WHERE ROUND 91 SAW FOUR FAILURES IN THE SAME SECTION.** Three of them were TRANSIENT — and the check's own comments had
already said so ("under contention … a click can land while the view is mid-navigation and silently do nothing (device-caught)";
"the external Learn more link's cross-origin redirect chain flakes under load"). **Two rounds then reasoned about those numbers as
if they were a baseline**: round 91 called the cluster "four failures in one section" and round 92 split them into "two different
failures". Both statements were true OF THAT RUN and neither was true of the INSTRUMENT, which is the difference this ledger keeps
insisting on.

**AND THAT DIAGNOSIS WAS WRONG — THE POLL IS ALREADY THERE (round 94).** Reading the block above the failing check:

```js
// Poll (not fixed sleep) for the navigate to become visible on CDP: after
// a transport switch the view can still show the previous probe's page
// for several seconds (device-caught: http probe kept seeing the stdio
// probe's iana.org landing past the old fixed 6s sleep). Same predicate,
// more time — mirrors the click poll below.
for (let i = 0; i < 15; i++) { await sleep(1000); ... if (embedded.url.includes(marker)) break; }
```

**FIFTEEN POLLS OF ONE SECOND — the asymmetry round 93 named does not exist**, and the comment describes the failing
observation almost word for word: "the http probe kept seeing the stdio probe's iana.org landing past the old fixed 6s sleep",
where this run saw the stdio probe's `/inner-click-test` landing. So the mitigation was written, it was IN the code, and it was
not enough — **the honest statement is that the http transport's navigation did not become visible within fifteen seconds after a
transport switch, which is a longer stall than the fix assumed, not a missing poll.**

**AND THE SAME SECTION RUN THREE TIMES GIVES THE SAME ANSWER — IT IS DETERMINISTIC, NOT A FLAKE (round 95).**

```
run 1: == 11/12 passed ==   FAIL mcp http drives embedded view  -- https://example.com/inner-click-test
run 2: == 11/12 passed ==   FAIL mcp http drives embedded view  -- https://example.com/inner-click-test
run 3: == 11/12 passed ==   FAIL mcp http drives embedded view  -- https://example.com/inner-click-test
```

Three for three, the same check, the same URL — **and that URL is the one the STDIO phase's click leaves the view at**, because
`--only mcp` runs stdio first and http second. So the http phase's navigation never becomes visible, not "within fifteen seconds"
and not at all: **the view is still exactly where the previous transport left it, every single time.** Round 94's statement is
therefore true and stronger than it was written — this is a deterministic no-op, not a stall.

**AND IT EXPOSES A SECOND DEFECT, IN A CHECK THAT CANNOT FAIL.** `mcp http click drives embedded view` PASSES in these runs, and
it passes VACUOUSLY: its predicate is `url.includes('inner-click-test') || url.includes('iana.org')`, and the stale URL left by the
stdio click is `https://example.com/inner-click-test` — so the predicate is satisfied BEFORE the http click happens. **A check whose
success condition a stale value already meets reports success for a click that does nothing**, which is the same class as the panel
selector (round 90) and the discarded triage line (round 93), one level deeper: there the instrument measured the wrong thing, here
it measures the right thing against a value that was already true.

**AND THE PRODUCT ITSELF DOCUMENTS THE MECHANISM — IT IS THE HTTP TRANSPORT'S DESIGNED SELF-HEAL (round 96).**
`agent/src/plugins/mcp_client/tools.rs` opens with a measured theorem from round 137:

```
//! round-137 measured theorem (d1 real-device probe, 2026-08-25):
//! playwright-mcp 0.0.79's Streamable HTTP session is unconditionally reaped
//! ~4 s after each tools/call response completes ("Session not found").
```

and then states the only correct client shape: give up keepalive, accept that "every call may hit a dead session", and
**"re-handshake + navigate the page back to last_url (restore context), then retry the original call."** `last_url` is tracked
bidirectionally, and its own doc says the restored session "lands on a fresh blank page" without it.

**SO THE HTTP TRANSPORT IS BUILT TO NAVIGATE THE BROWSER BACKWARDS, BY DESIGN, ON EVERY SESSION DEATH — AND ITS SESSIONS DIE
EVERY ~4 SECONDS.** The e2e http phase does: connect, navigate to the marker, then POLL FOR FIFTEEN SECONDS. Its session is
guaranteed to be reaped inside that window. Two orderings then produce exactly what three runs showed, and telling them apart is a
one-experiment question rather than a speculation:

1. the restore-navigate runs LAST (restore lands after the retry, or the retry itself dies and the handler gives up having already
   restored) — so the view ends at the PREVIOUS url and the call still reports `ok`, because the RESTORE succeeded;
2. the retry never happens within the phase, and the view stays wherever the reap caught it.

## round 98 — THE LOG ANSWERS IT, AND THE ANSWER IS `[select]`: THE SESSION NEVER DIED

Round 97 said the shortest path was the transport's own diagnostic lines. They are at
`C:\ProgramData\Summrise\logs\mcp_diag.log`, and the tail after one e2e http phase reads:

```
[rpc] method=notifications/initialized id=None sid=d501bd72-… http=202
[rpc] method=tools/list id=Some(2) sid=d501bd72-… http=200
[select] auto-selecting the embedded-view tab (desktop CDP attached)
[rpc] method=tools/call id=Some(3) … http=200
[select] initial tab list text: ### Result
- 0: (current) [](about:blank)
[select] retry 1/4 — tab list not ready yet
[select] retry 2/4 — tab list not ready yet
[select] retry 3/4 — tab list not ready yet
[select] retry 4/4 — tab list not ready yet
[select] no embedded-view tab found after retries — leaving default selection
```

**NOT ONE `[heal]`, NOT ONE `[restore]`, NOT ONE 404: the session was ALIVE for the whole phase** (`sid=d501bd72-…`, every call
`http=200`). The round-137 reaping theorem — the mechanism rounds 96 and 97 built their explanations on — **never fired.** Two rounds
of increasingly careful reasoning about session death were reasoning about an event that did not happen, and the file that
disproved it had been sitting on the device since the first run.

**THE ACTUAL MECHANISM IS THE TAB SELECTION, AND THE TRANSPORT SAYS SO IN WORDS**: it tries to attach to the tab the desktop CDP is
driving, reads a tab list containing only `0: (current) [](about:blank)`, retries four times, and then **"leaving default
selection"**. So every subsequent call — navigate, snapshot, click — operates on a BLANK TAB THAT NOTHING IS WATCHING, returns
`ok` because it genuinely succeeded there, and the embedded view is never touched. **The view keeps whatever the PREVIOUS transport
left on it, which is exactly the URL three runs showed.**

**EVERY OBSERVATION FINALLY FITS, INCLUDING THE ONES THAT CONTRADICTED MY THEORIES**: navigate looks like a no-op while reporting ok
(it navigated another tab) · the view stays at the stdio phase's `/inner-click-test` (untouched) · the http CLICK check passes
vacuously (the stale URL already satisfied its predicate) · and there are **zero heal lines**, which is what killed the reaping
story. **The product defect is precise: after a transport switch the http session cannot find the embedded-view tab, and it proceeds
on a blank one instead of failing the call.**

## round 100 — THE TEST ANSWERS IT IN ONE LINE: THE 9229 INSTANCE RUNS `--headless`

Round 99's distinguishing test, run on the device:

```
  Id ProcessName StartTime
4812 node        2026/9/26 4:36:58          <- exactly ONE playwright-mcp, no stale second instance

LocalAddress LocalPort OwningProcess
127.0.0.1          9333          4248       <- the desktop CDP
127.0.0.1          9229          4812       <- playwright-mcp

4812: node ... @playwright\mcp\cli.js --port 9229 --headless --browser chromium --o...
```

**THE STALE-INSTANCE HYPOTHESIS IS REFUTED — there is exactly one instance and it owns the port.** And the command line answers
the question instead: **the 9229 server was started `--headless`, so it drives its OWN chromium with no view at all.**

**SO THE WHOLE MECHANISM IS ORDINARY AND COMPLETE**:

- the **stdio** transport spawns and owns its own playwright-mcp child — a browser the desktop view can attach to — so its
  navigate moves the embedded view, and it passes;
- the **http** transport talks to the long-running 9229 instance, which is **headless and separate**: its tab list is one
  `about:blank` because that is what a headless browser has, `[select]` cannot find the embedded-view tab because that tab
  belongs to a DIFFERENT browser, and every call it makes succeeds — on a chromium nobody is looking at.

**THE CHECK WAS NEVER MEASURING THE PRODUCT; IT WAS MEASURING A HEADLESS BROWSER'S INABILITY TO BE THE EMBEDDED VIEW.** `mcp http
drives embedded view` cannot pass on a device whose 9229 runs `--headless`, and no amount of polling, retrying or self-healing
would have changed that — which is what ten rounds of increasingly specific theories failed to notice, and what one
`Get-CimInstance Win32_Process` printed on the first try.

**TEN ROUNDS, ONE LINE.** Round 91 called it four failures; 92 two kinds of failure; 93 flake; 94 corrected 93; 95 made it
deterministic; 96 blamed session reaping; 97 read the code that would have implemented the blame; 98 read the log and found no
reaping at all; 99 named the stale-instance hazard and wrote the test; 100 ran it. **Every one of those rounds after the first
was reasoning about an instrument, and the answer was in the environment the instrument was pointed at.** The rule this thread
earns, in its plainest form: **before explaining what a measurement means, confirm what it was measuring.**

**WHAT TO DO ABOUT IT IS NOW A DESIGN QUESTION, NOT A MYSTERY**: either the `mcp http` arm of the e2e section asserts something
that only holds when 9229 is launched with a view, or the device should not run 9229 headless, or the section should assert the
headless arm's real contract (that it drives ITS browser) and leave the embedded view to stdio. All three are decidable now.

**AND THE CODE SAYS THE `--headless` IS A FALLBACK, NOT A CONFIGURATION (round 101).** `tools.rs:507-513`:

```rust
/// ATTACH (panel screenshot sees everything); else private headless
    None => vec!["--headless".into(), "--browser".into(), "chromium".into()],
```

with `:530` — "when up, else none (headless fork). Event-driven probe, 300 ms budget" — and `:683-685` — "playwright-mcp spawn
launched its OWN headless browser — AI … loopback 9333; attach there when it is up. Private-headless stays" — plus two tests that
assert the ATTACH arm does NOT carry `--headless`. **So the product probes for the desktop CDP and attaches when it is up; the
`--headless` on this device is the ELSE branch, taken because the panel was not up when that long-running 9229 was launched.**

**THAT COMPLETES THE PICTURE AND REASSIGNS THE BLAME ONE LAST TIME.** The http arm of the e2e section asserts "the embedded view
followed", which is true only on the ATTACH arm; this device is on the private-headless arm, where the honest contract is "the tool
drove its OWN browser". **The check is not wrong about the product — it is missing its precondition**, and the suite had no way to
say so because nothing in it has ever run. The two facts that settle it are now both measured: the tab list is one `about:blank`
(a private browser) and the process command line carries `--headless` (the else branch).

**THE FIX IS THEREFORE THE ONE THE REPO'S OWN CULTURE PRESCRIBES**: make the check state the arm it is asserting — attach or
private — and assert that arm's real contract, rather than asserting visibility unconditionally on a transport whose whole purpose
is to work when there is no view. The `panel` section, which passes 2/2, is the section that genuinely tests visibility; the `mcp`
section tests the MCP surface, and on a headless fork its click and navigate should be asserted against the browser it owns.

**AND THE FALLBACK IS SILENT BY CONSTRUCTION (round 99)** — the whole branch is:

```rust
    } else {
        diag_log("[select] no embedded-view tab found after retries — leaving default selection");
    }
    Ok(())
```

The function returns `Ok(())` either way, so **a transport that cannot find the embedded view behaves exactly like one that found
it** — every later call succeeds against whatever the default selection happens to be. That is the fail-open shape this repository
treats as a defect everywhere else, and here it is load-bearing: the blank tab is not an error path, it is the DESIGN.

**AND THE EVIDENCE POINTS AT THE HAZARD `DEFAULT_URL` ALREADY WARNS ABOUT.** The log read a tab list of exactly
`0: (current) [](about:blank)` — one blank page, which is what a playwright-mcp instance looks like **before it has driven
anything**, and the http phase drives nothing because it selected that blank tab. The constant's own comment says:

```rust
/// round-118: 127.0.0.1, not "localhost" — localhost resolves to [::1] first
/// on Windows, so a client could latch onto a stale instance instead of the
/// one the task actually hosts.
```

So the hypothesis the log supports is: **the http transport attached to an instance that is not the one hosting the embedded view**
— a stale or second playwright-mcp — and every observation follows, including why stdio (which spawns and owns its child) works.
**The distinguishing test is one command on the device**: what is listening on 9229, and how many playwright-mcp processes exist.
One is correct; two is the defect. That is written here so the next round measures it instead of arguing it — which is what the
last six rounds should have done with the log.

**AND THE LESSON IS THE ONE THIS THREAD HAS NOW LEARNED FOUR TIMES, EACH TIME ONE LAYER DEEPER**: round 90 — a warning beside the
code does not reach the code that tests it; round 93 — a diagnostic in the output does not reach a reader filtering for verdicts;
round 97 — a mechanism the product logs does not reach a reader who reasons first; round 98 — **and the log itself does not reach a
reader who has already decided what it will say.** Six rounds of inference lost to one `Get-Content -Tail 20`.

**AND THE CODE SETTLES THE ORDERING — THE RESTORE RUNS FIRST, THE RETRY AFTER (round 97).** Two lines decide it:

```rust
/// Self-heal: drop the stale mcp-session-id → re-handshake → use last_url to …   (tools.rs:1089)
diag_log(&format!("[restore] navigating back to {url}"));                          //   :1118
// A failed restore-navigation is not fatal: the original call retries             //   :1119
```

So of round 96's two candidates, it is the FIRST: **heal-and-restore navigates the browser BACK, and only then does the original
call run again.** Which means the observed end state — the view at the PREVIOUS url while the call reports `ok` — is not a
give-up path. **The restore succeeded, and the retry that followed it did not move the view.** That is a stronger and stranger
statement than "the navigate is a no-op": the transport can drive the browser backwards and apparently not forwards, on a session
it has just re-handshaked.

**AND THE INSTRUMENT FOR THE NEXT STEP IS ALREADY ON THE DEVICE, PUT THERE BY ROUND 137 FOR EXACTLY THIS.** The header says:
"round-137 added timestamps and `[heal]`/`[restore]` markers — before that [the log was unreadable]". The http transport writes
`[rpc] method=… id=… sid=… http=<status> body_head=…` for every round trip plus a `[restore] navigating back to <url>` line
whenever it heals. **So the next round does not need a hypothesis, a probe or a code change: it needs one e2e run and the device's
own diagnostic lines, which will print the exact sequence — whether the retry happened, what status it got, and which url the
restore chose.** Six rounds have now been spent reasoning about a mechanism the product logs on every call; reading that log is
the shortest path left, and it is the one instrument nobody has looked at.

**EITHER WAY THE OBSERVED URL IS THE SIGNATURE OF THE DESIGN**: `https://example.com/inner-click-test` is not a random stale page;
it is the `last_url` the http session inherited and dutifully restored. **The check is measuring a transport's documented recovery
behaviour and calling it a navigation.**

**WHAT THE THREE RUNS ESTABLISH, STATED SO IT CAN BE ATTACKED**: with `--only mcp` on this device, the http transport's
`browser_navigate` reports `ok` and the embedded view does not move — reproducibly, three times, with the leftover URL as the
evidence — while the stdio transport moves it and the http CLICK does move it (its own pass, however vacuous, came after a real
`browser_click` whose ref `f1e7` was found). **So the failure is specific to http NAVIGATE, not to the http transport as a whole**,
and that is the smallest statement the evidence supports.

**TWO ROUNDS, TWO SELF-CORRECTIONS, BOTH BY READING THE CODE**: round 93 corrected rounds 91-92 (flake read as a baseline) and
round 94 corrects round 93 (a missing poll that was present). Neither correction came from more reasoning about the numbers; both
came from opening the file. **The ledger's rule that "the count is a summary, the log is the measurement" has a third clause this
episode earns: the CODE is the instrument, and a claim about how a check behaves is a claim about a file.**

**AND THE SURVIVOR IS [still] A RACE-LIKE STALL**: the check requires the view to be at the marker URL, and the failure
reads `https://example.com/inner-click-test` — **the URL the PREVIOUS transport's run left there.** The check looks at the view
before its own navigation has committed. The click check in the same function polls fifteen times over fifteen seconds for exactly
this reason; the navigate check does not poll at all. **That asymmetry is the bug, and it is one more instance of this round's
theme: the instrument's own code names what it needs.**

**THE CORRECTION IS THE ROUND'S RESULT**: a first execution gives a BASELINE only for checks that are deterministic, and this
section says in its comments that it is not. The honest record of round 91 is therefore not "45/49" but **"45/49 on a run whose
failures were largely timing"**, and the honest next step is to make the navigate check wait the way the click check does — not to
diagnose a product defect that three re-runs have now declined to reproduce.

**AND THE INSTRUMENT ALREADY COMPUTES THE NEXT MEASUREMENT — MY OWN FILTER THREW IT AWAY (round 93).** The click check is not
naive: it INJECTS a same-origin link into the embedded view (`document.body.innerHTML = '<a id=e2e href=/inner-click-test …>'`),
snapshots, finds that link's ref, clicks it through `browser_click`, and polls up to fifteen seconds for the view to reach
`inner-click-test` or `iana.org`. When the view does not follow, it prints

```
  [triage] click missed; geometry: …
```
— and the comment says why: "a physical click that misses for viewport reasons looks identical to a broken click path; log
viewport + link rect so the next failure is instantly triaged instead of needing a CDP probe round-trip."

**THE ROUND-91 COMMAND RAN `Select-String -Pattern '^(PASS|FAIL|== )'`, AND THAT LINE BEGINS WITH TWO SPACES.** So the one piece
of evidence written FOR this exact failure was filtered out by the very command that provoked it, and two rounds then reasoned
about a click they could have measured. The fix is not a probe: it is re-running the same section WITHOUT the pattern filter and
reading the geometry the instrument was already told to print.

**AND THAT IS THE SAME LESSON AS THE PANEL SELECTOR, ONE LAYER UP**: round 90 learned that a warning living beside the code it
protects does not reach the code that tests it. This round learns that **a diagnostic living in the output does not reach a reader
who filters for verdicts.** Grepping a test run for PASS/FAIL is reading the summary; the triage line is the measurement, and it
is indented precisely because it is not a verdict.

**WHAT THIS ROUND DOES NOT CLAIM**: it does not say the MCP click is broken in the product. The panel round is the cautionary
tale from fifteen minutes earlier — a check that had been wrong since the day it was written — and these four have exactly the
same standing: never executed, therefore never baselined. **They are now known: 45 passing checks that had never run, and four
failing ones that had never run either**, which is a far better position than 49 unknowns.

**AND THE ESTIMATE WAS RIGHT, WHICH IS THE POINT**: round 90 said an instrument nobody runs accumulates its own defects. One
round later, running the other six produced four failures in one of them. The panel selector was not bad luck; it was the
expected state of anything never executed.

## round 85 — the cadence gets its transport, and the transport is the one the repo already uses

Round 83 corrected the e2e cadence to "get the script onto the device, then run its section there with the device's own
token and base". This round built the first half of that — and found the second thing the wrong version had hidden:

- the Linux→device INBOX route needs an admin token that is not in any file on this box, so it is not a route a round can
  take on its own;
- but the repo ALREADY has a sanctioned transport for exactly this shape of problem, and it is the one `live-panel-probe.mjs`
  uses: **emit into the CDN's public directory, deploy the index, and let the device fetch it over an outbound GET.** That
  route needs no inbound listener (which the server rules forbid anyway), no token beyond the deploy credential the index
  build already uses, and no content in an AI context.

So `agent/scripts/e2e/e2e.js` is now also published at `https://agent.saisi.online/summrise-agent/e2e.js` — verified live with
`200` and **54,783 bytes**, the file's exact size, which is the check that says the deployed bytes are the script and not an
error page. **IT IS COMMITTED LIKE THE PANEL BUILD AND THE PROBE ARE**, for the reason that section of AGENTS.md gives: Workers
Assets uploads the directory but HONOURS `.gitignore`, so an ignored-or-untracked file is silently absent from the deploy —
the failure mode where the URL works today and 404s after the next clean checkout.

## round 87 — THE CADENCE PRODUCED ITS FIRST VERDICT, AND IT IS A FAILURE

The corrected cadence ran for the first time, end to end, on the device: the script came over the CDN (round 86), the token was
found where it lives (`D:\Summrise\etc\config.yaml`, `device_token`, 64 chars — **read into a variable and never printed**, because
a token in the audit trail is a token in the record), and the `panel` section ran against the live panel:

```
PASS panel ai write  -- state=partial
FAIL panel xterm shows ai output  -- marker=PANEL-VIS-179036981456

== 1/2 passed ==
```

**AN INSTRUMENT THAT RAN NOWHERE FOR ITS WHOLE LIFE FOUND A REAL DEFECT ON ITS FIRST EXECUTION.** The first check writes through the
panel and sees the write land; the second asks whether the AI's own output actually appears in the terminal view, and the marker it
planted (`PANEL-VIS-179036981456`) is not there. That is the panel not SHOWING what the agent produced — the exact class the section
exists to catch, and it sits in the area rounds 73 and 78 touched (the read seam `SettingsPage` and `ConnModal` migrated onto).

## round 90 — the selector was the whole bug: the panel section passes on the device, 2/2

Round 89's probe said the check clicked the last `[role=tab]` — the `Path` VIEW-SWITCH tab — instead of the newest session. This
round scoped the selector to the session strip and re-ran it on the device:

```
PASS panel ai write  -- state=partial
PASS panel xterm shows ai output  -- marker=PANEL-VIS-179037089927

== 2/2 passed ==
```

**AND THE PANEL'S OWN TEST FILE HAD ALREADY WRITTEN THE WARNING DOWN.** `DesktopShell.test.tsx` scopes its queries to
`.dtab[role="tab"]` and says in a comment that `role=tab` is ALSO the view switch's. The e2e suite was the one place that had not
been told — so the same trap the panel's tests were written to avoid sat in the instrument that measures the panel. **A warning
that lives only beside the code it protects does not reach the code that tests it.**

**WHAT THIS SETTLES, AND WHAT IT LEAVES SETTLED FOREVER**: the panel was never broken; rounds 73 and 78 caused no regression
(the marker now appears in the live xterm); and the `panel` section has a PASSING baseline for the first time in its life, which
is the thing every future round needs in order to read a failure as a change rather than as the status quo.

**THE WHOLE ARC, IN FIVE ROUNDS, IS THE ARGUMENT FOR THE CADENCE**: 82 wrote a cadence for seven sections nothing ran → 83
FOLLOWED it and found the sentence unexecutable → 84-86 gave it a transport and proved it from the device → 87 its first verdict
was a FAILURE → 88 the failure was stable → 89 a probe showed the CHECK was broken, not the product → **90 the fix, verified
2/2 on the device.** An instrument nobody runs does not just fail to find defects; **it accumulates its own**, and this one had
been carrying a broken selector for as long as it had existed.

## round 89 — THE PROBE ANSWERS IT: THE CHECK IS BROKEN, THE PANEL IS NOT

The SPA probe (built this round, on the transport round 85 created) prints one JSON verdict, and it ends the question round 87
opened:

```json
{
  "spaTarget":    "http://127.0.0.1:18080/desktop/",
  "tabs":         6,
  "tabText":      "powershell|powershell 2|powershell 3|Terminal|Trajectory|Path",
  "activeTab":    "powershell",
  "visibleXterm": "NO_VISIBLE_TERM_HOST",
  "diagnosis":    "C: no visible term-host (nothing rendered to read)"
}
```

**THE TAB LIST MIXES SESSIONS AND PAGES.** `powershell`, `powershell 2` and `powershell 3` are sessions; `Terminal`, `Trajectory`
and `Path` are NAVIGATION tabs — and the e2e check clicks `ts[ts.length - 1]`, THE LAST `[role=tab]`, on the assumption that the
newest session is last. **It is not: the last one is `Path`.** So the section clicks the Path PAGE, the SPA navigates away from
the terminal, no `.term-host` is rendered at all, and the marker can never be found — six reads over twelve seconds, by
construction, forever.

**SO THE ANSWER TO "REGRESSION OR NEVER PASSED?" IS NEITHER OF THE TWO THE EARLIER ROUNDS OFFERED**: it is a broken CHECK. It
would only ever pass in a tab layout whose last element happened to be a session, which this panel does not produce. **The panel
never failed to show the AI's output here; nothing was ever asked to show it.** Rounds 87 and 88 were right to refuse to blame
the read-seam rounds without a before-picture — and the picture, when it came, pointed at the instrument.

**AND THE METHOD LESSON IS THE ONE THIS REPO KEEPS TEACHING**: the first version of this probe was a PowerShell `node -e`
one-liner, PowerShell stripped the inner double quotes, and node died on `fetch(http://127.0.0.1:9333/json/list)`. **A probe
belongs in a FILE** — the same rule AGENTS.md already carries for the 38 KB panel probe, which is why this one is now published
at `https://agent.saisi.online/summrise-agent/panel-spa-probe.js` and fetched by the device like `e2e.js` is. A measurement
instrument that only exists inside a shell one-liner is a measurement instrument that keeps breaking on quoting.

**THE FIX IS IN `e2e.js`, NOT IN THE PANEL**: the section must select the newest SESSION tab (by its name, or by scoping the query
to the session list) rather than the last tab of any kind. That is the next round's change, and it is a one-line selection plus
a test of the selection itself.

**AND THE FAILURE IS STABLE, WHICH NARROWS IT CONSIDERABLY (round 88).** Run twice more, with a fresh marker each time:

```
PASS panel ai write  -- state=partial
FAIL panel xterm shows ai output  -- marker=PANEL-VIS-179037005890

== 1/2 passed ==
```

The same two lines, twice. So this is not a flake to re-run away, and the check's own code says where it is NOT: it reads the
desktop SPA over CDP at `127.0.0.1:9333`, and when no `/desktop/` target exists it reports `no desktop SPA target` — **a message
this run never printed**, so the SPA was found, the rail's Terminal button was clicked, the LAST session tab was clicked, and the
visible `.term-host .xterm-rows` was read SIX times over twelve seconds without the marker appearing. **The marker is written
successfully through the API (check 1 passes) and never reaches the desktop SPA's terminal view.**

**AND THIS CLASS HAS HISTORY**: the ledger already records `useSessions`'s 1.2-second retry as "round-245's fix for an AI-opened
session that never appeared". Whether the SPA is missing the session entirely, failing to switch to it, or not receiving its
stream is the next round's question — and it is a question with three distinguishable answers, each testable on the device that
just produced this verdict twice.

**WHAT THIS ROUND DOES NOT CLAIM**: it does not say those rounds CAUSED it. The check has never run before, so there is no
before-picture to compare against — which is precisely the cost of an instrument that nothing ran, and precisely why the cadence
was worth building. The next round's first question is whether the marker is missing because of a regression or because the check
never passed: `git stash`-free, that is answered by running the same section against an older release, and the answer decides
whether this is a fix or a baseline.

**AND THE TRANSPORT IS NOW VERIFIED FROM THE DEVICE, NOT ONLY FROM HERE (round 86)**: the device ran
`Invoke-WebRequest -Uri https://agent.saisi.online/summrise-agent/e2e.js -OutFile D:\Summrise\e2e.js` and the file landed at
**54,783 bytes** — the source's exact size, on the far side, over an OUTBOUND GET, with no listener opened on either machine.
That is the shape the server rules require and the shape the previous two versions of this cadence lacked: a transfer the
device initiates. **WHAT IS STILL OPEN** is now only the last step: the device has the script and has not yet been given the
token and base to run a section with, so no VERDICT exists. The config listing under `D:\Summrise\data` returned no YAML,
which is where the next round starts — the token's home is the one thing this round did not find, and it is written down here
rather than left in a session that ends.

**AND THE EARLIER NOTE, KEPT BECAUSE IT IS STILL TRUE**: the second half — running a section on the
device against the live panel — has not been executed. The cadence now has a transport and a URL; what it does not yet have is
a recorded VERDICT from the device, and that is the thing the cadence exists to produce.


**Round 84, one line**: end-of-session state verified by command — tree clean and `main == origin/main`; CDN and npm both
**1.2.474**; ledger **301,320 B** of 400,000 and its appendix **396,139 B** under the same gate; AGENTS.md **22,524 B** of 48,000;
both instruction gates green; the reconcile ledger still holding the historical 1.2.453 (1.2.472's missing asset is named by the
NEXT publish, not by this file).

## round 83 — the cadence written last round could not be followed, and running it is what proved that

Round 82 closed the inventory's §5.8 gap by giving the seven device-targeted e2e sections a documented cadence. Round 83
FOLLOWED that cadence — the panel's wiring had changed twice — and the device answered:

```
Cannot find module 'D:\Summrise\agent\scripts\e2e\e2e.js'
```

**Because that is a REPOSITORY path, and an installed device has the PRODUCT, not the repo.** The script's own header says
what it needs — `node e2e.js --token <agent-token> [--base http://127.0.0.1:18080]` — and CI runs it against an agent it
launches itself, which is a third shape again. So the corrected cadence names the TRANSFER (the one `live-panel-probe.mjs`
already uses: emit to the CDN's public dir and let the device fetch it, or `system_file_download`) and the two arguments
the device supplies.

**THE LESSON IS THE ONE THIS FILE KEEPS RELEARNING, IN ITS PUREST FORM YET**: an instruction that reads as actionable and
is not is worse than no instruction, because the next reader spends a round discovering that. It took one command to find
out — *follow your own rule once* — and the rule had been written one round earlier by the same loop that then failed to
run it. **A cadence is a claim about the world, and claims about the world are testable.**


**Round 82, one line**: the inventory's last open instrument gap (§5.8 — seven e2e sections that CI cannot run because
they need a device) was closed the only way it can be: **a documented cadence in AGENTS.md**, naming the sections, the command
(`node agent/scripts/e2e/e2e.js --only <section>`), and the trigger — the paths whose change makes a section the thing to run.
A test nothing runs and nobody is told to run is a test that rots silently; the gap was never the missing CI job, it was the
missing sentence.


**Round 81, one line**: the full suite re-run after the panel rounds (73, 78 — the read module gained `keepEdits` and then
`read`, and `SettingsPage`/`ConnModal` migrated onto them): `bash scripts/test/all-gates.bash` → **57 ok, 0 failed, 1 not runnable
here (of 58)**, unchanged. Rounds that move a SEAM the rest of the panel reads through earn the whole suite, not the panel's own
tests alone — and the verdict is the same as before them.


**Round 80, one line**: **1.2.474 shipped** the panel read-seam change (device-affecting: the panel is embedded in the
exe) — tag `9150f240`, CI green before the tag, release.yml success, npm and the CDN updated, the device to follow. The debt
unchanged: 1.2.472 has no GitHub release asset, and the next publish is what names it.


**Round 79, one line**: the panel cell re-measured after round 78 (the `read` option and `ConnModal`'s migration; suite 890),
AND **1.2.474 IS OWED** — that round changed the panel, which is embedded in the exe the device serves, so it is device-affecting.
Stating it here is what got the previous three batches shipped (rounds 54+56 → 1.2.471, round 65 → 1.2.472, round 73 → 1.2.473).

## round 78 — the read module learns the shape it was missing, and `callTool` cannot be the one to use it

Round 73 measured the block precisely: the module states exactly ONE route (`callApi(route)`, a GET with no init and no body), which
is what keeps every `callTool` reader outside it. This round added the missing shape — `read?: () => Promise<unknown>`, replacing the
module's own fetch while the fold, `reason`, the cadence options and `keepEdits` behave exactly as before — and migrated `ConnModal`
onto it, the one site whose block was purely the transport.

**AND THE MIGRATION TURNED UP A CONTRACT FACT WORTH MORE THAN THE OPTION**: `callTool` CANNOT be handed over as `read`. The module's
refusal guard runs on whatever `read` RESOLVES, and `callTool` returns `result` — which has no `ok` key — so `deviceRefused` would
call every answer a refusal and the fold would never run. `ConnModal` therefore states its POST itself (the same bytes as before)
and unwraps inside its fold, exactly as its old `.then` did. **The lesson is about where a guard runs**: a seam that validates the
answer cannot accept a transport that has already unwrapped it, and the header now says so instead of implying that any door would
do.

**TWO DEVIATIONS, BOTH STATED RATHER THAN SMUGGLED**: `read` is typed `() => Promise<unknown>` because it replaces the FETCH, not the
fold (a `Promise<T>` would force an identity fold or a cast on every caller); and a refusal or malformed answer now KEEPS the last
list and reports `reason`, where the old code silently replaced it with an empty one. A kind switch re-reads through `refresh`
rather than `resetKey`, because a reset would blank the dropdown for one round trip.

**TESTS 882 → 890**: five for the module (including "read is used INSTEAD of the path — `callApi` is never called", a throwing
`read` reported through `reason` rather than as a silent empty, the refusal never being folded, and neither door failing closed)
and three for `ConnModal` (the same POST bytes, the kind filter, a pick pre-filling its values, and the old list surviving).
`tsc --noEmit` clean; the committed bundle rebuilt (deterministic — the same md5 across rebuilds).


**Round 77, one line**: the inventory's delete list (never executed) was walked, and its remaining items are blocked on §6.4 —
an OUT-OF-REPO client — not on engineering. The measurement was re-taken to be sure: `GET /api/events` and `/api/events/poll` really have
no in-repo client (the built panel's single `api/events` match is `api/events/term`, a different live route), so **deleting them is the
operator's call**, and the cell now says so with the distinction rather than a bare count.


**Round 75, one line**: **1.2.473 shipped** the panel read-seam change (device-affecting: the panel is embedded in the
exe) — tag `7accb877`, CI green before the tag, npm and the CDN both updated, the device to follow. This is also the first
release whose `release.yml` ran with round 67's SIGPIPE fix, so its log carries the `.data` symbol listing intact.


**Round 74, one line**: the panel cell re-measured after round 73 — 42,843 lines → the number above, with the committed
bundle at its new size and the panel suite at 882 tests; the other cells were re-run and are unchanged. A round whose output is a
measurement writes one line here rather than a section.

## round 73 — the read seam takes the shape it was missing, and the group that cannot move is now measured

Two groups were left outside `useDeviceRead` when it landed: readers that go through `callTool` (`useSessions`, the terminal pane), and
reads that SEED EDITABLE FORM STATE (`SettingsPage`, `ConnModal`). This round took the second, and turned the first from "deferred"
into a measured blocker:

- the module states exactly ONE route — `callApi(route)` in `refresh`, a GET with no init and no body — while `callTool` POSTs a JSON
  body and unwraps `result`, so `path` cannot express a tool read at all;
- even given a `read` function, `useSessions` folds one reply TWO ways (an event path that tombstones, a 30 s sweep that only adds)
  and retries a failure after 1.2 s — the module has one `reduce`, one cadence, no retry;
- the terminal pane's read is not a value read: it pages a cursor into xterm, so there is no `T` to hand over.

**AND THE FORM READ GOT THE OPTION IT NEEDED, NOT A HOOK BESIDE IT**: `keepEdits?: () => Partial<T> | undefined` — at settle time,
AFTER the fold, the caller's in-flight edits are merged back over the device's answer. **The round chose merge over "suppress the
write-back while dirty", and the reason is measured**: one settle carries FIVE independent fields, so withholding it would leave the
untouched ones showing the page's own defaults — the silent-write risk the settings fixture names. `SettingsPage` now reads through
the module (same single GET at mount, no cadence, same failure sentence), and one field deliberately stays a direct call because it
decides a write-side key wipe that an edit must not override.

**FIVE NEW TESTS, INCLUDING THE ONE THAT MAKES IT REAL**: type into a field, let a late read land, assert the typed value survives —
plus a field emptied to `""`, an untouched field taking the device's value, the race, edits read at settle time, `{}` meaning no
edits, and a throwing source reported as a failed read. Panel suite **877 → 882**, `tsc --noEmit` clean, `panel.js` regenerated
(it is embedded with `include_str!`, so the bundle is part of the diff). The module's header now carries the group-1 blockers with
LINE NUMBERS rather than the word "deferred", which is the difference between a queue and a decision.


**Round 72, one line**: the full suite re-run after rounds 68-71 (which touched `spawn.rs`) — `bash scripts/test/all-gates.bash`
→ **57 ok, 0 failed, 1 not runnable here (of 58 gate commands)**, the same totals as before them. Rounds that edit a GATE and the
scan behind it earn the whole suite rather than the checks next to them, and the verdict is unchanged.


**Round 71, one line**: the Rust cells re-measured after round 70 — `agent/src` and `spawn.rs` both grew (the fixtures for the
third cfg spelling), while `EXEMPT` stayed at zero; the subdirectory cells were re-run and say so. A round whose output is a
measurement writes one line here rather than a section.


**Round 70, one line**: the spawn scan now reads a THIRD dead-on-Windows spelling — `#[cfg(target_os = "linux")]` — with
three fixtures (passes unasked inside it, fails under `#[cfg(windows)]`, fails AFTER it so the region's end is respected) and two
mutations proving they bite. `#[cfg(not(any(unix, windows)))]` stays unread, and the reason is now written down with its
measurement: it IS dead on Windows, but it is false on the unix test box too, so no build here compiles such an item — reading it
would be pointless rather than wrong, and fail-closed already answers a spawn inside one with the rule's cheapest demand. EXEMPT
is still zero; spawn tests 15 passing.


**Round 69, one line**: the two commits that closed the dual-builder thread (the `grep|head` fix in `release.yml` and the
claim-correction) ran green in CI — **10/11 with no failures**, the 11th skipped by design — so the investigation ends with the tree
verified rather than merely pushed. The one debt is unchanged and named by the gate: 1.2.472 has no GitHub release asset.


**Round 68, one line**: end-of-session state verified rather than assumed — tree clean and `main == origin/main`; CDN **1.2.472**
and npm `latest` **1.2.472** (alpha still the historical 1.2.453); the device current; ledger 289 KB of 400 KB and AGENTS.md 20.8 KB of
48 KB. The ONE debt is 1.2.472's missing GitHub asset, and **it cannot be recovered without moving the tag**, which is the hazard the
reconcile ledger exists for — so the next publish is what will name it, not this file.


**Rounds 66-67, one line each**: 1.2.472 SHIPPED the CLI's last shell residual — tag `664c5a25`, release.yml success, the
CDN smoke green. The dual-builder audit still refuses (the known single-static divergence), and the NEXT release is the one whose
log carries the `.data` symbol list with the CGU hashes and the link order — the instrument rounds 61-64 set up.

## round 65 — the last shell residual, and the escape that protected nothing

Round 54 closed the CLI's shell door and left exactly ONE site carrying a `cmd-% residual` marker, capped at one. This round
converted it to argv and **the cap is now zero** — the pin refuses the token ANYWHERE in the file, so a marker parked off-site
is no longer invisible (the old check only looked at call sites).

**AND IT DELETED A `.replace(/"/g, '\\"')` THAT WAS BOTH DEAD AND CORRUPTING.** `\"` is not a cmd escape (cmd's is `^`), so it
protected nothing — and it ran over the WHOLE joined script, so it also hit the two double quotes inside the generated
PowerShell's single-quoted strings (the `.lnk` Arguments value): measured against the builder, exactly those two characters
changed. **WHAT LET IT SURVIVE IS FLAGGED IN THE CODE AS REASONED, NOT MEASURED** — node's CreateProcess quoting, cmd's
`/s /c` verbatim tail and powershell.exe's own `\"` unescaping compose to hide it, and there is no cmd.exe on this box to
confirm the chain end to end. Saying which half is measured is the difference between a comment and a claim.

**TWO MUTATIONS, VERBATIM**: turning one `literal` marker into a `residual` fails with "expected NO `cmd-% residual` marker —
the kind is CLOSED at zero"; reverting the site to the quoted `sh()` form fails with "the desktop-shortcut repair must reach
PowerShell as argv (`ps()`), not as a cmd string — if this fires, the site was reverted rather than the extractor being
broken", plus the cap refusal. CLI tests stay **64 passing**; the emit was rebuilt and `cmp`-identical to a fresh compile.

**AND THE COMMENTS THE CHANGE MADE FALSE WERE CORRECTED**: the helper's "callers must backslash-escape" doc, step 7's escape
line, and two test references to the old line number — a comment that contradicts the code is the one thing this repo treats
as a violation everywhere.

## round 59 — the shift hypothesis is dead, and my own framing was wrong

Rounds 34-35 read the divergence as "one region placed 304 bytes apart, with 1,168 stored offsets compensating". This round
tested the obvious consequence of that reading — if a region moved, the FILES should match when one is shifted — and the
reading is WRONG:

| alignment | sampled bytes that match |
|---|---|
| unshifted | **2,519,418 / 2,519,625 (99.99%)** |
| CI shifted by +304 | 9.35% |
| CI shifted by −304 | 9.39% |

**THE TWO IMAGES ARE IDENTICAL EXCEPT FOR 1,484 SCATTERED BYTES.** Nothing moved: 1,168 four-byte VALUES differ by exactly
`-0x130` while every other byte in a 17.6 MB file agrees — including the bytes at the addresses those values point to, which is
why round 35's "target content did not move" test found zero hits: there was no move to find. A constant delta in stored values
with no positional change is a table whose ENTRIES were recomputed, not a region that shifted — and the honest statement is that
the earlier sections said "displacement" where the measurement supports only "1,168 values, each 0x130 smaller in CI".

**AND TWO MORE STRUCTURAL READINGS WERE TESTED AND DIED THE SAME DAY**: if 1,168 values are each `0x130` smaller with no
positional change, the natural readings are "they reference a table whose records are 304 bytes apart" or "there is a 304-byte
stride in `.rdata`". Measured: **0 of 500 pairs** find their target content at the record `0x130` earlier, and a self-similarity
sweep over a 512 KB `.rdata` window at strides 304/608/152/8/16/24/32 finds nothing above noise (5, 3, 3, 9, 12, 22, 11 eight-byte
blocks out of 128). So the values have **no local structure at that stride** — they are simply values that differ, and the
empirical statement stays exactly as round 59 left it: **1,168 four-byte values in `.rdata`, each `0x130` smaller in CI, every
other byte of a 17.6 MB image identical.**

### THE ANSWER: ONE STATIC, `__rust_panic_type_info`, AND 1,168 REFERENCES TO IT (round 61)

Round 60 left the statement "1,168 four-byte values in `.rdata`, each `0x130` smaller in CI, everything else identical". This
round asked what those values ARE, and the answer took three measurements that each shrank the question:

1. **They are all the SAME value pair.** Grouping the 1,168 differing 4-byte values by `(local, CI)` yields **exactly ONE
   distinct pair** — local `0x102d130`, CI `0x102d000`, repeated 1,168 times. Not 1,168 facts; **one fact, 1,168 times**.
2. **`0x102d000` IS `.data`'s start address** (the section table has `.data` at `0x102d000`, and its size and address are
   identical in both builds). So the values are POINTERS TO A STATIC IN `.data`, and that static sits at **offset `0x130` in this
   box's build and offset `0` in CI's**.
3. **The map names it.** `grep "0003:" /tmp/agent-map.txt` (the local map from round 22, section `0003` = `.data`) lists
   `0003:00000130  _RNvCs1njKG4L9aB3_7___rustc22___rust_panic_type_info` — **`rustc::__rust_panic_type_info`, a Rust runtime
   static**, emitted from `summrise_agent…cgu.13.rcgu.o`.

**SO THE DIVERGENCE IS ONE SYMBOL'S POSITION WITHIN `.data`, AND NOTHING ELSE.** The linker lays out `.data` contributions by
object file, and the map shows what precedes it in THIS build: a run of `___CALLSITE` panic-location records from the agent's own
codegen units (`cgu.08`, `cgu.13`), 24 bytes apart, starting at `0003:00000028`. In CI's build that block is not ahead of this
static, so it lands at offset 0 instead of `0x130` — and every reference to it moves by the same 304 bytes, which is exactly the
1,168 values this investigation has been chasing since round 34.

**AND THE COMPARISON TARGET IS NOW WRITTEN DOWN (round 63)**, because the next step needs two specific strings rather than
another hypothesis. Measured on this box with `cargo xwin build -v`: the BIN crate is partitioned into **16 codegen units whose
object files are named `summrise_agent.45d6daa740c90b90-cgu.00 … -cgu.15`** (the `45d6daa740c90b90` is the crate hash the
compiler derives), the LIB crate has its own `summrise_agent.84e17ef914540501-cgu.*` set, and **the `.data` contributions that
precede `__rust_panic_type_info` in this build come from the BIN crate's `cgu.08` and `cgu.13`** — i.e. the binary's own object
files are laid down before that static here. **The next release's log needs TWO things to settle it**: the CGU hash in those
object names, and the ORDER the linker received them in (`-v` on the build step, or the map's per-object list with
`/MAPINFO`). If the hashes match and the order differs, the difference is in how cargo enumerated the objects; if the HASHES
differ, one side compiled a different partitioning of the same source — which is a fact about the compiler, not about the code.

**AND THE FIRST CANDIDATE FOR "WHY THE ORDER DIFFERS" IS ALREADY DEAD (round 62)**: CI's rust-toolchain action sets
`CARGO_INCREMENTAL=0` explicitly (its step is even named "Disable incremental compilation"), while this box leaves the variable
UNSET — a difference in the build environment that no earlier round had checked. Tested the only honest way, by building the
SAME version twice and comparing MY OWN two builds: `CARGO_INCREMENTAL` unset vs `=0` produce **0 differing bytes**, which is
what cargo's own rule predicts (a release profile is non-incremental unless the variable overrides it). So the variable is not
the cause, and the first version of this test was WRONG in a way worth recording: it compared a 1.2.471 build against CI's
1.2.470 asset and read 634,076 differing bytes — a number about the VERSION, not about the variable. **A cross-version
comparison is not a measurement of a build flag**; the same-version pair is, and it was one command away.

**AND THE INSTRUMENT'S SECOND CLEAN RUN CONFIRMS IT AS A STABLE PROPERTY (round 76)**, which matters because a
one-off measurement explains an incident and a repeated one explains a build. 1.2.473's release log — the first whose
`release.yml` completed the `.data` listing, after round 67 fixed the SIGPIPE that killed 1.2.472's — prints:

```
CI 1.2.473   .data+0x000  __rust_panic_type_info  ...  45d6daa740c90b90-cgu.00.rcgu.o
this box     .data+0x130  __rust_panic_type_info  ...  45d6daa740c90b90-cgu.13.rcgu.o
```

**Same crate hash on both sides, same symbol, a different codegen unit — and the same answer two releases running.** So it
is not flake and not a one-round artefact: each build environment assigns this runtime item to a different unit, and the
1,168 values that differ between the two artifacts are nothing but the references to where it landed. The audit will keep
refusing for as long as that is true, and it should: **the CDN is authoritative for devices, and a builder-vs-builder
disagreement that is fully understood is still a disagreement.**

### THE ANSWER, COMPLETE: THE SAME CGU HASH, A DIFFERENT CODEGEN UNIT (round 67)

The instrument rounds 61-64 placed printed CI's `.data` symbol list during the 1.2.472 release — and the log carried the
answer even though that same step killed the release (below):

```
CI     .data+0x000  _RNvCs1njKG4L9aB3_7___rustc22___rust_panic_type_info  ...  -cgu.00.rcgu.o
this   .data+0x130  _RNvCs1njKG4L9aB3_7___rustc22___rust_panic_type_info  ...  -cgu.13.rcgu.o
```

**The crate hash is IDENTICAL on both sides (`45d6daa740c90b90`), so the partitioning into sixteen units is the same — but
`__rust_panic_type_info` is emitted into `cgu.00` on the runner and `cgu.13` here.** That single membership difference is the
whole divergence: `.data` contributions are laid out by object file, so a symbol in `cgu.00` lands before the panic-location
CALLSITEs while one in `cgu.13` lands after them — 304 bytes apart — and all 1,168 references follow it. Nothing about the
source, the toolchain, the flags or the environment differs; **the compiler assigned one item to a different codegen unit.**

**AND THE INSTRUMENT PAID FOR ITSELF BY BREAKING THE RELEASE, WHICH IS RECORDED HERE RATHER THAN FIXED QUIETLY**: the step I
added in round 64 ended `grep -E "^ +0003:" /tmp/agent-map.txt | head -40`, and under `set -euo pipefail` a `head` that
closes the pipe gives `grep` a SIGPIPE — `write error: Broken pipe`, exit 2 — so **the `Cross-compile the agent exe` step
failed and 1.2.472 has no GitHub release asset**, while its CDN and npm halves shipped and the device is unaffected. The fix
is in the workflow now (write to a file, then read from it) and the lesson is general: **a diagnostic must not be able to
fail the build it is describing.** **AND THE DEBT IS NOT YET IN THE RECONCILE FILE, WHICH THIS SECTION SAID IT WAS** — corrected here rather than
left as a claim: `docs/agents/release-reconcile.txt` still holds only `1.2.453`, because that file is written by
the publish path's reconcile step and `--acknowledge-unreconciled` did not append to it on this run. **The gate is
what makes this safe**: the NEXT publish refuses while a CDN version has no GitHub release to audit against, so
1.2.472 will be named by the tool rather than remembered by a reader. Moving the tag was never an option — a moved
tag is the hazard that ledger exists for.

**WHAT THIS DOES AND DOES NOT EXPLAIN**: it explains the whole divergence — there is nothing else different in the image — and it
converts "the two builders disagree by 1,484 bytes" into "**the two builders place one Rust runtime static 304 bytes apart within
`.data`**", which is a property of the `/Brepro`-reproducible link whose *cause* is the ordering of codegen-unit contributions
ahead of it. It does NOT yet say WHY the order differs between the runner and this box; the instrument for that is the object
list the linker received (`/MAPINFO` or a verbose link), which is the same two-line change the earlier rounds specified — but it
is now a question about ONE object file's position rather than about an unnamed byte range.

**THE INSTRUMENT THAT WOULD NAME IT IS UNCHANGED AND UNUSED**: the map's per-object contribution list (`/MAPINFO`), which the
segment table cannot show because it lists `$`-named segments rather than the anonymous `.rdata` contributions inside them. That
is a two-line change to `release.yml` and a diff of one release log — and after this round it is the ONLY remaining step, because
the positional hypothesis it was going to be used to confirm is now excluded.


**Rounds 56-58, one line each**: round 56 closed the panel ratchet's gap (command facts) and round 57 re-measured the
cells it moved; round 58 SHIPPED both device-affecting rounds as **1.2.471** — tag `ca956aa0`, release.yml success, the CDN
smoke green, and the device reporting `release: 1.2.471 · this CLI: 1.2.471 · this device is current`. The dual-builder audit
still refuses (CI `74e29766…` vs this box) — the one open thread, whose next instrument is the map's `/MAPINFO` per-object list.

## round 56 — the ratchet had a gap, and one site was passing through it

The inventory measured this in §5.4 and it stayed open: `sessionFacts.test.ts` is a ratchet that forbids a component from reading
four SESSION fields directly, and **command facts were not on the list** — `exitCode`, `reason`, `ended`. So an inline
`row.exitCode === 0 ? …` walked past the guard whose whole purpose is to keep ONE owner for "how did this command end".

**WHAT IT NOW FORBIDS**, in the ratchet's own idiom (one map, one exemption list, no new mechanism): a `.tsx` that COMPARES a
command fact against a value — `exitCode`, `exit_code`, `reason`, `ended`, `lastExitCode`. **Presence tests and printing stay
legal**, because the file already argues that a blanket ban "would forbid display to catch derivation"; the two extra keys are the
same question under its other names, and all five hit zero sites today.

**ONE SITE WAS FLAGGED**: `ActivityPage.tsx:98`'s inline `row.exitCode === 0 ? "zero" : "nonzero"`, now
`stateFromEnd(true, row.exitCode, null).state === "ok" ? …`. The output is IDENTICAL — under the `!= null` guard above it, the
exit-code branch is the only reachable one — and two existing ActivityPage tests already pinned both outcomes, so the change is
covered rather than assumed. **And defect 5.1's private deriver is NOT reachable any more**: `TrajectoryView`'s `roundState` and
`eventDotState` both call `cardState`/`stateFromEnd`, which is what the earlier round recorded as fixed.

**BOTH HALVES PROVEN, VERBATIM FROM THE RATCHET**: a planted `group.rows[0]?.exitCode === 0` in `RunStrip.tsx` fails with
"…That mapping has ONE owner (stateFromEnd in lib/path.ts), and a second copy is how one backgrounded command came to wear `bg`
in its round marker and `warn` in its own event row", naming the file, the line and the fix; a planted
`card.reason === "backgrounded"` fails on `reason`. Both reverted. The panel suite is **877 tests, 110 files, all passing**, and
`tsc --noEmit` is clean.

**AND THE ROUND HAD TO REMEMBER THE BUNDLE**: `panel.js` is committed and embedded with `include_str!`, so the source change
required `npm run build` before the commit — the built artifact is part of the diff, not a by-product, and `build.rs` gates the
build on its freshness. The ratchet also carries what it CANNOT see, in its own header: text rather than code (`const c =
row.exitCode; c === 0`), `.ts` files (where the owners live), and the scope of its scan.


**Round 55, one line**: the CLI cells re-measured after the round-54 shell work — `summrise.ts` 3,790 → **3,880** lines and
`cli.test.mjs` 2,617 → **2,891** (eight argv migrations, the comments that record the measurement, and a pin that checks quoting
is not enough); `agent/src` re-run and unchanged at 54,823. A count that did not move is evidence too.

## round 54 — a quoted value is not a safe value, and my suggested fix was wrong

Round 32 fixed the CLI's shell door and named what it left open: cmd expands `%NAME%` **even inside double quotes**, so a
path interpolation can be re-parsed as a variable reference. This round closed it — and the fix I proposed in the brief
was **disproved before it was applied**: `%%` does NOT collapse on a `cmd /d /s /c "…"` line (that is a batch-FILE rule),
so escaping by doubling would have put a DOUBLED sign into a real path. Measured on the device, through the exact
`spawnSync(cmd, { shell: true })` path: `echo [100%%]` → `[100%%]`; `echo "C:\%ProgramFiles%\Summrise"` →
`C:\C:\Program Files\Summrise`; argv → `%ProgramFiles%` untouched; and an undefined `%NAME%` passes through as text.

**WHAT THE SITES NEEDED INSTEAD WAS argv**, in eight places, including two `rmdir /s /q "${DIR}"` calls that had to become
`Remove-Item -LiteralPath` because rmdir is a cmd BUILTIN with no executable to point argv at. Where the shell genuinely
cannot go — npm is a `.cmd` batch shim and re-parses its own arguments — the value moved into the ENVIRONMENT and the
command line references `%SUMMRISE_NPM_PREFIX%`, which was measured through npm.cmd itself. Two categories stay with a
checked reason (a literal, a semver re-read from package.json), and exactly ONE residual is allowed and capped.

**THE PIN NOW KNOWS THAT QUOTING IS NOT ENOUGH**: every interpolating `sh()`/shell site must be argv or carry a
`cmd-% <kind>: reason` comment whose kind is CHECKED against the value, `%%` in cmd text is rejected, and every
`%NAME%` must be defined in that call's `env`. It bites where round 32's pin passed: reverting one site to the quoted
form fails with "cmd expands %NAME% in a quoted value too (measured on d1)". CLI tests 62 → **64**; the emit was rebuilt
and `cmp`-verified against a fresh compile.

**AND THE ROUND'S REAL LESSON IS ABOUT BRIEFS, NOT SHELLS**: a fix proposed from the outside, however plausible, is a
hypothesis until the mechanism is measured — and this one was measured on the machine whose cmd.exe actually runs it.


**Verified after the three gate edits of rounds 49-52** (the ledger split, the two check patches and the PowerShell
scanner extension): `bash scripts/test/all-gates.bash` → **57 ok, 0 failed, 1 not runnable here (of 58 gate commands)**, the
same totals as before them — editing a gate is editing the thing that guards everything else, so it is the one change
that gets the whole suite run rather than the checks next to it. One line, not a section, per the policy above.

