# The product

> **This file is the canonical statement of what Summrise is.** The landing page, the README and the
> design records point here instead of restating it, because a product story told in four places is
> four stories that drift. The plan that makes this statement true is
> `docs/design/capability-hub-2026-11-02.md`.

## One sentence

> **Summrise is the capability hub for AI on a real machine: one MCP endpoint with hands that work on
> install, plugins when one is missing, and a console that shows every call and lets its owner take
> the keyboard back.**

The second half is the product. Every tool in this space can give an agent a shell; what almost none
do is make handing a real machine over **something a person can watch and reverse**. A hub that
cannot be watched is a remote shell with branding; a console that cannot extend the hub is a
monitoring page. Summrise is the two halves in one runtime.

## Who it is for

One operator, a small number of real Windows machines, and AI clients that need to do work rather than
describe it. The machine may be on a bad network and behind a NAT. The operator may be nowhere near it.
The free path must stay free.

**Not for:** multi-tenant teams that need policy and compliance evidence, people who want a hosted
sandbox, or people who want an agent orchestrator with tasks, plans and teams. Those are different
products.

## The model

```
Capability ── declares ──▶ Tool ── called by ──▶ Call ── recorded as ──▶ Record
     built-in (Rust) or plugin (MCP server)      on a Device, reached through Reachability
```

The four nouns are kept; **Capability** is the object the hub organizes.

| noun | what it is | the failure when it is wrong |
|---|---|---|
| **Capability** | a named group of tools with one job: `terminal`, `files`, `browser`, `runs`, or a plugin's namespace | a catalogue nobody can explain, or a plugin that is a second product |
| **Device** | a real Windows machine: `summrise-launch` supervises `summrise-device` | an update that cannot be recalled, or a machine that cannot repair itself |
| **Session** | the live object a stateful tool returns: a terminal, an SSH or serial link, a browser page, a run | "it said it ran it" with no evidence |
| **Record** | the audit row every call leaves, and the token scope that allowed it | an action nobody can attribute, or a permission nobody can revoke |
| **Reachability** | how the operator gets to the device: the console from anywhere, the panel on the machine | a device unreachable exactly when it is wanted |

A **grant** is an internal wire format between the console and the device, not a noun the operator
manages. The consent the operator gives is the **token**: which tools, which device, destructive or
not. There is no approval queue in the console and no ninety-second hold; the decision happens when
the token is minted, and the record shows every call it made.

## The pillars

Five promises, in the order an operator feels them. Each names the code that keeps it.

1. **Hands — the agent operates the real machine.** Terminal, SSH, serial, Chrome on the device's own
   network and profile, files under a declared root. Not a lookalike. *(device/device capabilities;
   the tool catalogue the device serves.)*
2. **One door — the hub is extendable without a fork.** One MCP endpoint; the built-ins come first;
   a missing capability is a plugin away, and its tools appear behind the same door, under the same
   token, in the same record. *(the MCP surface; the plugin host; `plugins.*`.)*
3. **Record and brake — handing it over stays reversible.** Every call is an audit row; a token names
   its tools and whether destructive calls are allowed, and is revoked in one action. The operator
   opens the same machine's terminal or screen and takes the keyboard back; the workspace is the
   brake, not a queue that asks permission after the fact. *(the JSON audit; token scope; the Device
   destination.)*
4. **Operations — the machine maintains itself.** The launcher owns the app binary: verifies at boot,
   repairs what is missing, swaps only against a signed release document. A bad network is a design
   input. *(summrise-launch; release signatures; the repair path.)*
5. **Distance — the operator can be anywhere.** The device dials out; nothing listens inbound by
   default. One console sees the fleet. The panel at `127.0.0.1` still works when the console does not,
   and it is the only surface that can say why the console cannot. *(the console channel; the panel.)*

## The five surfaces

The console is one hub per machine, not a control panel of features:

**Home · Capabilities · Plugins · Activity · Device**

- **Home** is the hub at a glance: running, the endpoint, what the AI can do, the next action. Before a
  machine is chosen it is the fleet.
- **Capabilities** is every tool the device advertises — built-in and plugin — grouped by capability,
  with a schema and **Try it**.
- **Plugins** installs, watches, restarts and removes the capabilities that are not built in.
- **Activity** is the record: every call, its token, its device and its result.
- **Device** is the operator's hands: Terminal · Screen · Files · Runs · Machine.

