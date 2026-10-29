# The console, measured: what to delete and what to fix

> **Status: two rounds of fixes landed, the rest is a plan for the owner, 2026-10-28.** Three
> read-only audits produced this: information architecture, visual consistency, and whether the
> product's documents still tell the truth. Line numbers were taken at `edc28a8`.

## What the page actually offered

- **Ten of the eleven rail icons led back to the page the reader was on.** A device page's href is
  built from a device id, and on the fleet there is none, so every one of them fell back to `/next`.
- **A fleet row printed eleven page links** — `Terminal · Files · Runs · System · State · Memory ·
  Screen · Monitors · Browser · Connect · Vitals` — so the four links that answer a daily job looked
  exactly like the seven that do not. With three devices the table carried 33 links.
- The device's own name was not a link; the only ways into a machine were those eleven links and
  `Manage`.
- One link (`State`) was not even a rail page; it renders the state document and marks the rail's
  `System` as current.

## What this round changed

| change | why | evidence |
|---|---|---|
| the rail shows only Fleet until a device is chosen | ten dead links removed; progressive disclosure (PRODUCT.md rule 5) | `rail()` in `device/console/src/http/page.rs`; the test pins 1 anchor on the fleet and 11 on a device page |
| a fleet row shows `Terminal · Screen · Files · Connect · More` | the four daily destinations stay; the other seven live in a `details` disclosure and stay ordinary anchors | same file, the row's `page-links` nav; the test asserts all seven are still in the markup |
| deleted the Files rail's `Up one level` | it built `/next/files/{id}{parent}` — no `?path=`, no separator — and 404ed on every nested directory; the card-head control is correct and stays | the card-head control at the same file |
| deleted the terminal's `Close all` | it was an `<a>` to the page itself: it reloaded and left every session running | the test now asserts it is gone |
| `--ok-soft` no longer references itself | `--ok-soft:var(--ok-soft)` computed to nothing, so the light "online" chip lost its fill; no contrast pair could see a transparent pair | new test: no token references itself |
| dark `--warn-soft` exists | the dark warn chip inherited the light cream and measured **2.02:1** | token block; the contrast test now names `warn` on `warn-soft` |
| `--faint` moved `#74757f` → `#696a74` | it measured **4.23:1** on the canvas and **4.16:1** on the context rail, at 10.5–11.5px | the contrast test now names `faint` on `bg` and on `panel-2` |
| input focus ring is the accent, not `accent-soft` | the soft ring measured **1.11:1** on the panel — an outline nobody can see | new assertion in the same test |
| `--hover` is declared | two rules referenced a token that did not exist, so the hover computed to nothing | token block |
| the terminal empty state uses tokens | it hard-coded `#a2a6b0` (**2.44:1**) and `#ffa36b` (**1.96:1** as a graphic) | same file, `.terminal-empty` |

The measured gate went from 10 colour pairs to 14, and from "no pair may fail" to "a token may not
reference itself". The screenshots moved exactly where expected: every page by 0.03–0.56% (the faint
ink), the dashboard pages by more (the rail and the row). The baseline was updated after review.

## The second round: the scales

| change | why | gate |
|---|---|---|
| fourteen font sizes → eight (9, 11, 12, 13, 14, 15, 18, 22) | 10/10.5 and 11.5/12 and 12.5/13 were the same role drawn three ways; the two `font:` shorthands became longhands so the scale is readable | a test parses every `font-size:` and fails off-scale |
| seven radius literals → tokens | 5, 6, 7 and 8px drew the same kind of small control | a test admits only `var(--radius*)`, `999px`, `50%` |
| the card family snapped to 4/8/12/16/24/32 | card heads, stats, alerts, code blocks, fields, table cells and breadcrumbs each had their own step | reviewed in the 29 screenshots at 1280 and 420 |
| a filter's Apply is no longer the filled button | System's Filter and Memory's Search were plain, so the same job looked primary on one page and secondary on two | written down beside the rule |
| the audit filter stacks in two columns below 720px | the global `select{width:100%}` met a shrinking flex item and every select collapsed to its caret at 390px | the 420px screenshot |

## The copy audit: what the console said that was not true

A second read of every explanatory sentence against the code found ten more. The three that
mattered, all fixed here:

- **"read-only"** described a token with `allow_destructive` unset. It means "no destructive tools":
  without that consent the token still calls `files.mkdir` and `terminal.close`. The chip now says
  what it means.
- **"until the launcher brings it back"** promised recovery on a page whose Restart control is used
  exactly when a device is wedged — and the launcher gives up after three consecutive restarts. Both
  the menu and the post-restart notice now say so.
- **"the device ended {killed} process(es)"** read `killed` as a number; the device answers with an
  array of pids, so a successful kill always rendered "ended 0". The fixture faked the number shape,
  which is why the test passed.

Also corrected: the browser runs headless unless `SUMMRISE_BROWSER_VISIBLE` is set (the page said
"in the user's session when the helper is there"); the ninety-second hold belongs to the console,
not the device (the device refuses immediately); a minted token's scope is this form's choice, not
"every tool"; the denials log caps at a hundred a minute; "signed in with the operator token" is
really "this browser's session"; a held call also disappears when the client stops waiting.

