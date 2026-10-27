// The terminal page's glue: xterm owns the screen, and this file is the loop between it and the
// console's `/next/terminal` endpoints. It is JavaScript because xterm is; every decision that
// matters (which device, who may type, what a session is) stays on the Rust side.
//
// The protocol is the device's own `terminal.*` tools, relayed: create returns a session id, read
// takes a cursor and returns the output after it, write sends keystrokes, resize reports the grid.
// Polling rather than a socket because the console's page cookie is `HttpOnly` and the endpoints are
// already behind it — a second transport would be a second gate.
const host = document.getElementById("terminal");
const status = document.getElementById("terminal-status");
const device = host.dataset.device;
const csrf = host.dataset.csrf;

const term = new Terminal({
  cursorBlink: true,
  fontSize: 13,
  scrollback: 5000,
  theme: { background: "#000000", foreground: "#d8d4cd", cursor: "#e07a4a" }
});
const fit = new FitAddon.FitAddon();
term.loadAddon(fit);
term.open(host);
fit.fit();
term.focus();

let session = null;
let cursor = 0;
let closed = false;
let pending = "";
let flushTimer = null;

const base = () => `/next/terminal/${device}/${session}`;
const say = (text) => {
  status.textContent = text;
};

async function call(path, method = "GET", body) {
  const headers = { "x-csrf": csrf };
  if (body !== undefined) headers["content-type"] = "application/json";
  const response = await fetch(path, {
    method,
    headers,
    body: body === undefined ? undefined : JSON.stringify(body),
    cache: "no-store"
  });
  const text = await response.text();
  let answer = null;
  try {
    answer = text ? JSON.parse(text) : null;
  } catch {
    // A refusal that is not JSON is shown as it came.
  }
  if (!response.ok) {
    throw new Error((answer && answer.error) || text || `HTTP ${response.status}`);
  }
  return answer;
}

// One write per tick, not one per keystroke: a paste is thousands of onData calls, and the device
// sees a line of audit either way.
function flush() {
  flushTimer = null;
  if (!session || pending === "" || closed) return;
  const data = pending;
  pending = "";
  call(base() + "/write", "POST", { data }).catch((error) => say(String(error.message || error)));
}

function queue(chunk) {
  pending += chunk;
  if (pending.length >= 4096) {
    if (flushTimer) {
      clearTimeout(flushTimer);
      flushTimer = null;
    }
    flush();
    return;
  }
  if (!flushTimer) flushTimer = setTimeout(flush, 40);
}

async function read() {
  while (!closed) {
    let answer;
    try {
      answer = await call(`${base()}/read?cursor=${cursor}`);
    } catch (error) {
      say(String(error.message || error));
      return;
    }
    if (answer.data) term.write(answer.data);
    cursor = answer.cursor;
    if (!answer.alive) {
      say("the session ended");
      return;
    }
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
}

async function start() {
  say("opening a shell…");
  try {
    const created = await call(`/next/terminal/${device}/create`, "POST", {
      kind: "pty",
      rows: term.rows,
      cols: term.cols
    });
    session = created.id;
    say(`session ${session} · ${created.shell || "pty"}`);
    term.onData(queue);
    term.onResize((size) => {
      if (!session) return;
      call(base() + "/resize", "POST", { rows: size.rows, cols: size.cols }).catch(() => {});
    });
    await read();
  } catch (error) {
    say(String(error.message || error));
  }
}

window.addEventListener("resize", () => fit.fit());
window.addEventListener("beforeunload", () => {
  closed = true;
  if (!session) return;
  fetch(`${base()}/close`, {
    method: "POST",
    headers: { "x-csrf": csrf },
    keepalive: true
  });
});

start();
