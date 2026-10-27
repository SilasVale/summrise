// The terminal page's glue: one xterm per session, a tab strip over them, and the poll loop between
// each terminal and the console's `/next/terminal` endpoints. It is JavaScript because xterm is;
// every decision that matters (which device, who may type, what a session is) stays in Rust.
//
// The protocol is the device's own `terminal.*` tools, relayed: create returns a session id, read
// takes a cursor and returns the output after it, write sends keystrokes, resize reports the grid.
// Polling rather than a socket because the console's page cookie is `HttpOnly` and the endpoints are
// already behind it — a second transport would be a second gate.
const host = document.getElementById("terminal");
const status = document.getElementById("terminal-status");
const form = document.getElementById("terminal-form");
const kind = document.getElementById("kind");
const tabs = document.getElementById("terminal-tabs");
const panes = document.getElementById("terminal-panes");
const device = host.dataset.device;
const csrf = host.dataset.csrf;

/** Every open session: id → the terminal, its pane, its tab and its cursor. */
const sessions = new Map();
let active = null;

const base = (id) => `/next/terminal/${device}/${id}`;
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
function makeWriter(session) {
  let pending = "";
  let timer = null;
  const flush = () => {
    timer = null;
    if (!session.id || pending === "" || session.closed) return;
    const data = pending;
    pending = "";
    call(base(session.id) + "/write", "POST", { data }).catch((error) =>
      say(String(error.message || error))
    );
  };
  return (chunk) => {
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
  };
}

async function readLoop(session) {
  while (!session.closed) {
    let answer;
    try {
      answer = await call(`${base(session.id)}/read?cursor=${session.cursor}`);
    } catch (error) {
      say(String(error.message || error));
      return;
    }
    // The device answers both the lossy text (`data`) and the bytes (`data_base64`): a multi-byte
    // character split by the poll boundary is U+FFFD in the text and intact in the bytes.
    if (answer.data_base64) {
      session.term.write(Uint8Array.from(atob(answer.data_base64), (c) => c.charCodeAt(0)));
    } else if (answer.data) {
      session.term.write(answer.data);
    }
    session.cursor = answer.cursor;
    if (!answer.alive) {
      say("the session ended");
      return;
    }
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
}

function activate(id) {
  active = id;
  for (const [other, session] of sessions) {
    const on = other === id;
    session.pane.hidden = !on;
    session.tab.classList.toggle("active", on);
    if (on) {
      session.fit.fit();
      session.term.focus();
    }
  }
}

function addSession(created, label) {
  const id = created.id;
  const pane = document.createElement("div");
  pane.className = "pane";
  panes.append(pane);

  const tab = document.createElement("button");
  tab.type = "button";
  tab.className = "tab";
  tab.textContent = label;
  tab.addEventListener("click", () => activate(id));
  tabs.append(tab);

  const close = document.createElement("button");
  close.type = "button";
  close.className = "tab-close";
  close.textContent = "×";
  close.title = "close this session";
  close.addEventListener("click", (event) => {
    event.stopPropagation();
    void closeSession(id);
  });
  tab.append(close);

  const term = new Terminal({
    cursorBlink: true,
    fontSize: 13,
    scrollback: 5000,
    theme: { background: "#000000", foreground: "#d8d4cd", cursor: "#e07a4a" }
  });
  const fit = new FitAddon.FitAddon();
  term.loadAddon(fit);

  const session = { id, term, fit, pane, tab, cursor: 0, closed: false };
  sessions.set(id, session);
  activate(id);
  term.open(pane);
  fit.fit();
  term.focus();
  term.onData(makeWriter(session));
  term.onResize((size) => {
    if (session.closed) return;
    call(base(id) + "/resize", "POST", { rows: size.rows, cols: size.cols }).catch(() => {});
  });
  term.onTitleChange((title) => {
    if (title) label = title;
  });
  void readLoop(session);
}

async function closeSession(id) {
  const session = sessions.get(id);
  if (!session) return;
  session.closed = true;
  sessions.delete(id);
  session.pane.remove();
  session.tab.remove();
  try {
    await call(base(id) + "/close", "POST");
  } catch {
    // The session may already be gone; the row is what matters.
  }
  if (sessions.size === 0) {
    active = null;
    say("choose a kind and start a session");
    return;
  }
  if (active === id) activate(sessions.keys().next().value);
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
  const body = { kind: kind.value, rows: 24, cols: 80 };
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

form.addEventListener("submit", (event) => {
  event.preventDefault();
  const body = bodyFromForm();
  say("opening a session…");
  call(`/next/terminal/${device}/create`, "POST", body)
    .then((created) => {
      const label = created.shell || created.kind || body.kind;
      addSession(created, String(label));
      say(`session ${created.id} · ${label}`);
    })
    .catch((error) => say(String(error.message || error)));
});

// `?attach=<id>` opens a session that already exists (the rail's links). Reading from cursor 0
// replays what the session has retained, which is what an operator expects when they click in.
function attachFromQuery() {
  const id = new URLSearchParams(window.location.search).get("attach");
  if (!id) return;
  addSession({ id, kind: "session", shell: id.slice(0, 10) }, id.slice(0, 10));
  say(`attached to ${id}`);
}

attachFromQuery();

window.addEventListener("resize", () => {
  if (active && sessions.has(active)) sessions.get(active).fit.fit();
});
window.addEventListener("beforeunload", () => {
  for (const session of sessions.values()) {
    session.closed = true;
    fetch(base(session.id) + "/close", {
      method: "POST",
      headers: { "x-csrf": csrf },
      keepalive: true
    });
  }
});
