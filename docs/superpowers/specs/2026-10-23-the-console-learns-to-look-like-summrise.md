# The console learns to look like Summrise

*A record of the rounds that turned the operator surface from a hand-written page into something with a
design, and — more useful — the list of defects that looking at it found.*

## Why the page moved to React

Not because React is better, but because the hand-written page had reached the point where its failures
were invisible:

* **1,400 lines of HTML and inline JavaScript**, no components: every button, chip and card was a copy;
* **no type check**: a deleted function was a dead button, discovered only by a browser;
* **no build**: nothing could catch a stale artifact.

The constraint that had to survive the move: the page is embedded in the console binary with
`include_str!`, so the runtime stays one executable with no Node near it. That is why the build produces
**one self-contained index.html** (581 KB, 162 KB gzip, logo inlined) rather than a directory of assets.

React is a **build** dependency. `node_modules` never ships.

## What the migration actually cost, in defects found by looking

Every one of these was found by rendering the page and looking at it, not by a test:

| Defect | How it surfaced | Fix |
|---|---|---|
| **Seven functions deleted, call sites kept** | Playwright: `PAGEERROR buildToolList is not defined` — the script threw where the login handler was registered, so every binding after it was dead | restored; a test now checks every bound handler is defined (mutation: renaming `connect` turns it red) |
| **A grant sent without its signature** | the device refused everything with `unknown_tool`-shaped noise | send the whole envelope; stringify JSON-RPC errors instead of `[object Object]` |
| **The page carried its own tool list** and offered `files.delete`, which the registry never declared | "select all" minted a grant the console refused | `GET /tools` serves the registry; a test fails if they diverge |
| **Two 401s on every cold load** | the smoke check failed on console errors | ask `/session` first, fetch data only when something answered |
| **The status bar's version was typed into the page** | it said 0.3.17 for a day after 0.3.18 shipped | comes from `/health`; the build injects the console's version |
| **"Grant access" stretched the full panel width** | dark-mode screenshot | a button in a grid panel is a control: `justify-self: start` |
| **The active rail button was a light square in dark mode** | dark-mode screenshot | `--accent-soft` is a *surface*; it needed a dark value |
| **The terminal was a different black from the page** | dark-mode screenshot | xterm's palette follows the page's surface and ink |
| **The diagnostics button was unreachable** | Playwright timed out: opening a device removes the workbench that held it | moved onto the device card |
| **The smoke check called reconnecting devices broken** | failed right after a deploy | it waits for the channels, up to 90 s |

The pattern is consistent: **the repository was green while the product was broken**. Every fix above is
paired with something that would fail next time — a test, a check, or a rule in a document.

## The rules the surfaces now follow

1. **Every number has a source.** The console's version is injected at build time, `/health` reports it,
   devices announce their own, `/tools` serves the catalogue. Each replaced a literal that had drifted.
2. **Progressive disclosure.** The console shows devices and the audit; the grant, terminal and browser
   appear only after a device is chosen. A screen that shows everything says nothing at a glance.
3. **Say what you compared.** A device behind the newest on the list gets an amber chip whose tooltip
   reads "another device in this list runs 0.3.23", not "outdated" — the console cannot know the latter.
4. **"offline" is not a diagnosis.** The device reports how its previous attempt ended on the next
   connect; both surfaces say the same sentence about it.
5. **Verify in both schemes and two widths.** Every surface was captured in light and dark, at 1300 px and
   at 420 px, before it was called done.
6. **A control you cannot reach is not a feature.** Anything that acts on a device lives where the device
   is visible.

## The verification matrix, as it stands

Every cell was captured from the deployed product, not from a local build.

| surface | light 1300 px | dark 1300 px | light 420 px | dark 420 px |
|---|---|---|---|---|
| console — devices | ✓ | ✓ | ✓ (overflow 0) | ✓ (overflow 0) |
| console — grant | ✓ | ✓ | ✓ | ✓ |
| console — terminal | ✓ | ✓ | — | — |
| console — browser pane | ✓ (example.com, 764×485 capture) | — | — | — |
| device panel | ✓ | ✓ | ✓ (overflow 0) | ✓ (overflow 0) |

## What is still not verified

* **The browser pane in dark mode** — it was captured in light only. The pane shows an image, so the theme
  mostly governs its frame, but "mostly" is not "measured".
* **The terminal at 420 px** — the pane is 22 rem tall and fixed; a phone-sized window would give it a
  keyboard and almost no room. Nothing here has been tried on a touch device.
* **Two operators in one browser** — two sessions coexist on the console (measured: two `POST /auth`
  calls return distinct tokens, both answer `/devices` 200, and the first still answers after the second
  signs in), but both were driven by the same machine and the same person. What two humans doing two
  things at once does to the audit and the device cards has not been watched.
