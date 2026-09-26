# Summrise

Summrise turns a Windows machine into an AI-controllable workspace: one operator hands a real box to an AI and watches it
work from a browser. This file is the project's GLOSSARY — the words that mean something specific here, and the words that
do not. It is not a spec, not a scratch pad, and it carries no implementation decisions.

## The surfaces

**Summrise Gate**:
The AI gateway console — login, admin/user roles, invite codes — running on a Cloudflare Worker. It is the front door.
_Avoid_: the server, the backend, the gateway (unqualified).

**Summrise Agent**:
The headless MCP server on the Windows machine, exposed through a Cloudflare Tunnel, one subdomain per machine.
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
