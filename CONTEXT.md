# Summrise

Summrise turns a Windows machine into an AI-controllable workspace: one operator hands a real box to an AI and watches it
work from a browser. This file is the project's GLOSSARY — the words that mean something specific here, and the words that
do not. It is not a spec, not a scratch pad, and it carries no implementation decisions.

## The surfaces

**Summrise Gate**:
The AI gateway console — login, admin/user roles, invite codes — running on Cloudflare Workers. **TWO WORKERS, AND THE
DISTINCTION IS LOAD-BEARING**: the **worker** (`gateway/wasm`, Rust) serves every API route, `/v1`, the console's static
page and its own front-door gates; the **console worker** (`gateway/src`, TypeScript) is the Durable Object host — a DO
class cannot be defined in Rust — and serves nothing else once the domains move to the worker (`scripts/GATEWAY.md`).
"The gate" unqualified means the first one.
_Avoid_: the server, the backend, the gateway (unqualified).

**Summrise Agent**:
The headless MCP server on the Windows machine. It opens **NO INBOUND PORT**: it is reached either through a Cloudflare
Tunnel (one subdomain per machine) or by dialling OUT to an **outbound relay**, and which one is the device's own
declaration — see **Transport** below.
_Avoid_: the server, the daemon, the client.

**Console**:
The Summrise Gate's own user interface. An operator reads devices and sessions here, from anywhere.
_Avoid_: the dashboard, the web app, the UI.

**Panel**:
The Summrise Agent's device-local user interface. It answers "what is this machine doing right now", and it is the only
surface that can, because it is on the machine.
_Avoid_: the UI, the terminal page — it is more than a terminal.

**Device**:
A Windows machine running a Summrise Agent and registered with the Gate. It is the unit an operator adds, names, and
reaches.
_Avoid_: machine, host, node, box.

**Transport** (of a device):
**HOW THE GATE REACHES A DEVICE, WHICH IS THE DEVICE'S OWN ANSWER AND NOT THE OPERATOR'S CHOICE.** Two exist: the
**tunnel** (`hostname` on the record — the device is a public server and the Gate is its client) and the **outbound
relay** (`via: relay` — the device polls out and the Gate dials the relay by name, opening nothing inbound). A device
declares which one it is at self-registration, from its own config, so removing `relay_url` on the device moves it back
to the tunnel. **NOT TO BE CONFUSED WITH "TRANSPORT LANE" BELOW**, which is about a session's backend inside one device —
this word is about the wire between two machines.
_Avoid_: connection (the panel has saved connections for WORKSPACE HOSTS), channel (that is a model provider).

**Outbound relay**:
The process a NAT'd device dials out to (`proxies/summrise-relay`), which then carries whole HTTP requests to it. It is
one relay for MANY devices: a device polls with its own name and a client reaches it at `/d/<name>/…`. **`summrise
connect <relay-url>` on the device is what configures it, `summrise disconnect` is what undoes it.**
_Avoid_: **"relay" unqualified — THE WORD IS NOW SHARED BY THREE THINGS, and the runbooks name them apart**: this one
(HTTP, carries a device's API), **DERP relay** below (the network layer, carries a tailnet's packets) and **Relay pair**
below (the file tools). They share a word because each moves something between two machines, and they share nothing else.

**DERP relay**:
Tailscale's own fallback relay, for when a direct path between two tailnet peers dies. **Self-hosted on the same box as
the exit node**, because the public one was carrying the whole internet for an exit-node user at relayed latency; the
runbook with the measurements is `scripts/DERP.md`. It moves PACKETS and knows nothing about Summrise.
_Avoid_: **outbound relay** above — a different layer, a different program, and no traffic of ours is HTTP to it.

## Workspaces

**Workspace**:
A path that the harness works in. Its files and commands live on the workspace host that path belongs
to, not on the device.
_Avoid_: project, folder (that names a directory on some machine, not the thing the harness opens).

**Workspace host**:
The machine a workspace's files and commands actually live on — reached over ssh, through a connection
the agent already saved. It is NOT a Device: no Summrise agent runs there, and nothing about it is
managed.
_Avoid_: machine, host, box, server — each of those either means the Device or means nothing here.

**Mapping**:
One `path prefix → saved connection` record. The longest matching prefix wins, so a mapping at `/` says
"everything on this host" and a mapping at `/srv/x` overrides it for that subtree. Registering a mapping
is what makes a path resolvable; it carries no credential, only the connection's id.
_Avoid_: mount, route, alias — those name mechanisms this is not.