**Connect is a sheet, not a surface**: a verb with a focused job (endpoint, client config, tokens).
The fleet is Home's zero state; Settings and About live in the account menu. A capability the AI
reaches through MCP does not earn a permanent rail seat, and a sixth destination needs a deletion in
the same change.

## What the product is not

- **Not a cloud IDE or sandbox.** The work happens on the machine the work belongs to.
- **Not remote desktop.** A person does not watch a person; an agent works and a person can intervene.
- **Not multi-tenant SaaS.** One operator, a handful of machines, each enrolled deliberately.
- **Not a per-device public hostname.** A tunnel per device is the shape this replaces.
- **Not a plugin marketplace.** v1 installs an MCP server from a directory the operator trusts. There
  is no registry, signature or sandbox, and installing is trusting; the product says so plainly.
- **Not an orchestrator.** Tasks, plans, teams and conversation threads belong to the agent client.
- **Not an Electron/npm runtime.** One launcher, one app, one console. The console page is React 19
  built at build time into one HTML file and embedded in the Rust binary; there is no Node at
  runtime, no Electron and no cloud IDE.

## The design rules

The operator asked for the Apple register. These are the rules that register reduces to here — each one
is a constraint on a change, not a mood.

1. **The product is the subject.** A screen shows the machine, the capability or the call before it
   explains anything about itself. Chrome recedes.
2. **One primary action, one accent.** The accent marks the action you want taken. If two things are
   orange, one of them is wrong. Destructive actions are never the accent fill.
3. **One type scale, one spacing rhythm.** Seven sizes, 13px body; space comes from a named scale, so
   two screens built at different times still line up.
4. **Progressive disclosure.** Fleet → hub → capability → tool → call. A first visit shows the
   machines; the catalogue appears once a machine is chosen; a tool's form appears when asked for.
5. **States are visible without hovering.** Focus, hover, current, disabled, empty, loading, waiting,
   offline, failed. Every state has words and a next action.
6. **Motion explains change, and nothing else.** 120–180 ms; with `prefers-reduced-motion`, the
   product is still complete.
7. **Copy is the interface.** Verb-first buttons, sentence case, no jargon before it is defined, no
   policy language. An error names the next action; an empty state teaches the first step.
8. **The keyboard is a first-class surface.** Every action reachable by Tab, a visible focus ring,
   Escape closes a sheet, the first Tab stop is Skip to content.
9. **Trust is stated, not implied.** "Installing a plugin runs its code with this machine's
   privileges" appears where the install happens.
10. **Deletion is part of the design.** Every addition names what it replaces. The default answer to
    "and also" is no.
11. **Measured, not argued.** Contrast and type are a Rust test; layout is `scripts/layout-gate.mjs`;
    the page is compared at 360/390/800/1280 in light and dark by
    `device/console/web/scripts/screenshot.mjs`. A change that moves a page nobody meant to touch is a
    finding.

## How we know it works

Numbers read from the world, not from the repository:

- minutes and steps from a clean machine to a first successful MCP call;
- share of sessions that end with a recorded call rather than an unattributed action;
- plugin installs that reach a healthy `tools/list`, and the time to get there;
- calls that needed the operator to take over, and whether the takeover worked;
- install and upgrade success on a real machine, and incidents within 24 h of an upgrade;
- how many devices run the current release, and how many can say what they run;
- audit completeness: calls that left a row, and calls that did not.

The free path stays free, and no process listens on a non-loopback socket on a development box.

## Where the words live

- **This file** — the product, its model, its pillars, its surfaces and its rules.
- `docs/design/capability-hub-2026-11-02.md` — the plan of record: the IA, the screen specs, the
  plugin contract and the acceptance lines.
- `docs/superpowers/plans/2026-11-02-capability-hub-v1.md` — the task map that implements it.
- `landing/` — the public page; English in `index.html`, Chinese in `assets/lang-zh.js`, both checked
  by `landing/check.mjs`.
- `README.md`, `docs/QUICKSTART.md`, `docs/CONNECT-MCP.md`, `docs/PLUGINS.md` — how to install,
  connect and write a plugin.
- `device/console/web/` — the React page; `device/console/` — the Rust console that embeds it.

When the story changes, it changes here first; everything else points at it.
