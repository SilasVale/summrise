# The product

> **This file is the canonical statement of what Summrise is.** The landing page, the README and the
> design records point here instead of restating it, because a product story told in four places is
> four stories that drift.

## One sentence

> **Summrise turns a Windows machine into an AI-operable workspace — and gives its owner one console
> to see what happened and take the keyboard back.**

The second clause is the product. Every tool in this space can give an agent a shell; what almost none
of them do is make handing a real machine over **something a person can watch and reverse**.

## Who it is for

One operator, a small number of real Windows machines, and AI clients that need to do work rather than
describe it. The machine may be on a bad network and behind a NAT. The operator may be nowhere near it.
The free path must stay free.

## The four nouns

Everything the product does is one of these. A feature that cannot be stated as one of them is a
delivery detail, not a capability.

| noun | what it is | the failure when it is wrong |
|---|---|---|
| **Device** | a real Windows machine running the runtime: `summrise-launch` supervises `summrise-device` | an update that cannot be recalled, or a machine that cannot repair itself |
| **Session** | something the AI can drive: terminal, SSH, serial, browser, files, runs | "it said it ran it" with no evidence |
| **Record** | the audit row every call leaves, and the token scope that allowed it | an action nobody can attribute, or a permission nobody can revoke |
| **Reachability** | how an operator gets to the device: the console from anywhere, the panel on the machine | a device unreachable exactly when it is wanted |

A **grant** is an internal wire format between the console and the device, not a noun the operator
manages. The consent the operator gives is the **token**: which tools, which device, destructive or
not. There is no approval queue in the console and no ninety-second hold; the decision happens when
the token is minted, and the record shows every call it made.

## The pillars

Four promises, in the order an operator feels them. Each names the code that keeps it, so the promise
is checkable rather than aspirational.

1. **Hands — the agent operates the real machine.** Terminal, SSH, serial, Chrome on the device's own
   network and profile, files under a declared root. Not a lookalike. *(device/device capabilities;
   the tool catalogue the device serves.)*
2. **Record and brake — handing it over stays reversible.** Every call is an audit row; a token names
   its tools and whether destructive calls are allowed, and is revoked in one action. The operator
   opens the same machine's terminal or screen and takes the keyboard back; the workspace is the
   brake, not a queue that asks permission after the fact. *(the JSON audit; token scope; the React
   workspace.)*
3. **Operations — the machine maintains itself.** The launcher owns the app binary: verifies at boot,
   repairs what is missing, swaps only against a signed release document. A bad network is a design
   input. *(summrise-launch; release signatures; the repair path.)*
4. **Distance — the operator can be anywhere.** The device dials out; nothing listens inbound by
   default. One console sees the fleet. The panel at `127.0.0.1` still works when the console does not,
   and it is the only surface that can say why the console cannot. *(the console channel; the panel.)*

## The seven surfaces

The operator's console is one workspace per machine, not a control panel of features:

**Devices · Terminal · Screen · Files · Runs · Machine · Activity**

- **Devices** is the picker: one column of machines, then the one way to add one.
- **Terminal** is the workspace: a selected machine, its sessions and its shell.
- **Screen** is the machine's screen, with Browser as a second view of the same surface.
- **Files**, **Runs** and **Machine** are the same device context, one capability at a time.
- **Activity** is the record: every call, its token, its device and its result.

A capability the AI reaches through MCP does not earn a permanent rail seat. Memory, Monitors, Events
and Security were such pages; they are gone from navigation.

## What the product is not

- **Not a cloud IDE or sandbox.** The work happens on the machine the work belongs to.
- **Not remote desktop.** A person does not watch a person; an agent works and a person can intervene.
- **Not multi-tenant SaaS.** One operator, a handful of machines, each enrolled deliberately.
- **Not a per-device public hostname.** A tunnel per device is the shape this replaces.
- **Not an Electron/npm runtime.** One launcher, one app, one console. The console page is React 19
  built at build time into one HTML file and embedded in the Rust binary; there is no Node at
  runtime, no Electron and no cloud IDE.

## The design rules

The operator asked for the Apple register. These are the rules that register reduces to here — each one
is a constraint on a change, not a mood.

1. **The product is the subject.** A screen shows the thing itself (the machine, the session, the
   terminal) before it explains anything about it.
2. **One accent, one job.** The accent marks the action you want taken. If two things are orange, one
   of them is wrong.
3. **One type scale.** The same role always takes the same size and weight; a heading gets its rank
   from scale and space, not from colour or decoration. The page has seven sizes, 13px body.
4. **One spacing rhythm.** Space comes from a named scale, so two screens built at different times
   still line up.
5. **Progressive disclosure.** Machine → workspace → view. A first visit shows the machines; the
   terminal, screen and files appear once a machine is chosen. A selected machine never asks for a
   grant before it opens.
6. **States are visible without hovering.** Focus, hover, current, disabled, empty, waiting. A state
   that needs a tooltip is not shown.
7. **Motion explains change, and nothing else.** With `prefers-reduced-motion`, the product is still
   complete.
8. **Measured, not argued.** Contrast and type are a Rust test (`device/console/tests/tokens.rs`);
   layout is `scripts/layout-gate.mjs`; the page is compared at 360/390/800/1280 and in light/dark by
   `device/console/web/scripts/screenshot.mjs`. A change that moves a page nobody meant to touch is a
   finding.
9. **Deletion is part of the design.** Every addition names what it replaces. The default answer to
   "and also" is no.

## How we know it works

Numbers read from the world, not from the repository:

- install and upgrade success on a real machine, and incidents within 24 h of an upgrade;
- steps and minutes from a clean machine to an AI driving it;
- answer rate and tail latency through the console (the 504 rate after the proxy bound);
- how many devices run the current release, and how many can say what they run;
- audit completeness: calls that left a row, and calls that did not.

The free path stays free, and no process listens on a non-loopback socket on a development box.

## Where the words live

- **This file** — the product, its pillars and its rules.
- `landing/` — the public page; English in `index.html`, Chinese in `assets/lang-zh.js`, both checked
  by `landing/check.mjs` (a string that exists on only one side fails).
- `README.md`, `docs/QUICKSTART.md`, `docs/CONNECT-MCP.md` — how to install and connect.
- `device/console/web/` — the React page; `device/console/` — the Rust console that embeds it.
- `docs/superpowers/specs/` — the design record, including the decisions that were wrong.

When the story changes, it changes here first; everything else points at it.