Also closed: the four empty states that said "nothing there" when the device had not answered
(terminal sessions, runs, memory, processes) now carry the failure into the renderer and suppress
the empty sentence; a filtered log with no matches says "No line matches this filter." A refusal or
a kill result still wins over the list failure, because the operator's own message is the news.

## What the audits found and this round did NOT do

Ranked by what an operator loses, not by what is cheapest.

| candidate | cost | risk | who decides |
|---|---|---|---|
| Fleet row leads to **one device**, not a list of tools | one click to a tool | markup + tests + smoke; the name is still a `strong` | owner: which destination (a device home vs Terminal) |
| merge `State` into `System` | one click through System | redirect + two tests; the content has a job (poisoned state) | owner: the page was deliberate (`16a0378`) |
| one **Records** page for Audit and Denials | one extra click to switch | two stores, two filters; medium | owner: a security surface |
| move System's ad-hoc reachability card to **Monitors** | none | markup; the route stays | owner, one line |
| merge `Screen` and `Browser` | browser tools one tab deeper | high: two routes, JS, tests | owner: one job or two |
| delete the Vitals trend card | lose a visit-only sparkline | markup + dead store | owner: visible feature, weak job |
| one name per thing (`Tokens`/`Connect`, `System`/`Processes`), stop printing the build twice | none | strings + CSS | owner picks the word |
| Files page: delete the "Read a file" card | — | markup | **not taken**: it is the only way to read a path that is not in the listing, and Write has the same field. Deleting it would remove a capability, not a duplicate |
| Denials: hide the 11 impossible kind options and use its own lead | none | one call-site argument | safe next round |

## What must not be cut

The fleet and enrollment (the only way a device enters the registry), the approvals card and badge
(the human half of the brake), Audit and Denials (merge the page if you like; never the stores),
Tokens (the only credential mint/revoke surface), restart/kill and the install journal (the only
remote recovery controls), rename/remove, and the device-local panel (it is a different surface and
must keep working when the console does not).

## The documents were telling three lies

Found by the same review, fixed in this round:

1. The page said grants are "revoked in one click". A grant is valid until it expires; only tokens
   and device removal are one-click revocations.
2. The page said destructive tools "are refused unless the token allows them". The device answers
   `Ask`; the console holds the call for ninety seconds and refuses it if nobody answers. The same
   sentence was in `QUICKSTART.md` and `CONNECT-MCP.md`.
3. The page put the panel at `127.0.0.1:18090` and described State/Repair as controls the panel does
   not have. The default install is `18080`; the panel shows sessions, install, verdict and channel
   health, and it cannot start, stop or update anything.

Also fixed: the README's verify step used 18090; its `--data` default was the wrong store for an
installed binary (the service reads `%ProgramData%\Summrise`); and two references pointed at
`panel.html`, which no longer exists.

## The layout gate (2026-10-28)

The screenshots are read by an eye, and an eye missed four pages whose cards were wider than a
phone: the memory filter's two columns, the system page's filter form and reachability row, and the
vitals install card's sha256. Each showed a horizontal scrollbar inside its own card, which reads as
"nothing here" until you look for it.

`node scripts/layout-gate.mjs <pages-dir> 390 800 1280` renders nothing itself — it drives headless
Chrome over the pages `scripts/console-screenshots.bash` wrote and measures three things per page:

- the document does not scroll sideways;
- no element with `overflow:auto|scroll` has more scrollWidth than clientWidth (a card that
  overflows raises that scrollbar; a table that scrolls by design is not an overflow);
- no visible element is painted past its nearest clipping box (content that is cut, not scrollable).

It also asserts one `<h1>` per page.

The width floor is 360 (a common Android width); 320 was measured too, and one page fails there by six
pixels — the fleet table's first cell is wider than the table at that width, which the dense-table
scroll rule then hides behind a scrollbar. That is below the floor the product claims, and it is
recorded here rather than left as a surprise. Mutation: a 900px div in a dashboard card body →
`FAIL dashboard.html @390: panes=section +586`, exit 1; the unmutated set exits 0 with
`30 pages x 390/800/1280 -> 0 failure(s)`.

## What the review proposed, and what shipped (2026-10-28, later)

The review's navigational recommendations were three merges. All three shipped, each keeping its old
addresses alive:

| proposed | shipped | old URLs |
| --- | --- | --- |
| Audit + Denials as one page | one **Records** page, `Actions` / `Refusals` segments; the fleet footer links it once | both routes render the same page |
| State as part of System | the state document is a card under the System page's actions | `/next/state/…` redirects to `/next/system/…` |
| Screen + Browser as one surface | one **Screen** page, `Desktop` / `Browser` segments; the rail lists ten destinations | `/next/browser/…` renders on its own |

Two things the review did not ask for and the work found:

- **A refused action is a page.** Reading, writing, running, killing, saving, watching and browser
  actions used to answer a device refusal as `{"error":"…"}`. They render the sentence, the rail and
  the way back now, under the status the device's failure carried (a 502). The terminal's five calls
  stay JSON on purpose — their client draws the error in the terminal.
- **An empty list is a claim.** Files, Runs, Monitors and Browser said "none" whenever their list was
  empty, including when the device had not answered. With an error they say what is unknown; the
  "none" sentence is kept for a device that answered.

Coverage at the end of it: 120 unit + 255 integration tests, clippy over nine crates, and the layout
gate measuring 31 console pages (plus the panel's two and the landing page) at 360/390/800/1280 in CI.
