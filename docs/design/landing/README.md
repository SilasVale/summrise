# The landing page: decisions and design record

> **Status: decided and shipped, 2026-10-28.** The operator chose the light product-page direction
> (`b-light`): English by default with a Chinese switch. It is built in `landing/` and deployed at
> **https://summrise.saisi.online**. The four directions in this directory stay as the record of
> what was rejected and why; each is a self-contained HTML file, so the record costs no binaries.

## The story the page tells

The page expresses the product statement that lives in **[`docs/PRODUCT.md`](../../PRODUCT.md)** — the
one sentence, the four nouns and the four pillars. That file is the single source; this one records the
page's own decisions and must not restate the product.

Two things follow from the pillars for this page specifically:

- the **brake** is the buying objection, so "approvals and audit" is a capability section, not a
  footnote — a page that only says "give your agent a shell" is every competitor's page;
- the **distance** pillar is why the console and the panel are shown as two surfaces with different
  jobs, rather than as one app with two windows.

## What the page has to do

1. **Say what it is in ten seconds** — hero, one sentence, one product shot.
2. **Answer "is it safe to hand over"** — grants, approvals, audit, the keyboard hand-back.
3. **Get to a download in one click** — installer, checksums, release document, and a quick start.

Only numbers that come from somewhere. The version and the download links are fetched from the newest
**published** release at load; nothing is typed twice.

## The four directions

| file | the argument it makes | shape |
|---|---|---|
| `a-editorial.html` | calm, documentation-adjacent, developer-credible | light paper, big type, one framed console shot, feature grid |
| `b-dark.html` | the agent is already running; this is an engineering product | near-black, warm glow, terminal card, hairline feature rows |
| `c-controlroom.html` | control is the product; the page should feel like the console | warm paper, orange, the console bleeding off the right edge, "watch it work / take it back" |
| `b-light.html` | **chosen** — the product page in daylight: the terminal is the proof, the fleet strip is the scope | warm off-white, dark terminal card, two-column capability grid |

`c-controlroom`'s headline ("Watch it work. Then take it back.") was the strongest single line of the
four and was carried into the decision conversation, but not into the shipped page: the chosen layout's
headline ("Your AI. A real machine under it.") states the product rather than the argument, which is
what the operator picked.

## The decisions

| decision | taken | what it cost |
|---|---|---|
| direction | `b-light`: light product page, terminal demo, fleet strip | the C mock's "why it exists" band was dropped — the page argues with product, not with a manifesto |
| language | English in the markup, Chinese in one dictionary, a persisted switch | two strings per element; `check.mjs` fails if either side is missing |
| host | `summrise.saisi.online`, a Workers static-assets site | `workers.dev` is poisoned on this network, so the custom domain is not cosmetic — it is the only reachable door |
| download | the newest *published* GitHub release, fetched at load | the newest published release is `v0.3.12` while the fleet runs `0.3.152`; the page must not claim otherwise |

## Apple's register, applied to this page

`docs/PRODUCT.md` carries the rules. What they changed here, concretely:

1. **The product is the hero.** No diagram, no illustrated metaphor: a terminal that enrolled a device
   and an MCP client listing its tools, then the fleet strip.
2. **One accent, one job.** Orange is the action. One filled button per region; the nav button is dark,
   everything else is ghost or text.
3. **Type does the hierarchy.** One scale, tight tracking on display sizes, 1.6–1.7 leading, balanced
   headings. No second colour to make a heading louder.
4. **One spacing rhythm.** Sections step on the same padding; the only exception is the hero, which is
   allowed to breathe more because it is the first screen.
5. **Motion explains change.** The terminal cursor and a 1px button lift; `prefers-reduced-motion`
   removes both without removing information.
6. **Measured, not argued.** Contrast is a gate (14 pairs at AA). `--faint` measured 3.31:1 and moved to
   `#6f6a60` (4.90:1); the gate names the pair, so taste cannot move it back.
7. **Deletion is part of the design.** Cut from the mockups: the manifesto band, the three-step
   installer section, the pinned version and a second accent. Each was defensible; none was the product.

## The one picture

The page argued that the product is the hero and then showed no product. Since 2026-10-28 a single
framed shot sits under the hero: `landing/public/assets/console-fleet.png`, cropped from the console's
own review renderer (the sanitized fixture, so the devices are STUDIO-PC / LAB-PC / EDGE-PC and the
ids are fabricated), and cut above the status strip so no build number is baked into a marketing
asset. One picture, no carousel, no device mockup around it — the same rule as the terminal.

## What would make this page wrong

If the download has to be hosted by the product instead of a releases page, or if a second audience
needs its own page (a Chinese-only site, a developer docs portal), the single-page shape stops holding
and this becomes two pages rather than one page with more sections.
