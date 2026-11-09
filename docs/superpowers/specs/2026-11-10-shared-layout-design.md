# One layout system for the console and the panel

**Date:** 2026-11-10 · **Trigger:** the operator's directive — *"console 和 panel 页面布局风格最好彻底重构"*.

## Intent

The console (`device/console/web`) and the desktop panel (`agent/resources/panel-react`) are two
frontends of one product. They already share a token scale by accident of history (both declare
`radius 14/10/8/6`, `fs 10…22`, `fw 400/500/600/700`, `sp 2…24`). They share **nothing else about
layout**, and both are built out of per-page boxes rather than a page frame plus a card grammar.

This is not a restyle. It replaces the ad-hoc boxes on both surfaces with one frame and one card
primitive, and it pins the numbers in a contract each repo gates against.

## What is measured (2026-11-10, live console `0.3.273-ux139`, panel built from `b688104c`)

| | console | panel |
|---|---|---|
| page column | `.page { width: min(1120px,100%) }` — max **1,100 px** measured at 1440 | **none**: `.desktop-content` is **1,271 px** at 1360 and **2,471 px** at 2560 |
| boxes per page | Home: `attention`, `fleet-table-wrap`, `installer`, `installer-strip` — 4 ad-hoc classes | Memory: `mem-card` (980 px); Plugins: `plug-search`, `plug-inventory`, `plug-card`, `plug-log`; Settings: `settings-fold` (1,221), `settings-section` (1,221), `connect-snippet` (1,153) |
| left edges in one page | 3 (314 head / 335 attention label / 355 row) | **Settings: 3 nested boxes at x=98, 115, 132** and **four widths** 1,221 / 1,187 / 1,153 / 980 in one window |
| card primitive | `.card` exists (border + radius 10 + pad 12 + gap 8) but Home does not use it | **none**: eight card-like class names, each writing its own border + radius + padding |
| rail | **232 px** measured (labels), declared `200px` in `.shell` | **52 px** icon rail (both densities) |

The two long ones: **the panel has no page column at all** — on a 2,560 px monitor its content is
2,471 px wide and its Memory cards stay 980 px, so a third of the window is empty and the reading
measure is the monitor's, not the text's. And **neither surface has a card grammar in use**: the
console has the atom and does not use it; the panel uses eight atoms.

## Goals

1. One **page frame**: rail · page column (max width) · page padding · vertical rhythm.
2. One **card**: border, radius, padding, gap, header row — the same in both repos.
3. The panel gets a page column it does not have; its nested folds collapse.
4. The console's Home blocks move onto the card grammar.
5. **Enforced by a gate in each repo**, not by habit: a checked-in contract both repos read, and a
   mutual hash so changing one without the other fails the other's gate.

## Non-goals

- Colour, type scale, weights: already shared and already gated (`designScale.test.ts` in the panel,
  the scale leg in the console's `browser-gate.mjs`).
- The rail's **content**: 232 px labelled destinations versus a 52 px icon rail is a genuine
  difference between a web console and a desktop app. The contract records both as declared
  densities; it does not force them equal.
- Behaviour, copy, data. This changes geometry only.

## Anti-goals

- A shared npm package or a submodule. Two repos, two CI pipelines, one HDD: a build step nobody
  runs is worse than a hash nobody updates.
- Restyling every page in one commit. Each page ships alone with its own screenshot.

## The contract

`layout-contract.json`, checked into **both** repos at the same content:

```json
{
  "page":    { "max": 1120, "pad": 24, "gap": 24 },
  "card":    { "radius": 14, "pad": 16, "gap": 8, "headerGap": 8 },
  "rail":    { "console": 232, "panel": 52 },
  "section": { "gap": 24 }
}
```

Each repo carries a copy. Each repo's gate:

1. parses the copy and asserts **its own rendered stylesheet** uses every number (the console reads
   `.page` / `.card`; the panel reads its page frame and card rule);
2. asserts the copy's `sha256` equals a literal recorded in **the other repo's** test file.

When one side changes the contract and does not update the other, the other's suite fails naming the
hash. That is the cross-repo drift alarm, and it costs one line per repo.

## Approaches considered

**A. One shared file, copied, mutually pinned by hash** (chosen). Cheap, works with two independent
CIs, and the failure is loud.

**B. A published package consumed by both** — rejected: the panel is embedded in a Rust binary via
`include_str!`, and a new build step on one HDD for six numbers is worse than the drift it prevents.

**C. Style the panel only, treat the console as the spec** — rejected by the operator's directive:
both surfaces are to move, and the console's Home blocks are ad-hoc too.

## Order of work

1. The contract and both gates (no visible change).
2. **The panel's page column** — the largest measured gap (2,471 px of content at 2,560).
3. The panel's card primitive, and Settings' nested folds collapsed.
4. The console's Home blocks onto the card grammar.
5. The console's rail: declared 200, measured 232 — the declaration becomes the measurement.
