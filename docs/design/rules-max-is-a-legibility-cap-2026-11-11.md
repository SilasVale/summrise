# `RULES_MAX = 4` is a legibility cap, and it does not bound the load (measured 2026-11-11)

**This answers one of the design's two remaining questions, and it answers it by measuring what the
cap is and is not.** §6 says the number "exists so a device's forward table stays a list somebody can
read and revoke", that "four is chosen, not derived", and asks whether 4 is right. This page settles
**which kind of limit it is**, because a resource limit can be derived from a measurement and a
legibility limit cannot be moved by one.

Reproduce: `scripts/measure-forward-cap.py` against a real `summrise-device` binary.

## What each rule costs the device process

| after | descriptors | RSS |
|---|---|---|
| no rule | 11 | 23 624 kB |
| rule 1 | 12 | 24 852 kB |
| rule 2 | 13 | 24 936 kB |
| rule 3 | 14 | 25 068 kB |
| rule 4 | 15 | 25 080 kB |

**+1 descriptor per rule, and it stays +1.** RSS moves 1.2 MB on the first rule and ~130 kB on each
after that — the first one pays for the registry's own lazily-built state, not for the rule.

## The cap, in the device's own sentence

```
forward.add: this device carries at most 4 forward rules; remove one before adding another
             (a forward is a listening port and an SSH hop, so the cap is small on purpose)
```

**An edit at the cap is NOT refused** (`refused: False`): a full device can still be maintained,
which is the property the cap's own code says it exists to preserve.

## What the cap does NOT bound — the finding

`RULES_MAX` bounds **rows**. It does not bound **connections**, and a connection is what costs:

> one rule, 64 connections opened at once, with its `via` accepting and never speaking:
> **descriptors 15 → 143, +128 for 64 connections, exactly 2.0 per connection.**
> Each is one client socket plus one SSH session attempt, held until `OPEN_TIMEOUT_MS` (30 s) or the
> far side answers.

Two per connection is the number: a client socket and an SSH session's socket. At 4 rules × 64 each,
that is 512 descriptors — far below any platform limit, which is why **nothing in the device refuses
a 65th connection and nothing should.** The cap is not protecting the descriptor table.

## So what is 4?

**Four is where a person can read the table and revoke one of it at a glance.** That is not derivable
from descriptors, memory, or CPU — the measurements above show the resource cost is trivial at 4, at
8, at 40. It is derivable from a different measurement, and here it is: the console's Machine tab draws
each rule as a row of about 90 px by four lines — a name, a state, a route sentence, and a
count-or-error line. Four rows are a section a person reads without scrolling; twenty are a table
nobody opens. **That is the whole argument, and it is a LEGIBILITY argument, stated as one.**

## What this changes

§6 asked "Whether 4 is the right number is the open part". It is not open in the way the sentence
implied — there is no measurement that would move it, because the resource cost at 4 is one descriptor
and the cost of raising it is a table nobody reads. The sentence can therefore be **closed as answered
the way it was written**: four is chosen, and chosen for legibility, and the measurement is the
legibility of the surface the rules are edited on (`ForwardRules.tsx`, merged 2026-11-11).

What IS a finding to act on is the asymmetry: **a cap on rows that reads as "four" invites the
assumption that four is where the device stops carrying.** It is not. The load a rule can take is
bounded by the far sshd's `MaxStartups` (measured separately: ~18–22 of 24 at the default
`10:30:100`), not by this number.

## What this does NOT say

- **It is not a benchmark of a busy rule.** `connections` was 0 on every rule in the table above,
  because nothing connected; the fan-in section is the only part that carries load, and it carries
  load that never completes.
- **The RSS figures are one debug build's.** They are recorded because they establish that a rule is
  not a memory cost, not as a resident-set claim for a release build.
- **It does not say 4 stays 4.** It says the reason is legibility, so the number moves if and only if
  the surface's capacity to show a rule moves — which is a decision about the surface, not about the
  device.
