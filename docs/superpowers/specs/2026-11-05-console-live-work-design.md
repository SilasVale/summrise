# The console as a place to watch work

**Status:** proposed, 2026-11-05. The timeline slice shipped as the day-grouped timeline; the rest is
still a plan the operator approves before it becomes work.

## What is wrong today

The console has the right facts and the wrong subject. Three surfaces express one thing, and none of
them is shaped like "work is happening":

- **Home leads with an endpoint.** The first thing on the hub is the URL an AI client connects to,
  with a Copy button. That is a setup task, not the answer to "what is this machine doing?".
- **Activity is an audit table.** Filters, a token column, CSV export, one row per call. It is an
  administrator's index; reading it, a person cannot tell one stretch of work from the next.
- **The terminal page is a black rectangle.** Tabs, Find, "+ New session", then `PS C:\…>`. It never
  says which machine, which shell, which session, how long it has been open, or what the last
  command was — the things that make a terminal *this* terminal.
- **The live band is bolted on.** It sits on the configuration page, above the endpoint, and the
  page below it does not change while the AI works.

The live data is already there: every console-routed call is audited, carries a human sentence, and
is streamed on `/events`. The problem is the shape drawn around it.

## The idea

**One event stream, three renderings by zoom. Not three features.**

| zoom | surface | answers | exists today |
|---|---|---|---|
| glance | the machine's Now band | is it working, how fast, what last? | a five-row band |
| scan | the timeline | what did it do, in what order? | an audit table |
| drill | the session | what is it doing in there right now? | a raw terminal |

**There is no fleet board.** The product's model is "choose a machine, then everything is about
it" — the toolbar's picker is that choice — and a board of every machine at once would be a second
navigation for a three-machine fleet. Every zoom lands on the selected machine.

An **action** is one event (a call, its outcome, its sentence, its duration), drawn as one row. A
**stretch** is consecutive actions on one machine, close in time — one stretch of work, separated
from the next by a 60-second gap or a different machine. A **session** is the terminal the actions
are typing into. The page's subject is always the selected machine.

Rules:

1. **One subject per surface.** The timeline's subject is the work; the page's is the machine; the
   session's is the shell. Anything that is not that subject moves to a sheet or a secondary view.
2. **Every visual is a fact.** A pulse means an event arrived in the last ninety seconds. No event,
   no pulse; the page says "quiet" and how long. Nothing animates to fill space.
3. **Grouping is derived, never stored.** Days group by the console's local date and a stretch is
   computed from the audit's own timestamps and device ids. No new file, no new table, no migration.
4. **The cube is a renderer, not a feature.** One machine's body, drawn once, is a place a cube
   could live; it is not built first, and it is not the product's spine.

## What moves

- **Home becomes the machine's live page.** For the selected machine: the Now band (presence, rate,
  the last actions), the timeline of its stretches, and a session preview — the shell, the last
  command and the last lines, with `Open the terminal` to open it. The endpoint card leaves the hub:
  it already exists in the Connect sheet, and a hub that leads with setup is a hub about setup.
- **Activity becomes the timeline.** One row per action, grouped by day, newest first; a 60-second
  gap or a different machine starts a new stretch. The table stays, behind a `Table` toggle, because
  filters and CSV are genuinely useful — they are just not the default reading.
- **Device → Terminal becomes the session.** A card first: machine, shell, started, last command, the
  last few lines; then the terminal itself. Attaching to the newest session stays automatic.
- **Controls:** Live/Pause stays on the timeline; filters become chips (machine) rather than selects.

## Non-goals

- **No AI reasoning events.** The console sees actions. A "decision" event would have to be published
  by the AI client; nothing here invents one.
- **No 3D yet.** The cube is deferred until the machine page exists and is honest; building it first would be
  decoration.
- **No new storage or routes beyond what exists** (`/events`, `/audit`). The stream is a rendering of
  the audit, not a second history.
- **No device release is required for slices 1–3.** Everything here is console-side. (Calls made to a
  device *without* the console in the path remain invisible; that is the device-side activity push,
  a separate design.)
- **No mobile redesign in this plan.**

## Slices

Each slice is shippable and verified on its own; the order is deliberate because each one is the
data the next one stands on.

1. **The timeline** (scan). One row per action, grouped by day, with a 60-second gap or a different
   machine separating stretches; Table behind a toggle.
   *Shipped:* `ActivityTimeline.tsx`, `groupDays`, the toggle in `App.tsx`.
2. **The machine page** (glance). Home's hub becomes the selected machine's live page — Now, timeline,
   session preview — and the endpoint card moves to the Connect sheet alone.
3. **The session** (drill). Session card over the terminal, in the same card language as a timeline row.
4. **The cube** (a renderer for the selected machine), only if the page feels right without it.

## Verification, per slice

- `npm test`, `npm run typecheck`, `scripts/build-web.sh --check`, the design-token gate
  (`cargo test -p summrise-console --test tokens`), and the browser gate.
- A live check on `console.saisi.online`: fire real calls through `/mcp` and read the surface back in
  a real browser; screenshots kept.
- The **idle check**: with no calls for two minutes, the surface must say so and must not pulse.

## Risks and their answers

- **Losing the table's power.** It stays, one toggle away, unfiltered and unchanged.
- **Self-pollution.** The page's own catalogue reads (`tools/list`, `plugins.list`) are real calls and
  are shown, but they are excluded from *presence and rate*: a page polling itself must not make every
  night look busy. This is a rule on the page, not a filter on the data.
- **One machine at a time.** A fleet-wide glance is deliberately not built: the picker is the model,
  and a compact presence dot beside each machine in the picker is enough if it ever is not.
- **A stretch is a guess.** Sixty seconds is the gap; the grouping is derived and shown as "one
  stretch", never as a claim about intent.

## What the operator decides

1. The order: **timeline → machine page → session → cube**.
2. That the endpoint leaves the hub for the Connect sheet.
3. Whether the cube is wanted at all once the machine page exists.
