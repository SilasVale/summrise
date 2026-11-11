# What one carried connection costs, and where the wall is (measured 2026-11-11)

**This is a measurement, not a design decision.** The forward branch (merged into `hub/v1` as
`68331e5`) ships a rule that carries bytes; the design's §6 leaves TLS open and says "NOT MEASURED".
This page does not answer that question. It answers the one this build's own shape raises and nobody
had asked:

> `carry()` calls `ssh::open` for **every client connection**. So one rule is **one SSH handshake per
> connection carried**. What does that cost, and where does it stop working?

Reproduce: `scripts/measure-forward-concurrency.py`, driven against a real `summrise-device` binary,
a real `sshd`, and a real HTTP target — the same three pieces the wire test uses.

```
SUMMRISE_FORWARD_VIA="$(whoami)@127.0.0.1:<sshd-port>" \
SUMMRISE_FORWARD_KEY=/abs/path/to/key \
SUMMRISE_FORWARD_SSHD_CONFIG=/abs/path/to/the/sshd_config \
  python3 scripts/measure-forward-concurrency.py
```

## The numbers

Measured on `DESKTOP`-class dev box (Linux, loopback), device binary in debug, one rule
`127.0.0.1:<listen>` → `127.0.0.1:<target>` via a throwaway sshd.

| what | reading |
|---|---|
| **the same request with NO forward in it** | **0 ms** |
| through the rule, alone | **158–171 ms** (median 161) |
| **what the forward adds** | **~160 ms per connection** |

| opened at once | carried | span | sshd |
|---|---|---|---|
| 4 | 4 | 208 ms | default |
| 6 | 6 | 203 ms | default |
| 12 | 12 | 218 ms | default |
| 24 | **18** | 239 ms | default (`MaxStartups 10:30:100`) |
| 24 | **24** | 260 ms | raised (`MaxStartups 200:30:400`) |

## What the numbers say

**1. The price is one SSH handshake, and it is the whole price.** The direct request costs 0 ms; the
carried one costs ~160. The difference is `ssh::open` — curve25519 key exchange, authentication, and
a `direct-tcpip` channel — run **once per connection**, because `carry()` opens a fresh session each
time. Nothing is reused. That is a design choice this page does not second-guess: a shared connection
would need a supervisor, a reconnect policy, and a decision about what one rule's failure does to
another's channels. It is recorded here because it is the cost that choice pays.

**2. The wall at 24 is the sshd's, not the device's.** With the sshd's default
`MaxStartups 10:30:100` — *10 unauthenticated connections, then 30% dropped* — 24 at once delivered
18–22. Raising the ceiling to `200:30:400` delivered **24 of 24**. So the device opened all 24; the
sshd dropped the surplus during the handshake. The device's own audit says exactly that, and it says
it in the right place:

```
{"event":"forward.refused","reason":"connecting to via zss@127.0.0.1:54153 failed:
   cannot connect to 127.0.0.1: the SSH channel failed: Connection reset by peer (os error 104)"}
```

**3. This is the number the "six connections per host" sentence was missing.** A browser opens ~6
connections per host over HTTP/1.1. A page behind a rule therefore pays 6 × ~160 ms of handshakes to
get its six pipes, against a `MaxStartups` that starts shedding at ten. That is why the design's D4
says a byte pipe is HTTP/1.1 only and a long-lived stream starves the rest — **and now the cost is a
number rather than a concern.**

**4. `connections` on the row counts CARRIED, and the measurement proves the difference is real.**
After the 24-at-once bursts at the default ceiling, the row read `connections` 44–51 while the audit
carried 9–11 `forward.refused` lines. The refused ones are logged and not counted, which is D6's
rule — and it is the only place the two numbers can be checked against each other.

## What this does NOT say

- **It is not a TLS answer.** D4's question is which front end to build and what it costs under
  starvation; this page measures the build that exists, which has no front end.
- **It is not a benchmark of the far side.** The target was loopback; a real target adds its own
  latency, which the forward does not change.
- **The device was built in debug, and only debug.** The table is one build; a release build was not
  measured, so this page does not claim the numbers would hold for one. What the debug build does
  establish is that the ~160 ms is not the handshake's *accounting* — the same binary carried five
  connections in a row at the same price, and the sshd's own log shows five `Accepted publickey` lines.

## Where this belongs in the design

D4 says "A TLS front end is a separate design question with an open answer". It still is. What
changed is that §6's "settle it by measuring the same six-connection starvation under each" now has a
**baseline to compare against**: six connections through this build cost ~1 s of handshakes before a
byte moves, and a front end that reused one connection would remove that entirely. That is the
comparison a TLS design should be argued on.
