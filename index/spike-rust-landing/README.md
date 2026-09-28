# Spike: can the landing page be Rust?

A spike, not a migration. It answers one question on the smallest surface — the
landing page, `index/src/page.js` — and it was built to be able to come back
**against** the idea. Reproduce with `./build.sh`; measure with `node measure.mjs`.

The page it replaces is pinned by blob `3434d7a0` (530 lines). The live page at
`https://agent.saisi.online/` renders **byte-identical** from that file, so the
comparison is against what is actually served, not against a description of it.

---

## 1. wasm bundle size — against 31,718 bytes served (9,963 gzipped)

| | bytes | gzipped |
|---|---:|---:|
| cargo, before `wasm-bindgen` | 704,312 | — |
| after `wasm-bindgen` | 65,348 | — |
| **after `wasm-opt -Oz`** | **58,016** | **33,935** |
| wasm-bindgen JS glue | 18,582 | 4,243 |
| **payload over the wire (wasm + glue)** | | **38,178** |
| the Rust page's HTML | 27,427 | 8,339 |
| **Rust page, total over the wire** | | **46,517** |
| **the page it replaces, total** | 31,718 | **9,963** |

**The migration is 4.7x the page's weight over the wire.** The wasm alone (33,935
gzipped) is 3.4x the *entire* current page (9,963 gzipped).

Command: `WASM_OPT=... ./build.sh` — the sizes are the last four lines it prints.
`gzip -9 -c` on the `.wasm`; `curl -sS -H 'Accept-Encoding: gzip'` on the live page.

### The single biggest number in this spike is a formatting call

The first working build was **42,814 gzipped**. Changing one line — the particle
field's `format!("rgba({}, {}, {}, {:.3})", ...)` to integer thousandths — took it
to **33,935**. `{:.3}` on an `f64` links Rust's float formatter into the binary;
thousandths as an integer draw the same pixels.

**8,879 bytes gzipped, ~26% of the payload, bought by not formatting a float.**
A naive port pays that tax and never sees it.

---

## 2. First render — 1280x900, one browser, one session, one evaluate

Four targets, the same `page.evaluate`, measured back to back. `pagejs` and `rust`
are served from the same loopback server, so the only difference is the page.

| | live | pagejs (local) | rust (local) | **rust, wasm delayed 2s** |
|---|---:|---:|---:|---:|
| first contentful paint | 820–1920 ms | 108 ms | 96 ms | **92 ms** |
| largest contentful paint | 820–2908 ms | 108 ms | 96 ms | **92 ms** |
| DOM content loaded | 3278–4825 ms | 70 ms | 61 ms | 68 ms |
| wasm ready | n/a | n/a | 79 ms | **2099 ms** |

**The last column is the whole design.** With the binary held back two full seconds,
first paint is *unchanged* at 92 ms. The text does not wait for the binary, because
the text is not produced by the binary — it is in the HTML before the module is
fetched. On loopback the wasm is fast enough to hide this; the delayed arm is what
makes it a measurement instead of a claim.

The live column is over the network (Cloudflare, from this box) and is context, not
the comparison; `pagejs` vs `rust` is the like-for-like, and Rust is 12 ms *faster*
because the page no longer carries 4,608 bytes of inline JavaScript.

### The shape is identical, and it is identical by construction

`verify.mjs` renders the page with this branch's `page.js`, renders it with the Rust
renderer, normalises **only** the two places the migration is about, and requires the
rest to be byte-identical. It passes (**27,427 vs 31,718 bytes**).

Because the bytes are the same bytes, the brief's baseline holds by construction —
and it was re-measured anyway, on all four arms:

| brief's baseline | live | rust |
|---|---|---|
| 0 contrast failures | 0 | 0 |
| worst 4.63 : 1 | **4.63** | **4.63** |
| type ladder `[16, 14, 13, 12, 11]` | exact | exact |
| h1 at 16px/600 | exact | exact |
| no `h2` | 0 | 0 |
| cost sentence above the button | `true` | `true` |
| button filled `rgb(176,58,10)`, radius 20px, 492x40, white | exact | exact |

**AND THE BEHAVIOUR WAS TESTED, NOT ASSUMED.** Clicking `.theme-toggle` flips the
attribute and writes `localStorage` (`toggleWorks: ok`), and the particle canvas has
non-zero painted pixels (`canvasPainted: ok (816 painted px)`, against 734 for the
JS original — both fields are random). A canvas in the DOM would have proved only
that the module parsed.

---

## 3. Source line count — 838 against 530

| | lines |
|---|---:|
| `index/src/page.js` (the file replaced) | **530** |
| — of which CSS | 291 |
| — of which the favicon data URI | 1 |
| Rust `src/*.rs` + `src/bin/*.rs` | 538 |
| Rust `assets/` (CSS 292, note 8, favicon 1) | 301 |
| **Rust total** | **838** |

