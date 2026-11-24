# The desktop shell — dropped, and what replaces it

> **Status: decision, 2026-10-22.** The parity matrix in the cutover plan has carried one row marked
> "decide: port or drop" since the split. This closes it. It is a product decision, not a technical
> one, and it is recorded here so that the next person does not re-open it by accident.

## What the old runtime had

The old agent shipped an **Electron shell**: a window, a tray icon, a chat surface, and the panel's
screens bundled inside the app. It was the product's face for users who never opened a browser tab.

## The three options, honestly stated

| # | Shape | What it costs | What it buys |
|---|---|---|---|
| A | Port the Electron app to the new runtime | a second runtime (Node + Chromium), a second update path, a second authorization surface, and a build that is not the Rust workspace | the old window, unchanged |
| B | A WebView2 shell in Rust | a Windows-only UI stack, a second front-end build, a UI process to supervise, and a new class of crash | a native window over the console page |
| C | **Drop the desktop shell**; the console page is the UI | nothing to port; one UI | a browser tab, which is not a window |

**C is chosen.** The reasoning is not "less work"; it is that the desktop shell's value was a window,
and its **cost** was a second, unaudited path to the same device:

- Every capability the shell could reach, the console can reach — and the console reaches it through
  the **audited** path: a grant, a channel, an `events.jsonl` line. A shell bundling its own client
  would either duplicate that path or need its own copy of it.
- A second UI means a second update channel and a second set of credentials on the user's machine.
  The whole point of the two-runtime architecture is that a device has **one** updater and **one**
  authorization function.
- The panel's screens are already a web app; the console page is a web app. A WebView2 shell would be
  a window frame around a page that works in a browser.

## What is lost, and how it is softened

- **A window.** The console page installs as a PWA (Chromium/Edge: "Install this site as an app"), so
  it gets an icon, its own window and no address bar. That is the same user-facing outcome as B without
  a UI stack in the binary.
- **Tray presence and notifications.** A browser cannot put an icon in the tray. If the operator needs
  to know a device went down while nobody was looking, the answer is the **monitor** the console
  already exposes (`/health`, device count, the device gate's watch), not a tray icon that only exists
  while a window does.
- **Offline/local-only use.** The console is a service; a shell would not have helped either.

## What would change the decision

- The product being sold as a **desktop application**, where the window is the product.
- A surface that the console page cannot host at all (local file drag-and-drop into a native API, a
  system-wide hotkey). Neither exists today.
- A customer requiring a signed desktop binary for policy reasons — then B, and the console page is
  still the content.

## Consequences

- Nothing in `device/` depends on a UI stack; the workspace stays Rust-only.
- The old `panel-react`/Electron tree stays in `summrise-legacy` and is **not** ported. That is a
  deletion the cutover plan can now list as decided rather than pending.
- The console page is the only operator UI, so it carries the weight: its audit view, its grants and
  its device list are the product surface, and a UI regression there is a product regression.

## 2026-10-24 — reopened for the icon, not for a client

The operator asked for the thing this decision did not weigh: a tray icon that says whether a device is
serving, the way Tailscale's does. A tray needs a user-session process, and once that process exists,
giving it a window is a small increment rather than a second application.

`summrise-shell` is that process, and its boundary is the point: it renders the console page in the
system's WebView2, reads the loopback health document, and opens URLs. It holds no credential and talks
to no device, so it adds neither an authorization path nor an updater — the two costs that made "C"
right for the shell the old runtime had. The window is built when it is first opened and rebuilt after
a close, which is why closing it does not end the icon.
