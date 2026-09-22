# CHARTER — what this project is, and what counts as success

> The agent's standing target function. **Owned by the user**: changing this
> file is the one thing the iteration loop may only PROPOSE (see
> `docs/agents/iteration-loop.md`). Thresholds below are the defaults written
> when this file was created (2026-09-14, at round 110); only the user changes
> them.

## Thesis (seed: `README.md` line 5)

Summrise turns a Windows machine into an **AI-controllable workspace**: terminal,
SSH, serial and browser sessions exposed to AI through MCP, plus an Electron
desktop shell and a device-local memory. One repository carries the front door
(gateway), the device agent, the download distribution and the satellites.

Who it is for: one operator who hands a real Windows box to an AI and keeps
watching it work from a browser — installable by someone who is not the author.

## Success metrics — EXTERNAL and observable only

A number the loop can raise by itself is not a metric. These are read from the
world, not from the repo:

- field install/upgrade success rate on a real device; incidents within 24 h of an upgrade
- devices in the field and their version distribution (`/api/version` vs `/api/status`)
- steps and minutes from a clean machine to "an AI is driving it"
- monthly cost (Cloudflare / VPS / model egress); the free path must still exist
- release cadence, rollback count, and how many published releases are actually on devices
- open ADRs/proposals, and how many were later reversed by another ADR

## Thresholds — inside decides, outside proposes

| Decision | Decide alone | Propose and wait |
|---|---|---|
| Monthly cost delta | ≤ $5 | > $5, or any new long-lived dependency or paid tier |
| Blast radius | canary device / local + CI | all devices, published releases, closing a deprecation window |
| Contract change | any change it can verify itself (migration path, dual-accept, rollback point, device regression) | irreversible or unverifiable ones: published versions that cannot be recalled, other people's machines |
| Security boundary | tightening, with tests | ANY loosening — always proposed, always recorded |
| This file, and the free-path rule | — | always proposed |

## Red lines (not negotiable, not delegable)

- The free path stays free (cloudflared tunnel); no mandatory paid hop.
- No process listens on a non-loopback socket on the dev box (company policy).
- Cross-machine files move ONLY through the Summrise relay pair (`system_file_upload` / `system_file_download`).
- Never `pm2 start` from an agent session; long-lived services start from the ecosystem file with a scrubbed env.
- DSH upgrades only through `/home/zhengsaisi/dsh-upgrade/upgrade-dsh.sh`.
- `install` / `update` / `rollback` must keep working on a device that is already running, and stay recoverable.
- Device regression is a HARD GATE BEFORE release, never a post-release check.

## Review cadence

Every 10 rounds, a design review written into the journal: did any metric move,
was any ADR reversed, was the same place changed twice, and **is any metric one
the loop could have raised by itself?** The last question is the one that keeps
the rest honest.
