# The device's wire shapes, as the FLEET answers them

**Measured 2026-11-12 on DESKTOP-9VH92BJ, release `0.3.292`**, through the console's own proxy
(`POST https://console.saisi.online/mcp`, a JSON-RPC `tools/call`). This file exists because **five console defects in one week
came from one cause**: the page was written against the shape the *source tree* produces, and the *deployed build* answers an older
or different one.

**THE SOURCE IS NOT THE WIRE.** `agent/src/plugins/*/tools.rs` in this tree is the newest code; the fleet runs a build. Where the two
disagree, the fleet is right and the page must read both.

## The envelope, before any tool's own answer

A tool call answers:

```json
{"jsonrpc":"2.0","id":1,"result":{"content":[{"text":"<THE ANSWER>","type":"text"}],"isError":false}}
```

**`text` is a STRING, and it is not always JSON.** A success usually carries a JSON document; a refusal (`isError: true`) can carry a
plain sentence with no structure at all — measured:

    monitor.status {id:"x"}  →  text: "monitor.status: target x is not watched"   isError: true

So a reader must: take `content[0].text`, try to parse it, and fall back to the sentence — never assume an object.

## What each tool answers on this build

| tool | measured answer | what a reader must accept | the console's reader |
|---|---|---|---|
| `monitor.list` | **a bare array** `[{id, host, port, path, kind, interval_ms, summary, series?}]` — `summary` is `{drops, last_status, since_ms, up_now, up_pct}`; **no `probes` count and no `series`** on this build | bare array **and** `{ok, interval_secs, targets}` | `monitorApi.readMonitors` |
| `monitor.add` | `{id, host, port, kind, interval_ms, path}` echoed back | **requires `kind`, `host`, `port`, `interval_ms` (≥ 5000)**, `additionalProperties: false` | `monitorApi.addWatch` |
| `monitor.remove` | `{"removed": true}` | the object; `removed` says whether anything was there | `monitorApi.removeWatch` |
| `monitor.probe` | `{"latency_ms": null, "ok": false, "ts": 1794478559064}` | the object; `ok` is the sample | `monitorApi.probeNow` |
| `monitor.status` | the SAME summary as `monitor.list`'s `summary` | — | **deliberately not wired**: a second door to one fact |
| `memory.list` | **a bare array** `[{id, tags, text}]` — no title, no content, no namespace, no timestamps | bare array **and** `{ok, results:[…]}`; both record shapes | `memoryApi.readMemory` |
| `memory.search` | `{ok, results:[…]}` | the object; only `query` + `limit` are accepted | `memoryApi.readMemory` |
| `memory.*` writes | **`memory.list` accepts only `limit` and `tag`** — no `include_deleted`, no `namespace` | — | **not wired**: the soft delete needs `include_deleted`, and without it the delete is a one-way door |
| `state.get` | `{"health": {"pid", "poisoned", "status", "version"}, "state": {…}, "events": […]}` | **nested `health`** and the flat fallback | `stateApi.readDeviceState` |
| `host.facts` | a bare object: `caps, cpu, disks, helper, machine, memory, os, sessions, uptime_s, …` | the object itself (no `ok`/`data` wrapper) | `hostFactsApi.readHostFacts` |
| `terminal.list` | **a bare array** (`[]` when no session is open) | bare array | `TerminalWorkspace` |
| `terminal.screen` | `{lines: […]}` per the source — **NOT measured**: the fleet had no open session at the time | both `{lines}` and a plain string | `TerminalWorkspace.SessionScreen` |
| `forward.list` | **NOT measured**: no live build carries the forward plugin | `{rules:[{name, listen, target, via, state:{…}}]}` per the source | `forwardApi` (the page draws only when the tool exists) |

## The rule this file exists for

**A NEW PAGE READS THE LIVE ANSWER BEFORE IT TRUSTS THE SOURCE.** One `curl` against `/mcp` with the tool's name costs seconds; the
five defects below cost a round each:

| # | defect | the shape that would have shown it |
|---|---|---|
| 1 | `the machine answered monitor.list without a list` | the bare array |
| 2 | `monitor.add: the arguments do not match the tool's input schema` | `required: [kind, host, port, interval_ms]` |
| 3 | `memory.list: the arguments do not match the tool's input schema` | props `[limit, tag]` |
| 4 | `the machine answered without a list` (memory) | the bare array of `{id, tags, text}` |
| 5 | `unknown` beside ten real events (state) | the nested `health` |

**AND A TEST PER SHAPE FOUND.** Each fix above carries a test named for the shape the FLEET answers with, so a future page that
"simplifies" the reader back to one shape fails on the fleet's own words.