**1.6x overall; 2.3x once the shared CSS is excluded** (page.js 239 lines of actual
logic against Rust's 546). The CSS is the same stylesheet either way and is not the
migration; the logic is, and it more than doubles.

The Rust is not padded: 538 lines carry a real WHATWG URL whitelist (the `url` crate,
because `new URL(u, base)` resolves *relative* strings and a prefix match would have
rejected a legitimate installer path), HTML escaping, the two-arm setup block, and a
faithful port of the particle field including DPR handling, per-frame palette
resolution and the reduced-motion guard.

---

## The framework choice, on evidence

A framework's floor was measured, not looked up: the smallest app that has a heading,
a button, one reactive signal and one click handler — and nothing else. Then
`wasm-bindgen` → `wasm-opt -Oz`, the same pipeline as the spike.

| | wasm `-Oz` | wasm gz | glue gz | **total gz** |
|---|---:|---:|---:|---:|
| **this spike (no framework)** | 58,016 | 33,935 | 4,243 | **38,178** |
| Sycamore 0.9 | 46,307 | 21,138 | 4,137 | 25,275 |
| Leptos 0.8 (csr) | 66,726 | 28,119 | 5,275 | 33,394 |
| Yew 0.23 (csr) | 119,411 | 51,244 | 5,824 | 57,068 |

**Chosen: no framework.** The reasoning is the table, not a preference:

* **The cheapest framework's floor (Sycamore, 25 KB gz) is 74% of this page's entire
  payload (34 KB gz)** — and that floor is a counter. The particle field would still
  have to be written on top of it.
* **This page has no component tree, no routing and no state graph.** Two behaviours,
  both DOM-level. A framework's product is the reactive model, and this page has
  nothing for it to model. Leptos would be the credible choice if a framework were
  required — it is the smallest of the three that does SSR *and* islands, which is
  exactly the architecture this spike validated — but "smallest framework" is still
  larger than "no framework" for work that is not a component tree.
* **Dioxus was not measured**: the current `dioxus-web` on crates.io is
  `0.8.0-alpha.1`, and a prerelease is not a basis for a migration decision.

**This is a statement about this surface only.** See the trade below.

---

## The honest trade

**On this page, Rust loses on every axis except one.**

It is 4.7x the weight over the wire (46,517 gzipped against 9,963), 1.6x the lines
(838 against 530), and it buys — for a static page — nothing a reader can see. The
page has two behaviours totalling ~85 lines of JavaScript. Replacing 85 lines of JS
with a 58 KB binary is not a good trade, and a spike that reported otherwise would
have been confirming a thesis rather than testing one.

The one axis it wins is the one that does not show up in a landing page: **the
behaviours are now typed, and the same language as the agent.** The particle field's
palette resolution, the reduced-motion guard and the theme state are the kind of code
that silently rots in JS and fails at compile time in Rust. That is worth something —
just not 38 KB on a page whose whole job is one download button.

**And the architecture the spike validated is the transferable result**, not the
framework: *render in Rust at build time, ship the text as HTML, hydrate the
behaviours from wasm after paint.* That is what makes the 92 ms first paint possible
with a 2-second binary, and it is the only shape in which Rust→wasm belongs on a
public page at all.

### What that implies for the panel and the console

They are ~65,000 lines of TypeScript on Radix and Tailwind, and **the two numbers
above do not transfer to them — the ratio inverts.** A landing page is nearly all
text and nearly no logic, so a binary is pure overhead. The panel is nearly all logic
and very little text: a component tree, a state graph, an event stream, and a device
protocol. That is precisely what Leptos and Dioxus exist to express, and it is where
a 30–50 KB runtime stops being a tax and starts being the cheapest part of the page.

So the honest reading of these three numbers is **not** "Rust is too big for this
product". It is:

1. **Do not migrate the landing page.** 38 KB to replace 85 lines of JS is a
   regression a reader can feel, and this page is one of the five surfaces the
   product is judged on. Leave it in JS. (If the operator wants Rust here for
   consistency, the honest version is: keep the HTML as HTML, port only the two
   behaviours, and accept 38 KB for type safety on ~85 lines. That is a preference,
   not a win.)
2. **The panel and the console are a genuinely different question**, and this spike
   cannot answer it — but it did produce the two facts that bound it: the framework
   floor is 25–33 KB gzipped, and the "text in HTML, hydrate after paint" architecture
   works and is measurable. A migration there should be judged on the component tree
   it replaces, not on this page's ratio.
3. **Budget for the formatter.** The 26% saving from one `format!` is the warning that
   the first Rust build of any of these surfaces will be materially larger than its
   floor, for reasons that have nothing to do with the design.

---

## Running the measurement

`measure.mjs` needs `playwright-core` and a Chromium. On this box neither is in the
spike (both are gitignored), and the recipe is the one
`agent/resources/panel-react/scripts/local-browser.mjs` documents:

```bash
mkdir -p /tmp/chromium-libs/{debs,prefix} && cd /tmp/chromium-libs/debs
apt-get download libatk1.0-0 libatk-bridge2.0-0 libxkbcommon0 libgbm1 libpango-1.0-0 \
  libxcomposite1 libxdamage1 libxfixes3 libxrandr2 libatspi2.0-0 \
  libharfbuzz0b libthai0 libwayland-server0 libdatrie1 libgraphite2-3 libwayland-client0
for d in *.deb; do dpkg-deb -x "$d" ../prefix; done

ln -s <a checkout with playwright-core>/node_modules node_modules
LD_LIBRARY_PATH=/tmp/chromium-libs/prefix/usr/lib/x86_64-linux-gnu \
SUMMRISE_CHROMIUM_PATH=~/.cache/ms-playwright/chromium-1228/chrome-linux64/chrome \
  node measure.mjs
```

## What this spike deliberately does not do

No gate, no CI job, no `build.sh` wiring — per the operator's instruction. `verify.mjs`
is not a test harness; it is the fidelity proof for the three numbers above, and it
asserts the honesty properties (cost sentence above the button, fallback channel
present, no installer button on a tgz-only release) because those are the claims the
page makes.

**Nothing about the live worker changes.** `dist/` is committed so the artifact can be
opened and re-measured without a Rust toolchain; `target/` and `node_modules/` are not.