**Registry** (the workspace registry):
The agent's store of mappings, and the four routes that read and write it. The agent is the only
authority for which connection serves a path AND for that connection's credentials; the harness and the
panel hold a connection id and never a password.
_Avoid_: config (it is not a file an operator edits), workspace list (that is the harness's own).

## Work

**Session**:
One connection on a device that an AI can drive — a shell, an SSH login, or a serial line. A device has many.
_Avoid_: terminal — that names one transport of a session, not the session.

**Transport lane**:
Which backend a session runs on: `pty`, `ssh` or `serial`.
_Avoid_: type, kind, mode.

**Run**:
One execution of an AI's work on a device, declared explicitly by the AI and named by the `run_id` it gets back. **A run
exists only where a session declared one** — records from a session that never declared one belong to no run, and are shown
apart rather than folded into a neighbouring one.
_Avoid_: task, job, batch, invocation.

**Goal**:
The objective a session is pursuing, set by the OPERATOR. It is not a run's goal: one goal can span several runs — a retry
after a failure is the same goal in a new run — and a run can carry no goal at all.
_Avoid_: using "goal" for both without saying which.

## Change

**Corpus**:
A committed set of recorded inputs and the ANSWERS THE SHIPPING IMPLEMENTATION GAVE, replayed against the port. It is what
makes "the same" a measurement rather than a claim: a route is cut over when its corpus is green, and a corpus case that
a port cannot satisfy is a finding, not an obstacle. **A CASE WHOSE LIVE PEER NO LONGER EXISTS BECOMES A PIN** — the
recording stays and the behaviour is asserted directly, rather than the case being deleted with the code it compared
against.
_Avoid_: fixture (that is the file; the corpus is the practice), test (a corpus REPLAYS, it does not invent).

**Cutover**:
Moving a surface's live traffic from one implementation to the other, after the corpus that compares them is green. **A
LANDING IS NOT A CUTOVER** — a landing ports a route and hands it over; the cutover is the deploy, and it is the
operator's call.
_Avoid_: migration (that word covers the whole port, of which a cutover is one step), release.

## State

**Liveness**:
The one state an entity is in, and there are exactly five because five are derivable from what a device can report:
`off` (a transport that is down), `waiting` (a question holding), `working` (activity), `failed` (a non-zero exit), and
`idle` (neither). **TWO ORDERS, AND THEY ARE NOT THE SAME ONE — CONFUSING THEM IS THE MISTAKE THIS ENTRY MADE TWICE.**

| order | what it answers | sequence |
|---|---|---|
| **the decision order** | *which state is this entity IN* — first match wins | `off` → `waiting` → `working` → `failed` → `idle` |
| **the urgency order** | *how LOUD may that state's mark be* — the code calls it the contract | `waiting` > `working` > `failed` > `idle` > `off` |

**The two ends are opposite**, and that is what a reader gets wrong: `off` decides FIRST because nothing below it can be
answered, and it is the QUIETEST mark because it says nothing can be done either way; `waiting` decides second and is the
LOUDEST, because a question decays if it is not seen while work continues. **`failed` sits below `working` in both** — a
session that is busy is not also drawn as failed.

(The authority is `panel-react/src/lib/liveness.ts`: `livenessOf` for the decision order, `URGENCY` for the other. This
entry first said "a question outranks activity, and activity outranks quiet" — three of five — and then, corrected once,
used the word "outranks" for the DECISION order while the code reserves it for urgency, where `off` outranks nothing. Both
versions were wrong the same way: one order where there are two.)
_Avoid_: status, health, condition.

**Mark**:
The silhouette that carries liveness. **Shape first, colour second**, so a mark still reads in greyscale, under a
colour-vision deficiency, and with motion reduced. `waiting` is a diamond, `working` a haloed solid, `failed` a triangle,
`idle` a ring, `off` a dashed ring.
_Avoid_: icon, dot, badge, indicator.

**Held by human**:
Who has the keyboard on a session — the AI or a person. It is a state, not an action.
_Avoid_: control, ownership, lock.

**Approval gate**:
Whether each command on a session waits for a person to say yes before it runs.
_Avoid_: confirmation, permission, prompt.

## Moving things between machines

**Relay pair**:
`system_file_upload` and `system_file_download` — the only sanctioned way a file crosses from one machine to another. The
bytes never pass through an AI's context, and neither end opens a listening port.
_Avoid_: transfer, sftp, copy — each names something this deliberately is not.
