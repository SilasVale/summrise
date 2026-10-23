# Two surfaces, one product

*What the console and the device panel are each for, what they borrow from the panel that came before,
and what is deliberately not shared.*

## The surfaces, again

`CONTEXT.md` draws the line: the **console** answers "how are my devices", the **panel** answers "what is
this machine doing right now". The rewrite kept the console and, for a while, dropped the panel — which
was a scope change dressed as an implementation detail. Both exist now, and the reason is not symmetry:

| | console | panel |
|---|---|---|
| Where | `https://console.saisi.online` (Cloudflare Access in front) | `http://127.0.0.1:18090` (loopback only) |
| Reaches the device | over the channel the device dialled out | in the same process |
| Answers when the other end is down | no — it *is* the other end | yes, which is the point |
| Login | Access identity, or the operator token | none: being able to open it means being on the box |

The panel is not a smaller console. It is the surface that still works when the console does not, and it
is the only one that can say *why* a channel is down, because a device cannot report a failure over the
connection that failed.

## What was borrowed, and from where

The previous panel was designed over months and its decisions were written down next to the CSS. Those
are the parts worth keeping:

* **`tokens.css` is the single source of truth.** The brand is the sunrise amber, and the console's
  accent is `#bf3a0a` rather than the brighter `#d9480f` because a contrast measurement in the legacy
  console demanded both surfaces agree. The dark block **redefines the same names** instead of declaring
  a second set — the legacy file explains at length what happens when a surface token is not redefined
  (light chips glowing on a dark page, measured at 2.28 contrast).
* **Tabular figures on the body.** Versions, uptimes, ids and timestamps sit in columns and change while
  you watch; proportional digits re-flow every row around an update.
* **One chip vocabulary** (ok / warn / off) for every status in both surfaces.
* **The rail** — destinations on the left, connection state at the foot — is the shell the legacy panel
  arrived at after its own detours. The console's rail is that shape, with four entries.
* **A status bar** that carries the facts an operator checks without looking: counts, identity, build.

## What is deliberately not shared

**The panel is hand-written HTML; the console is React built into one file.** That looks inconsistent
until the constraint is stated: the console page is embedded in the console binary with `include_str!`,
so a build step is acceptable there and only there. The panel is embedded in the *device* binary, which
must stay a single executable with no toolchain near it. 9.5 KB of HTML and CSS with the same tokens gets
the same look without putting a bundler in the device's build.

The cost is real and accepted: two places to change a style. The mitigation is that both read the same
token values and neither invents a colour.

## What the rewrite added to the borrowed design

* **Progressive disclosure.** The console shows devices and the audit; the grant, the terminal and the
  browser appear only after a device is chosen. A screen that shows everything at once is a screen that
  says nothing at a glance.
* **Version-behind marking.** A device older than the newest on the list gets an amber chip whose tooltip
  says what it compared ("another device in this list runs 0.3.20") rather than claiming the device is
  outdated, which the console cannot know.
* **The retry note.** "offline" is not a diagnosis. The device sends how its previous attempt ended on the
  next connect (`?a=…&e=…`), the console shows it, and the panel shows the same fact locally.
* **No hand-written labels.** Every number in either surface comes from somewhere: the build injects the
  console's version, `/health` reports it, the device announces its own, `/tools` serves the catalogue.
  Each of those replaced a literal that had already drifted once.

## What would make this document wrong

If the console ever needs to work while its own process is down, or if a second device ever needs to be
managed from a machine that is not on its loopback, the split above stops holding and the surfaces have
to be reconsidered rather than extended.
