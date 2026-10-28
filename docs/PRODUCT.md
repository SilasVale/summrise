# The product

> **This file is the canonical statement of what Summrise is.** The landing page, the README and the
> design records point here instead of restating it, because a product story told in four places is
> four stories that drift.

## One sentence

> **Summrise turns a Windows machine into an AI-operable workspace — and gives its owner one console to
> see, approve, and take back control from anywhere.**

The second clause is the product. Every tool in this space can give an agent a shell; what almost none
of them do is make handing a real machine over **a decision a person can reverse**.

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
| **Session** | something the AI can drive: terminal, SSH, serial, browser, files, monitors, runs | "it said it ran it" with no evidence |
| **Grant** | the signed, scoped, revocable permission behind every call | an agent doing something nobody can attribute |
| **Reachability** | how an operator gets to the device: the console from anywhere, the panel on the machine | a device unreachable exactly when it is wanted |

## The pillars

Four promises, in the order an operator feels them. Each names the code that keeps it, so the promise
is checkable rather than aspirational.

1. **Hands — the agent operates the real machine.** Terminal, SSH, serial, Chrome on the device's own
   network and profile, files under a declared root. Not a lookalike. *(device/device capabilities;
   the tool catalogue the device serves.)*
2. **Brake — handing it over stays reversible.** Every tool call is a grant and an audit row; a
   destructive call without the token's consent is held for approval (ninety seconds, then refused,
   never run); the operator can hold the keyboard and the AI waits; a token is revoked in one action,
   while a grant is valid until it expires. *(console grants + audit; the approval gate in the
   session, not only in a log.)*
3. **Operations — the machine maintains itself.** The launcher owns the app binary: verifies at boot,
   repairs what is missing, swaps only against a signed release document. A bad network is a design
   input. *(summrise-launch; release signatures; the repair path.)*
4. **Distance — the operator can be anywhere.** The device dials out; nothing listens inbound by default. One
   console sees the fleet. The panel at `127.0.0.1` still works when the console does not, and it is
   the only surface that can say why the console cannot. *(the console channel; the panel.)*

## What the product is not

- **Not a cloud IDE or sandbox.** The work happens on the machine the work belongs to.
- **Not remote desktop.** A person does not watch a person; an agent works and a person can intervene.
- **Not multi-tenant SaaS.** One operator, a handful of machines, each enrolled deliberately.
- **Not a per-device public hostname.** A tunnel per device is the shape this replaces.
- **Not an Electron/npm stack.** One launcher, one app, one console; Rust, and a page rendered from it.

## The design rules

The operator asked for the Apple register. These are the rules that register reduces to here — each one
is a constraint on a change, not a mood.

1. **The product is the subject.** A screen shows the thing itself (the fleet, the session, the
   terminal) before it explains anything about it.
2. **One accent, one job.** The accent marks the action you want taken. If two things are orange, one
   of them is wrong.
3. **One type scale.** The same role always takes the same size and weight; a heading gets its rank
   from scale and space, not from colour or decoration.
4. **One spacing rhythm.** Space comes from a scale, so two screens built at different times still
   line up.
5. **Progressive disclosure.** Fleet → device → tool. A first visit shows the machines; a session,
   a grant and the terminal appear once a device is chosen.
6. **States are visible without hovering.** Focus, hover, current, disabled, empty, waiting. A state
   that needs a tooltip is not shown.
7. **Motion explains change, and nothing else.** With `prefers-reduced-motion`, the product is still
   complete.
8. **Measured, not argued.** Contrast, layout and transitions are gates: `check.mjs` for the page,
   `cargo test -p summrise-console` and `console-screenshots.bash --diff` for the console. A change
   that moves a page nobody meant to touch is a finding.
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
- `docs/superpowers/specs/` — the design record, including the decisions that were wrong.

When the story changes, it changes here first; everything else points at it.
