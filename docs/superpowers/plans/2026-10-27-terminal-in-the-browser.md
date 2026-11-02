# The terminal in the browser

**Date:** 2026-10-27
**Status:** research, not a plan to execute. No code was changed. Claims that rest on something I
built or opened say so (*measured*, *read*); anything I could not check is marked *unverified* and
collected in the last section.

## Problem

Two pages render a browser terminal: the console's terminal page (`maud` in
`device/console/src/http/page.rs::terminal`, glued by `device/console/assets/terminal.js`) and the
device panel (`device/device/src/panel.html`, whose glue is being moved to
`device/device/assets/panel-terminal.js` by the panel-in-Rust plan of the same date). Both load the
same vendored xterm.js — byte-identical copies, `7ab162c8b43e3d6590400143d9b4388f`, 289,441 bytes
each — plus a 1,497-byte FitAddon and 5,559 bytes of CSS.

That is the product's one acknowledged exception to *PREFER RUST*. `AGENTS.md` names it: "The only
JavaScript left is vendored xterm and the terminal page's glue (`device/console/assets/`)". The
console-page spec says the same: xterm.js "is the one browser API the [project] does not own".

The question this document answers: what would it take to replace xterm.js with a terminal emulator
written in Rust, compiled to WebAssembly, for both pages — and should we?

The short answer is at the end. It is **keep xterm.js**, with reasons that are measurements rather
than preferences, and a list of conditions that would change it.

## What the two pages do today

**The console page.** `page::terminal` renders `#terminal` (one hidden div, `data-device`,
`data-csrf`) inside `#terminal-panes`, a `#terminal-tabs` strip, and `#terminal-status`. The glue
(`device/console/assets/terminal.js`, 229 lines, 7,012 bytes / 2,676 gzipped) creates one xterm per
session with `{cursorBlink: true, fontSize: 13, scrollback: 5000, theme:{background:#000000,
foreground:#d8d4cd, cursor:#e07a4a}}`, loads the FitAddon, and then runs three loops:

- **read**: `GET /next/terminal/:device/:id/read?cursor=N` every 200 ms; the answer is JSON
  `{data, cursor, alive, exit_code, signal}`; `term.write(answer.data)`.
- **write**: `term.onData` accumulates into a string, flushed as one POST after 40 ms or at 4,096
  characters — "one write per tick, not one per keystroke: a paste is thousands of onData calls".
- **resize**: `term.onResize` → `POST .../resize {rows, cols}`, driven by FitAddon's `fit()`; every
  tab is refitted when it is activated and on window resize.

**The device panel.** The same shape, smaller: `/api/terminal/{create,read,write,resize,close}`,
`term.write(answer.text)` every 150 ms, a 40 ms / 4,096-character write coalescer, `term.reset()` on
a new session, `fit.fit()` on resize. The panel's create call is fixed at `rows: 30, cols: 100`
before the first `fit()`.

**The wire is text, and lossily so.** The device's `terminal.read` renders the ring's bytes with
`String::from_utf8_lossy` (`device/device/src/mcp.rs`, the comment above the reply). The panel route
does the same (`device/device/src/http.rs::panel_terminal_read`, field `"text"`), and the console
relays the MCP answer unchanged (field `"data"`). The comment in `mcp.rs` says this is deliberate:
"a read can split a multi-byte sequence at the cursor, and the client's cursor is already past it,
so the replacement character is the honest answer rather than a retry the capability cannot prove
is safe."

That is honest, and it is also a real defect an operator can see: whenever a multi-byte character
straddles a poll boundary, the terminal shows U+FFFD — twice, one replacement for each half. For a
CJK operator this is not theoretical. It is worth fixing whatever happens to the emulator question,
because it is a property of the transport, not of xterm. Its existence matters here because **a
WASM emulator wants bytes**, so option (b) has this fix as a prerequisite (see *Integration*).

**The device does not emulate a screen, and does not pretend to.** The one thing in the Rust tree
that resembles an emulator is `terminal.screen`: `PtyRegistry::screen(id, lines)` truncates the ring
to its last N lines and `screen_text` removes escape sequences, and the tool's own description says
so — "as a person would see it — escape sequences removed, the last N lines". It cannot place a
cursor, honour a scroll region, show a colour or draw an alternate screen, and it is not a
candidate for what the browser does; it is what a *client without a terminal* gets. The browser's
emulator and this helper answer different questions, and a WASM terminal would not retire it.

**What xterm.js is doing for those pages right now**, in its own words from the vendored bundle's
behaviour and the legacy React pane's history: VT parsing, the grid, the scrollback, the alternate
screen, mouse reporting modes, bracketed paste, title changes (OSC 0/2 — the console glue listens on
`onTitleChange`), 256-colour and truecolour SGR, wide and combining characters, selection and copy,
the hidden textarea that carries IME composition, accessibility rows, cell measurement, and the
paint. Both pages use its **default DOM renderer** (neither loads a renderer addon).

## What the legacy repository actually does

I looked, because the prompt asked, and the answer is unambiguous: **xterm.js, twice, and no Rust
terminal emulator anywhere.**

