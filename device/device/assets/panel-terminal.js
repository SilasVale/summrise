// The panel's terminal: the browser half of the panel's own create/write/read/close. Everything
// else on the page is server-rendered, so this file only moves bytes between xterm and
// /api/terminal/*; the token in the page's meta tag is what proves the requests came from it.
(function () {
  // Read the device-local sheet; ANSI color roles stay on the shared product tokens.
  function panelTerminalTheme() {
    const css = getComputedStyle(document.documentElement);
    const token = (name, fallback) => (css.getPropertyValue(name).trim() || fallback);
    const ink = token("--ink", "#22312c");
    const canvas = token("--term-canvas", token("--surface", "#ffffff"));
    return {
      background: canvas,
      foreground: ink,
      cursor: token("--accent", "#335f51"),
      cursorAccent: canvas,
      selectionBackground: token("--term-selection", "rgba(51,95,81,.18)"),
      black: ink,
      red: token("--term-red", "#b3261e"),
      green: token("--term-green", "#1d7a3c"),
      yellow: token("--term-yellow", "#a8620a"),
      blue: token("--term-blue", "#1f5fbf"),
      magenta: token("--term-magenta", "#8a3ffc"),
      cyan: token("--term-cyan", "#0e7490"),
      white: token("--muted", "#465750"),
      brightBlack: token("--faint", "#5c6f65"),
      brightRed: token("--error", "#934f3b"),
      brightGreen: token("--ok", "#335f51"),
      brightYellow: token("--warn", "#934f3b"),
      brightBlue: "#2563eb",
      brightMagenta: "#a855f7",
      brightCyan: "#0891b2",
      brightWhite: ink
    };
  }

  const term = new Terminal({
    cursorBlink: true,
    fontSize: 13,
    fontFamily: getComputedStyle(document.documentElement).getPropertyValue("--font-mono").trim() || "monospace",
    scrollback: 5000,
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
  let viewGeneration = 0;
  let wanted = null;
  let runner = null;
  // Single-flight creation and cleanup-before-create keep this ledger bounded to one owned id.
  const cleanup = new Map();

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

  function clearInput() {
    if (timer) clearTimeout(timer);
    timer = null;
    pending = "";
  }

  function queue(chunk) {
    if (!session || closed) return;
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

  function create(kind) {
    // Capture the fields with the user's intent, even when another create is still pending.
    const body = { kind, rows: 30, cols: 100 };
    if (kind === "serial") {
      body.port = document.getElementById("term-port").value;
      body.baud = Number(document.getElementById("term-baud").value) || 115200;
    }
    return requestAction(body);
  }

  function requestAction(body) {
    wanted = { generation: ++viewGeneration, body };
    retireSession();
    status(cleanup.size || !body ? "closing session" : "opening session");
    return startActions();
  }

  function startActions() {
    if (!runner) {
      runner = runActions().finally(() => {
        runner = null;
        if (wanted) startActions();
      });
    }
    return runner;
  }

  async function releaseCleanup() {
    for (const [id, entry] of cleanup) {
      entry.inflight ||= call(`/api/terminal/${id}/close`, { method: "POST" }).catch(() => null);
      const answer = await entry.inflight;
      entry.inflight = null;
      if (answer?.closed !== true && answer?.error !== "unknown_session") return false;
      cleanup.delete(id);
    }
    return true;
  }

  function releaseFailed() {
    // Discard coalesced intent: a failed release retries only after another explicit action.
    wanted = null;
    status("Close failed. Use Close or New to retry releasing the session.");
  }

  async function runActions() {
    while (wanted) {
      const intent = wanted;
      wanted = null;
      if (cleanup.size && !await releaseCleanup()) {
        releaseFailed();
        return;
      }
      if (intent.generation !== viewGeneration) continue;
      if (!intent.body) {
        status("session closed");
        continue;
      }
      const answer = await call("/api/terminal/create", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(intent.body),
      }).catch(() => null);
      if (intent.generation !== viewGeneration) {
        if (answer?.id) {
          cleanup.set(answer.id, { inflight: null });
          if (!await releaseCleanup()) {
            releaseFailed();
            return;
          }
        }
        continue;
      }
      if (!answer?.id) {
        status(answer ? JSON.stringify(answer) : "Could not open the session. Use New to retry.");
        continue;
      }
      session = answer.id;
      cursor = 0;
      closed = false;
      term.reset();
      open();
      fit.fit();
      term.focus();
      status(`${answer.kind} ${answer.id} · live`);
      void loop();
    }
  }

  async function loop() {
    const id = session;
    while (session === id && !closed) {
      const answer = await call(
        `/api/terminal/${id}/read?cursor=${cursor}&wait_ms=15000`,
        {},
      ).catch(() => null);
      // Close or New pty can replace the session while this read is in flight.
      // Its bytes, cursor and end fact belong only to the session that requested them.
      if (session !== id || closed) return;
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
      if (typeof answer.cursor === "number") cursor = answer.cursor;
      // A session that ended is a fact, not a failed poll: say so and stop the loop. Without this,
      // the pane kept polling a dead id and looked like a live terminal with no output.
      if (answer.alive === false) {
        const code = typeof answer.exit_code === "number" ? ` (exit ${answer.exit_code})` : "";
        status(`session ended${code}`);
        clearInput();
        closed = true;
        // Retain the ended id so Close or New can release its registry row.
        return;
      }
      // The read long-polls for up to 15 s, so this pause is only the fallback for an older device
      // that answers immediately: it keeps that case from becoming a busy loop without adding a
      // visible delay to the live case, which returns as soon as bytes arrive.
      await new Promise((resolve) => setTimeout(resolve, 50));
    }
  }

  function close() {
    return requestAction(null);
  }

  function retireSession() {
    if (!session) return;
    cleanup.set(session, { inflight: null });
    clearInput();
    closed = true;
    session = null;
  }

  window.addEventListener("resize", () => {
    if (opened) fit.fit();
  });

  // A session may already be running when this page loads — the console can have opened it, or
  // this panel was reloaded. The old page showed a count but an empty xterm until New pty, which
  // read as "the terminal is not live". Attach to the first session and read from the beginning of
  // what the device retained; New pty remains the way to start another.
  async function attachExisting() {
    const generation = viewGeneration;
    const answer = await call("/api/terminal", {}).catch(() => null);
    // The startup list cannot reclaim the view after a New or Close action.
    if (generation !== viewGeneration) return;
    // The route answers `{sessions:[…]}`; an older build answered a bare array. Accept both, so a
    // panel reload attaches to a live session instead of silently leaving the canvas empty.
    const list = Array.isArray(answer)
      ? answer
      : Array.isArray(answer && answer.sessions)
        ? answer.sessions
        : [];
    if (list.length === 0) return;
    const first = list[0];
    if (!first || !first.id) return;
    session = first.id;
    cursor = 0;
    closed = false;
    term.reset();
    open();
    fit.fit();
    term.focus();
    status(`${first.kind || "session"} ${first.id} · live`);
    void loop();
  }

  document.getElementById("term-pty").addEventListener("click", () => create("pty"));
  document.getElementById("term-serial").addEventListener("click", () => create("serial"));
  document.getElementById("term-close").addEventListener("click", close);
  void attachExisting();
})();
