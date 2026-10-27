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
const form = document.getElementById("terminal-form");
const kind = document.getElementById("kind");
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

// The form's visible group follows the kind, so an operator never types an SSH host into a serial
// port field.
function showKindFields() {
  for (const group of form.querySelectorAll("[data-kind]")) {
    group.hidden = group.dataset.kind !== kind.value;
  }
}
kind.addEventListener("change", showKindFields);
showKindFields();

function bodyFromForm() {
  const body = { kind: kind.value, rows: term.rows || 24, cols: term.cols || 80 };
  if (kind.value === "ssh") {
    body.host = document.getElementById("host").value.trim();
    body.ssh_port = Number(document.getElementById("ssh_port").value || 22);
    body.user = document.getElementById("user").value.trim();
    body.password = document.getElementById("password").value;
  }
  if (kind.value === "serial") {
    body.serial_port = document.getElementById("serial_port").value.trim();
    body.baud = Number(document.getElementById("baud").value || 115200);
  }
  return body;
}

async function start() {
  const body = bodyFromForm();
  say("opening a session…");
  let created;
  try {
    created = await call(`/next/terminal/${device}/create`, "POST", body);
  } catch (error) {
    say(String(error.message || error));
    return;
  }
  session = created.id;
  form.hidden = true;
  host.hidden = false;
  term.open(host);
  fit.fit();
  term.focus();
  say(`session ${session} · ${created.shell || created.kind || body.kind}`);
  term.onData(queue);
  term.onResize((size) => {
    if (!session) return;
    call(base() + "/resize", "POST", { rows: size.rows, cols: size.cols }).catch(() => {});
  });
  await read();
}

form.addEventListener("submit", (event) => {
  event.preventDefault();
  void start();
});

window.addEventListener("resize", () => {
  if (!host.hidden) fit.fit();
});
window.addEventListener("beforeunload", () => {
  closed = true;
  if (!session) return;
  fetch(`${base()}/close`, {
    method: "POST",
    headers: { "x-csrf": csrf },
    keepalive: true
  });
});
