# The tool surface: 56 tools became 28

> **Status: audit, 2026-10-24.** Written because the cutover plan's parity matrix is
> capability-level, and its deletion gate (D3: "the parity matrix has no `no` row") cannot be
> satisfied by a capability row that hides twenty-one tools.

## Why this file exists

`2026-10-21-phase-5-cutover-plan.md` asks whether *terminal* has parity and answers yes:
`terminal.create {kind:"pty"|"ssh"|"serial"}` exists and was exercised on the canary. That answer is
true and insufficient. The old runtime had **27 terminal tools**, and the new registry has **6** — the
matrix never had a row for any of the other twenty-one, and that difference is the distance between "a
terminal works" and "the terminal works the way the agent that drives it expects".

This file is the tool-level ledger. Every tool in the old registry
(`summrise-legacy: agent/spec-tools.json`, 56 tools in 9 plugins) is given one disposition, and every
tool in the new registry (`device/core/src/registry.rs`, 28 tools in 7 capabilities) is accounted for.
It is the input D3 was missing.

## The four dispositions

| Disposition | Means |
|---|---|
| **ported** | a new tool does the same thing, possibly under a different name |
| **replaced** | a different mechanism does the job; both the name and the shape changed |
| **dropped** | no counterpart. The reason is given, and whether it was a decision or an omission |
| **undecided** | nobody has chosen. Listed so it cannot be mistaken for deliberate |

Counts: **15 ported, 9 replaced, 25 dropped, 7 undecided** — 56 accounted for.

## The old tools, one by one

### `design` (1)

| old tool | what it did | new runtime | disposition |
|---|---|---|---|
| `page_view` | served the product page's own HTML/CSS so an agent could review its design | the console serves the page; the panel is 9.5 KB of HTML | **dropped** — a page is not a device capability |

### `mcp-client` (4)

| old tool | what it did | new runtime | disposition |
|---|---|---|---|
| `mcp_client_connect` | spawn or attach a browser MCP server (`playwright-mcp`) | `browser.*` drives Chrome over CDP directly | **replaced** |
| `mcp_client_list` | list that server's tools | the `browser.*` catalogue | **replaced** |
| `mcp_client_call` | call one of its tools | `browser.navigate` / `browser.eval` / … | **replaced** |
| `mcp_client_disconnect` | close the client and the spawned process | `browser.close` / `browser.shutdown` | **replaced** |

The replacement removes a capability, not just a name: the device can no longer act as an MCP
**client** toward a server it does not own. Nothing in the console asks for that today.

### `memory` (6)

| old tool | what it did | new runtime | disposition |
|---|---|---|---|
| `memory_save` | store an entry | `memory.append` | **ported** |
| `memory_search` | substring search over title/content/tags | `memory.search` | **ported** |
| `memory_list` | enumerate entries | — | **undecided** |
| `memory_update` | edit an entry by id | — | **undecided** |
| `memory_delete` | soft-delete by id | — | **undecided** |
| `memory_export` | whole store as JSONL | — | **undecided** |

`memory.append` and `memory.search` cover writing and reading. Everything an operator does when the
store is *wrong* — list, fix, delete, back up — has no tool and no console surface. That is the
largest single gap in this table for a store that is meant to be shared.

### `monitor` (4)

| old tool | what it did | new runtime | disposition |
|---|---|---|---|
| `monitor_add` | persist a host:port watch | `monitor.add` | **ported** |
| `monitor_list` | watches with their latest probes | `monitor.list` | **ported** |
| `monitor_probe` | probe one target now | `monitor.probe` | **ported** |
| `monitor_remove` | stop watching | `monitor.remove` | **ported** |

`monitor.status` is new (summary without a probe) and has no old counterpart.

### `playwright` (2)

| old tool | what it did | new runtime | disposition |
|---|---|---|---|
| `browser_pw_info` | describe the bundled Playwright runtime | — no bundle exists | **replaced** — a Chrome on the device is now a deployment requirement (`RELEASE.md`) |
| `browser_run_script` | run an arbitrary Node/Playwright script | `browser.eval` (JavaScript inside one page) | **replaced** — with a named loss: no arbitrary script, no bundled Chromium |

The old device could drive a browser with nothing installed. The new device needs a browser installed
and cannot run a script the registry does not describe. That trade was made for a smaller install and
a Rust-only runtime; this row is where its cost is written down.

### `runs` (2)

| old tool | what it did | new runtime | disposition |
|---|---|---|---|
| `run_begin` | declare the start of one attributed run | — | **dropped** — attribution moved to the console's audit index |
| `run_end` | close that run | — | **dropped** |

The concept survives, in a different place: a grant names a tool, the console records the call, and
`/audit` is the interval an operator reads. The device no longer has an opinion about "a run".

### `system` (9)

| old tool | what it did | new runtime | disposition |
|---|---|---|---|
| `system_file_read` | read a file on the device | `files.read` | **ported** |
| `system_file_write` | write text or base64 | `files.write` | **ported** |
| `system_file_list` | list a directory | `files.list` | **ported** |
| `system_file_stat` | size, mtime, kind | — | **dropped** — `files.list` carries name, kind and size, but not mtime |
| `system_file_upload` | send to the relay, get a one-time URL | — | **replaced** — the console proxy carries files; `files.*` is base64, ≤1 MiB per call |
| `system_file_download` | fetch a URL onto the device | — | **dropped** — the device no longer fetches arbitrary URLs |
| `system_process_list` | processes with pid/name/memory | — | **undecided** |
| `system_process_kill` | kill by pid or name | — | **undecided** |
| `system_net_test` | ad-hoc TCP connect and ICMP diagnostics | `monitor.*` watches a configured target; nothing tests on demand | **undecided** |

