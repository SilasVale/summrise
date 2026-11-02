# The device panel, in Rust

**Why.** `device/device/src/panel.html` is the last project-authored page that is not Rust: 650 lines
of HTML, CSS and a polling terminal. The console page made the same move on 2026-10-27 (`maud` served
by the console itself, `web/` deleted) and the machinery is now familiar. The panel is the surface a
person standing at the machine uses, and the one that still answers when the console is down — so the
port must not lose a fact it shows today.

**What stays JavaScript.** xterm and the glue that feeds it. There is no honest way around a browser
terminal library; the console page has the same exception, and both now use one emulator
(`device/device/assets/xterm.js`).

**Shape.** A `panel` module in the device binary renders the page with `maud`; the device already
serves it, so nothing new listens. The page's facts come from the same calls `/api/panel` makes
(`host::facts`, `store::load`, the console link, `terminal.list`).

## Tasks

1. **The page as data.** A `PanelModel` built by one function from the same sources `/api/panel` uses,
   with a unit test per section rather than per string: build facts, console link, sessions, local
   config.
2. **The template.** `maud` reproduces the existing sections — local MCP config, terminal, sessions,
   build, console link, enrollment — with the token substituted at render time. The current page is
   the reference for what each section must say, not for its markup.
3. **Enrollment as a form.** `POST /panel/enroll` takes the same fields as `/api/enroll`, checks the
   panel token, and redirects with `?enrolled=1` or `?error=<code>`; the JSON route stays for the
   script and for compatibility.
4. **The terminal glue as an asset.** Move the xterm wiring out of the inline script into
   `assets/panel-terminal.js`, served like the emulator; the page's own JavaScript disappears.
5. **Delete `panel.html`.** `GET /` renders the template; the `__PANEL_TOKEN__` substitution and the
   inline script go with the file.
6. **Prove it where it runs.** The device's HTTP tests keep covering the facts; the browser check is
   `scripts/console-smoke.mjs`'s sibling for the panel (a small Playwright leg that opens a pty and
   reads a marker back), run on a machine with a real browser session.

## What must not change

- The page is loopback-only and refuses every request without this boot's panel token.
- It shows exactly the facts it shows today: version/channel/sha256/pid, the console link state, the
  live terminal sessions, the local MCP endpoint and token path.
- A device restart still ends every terminal session; the page says so.

## Escape hatch

The panel is the fallback surface. If the port cannot be finished in one pass, keep `panel.html`
served and add the Rust page under a different path first — the console's `/next` did exactly that
for a week before `/` moved.
