# A forward is a rule, not a gateway

> **Status: implemented 2026-11-10, on `hub/v1`.** This is the design the branch
> `feat/a-forward-is-a-rule` argues from and the record of what was measured before it was written.
> The thing being added is a ROW in a table — the same shape a router's port-forward table has —
> not a role the device takes on.

---

## 0. The decision in one page

**`forward` is a rule: four values, one executor per row, and no verbs.** The operator asked for it
in one line, and the correction in that line is the design:

> 「不是这个转发不是一条规则吗，又不是网关 · 只是软路由吧」

One row:

```
forward  name=work  listen=100.117.43.107:443  target=10.10.61.83:7738  via=zhengsaisi@10.10.61.83:22122
```

It says "traffic arriving here leaves there". It does not describe a role for the device, does not
route anything, and does not make the machine a gateway. `monitor.add` is the sibling object already
in the product — `{host, port, interval, path, expect}`, a row the device keeps applying — and a
forward is the same kind of object with a different executor.

## 1. The facts that decided it (measured, not inferred)

| fact | how it was measured |
|---|---|
| **The far host may not gain a listening port** | The operator's constraint, in session: 「那台机器是公司服务器…那台机器不要外部端口，不然会被公司查的」. It already serves on `127.0.0.1:7738`. So the connection must be made BY ANOTHER HOST — that is what `via` is for, and it is the reason the row needs a `via` field at all. |
| **Such a hop works and is cheap where it counts** | A temporary `-R` from DESKTOP-84T3LIN to `10.10.61.83`, then on that server `curl -X POST http://127.0.0.1:7736/mcp/all -H 'Authorization: Bearer srm_…'` answered **200** with a real MCP `tools/list`, while `ss -ltn` showed the listener on **loopback only**. 2026-11-10. |
| **Placement matters, and it is measurable** | The same entry from the Linux box answered in **3.3 s**, because its tailnet route to that machine is DERP `phx` at 392–633 ms; from `desktop-14rjcr8` it answered in **0.15 s**. A rule that ignores placement produces a slow site that looks like a broken one. |
| **The executor must outlive a session** | Killing nginx and the tunnel by hand: both back in **~2 s**, site answering again. A rule with no executor is a line in a file. |
| **A raw TLS byte pipe is HTTP/1.1 only** | Measured the night before: the stdlib pipe was replaced by nginx BECAUSE six-connections-per-host starved DSH's event streams. Paid for twice, not reasoned about. |

## 2. The decisions

**D1 — The rule is data with four values, and it is the whole interface.** `{name, listen, target,
via}` plus the credential the `via` connection needs. No verbs, no script, no environment: if a
behaviour cannot be expressed by changing one of those values, it is not a forward. The deletion
test passes — delete the rule type and every caller is back to hand-assembling DNS + tunnel + server
+ certificate.

**D2 — A wildcard `listen` is refused.** `0.0.0.0`, `::` and `*` name no audience the operator
chose. A rule that silently widens who can reach `target` is the failure this prevents, and the row
has no fifth field to unlock it with. A LAN address or a tailnet address is a legitimate choice and
a visible one.

**D3 — The executor is the device, and it starts with the device.** The table is loaded at
construction and one executor starts per row, so a rule created in an earlier boot is listening
again before any client asks about it. `summrise-launch` keeps its single-child contract — that
contract is about device upgrades and is not where a rule's executor belongs.

**D4 — TLS is not this build's job, and the row does not pretend otherwise.** A forward is a byte
pipe, which is HTTP/1.1: six connections per host, and a long-lived stream starves the rest. The
build does NOT accept a `tls` value and then not serve one. A TLS front end is a separate design
question with an open answer.

**D5 — The credential belongs to the ROW, not to a side table.** `terminal.create` takes its
credential from a caller and holds it for that session; a rule OUTLIVES the caller and its executor
must reach `via` at boot with nobody present. Keying a table by `via` would also make two rules to
one host silently share one credential — a decision nobody made. It is not a server secret store, and
the file says so: `<data>/forwards.json`, protected by whatever protects `<data>`.

**D6 — A success is COUNTED, a refusal is WRITTEN.** One line per refused connection and per rule
change; carried connections increment the row's `connections`. Written literally, "one line per
connection" is a file written on the busiest path this module has, where a rule carrying a UI's
requests appends hundreds of lines a minute.

## 3. What this is NOT

- **Not a gateway.** No routing table, no default route, no NAT, no role. The row names one port
  pair; traffic that does not match it is not the device's business.
- **Not a process supervisor.** One listener per rule, restarted; not "run anything and keep it
  alive".
- **Not a replacement for `summrise-launch`.** D3 keeps its single-child contract intact.

## 4. The limits this build states rather than hides

- **The bind ladder is bounded.** An immediate attempt, then 1 s / 5 s / 15 s, then STOP. A permanent
  conflict leaves the row VISIBLE with its error instead of retrying forever behind a table that
  looks merely quiet.
- **A failed `accept` does not end the listener.** One reset connection must not take a rule down
  while its row still says `listening: true`.
- **A half-close is a half-close.** Each direction ends on its own EOF, so a client that sends its
  request and shuts down its write half still gets the answer. The obvious version — return as soon
  as either direction ends — loses that answer for the commonest client there is.
- **The in-process SSH fixture is not the authority for a forwarded channel's byte order.**
  Measured: with the fixture dialling nothing, reading nothing and replying nothing, a client that
  wrote `AAAA` still read `AAAA` back; a real sshd does not do that. The authority is
  `device/tests/forward_wire.rs`, which drives a real sshd and the real binary.

## 5. What a rule is worth, said plainly

`via` means the rule reaches a host that never agreed to be reached, and `listen` decides who can
use it. Those two values are worth a record of who set them — that is why both are destructive in
the registry's sense and why `forward.add` and `forward.remove` are the operator's own act.

## 6. Open

- **Which TLS front end** (D4) — bundle, embed, or HTTP/1.1 with a stated cap. Settle it by measuring
  the same six-connection starvation under each.
- **Where the rules are edited** — the console's device page is where an operator would look; the
  CLI is where they would be written. `forward.list` is the wire surface both read.
- **How many rules one device may carry** — the row is a list, `RULES_MAX` is 4 today, and the audit
  and the panel read better with a stated small maximum.