### `terminal` (27)

| old tool | what it did | new runtime | disposition |
|---|---|---|---|
| `terminal_open` | pty / ssh / serial | `terminal.create` | **ported** |
| `terminal_write` | write to a session, or a BREAK on serial | `terminal.write` | **ported** |
| `terminal_read` | cursor-based read | `terminal.read` | **ported** |
| `terminal_resize` | rows/cols | `terminal.resize` | **ported** |
| `terminal_close` | close | `terminal.close` | **ported** |
| `terminal_list` | live sessions | `terminal.list` | **ported** |
| `terminal_execute` | run a command in a session and wait | `runs.exec` (argv, captured output, no session) | **replaced** |
| `terminal_screen` | tail of the buffer, ANSI-stripped | — | **dropped** — `terminal.read` returns the cursor window instead |
| `terminal_select` | mark one session active | — | **dropped** — no "active session" concept |
| `terminal_history` | live **and closed** sessions | — | **dropped** — the new list is live only |
| `terminal_jobs` | background-job registry | — | **dropped** — `runs.list` lists executions, not jobs |
| `terminal_plan` | declare the steps you intend to take | — | **dropped** — planning belongs to the client, not the device |
| `terminal_env` | shell, install dir, bundled node path | — | **dropped** |
| `terminal_list_ports` | enumerate serial ports | — | **dropped, deliberately** — `terminal.create` says "a serial port is never enumerated or guessed" |
| `terminal_diag_read` | panel diagnostic ring buffer | — | **dropped** |
| `terminal_diag_write` | POST a diagnostic line from the panel | — | **dropped** |
| `terminal_saved_connections` | list saved connections | — | **dropped** |
| `terminal_connect_saved` | reconnect one | — | **dropped** |
| `terminal_forget_saved` | remove one and its password | — | **dropped** |
| `secret_set` / `secret_get` / `secret_delete` | SSH password store | — | **dropped** — the password is a per-call argument to `terminal.create` |
| `terminal_secret_set` / `_get` / `_delete` | the same three, under the terminal plugin | — | **dropped** — they were duplicates of the above |
| `sftp` / `terminal_sftp` | one-shot SFTP transfer | — | **dropped** — `files.*` is local; moving files between hosts needs an SSH client, not a file API |

The six ported tools are the ones an agent uses *while a session is open*. The twenty dropped ones
are the ones it uses to find, remember, inspect and clean up — which is why the old agent felt like a
workspace and the new one feels like a terminal with a registry.

### `update` (1)

| old tool | what it did | new runtime | disposition |
|---|---|---|---|
| `agent_update` | check the release server and install on demand | the launcher's signed updater, automatic | **replaced** |

The device no longer takes an update *command*; it follows its channel. The on-demand path survives as
`summrise-device --check-update`, which is an operator command, not a tool an agent holds.

## What the new runtime has that the old did not

| new tool | why it exists |
|---|---|
| `browser.list` / `open` / `navigate` / `eval` / `screenshot` / `close` / `shutdown` | CDP in Rust: tabs, navigation, evaluation, screenshots, with no Node |
| `files.mkdir` | a directory is not a file |
| `monitor.status` | the summary without paying for a probe |
| `runs.exec` / `list` / `get` | argv execution with captured output, and a way to read it back |
| `state.get` | the device's own state document, read through the same grant path as everything else |

## The undecided seven

| tool | why it matters | who decides |
|---|---|---|
| `memory_list` / `update` / `delete` / `export` | the shared store has no way to be corrected or backed up | the operator who owns the store |
| `system_process_list` / `kill` | "what is eating this machine" is a first question when a device misbehaves | the agent that answers incidents |
| `system_net_test` | `monitor.probe` says a port is closed; ICMP and DNS say why | the agent that answers incidents |

`terminal_screen` is a near-miss worth naming: it is one line over `terminal.read`, and the reason it
is gone is that the new read is cursor-based, not that anyone decided screen access was wrong.

## What would make this document wrong

- If the **console** grows the missing surfaces (a memory page, a process list on the device card), the
  device does not need those tools and the "undecided" rows become "replaced" by the console. That is
  a legitimate answer, and cheaper than restoring twenty-five tools.
- If a **mesh peer** becomes a first-class client, the tools it needs belong in the registry, not in
  the console, and the split above moves.
- If `summrise-legacy` keeps serving a fleet after D4, this table describes a race, not a cutover.

## Recommendation

Restore five before anything else is deleted, because they are what an agent reaches for when a device
is wrong and the console has no equivalent surface:

1. `terminal.screen` — one function over `terminal.read`;
2. `system.process_list` and `system.process_kill` — a device that cannot say what it is running is
   a device that cannot be diagnosed;
3. `system.net_test` — a closed port is a symptom, not a diagnosis;
4. `memory.list` and `memory.delete` — a store that cannot be corrected is a store that rots.

Everything else on the "dropped" rows can stay dropped, with this file as the record.
