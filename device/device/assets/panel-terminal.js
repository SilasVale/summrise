// The panel's terminal: the browser half of the panel's own create/write/read/close. Everything
// else on the page is server-rendered, so this file only moves bytes between xterm and
// /api/terminal/*; the token in the page's meta tag is what proves the requests came from it.
(function () {
  /// The xterm palette, from the panel's tokens (which mirror the console's).
function panelTerminalTheme() {
  const css = getComputedStyle(document.documentElement);
  const token = (name, fallback) => (css.getPropertyValue(name).trim() || fallback);
  const ink = token("--ink", "#18181b");
  const canvas = token("--term-canvas", token("--surface", "#ffffff"));
  return {
    background: canvas,
    foreground: ink,
    cursor: token("--accent", "#bf3a0a"),
    cursorAccent: canvas,
    selectionBackground: token("--term-selection", "rgba(191,58,10,.18)"),
    black: ink,
    red: token("--term-red", "#b3261e"),
    green: token("--term-green", "#1d7a3c"),
    yellow: token("--term-yellow", "#a8620a"),
    blue: token("--term-blue", "#1f5fbf"),
    magenta: token("--term-magenta", "#8a3ffc"),
    cyan: token("--term-cyan", "#0e7490"),
    white: token("--ink-muted", "#71717a"),
    brightBlack: token("--ink-faint", "#a1a1aa"),
    brightRed: token("--error", "#c02a1b"),
    brightGreen: token("--ok", "#128a4b"),
    brightYellow: token("--warn", "#a8620a"),
    brightBlue: "#2563eb",
    brightMagenta: "#a855f7",
    brightCyan: "#0891b2",
    brightWhite: ink
  };
}

const term = new Terminal({
    cursorBlink: true,
    fontSize: 13,
    scrollback: 5000,
    // THE SAME CANVAS AS THE CONSOLE'S TERMINAL (the operator's point, 2026-10-28: the two surfaces
    // are one product). Read from the page's tokens so the light theme gets the white canvas
    // summrise-legacy uses and the dark theme gets its near-black one.
    theme: panelTerminalTheme(),
  });
  const fit = new FitAddon.FitAddon();
  term.loadAddon(fit);

  let session = null;
  let cursor = 0;
  let opened = false;
  let pending = "";
  let timer = null;
  let closed = false;

  function status(text) {
    document.getElementById("term-status").textContent = text;
  }

  function token() {
    return document.querySelector('meta[name="summrise-panel"]').content;
  }

  async function call(path, options) {
    const response = await fetch(path, {
      ...options,
      headers: { "x-summrise-panel": token(), ...(options && options.headers) },
    });
    return response.json();
  }

  function flush() {
    timer = null;
    if (!session || pending === "" || closed) return;
    const data = pending;
    pending = "";
    call(`/api/terminal/${session}/write`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ data }),
    }).catch(() => {});
  }

  function queue(chunk) {
    pending += chunk;
    if (pending.length >= 4096) {
      if (timer) {
        clearTimeout(timer);
        timer = null;
      }
      flush();
      return;
    }
    if (!timer) timer = setTimeout(flush, 40);
  }

  function open() {
    if (opened) return;
    opened = true;
    term.open(document.getElementById("term-host"));
    fit.fit();
    term.focus();
    term.onData(queue);
    term.onResize((size) => {
      if (!session || closed) return;
      call(`/api/terminal/${session}/resize`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ rows: size.rows, cols: size.cols }),
      }).catch(() => {});
    });
  }

  async function create(kind) {
    if (session) await close();
    const body = { kind, rows: 30, cols: 100 };
    if (kind === "serial") {
      body.port = document.getElementById("term-port").value;
      body.baud = Number(document.getElementById("term-baud").value) || 115200;
    }
    const answer = await call("/api/terminal/create", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body),
    });
    if (!answer.id) {
      status(JSON.stringify(answer));
      return;
    }
    session = answer.id;
    cursor = 0;
    closed = false;
    term.reset();
    open();
    fit.fit();
    term.focus();
    status(`${answer.kind} ${answer.id}`);
    void loop();
  }

  async function loop() {
    while (session && !closed) {
      const answer = await call(
        `/api/terminal/${session}/read?cursor=${cursor}`,
        {},
      ).catch(() => null);
      if (!answer || answer.text === undefined) {
        await new Promise((resolve) => setTimeout(resolve, 300));
        continue;
      }
      // `text` is the lossy reading; `data_base64` is the bytes, which is what keeps a multi-byte
    // character intact when a poll boundary falls inside it.
    if (answer.data_base64) {
      term.write(Uint8Array.from(atob(answer.data_base64), (c) => c.charCodeAt(0)));
    } else if (answer.text) {
      term.write(answer.text);
    }
      cursor = answer.cursor;
      await new Promise((resolve) => setTimeout(resolve, 150));
    }
  }

  async function close() {
    if (!session) return;
    const id = session;
    closed = true;
    session = null;
    await call(`/api/terminal/${id}/close`, { method: "POST" }).catch(() => {});
    status("session closed");
  }

  window.addEventListener("resize", () => {
    if (opened) fit.fit();
  });

  document.getElementById("term-pty").addEventListener("click", () => create("pty"));
  document.getElementById("term-serial").addEventListener("click", () => create("serial"));
  document.getElementById("term-close").addEventListener("click", close);
})();