- `agent/resources/panel/vendor/xterm.min.js` (283,670 B), `xterm-addon-fit.min.js` (1,789 B),
  `xterm.css` (5,383 B) — the vanilla panel served by the Rust agent, wired by the bundled
  `agent/resources/panel/panel.js` (677,870 B, a minified React + panel bundle).
- `agent/resources/panel-react/src/components/TerminalPane.tsx` — the React panel. It imports
  `@xterm/xterm ^5.5.0`, `@xterm/addon-fit`, `@xterm/addon-search`, `@xterm/addon-webgl`
  (`agent/resources/panel-react/package.json`), scrollback 20,000, a per-pane font size in
  `localStorage`, Ctrl+F search, right-click paste, and two theme palettes of sixteen ANSI colours.

Two of that file's comments are the most useful evidence in either repository, because they are
scars:

- "**The WebGL renderer is gone**: on some GPUs/WebView2 the context creates fine but paints nothing
  (a silent blank terminal, not catchable)." The legacy project shipped a GPU renderer and removed
  it.
- "`reflowOnResize` re-wraps the buffer on a grid change — without it, a session adopted while
  `display:none` (inactive) stayed at the 80x24 grid with blank space (the 'half screen' bug)."

I grepped every `Cargo.lock` in the legacy tree for package entries named `alacritty_terminal`,
`vt100`, `vte`, `termwiz`, `wezterm-term`, `avt`: **zero hits**. I grepped the two wasm crates that
would plausibly hold one — `agent/resources/panel-logic` and `gateway/ui-logic` — for `vt100`,
`alacritty`, `emulator`: **zero hits**. The legacy repo's Rust-in-the-browser work is *logic*, not
rendering (`panel-logic`'s own header: "a crate of FUNCTIONS, not a second component tree … that is
why there is no `yew`/`leptos`/`sycamore` in Cargo.toml"). Its terminal stayed JavaScript in both
of its panels, and its console rendering is still React.

What the legacy repo *does* provide is the wasm precedent and its price list, measured on this same
machine (`agent/resources/panel-logic/Cargo.toml`, quoted verbatim):

```
serde_json          43,824 gz wasm + 2,140 gz glue   (a JSON string in, a JSON string out)
serde-wasm-bindgen  31,046 gz wasm + 3,967 gz glue   (JsValue in, JsValue out, no text)
this crate, js-sys    see build.sh's output           (JsValue in, JsValue out, no serde)
the floor for ANY wasm crate here is 7,016 gz wasm + 2,089 gz glue
```

That floor is the number to hold on to: a wasm-bindgen module that does almost nothing costs ~7 KB
gzipped of wasm and ~2 KB of glue. It also carries a **committed artifact + freshness gate**
pattern the product can copy: `agent/summrise-url-policy` ships a 1,108,704-byte module, and
`scripts/test/url-policy-wasm-freshness.bash` rebuilds it and compares the bytes. The product repo
has no wasm crate, artifact or CI job today (the word appears only in specs); its
`.github/workflows/ci.yml` is rustfmt, clippy `-D warnings`, `cargo test`, and the Windows lifecycle
suite.

## The candidate table

Everything in the "wasm" column was **built today on this box**: cargo 1.98.1, target
`wasm32-unknown-unknown`, a probe crate with `crate-type = ["cdylib"]`, profile
`opt-level="z", lto=true, codegen-units=1, panic="abort", strip=true`, then `wasm-opt -Oz` (binaryen
132) and `gzip -9`. The probe drivers allocate a grid, feed bytes, and read cells back — a realistic
minimum, not the full host. Sizes are *the emulation core plus a small driver*, not the product's
module. The baseline (a cdylib exporting `x + 1`, no dependency) is 117 bytes raw / 102 `-Oz`.

| crate | licence | builds for wasm32-unknown-unknown? | what it leaves to the host | `-Oz` wasm | `-Oz` + gzip |
|---|---|---|---|---|---|
| `vte` 0.15.0 | Apache-2.0 OR MIT | **yes** | everything except the byte-stream parser: no grid, no screen, no cursor, no scrollback, no input | 15,876 B | **6,919 B** |
| `vt100` 0.18.1 | MIT | **yes** | rendering, input encoding, selection/clipboard UI, font metrics, scrollback UI (it *has* a grid, cells, scrollback, alt screen, cursor state, mouse *modes*) | 50,416 B | **21,378 B** |
| `alacritty_terminal` 0.26.0 | Apache-2.0 | **no, as published** — `polling 3.11` has `compile_error!("polling does not support this target OS")`; `polling`, `libc` and `home` are unconditional, so all three are in the wasm graph | rendering, input encoding, clipboard plumbing, config; it *has* selection and `vi_mode` | — | — |
| the same crate, excised | Apache-2.0 | **yes**, after removing 3 modules and 3 deps (below) | rendering, input encoding, config | 84,052 B | **35,710 B** |
| `termwiz` 0.23.3 | MIT | **no** (three walls, below) | it is the only candidate that *encodes keys* (`KeyCode::encode`, `src/input.rs`), and it carries surface/widgets/line editing | — | — |
| `wezterm-term` 0.1.0 | MIT | **not attempted** — see below | grid + surface + selection + input types | — | — |
| `vte_gpu` | — | does not exist on crates.io (404) | — | — | — |

Notes on each row, because the details are where the decision lives.

**`vte`** is the Alacritty project's parser, and it is the floor for "parse the escape stream". It
gives a `Perform` trait (`print`, `execute`, `csi_dispatch`, `esc_dispatch`, `hook`, `put`,
`unhook`, `osc_dispatch`) and nothing else. To be a terminal you implement `Perform` and write the
grid, the SGR state machine, the scroll region, the charsets, the wide-character handling and the
scrollback yourself. That is option (c) with a head start on half of the parsing.

**`vt100`** is the smallest thing that is actually a terminal: "the terminal parser component of a
graphical terminal emulator pulled out into a separate crate" (its README). Its public surface on
`Screen` is the one to read before choosing it: `cell(row, col)`, `contents()`, `contents_between`,
`rows`, `cursor_position`, `alternate_screen`, `application_keypad`, `application_cursor`,
`hide_cursor`, `bracketed_paste`, `mouse_protocol_mode`, `mouse_protocol_encoding`, `cursor_style`,
`fgcolor`/`bgcolor` and the per-cell attribute flags, plus `set_scrollback`/`scrollback`. It has
**no selection type and no key encoder** — it reports that bracketed paste and the mouse protocol
are *on* and leaves the encoding to you. It does have OSC 52 callbacks
(`callbacks.copy_to_clipboard`, `src/perform.rs`), i.e. a hook, not a clipboard.

**`alacritty_terminal`** is the engine behind Alacritty: `Term::new(config, &dimensions,
event_proxy)`, a `vte::ansi::Processor` you drive yourself, `grid()`, `selection`, `vi_mode`. It is
the most complete grid in the set and it is the one whose published form cannot be built for wasm,
for a reason that has nothing to do with emulation: the crate bundles the *host* half. `src/lib.rs`
exposes `pub mod tty; pub mod event_loop; pub mod thread;` unconditionally, and `Cargo.toml` has
non-optional `polling`, `libc` and `home` dependencies. `polling` is the wall. There is no feature
flag for it; `[features]` is `default = ["serde"]` and `serde`, nothing else.

I proved the excision is small and that what remains is portable. In a copy of the crate I removed
`pub mod tty; pub mod event_loop; pub mod thread;` (651 lines of the 0.26.0 source), the
`polling`/`libc`/`home` dependency tables, and the two `[target."cfg(unix)"…]` /
`[target."cfg(windows)"…]` tables; it then compiled for `wasm32-unknown-unknown` and ran a
`Term::new` → `Processor::advance` → `grid[Point]` driver. The measured module is 84,052 B `-Oz` /
35,710 B gzipped. What that number does **not** include is `vi_mode` and `selection` code that the
driver never touched (LTO strips it), so a host that uses selection should expect more.

The cost of that route is not the patch, it is the **fork**: vendoring `alacritty_terminal` means
re-applying the excision on every upstream release, or pinning forever. The product already vendors
`device/vendor/portable-pty` for a similar reason (`vendor/portable-pty`, 128 KB, with a
`Cargo.toml.orig`), so this is a familiar cost, not a foreign one. The fork also brings something
worth having: the crate ships **45 reference cases** under `tests/ref/` (46 MB), each a directory of
`alacritty.recording` (raw PTY bytes), `size.json`, `config.json` and `grid.json` (the expected
serialized grid) — `vttest_cursor_movement_1`, `vttest_origin_mode_1`, `vim_24bitcolors_bce`,
`vim_large_window_scroll`, `tmux_htop`, `zsh_tab_completion`, `fish_cc`, `hyperlinks`, `zerowidth`,
`history` and more (`tests/ref.rs`). That is a ready-made seed for the corpus the verification
section below asks for — with the caveat that its expected grids are *Alacritty's*, not xterm's.

**`termwiz`** is the interesting failure. It is the only candidate that ships the *input* half —
`KeyCode::encode` in `src/input.rs` — plus `Surface` and widgets, which is exactly what a browser
terminal needs. It does not build for wasm, and I measured three separate walls:

1. `uuid 1.29.0`: `compile_error!("to use uuid on wasm32-unknown-unknown, specify a source of
   randomness using one of the js, rng-getrandom, or rng-rand features")`. Parent, measured with
   `cargo tree -i uuid`: `wezterm-blob-leases 0.1.1 → termwiz`.
2. with `uuid/js` enabled: `getrandom 0.3.4`: "The wasm32-unknown-unknown targets are not supported
   by default; you may need to enable the wasm_js configuration flag" — a `RUSTFLAGS` global, not a
   crate feature.
3. with `uuid/js` + the `getrandom_backend="wasm_js"` cfg: `mac_address 1.2.1` fails to compile
   (`use of unresolved module or crate os`) — a machine-identity crate with no wasm implementation.

All three are in the same corner of the graph (blob leases), not in the emulator. In the *published*
0.23.3 manifest `wezterm-blob-leases` is **non-optional**, so no feature combination removes it; the
git `main` manifest (0.24.0) marks it optional. I tried that build and could not finish it: cargo's
fetch of the wezterm monorepo had pulled **1.8 MB in about 25 minutes** on this box before I stopped
it, so whether termwiz clears the walls with blob leases off is **unverified** — and the fetch itself
is a fact worth recording, because that monorepo is where both termwiz's git form and `wezterm-term`
live. Even if it builds, `terminfo`, `filedescriptor` and `libc` are unconditional dependencies of
termwiz, so a wasm build carries a terminfo database and file descriptor plumbing a browser cannot
use.

**`wezterm-term`** is not a published crate. `crates.io/api/v1/crates/wezterm-term` answers 404 and the
sparse index has no entry; the crate lives in the wezterm monorepo (`term/Cargo.toml`, licence MIT,
version 0.1.0), so it would be a **git dependency on the whole wezterm workspace** — which this box
could not fetch at a useful speed (1.8 MB in about 25 minutes, measured while trying termwiz's git
form). Its manifest depends on `termwiz` (with `use_image`), `wezterm-surface`, `wezterm-cell`,
`wezterm-escape-parser`, `image`, `url`, `miniz_oxide`, `terminfo`, `humansize` and more. Since it
depends on termwiz and `wezterm-surface`, its wasm story is at best termwiz's, on a larger graph; I
did not attempt the build, so **unverified** — but the dependency list is a *read* fact, and it is
enough to rank it last.

**`vte_gpu`** does not exist as a crate on crates.io (404 on both the API and the sparse index). I
did not find a repository for it and will not guess at one; if it is a project rather than a crate,
it is a renderer for a terminal, not an embeddable emulator, and nothing in either repo mentions it.

## The rendering half

This is the part that decides the outcome, because it is the part no crate provides. Written out,
here is what xterm.js does for the two pages today and what a WASM terminal would have to
re-implement — all of it in project code, maintained by us.

**Painting a grid.** Three credible strategies, with the trade-off that matters:

- **DOM spans** (xterm's default renderer, and what both pages use today). One element per row, text
  nodes inside, attributes as classes. Free: text selection, copy, browser find, accessibility,
  screen readers, and text scaling. Cost: a row is a DOM node tree, so a full-screen 80x24 repaint
  touches ~24 row elements plus the attribute spans inside them; fine at 200 ms polling, and it is
  why xterm's default renderer has been good enough that neither page ever asked for another.
- **Canvas 2D.** One `fillText` per run of same-attribute cells, `fillRect` for the background.
  Fast and predictable under a flood, and *this is what a 5,000-line scrollback wants*. Cost:
  **there is no text on the page**. Selection must be hit-tested from mouse coordinates into
  (row, col) and painted as a rectangle; copy must be assembled from the grid; browser find, spell
  check, accessibility and OS text scaling are gone unless rebuilt. This is the single largest piece
  of hidden work in option (b), and it is the reason a canvas terminal usually feels "almost right"
  and is worse for a human who wants to copy an error message.
- **WebGL.** The legacy React pane shipped it and removed it: "on some GPUs/WebView2 the context
  creates fine but paints nothing (a silent blank terminal, not catchable)". Do not go here first.

**Font metrics and DPI.** A cell is a monospace character's advance width by the line height. Get
this wrong and every column drifts. xterm measures with a hidden DOM span (or a canvas
`measureText` for the canvas renderer), then multiplies by `devicePixelRatio` for the canvas
backing store and sets a CSS size in device-independent pixels; `FitAddon` divides the container's
content box by the cell size and floors, which is where rows/cols come from. A WASM terminal has to
either duplicate that measurement in JS (and then the Rust side only does the grid, which is the
right split) or measure inside wasm via `web-sys` (which drags the DOM into the module). Add to this:
font family fallback (`ui-monospace, "SF Mono", …, monospace` in the legacy pane), a per-pane font
size (the legacy pane has 9–22 in `localStorage`; the console page has a fixed 13), zoom, and the
fact that a fractional `devicePixelRatio` (1.25, 1.5 on Windows) makes a cell land on a half pixel
unless you round and re-derive.

**Wide characters, combining marks, emoji.** CJK is two cells; a combining accent is zero cells
applied to the previous cell; an emoji may be two cells and may be a ZWJ sequence. The grid model
must hold a cell that is a *base plus marks*, and the renderer must not let a wide character overlap
its neighbour. This product's operator is addressed in Chinese (`AGENTS.md`: "TALKING TO THE
OPERATOR IS IN CHINESE"), so this is not an edge case.

**Selection and copy.** Mouse-down/drag/up → anchor and focus in grid coordinates, then a rectangular
or linear range, then text that joins wrapped rows without inserting newlines, trims trailing
spaces, and respects the alternate screen (a selection does not survive a `vim` exit). Double-click
word selection, triple-click line, Shift-click extension, autoscroll while dragging past the edge.
Then the clipboard: `navigator.clipboard` needs a user gesture and a secure context (loopback is
secure, so the pages qualify), right-click paste (the legacy pane has it), and the middle-click
convention if you want it.

**IME composition.** xterm keeps a hidden `<textarea>` focused behind the grid and translates
`compositionstart`/`compositionupdate`/`compositionend` into text, positioning the candidate window
at the cursor cell. Without this, a Chinese or Japanese operator cannot type into the terminal at
all. This is genuinely hard to get right, it is invisible when it works, and it is the piece a
from-scratch terminal most often ships broken.

**Scrollback.** The device keeps 1 MiB per session (`BUFFER_BYTES` in
`device/device/src/capabilities/terminal.rs`) and the browser keeps its own history — 5,000 lines in
both current pages, 20,000 in the legacy React pane. A scrollback that is a `Vec<Row>` is easy; the
parts that are not: the alternate screen must not enter it, `resize` must **reflow** wrapped rows
(the legacy "half screen" bug is exactly a missing reflow), and the scrollbar must express "the
device's ring may have dropped bytes you never saw" differently from "you are scrolled up".

**Mouse reporting, bracketed paste, focus events, title, bell, OSC 52, OSC 8 hyperlinks.** Each is
small and each is a way to be subtly wrong. vt100 hands you the *mode flags* and stops; xterm.js
implements both the modes and the encoding.

**The honest summary of this section:** the grid is a few days; the *terminal as a human uses it* is
weeks, and the failure mode is not a crash, it is a page that works until someone selects text, or
pastes, or types Chinese, or opens `vim` in an 80-column pane after a resize.

## Integration: page → wasm-bindgen → the existing `terminal.*` API

The transport does not have to change shape. The glue keeps its `fetch` loop and its coalescer; it
gains one call per poll and a render step.

```
read loop (unchanged)                 wasm (Rust)                     page (JS)
GET .../read?cursor=N  ──► JSON ──►  feed(bytes)          ──►  take_damage()  ──► paint rows
term.onData            ◄── String ◄─  (or encode_key)     ◄──  keydown/composition
POST .../write {data}
```

**What the Rust side must export**, as a sketch to cost rather than a design to implement:

- `new(cols: u16, rows: u16, scrollback: u32) -> Terminal`
- `feed(&mut self, bytes: &[u8])` — the whole point of wasm here: bytes in, no string conversion.
- `resize(&mut self, cols: u16, rows: u16)` — must reflow, not truncate.
- `damage(&mut self) -> Damage` — the dirty row range plus a `full` flag; xterm's damage model is
  the one to copy, and `vt100`'s `state_diff`/`rows_diff` is a worked example of the same idea in a
  crate that builds.
- `row(&self, y: u16, out: &mut [u8])` — a flat buffer of `{ch: u32, fg: u32, bg: u32, flags: u16}`
  per cell, written into a JS-owned `Uint8Array`, so a frame allocates nothing and crosses the
  boundary once per dirty row.
- `cursor(&self) -> {x, y, visible, style}`, `title() -> Option<String>`, `bell(&mut self) -> bool`
- `selection_start/extend/clear/text()`, `scroll(delta)`, `scrollback_len()`
- `encode_key(code, mods, kind) -> String` and `mouse(button, x, y, mods) -> Option<String>` — the
  input half; this is the export no candidate crate provides except termwiz.
- Optionally `paste(data)` for bracketed paste, and `focus(bool)`.

**Two splits are worth naming before anyone estimates.** The first is *Rust owns the grid, JS owns
the pixels*: wasm returns cells and damage, a ~200-line canvas renderer in JS blits them. This keeps
the module at the measured sizes above, keeps `devicePixelRatio` and font measurement where the DOM
is, and matches the repo's own wasm rule — `docs/superpowers/specs/2026-10-08-every-decision-is-rust-design.md`
§6.6: "wasm crates in this repository are PEERS, and they compose through the HOST", written after
two wasm crates that depended on each other shipped the module graph twice. The second is the
opposite: Rust also owns the canvas via `web-sys`. That adds `web-sys`/`js-sys` to the module (the
legacy measurement says js-sys alone is affordable; `serde_json` was the 37 KB mistake, not
web-sys), but it puts the font metrics behind the boundary and makes the module's exports wider —
against the same §6.6 instinct. I would take the first.

**The wire change that option (b) forces.** `feed(bytes)` needs bytes, and both routes carry
lossy text today. The additive fix is a `data_base64` field on `terminal.read`'s answer (device
`mcp.rs` + `panel_terminal_read`, console relay pass-through, and the two glues decode with
`atob`/`Uint8Array`), keeping `data`/`text` for older clients — the same additive pattern
`alive`/`exit_code`/`signal` used in `terminal.read` per the session-end plan. This is worth doing on
its own: it removes the U+FFFD class of bug from the terminal that ships *today*.

**Where it would live.** A workspace member (or its own workspace, as `agent/resources/panel-logic`
is, to keep it out of the Windows build) — say `device/terminal-wasm`, `crate-type = ["cdylib",
"rlib"]` so `cargo test` can drive the same source natively, exactly the trick
`agent/summrise-url-policy/Cargo.toml` documents: "`cdylib` is what wasm-pack emits; `rlib` is what
`cargo test` links against, so the SAME source is both the shipped artifact and the thing the cases
run against." Then: a committed `.wasm` (the repo already commits a 1.1 MB one), a
`scripts/test/terminal-wasm-freshness.bash` that rebuilds and compares bytes, asset routes in
`device/console/src/http.rs` and `device/device/src/http.rs` with content type `application/wasm`
(the panel's asset table currently knows javascript and css only, and its test
`the_panel_serves_its_own_terminal_assets` asserts those three routes), and a CI job with the wasm
target and `wasm-pack` — none of which exist in `.github/workflows/ci.yml` today.

**Two consumers, one artifact.** Both binaries would `include_bytes!` the same module, just as both
`include_str!` the same xterm.js today (two copies, identical hashes). The console's `Asset`
responder caches for 86400 s with no content encoding; the module would be served the same way.

## The honest cost

Estimates in engineering days, for parity — where "parity" means the two pages do what they do now,
including selection, paste, IME, reflow and `vim`/`top` in the alternate screen. These are
judgements, not measurements; the measured parts are the sizes and the three build failures.

| work | days | why it is that size |
|---|---|---|
| crate choice, workspace member, committed artifact, CI job + wasm target, freshness script | 2–3 | mechanical, but new to this repo; CI has no wasm toolchain |
| the wire: `data_base64` on `terminal.read`, device + console + panel + tests | 2–3 | small and worth doing regardless |
| wasm boundary: exports, flat cell buffer, damage, wasm-bindgen glue | 3–5 | the §6.6 lesson says the seam is where the design time goes |
| canvas renderer: cell metrics, DPI, colours, cursor, damage blits, font size | 8–12 | the legacy WebGL incident is a warning about how this fails |
| input: key encoding (all modifiers, function keys, application cursor mode), IME composition, paste, mouse reporting | 8–12 | the half no crate but termwiz has; IME is its own project |
| selection, clipboard, scrollback UI, reflow on resize | 5–8 | the "half screen" bug says reflow is not free |
| the corpus harness and the parity fixes it finds | 10–15, and unbounded | every divergence is a real terminal behaviour someone depends on; Alacritty's 45 recordings seed the bytes, not the expected screens |
| **total** | **38–58** | for a terminal that is *at best* not worse |

For comparison, the thing being replaced: **289,441 bytes raw / 67,243 gzipped** of vendored
xterm.js, plus **7,012 bytes / 2,676 gzipped** of project glue that stays JavaScript either way
(it does `fetch`, the DOM and the coalescing). The measured Rust cores are **21,378 B gzipped**
(vt100, with a 5,000-line scrollback in the driver) and **35,710 B gzipped** (alacritty excised).
So the win, before writing a renderer, a selection model and an IME, is **31–46 KB gzipped**
(67,243 minus 21,378 and minus 35,710), against 38–58 days and a permanent maintenance surface. That
is the number that makes this a decision about ownership, not about bytes.

**The risks, concretely:**

- **Escape-sequence correctness.** xterm.js is the accumulation of a decade of bug reports from real
  shells, `vim`, `tmux`, `htop`, `less` and `ssh`. Neither `vt100` nor an excised `alacritty_terminal`
  has been exercised against *our* streams, and nothing in this document has run either against a
  single byte of ours. The first `tmux` or `htop` session will find divergences; the question is
  whether they are found by a corpus in CI or by an operator on a machine that is down.
- **Performance under a flood.** The device's ring is 1 MiB per session and `read` returns
  everything after the cursor; a `yes` or a kernel boot log arrives as large JSON strings every
  150–200 ms. The parser will keep up (vte is fast); the *paint* is the risk — a DOM renderer that
  diffs 24 rows per frame is fine, a naive canvas renderer that repaints the whole scrollback on every
  frame is not. And the existing failure mode is already there and unchanged: a cursor older than the ring is
  a typed refusal (`PtyError::CursorTooOld`), i.e. the client must keep up or lose bytes.
- **Input latency.** Today a keystroke goes straight to `term.onData` → fetch. A wasm round trip adds
  a boundary crossing and a render; on loopback that is microseconds, so this is the *least* risky
  item — but the reflow/repaint path can add a frame, and a terminal that echoes a keystroke late
  feels broken in a way a slow page does not.
- **A worse terminal, silently.** The failure mode is not a crash; it is "selection copies the wrong
  thing", "the cursor is one column off after a resize", "Chinese input works until it doesn't".
  Each is a bug report against the tool an operator uses to fix a broken machine.
- **The maintenance surface.** A VT parser, a renderer, a selection model and an IME bridge become
  ours; xterm.js's bugs are currently someone else's, fixed upstream, and vendored by a hash bump.
  The vendored `portable-pty` (with its `Cargo.toml.orig` and a 128 KB tree) is the model of what
  that costs, and it is one crate, not a terminal.

## Recommendation

The three options are the ones the question names: **(a)** keep xterm.js and accept the exception,
**(b)** build the terminal from a Rust crate compiled to wasm, **(c)** write a minimal renderer from
scratch. My recommendation is **(a)**, and make the exception pay rent instead of growing. If the
ownership argument ever wins, do (b) on `vt100`; (c) should not be considered.
The rest of this section says why, and what would change it.

**Why (a).** The rule in play is *PREFER RUST: "a change that adds another language needs a reason,
not a habit"*. The exception is already as small as an exception gets: no npm, no
lockfile, no build step, no project logic in the JavaScript beyond 229 lines of glue that would
survive the swap anyway (it does the fetching, not the deciding). The console-page spec already
records it as "two vendored libraries … not project source". Swapping the emulator would not remove
the JavaScript; it would remove *one vendored file* and add a Rust crate, a wasm toolchain, a
committed binary, a freshness gate and a renderer — a second artifact in a repo that just spent a
plan deleting its second build. And the measured saving is 31–46 KB gzipped, in a page whose own
`terminal.js` already costs 2.7 KB gzipped and whose xterm is cached for a day.

The strongest argument *for* (b) is not size or purity, it is the byte-accurate transport: a WASM
emulator would force the `data_base64` fix. But that fix does not need the emulator. Do it first.

**What making the exception pay rent looks like** (a day, not a project):

1. Record the vendored version and licence in `device/console/assets/` and
   `device/device/assets/` — today the file is 289 KB of minified JavaScript with no version string
   I could find in it (*unverified*; the legacy React pane pins `@xterm/xterm ^5.5.0`, which is a
   different artifact).
2. Make the duplication explicit — the two copies are byte-identical today, and a test that says so
   (or one shared `include_bytes!` path) is cheaper than discovering they diverged.
3. Fix the U+FFFD transport defect (`data_base64`), which is a real operator-visible bug and the one
   thing in this document that is worth doing whatever happens to the emulator.
4. Extend `scripts/console-smoke.mjs --terminal` with a CJK marker and an alternate-screen leg
   (`vim`-like), so "the terminal works" means more than "a marker echoed".

**Why (b) if it is ever chosen, and on what.** `vt100` (MIT, 21.4 KB gzipped, builds) for the grid,
with our own renderer, selection and input — accept that the input encoder is ours to write. Choose
the excised `alacritty_terminal` instead only if selection and `vi_mode` semantics matter more than
size and a vendored fork is acceptable (Apache-2.0, 35.7 KB gzipped, 651 lines excised, upstream
tracking forever). Do not choose termwiz until it builds for wasm; do not choose `wezterm-term` at
all while it is a git dependency on the wezterm monorepo through termwiz.

**Why (c) is rejected.** Writing a renderer and grid from scratch is the same 38–58 days as (b)
except the escape-sequence half is also ours, and `vte` (6.9 KB gzipped) already exists precisely so
that nobody has to write that half again. A hand-written emulator is how you get a terminal that is
worse than xterm.js in ways nobody predicted.

**What would have to be true for the recommendation to change:**

- A hard requirement that the product ship no third-party browser JavaScript (a customer or
  compliance constraint). Nothing in `docs/CHARTER.md` or `AGENTS.md` says that today; the spec says
  the opposite, explicitly.
- A Rust crate that ships the *whole* terminal for wasm — grid, input encoding, selection and a
  renderer — and builds without a fork. None exists today; termwiz is the closest and does not
  build, and its `KeyCode::encode` is the one piece worth watching.
- xterm.js becoming unmaintained, or a security advisory in it with no fix. It is vendored and
  offline, so the exposure is small, but a dead upstream would change the calculation.
- A second consumer that needs the same emulator outside a browser (a native console, an export
  renderer, a server-side replay of a session). Then the emulator is not being written *for the
  page*; the page becomes one host, and the amortisation changes everything.
- If the corpus harness (below) were built first and a wasm terminal measured at parity on it — that
  is the only evidence that could justify the swap, and building the harness is a few days, not
  weeks. It is the cheap experiment that should precede any commitment.

## How this would be verified

The only way to prove a new emulator is not worse is a **corpus of real byte streams replayed
through both emulators and compared screen-by-screen**, and the harness is worth having even if
option (b) is never taken, because it is also how the transport fix would be proved.

**Capture.** A fixture is a `.bin` of raw PTY bytes plus one line of provenance (what produced it,
on what device, what the expected screen is). Sources we already have: `terminal.read` from cursor 0
on a live session replays the ring exactly (the cursor protocol means the device can hand back
bytes); a Linux `script(1)` capture for the pty cases; a serial boot log for the serial cases. The
corpus should be small and nasty, not large and easy: `vim` entering and leaving the alternate
screen, `htop` redrawing with 256 colours, `tmux` splitting, `less` with a long line and a resize
mid-stream, a CJK `ls`/`echo` with wide characters, a combining-accent case, a truecolour gradient,
an OSC 0 title change, bracketed paste, mouse reporting, a kernel boot log with `\r` progress
rewrites, and a `yes` flood for performance.

There is already a seed for the byte half. `alacritty_terminal`'s `tests/ref/` carries 45 recordings
(see the candidate notes) covering vttest, vim, tmux, htop, zsh and fish — real streams, Apache-2.0,
each with the size it was recorded at. They are not *our* streams and their expected grids are
Alacritty's, so they can seed the corpus but cannot be the oracle: the xterm dumps still have to be
recorded. That cuts the fixture-writing work, not the differential work.

**Oracle.** `@xterm/headless` — the xterm project's core without a DOM, published on npm — runs in
Node as a **dev-only** dependency (never shipped, never in CI's product build) and dumps the screen at
the same offsets the Rust side dumps. Pin it to the same version as the vendored bundle, or the
differential measures two versions rather than two emulators: npm currently carries `@xterm/xterm` and
`@xterm/headless` at **6.0.0** (read, 2026-10-27) while the legacy React pane pins `^5.5.0`, and the
vendored bundle's own version is unrecorded (see the first item of the recommendation's rent list).

The comparison is over a canonical form: for every cell `(ch, fg, bg, bold, dim, italic, underline,
inverse, wide)`, plus `(cursor.x, cursor.y)`, the alternate-screen flag, the title, and the last K
scrollback lines. Diff that, not a screenshot: a pixel diff hides a wrong colour and a text diff over
`contents()` hides attributes.

**Where the comparison runs.** Two places, because they answer different questions:

- **In `cargo test`, against recorded dumps.** Record the oracle's dumps once with a small Node
  script and commit them next to the bytes; the Rust test then feeds the same bytes to the Rust
  emulator and diffs against the committed dumps, with no Node and no network in CI. This is the
  same shape as the oracle work the design record describes (`gateway/wasm/verify.mjs` in the legacy
  tree, and the url-policy differential "927 inputs against the shipping JavaScript, 0 differing").
- **In the browser, against the real page.** `scripts/console-smoke.mjs --terminal` already opens a
  shell, types a marker and reads it back through xterm, then opens a second session and requires
  two tabs. Extend it: a CJK marker typed through the page, a `vim`-like alternate-screen leg, a
  resize that changes the grid and then a `stty size`-style readback, and a selection copied to the
  clipboard. A WASM terminal passes the same smoke test as xterm — that is the point.

**Gates, so a divergence cannot land quietly:**

- The parity corpus fails on any *new* divergence, with a checked-in "known differences" list (each
  entry a sentence saying why, e.g. "no sixel"), so the list can only grow by review.
- A **freshness** gate: rebuild the module and compare bytes, the way
  `scripts/test/url-policy-wasm-freshness.bash` does for the legacy url-policy module.
- A **performance** gate: feed 5 MiB of `yes` output in a native test (not over the wire, where the
  ring caps at 1 MiB), assert the frame budget and that memory stays under a stated ceiling; then
  over the wire, feed a burst after a 3-second pause and assert the device's `CursorTooOld` refusal is
  still the client's problem (it is the device's contract, not the emulator's).
- An **input latency** budget in the smoke test: keydown to echoed glyph under 100 ms on loopback.
- **IME**: a Playwright leg that composes a CJK string through the page's hidden input and asserts
  the composed text arrives at the shell — the test that would have caught the class of bug that
  matters most to this operator.

**What would still not be proved** by any of this: that the new emulator is no worse on the streams
nobody captured. The corpus only contains what we thought to record, which is why the "known
divergences" list and a growing fixture set are part of the design rather than an afterthought.

## What I could not verify

- **termwiz from wezterm's git `main`** (0.24.0, where `wezterm-blob-leases` is optional): the build
  did not finish — the monorepo fetch had pulled 1.8 MB in about 25 minutes when I stopped it. The
  published 0.23.3's three walls are measured; whether removing blob leases clears them is not.
- **`wezterm-term`'s wasm build**: not attempted. Its dependency list (termwiz, wezterm-surface,
  `image`, `url`, `miniz_oxide`, `terminfo`) is read from its manifest; the conclusion that it is at
  best termwiz's is inference, not measurement.
- **The vendored xterm.js version**: the bundle is minified with no version string I could find, and
  neither asset directory carries a licence file. The legacy React pane's `^5.5.0` is a different
  artifact and is not evidence for this one.
- **Whether the console's `Asset` responder is compressed in transit**: it sets
  `Cache-Control: public, max-age=86400` and no `Content-Encoding`; whether a proxy in front
  compresses is outside this repo. The 67 KB gzipped figure is `gzip -9` of the file, not what the
  browser receives.
- **The `.wasm` sizes are probe crates**, not this product's module: a driver that allocates a grid,
  feeds bytes and reads cells, with `panic = "abort"`, `lto`, `opt-level="z"`, `strip`, then
  `wasm-opt -Oz` and `gzip -9`. Add the legacy floor's ~2 KB of wasm-bindgen glue and whatever the
  real export surface costs.
- **`vte_gpu`**: no crate, and I did not find a repository I could name with confidence.
- **Nothing in this document ran either candidate against a byte stream from this product.** The
  corpus in the verification section is a proposal, not a result.
