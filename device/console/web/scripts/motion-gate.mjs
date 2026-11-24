// motion-gate measures genuine active built-Console views under context-level reduced motion.
// HTTP/MCP fixtures below are deterministic UI data: no real PTY, remote browser or native capture.
// Uses the existing manifest-installed Playwright; never installs dependencies or downloads Chromium.
import { chromium } from "playwright";
import { createHash } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { mkdir, writeFile } from "node:fs/promises";
import { createServer } from "node:http";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import path from "node:path";

const require = createRequire(import.meta.url);
const HTML_PATH = new URL("../dist/index.html", import.meta.url);
const HTML = readFileSync(HTML_PATH);
const FAVICON_PNG = readFileSync(new URL("../../assets/icon.png", import.meta.url));
const EXECUTABLE = process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH || chromium.executablePath();
if (!existsSync(EXECUTABLE)) throw new Error("motion-gate: no installed Chromium; set PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH to an existing owned browser");
const htmlSha256 = createHash("sha256").update(HTML).digest("hex");
if (process.env.CONSOLE_GATE_HTML_SHA256 && process.env.CONSOLE_GATE_HTML_SHA256 !== htmlSha256) throw new Error(`motion-gate: HTML changed before launch: ${htmlSha256}`);
const OUT = path.resolve(process.env.CONSOLE_GATE_ROOT || path.join(tmpdir(), `console-motion-gate-${process.pid}-${Date.now()}`));
await mkdir(OUT); // A supplied artifact directory must be fresh.
const inputs = { fixture: "HTTP/MCP UI fixture, no native execution", html: HTML_PATH.pathname, htmlSha256,
  gateSha256: createHash("sha256").update(readFileSync(new URL(import.meta.url))).digest("hex"),
  node: process.version, playwright: require.resolve("playwright"), playwrightVersion: require("playwright/package.json").version,
  executable: EXECUTABLE, pid: process.pid, reducedMotion: "reduce" };
await writeFile(path.join(OUT, "inputs.json"), JSON.stringify(inputs, null, 2) + "\n");
const NOW = Date.now();
const LONG = "Fixture record — complete readable evidence / 夹具记录：完整可读的设备事实与操作结果。".repeat(3);
const PNG = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=";
const DEVICE = { id: "motion-device", name: "DESKTOP-MOTION", online: true, version: "0.3.42", last_seen_ms: NOW,
  host: { os: { name: "Linux", release: "Linux", version: "fixture", arch: "x86_64" }, cpu: { cores: 4 },
    sessions: [{ id: 2, state: "active" }], helper: { registered: true, pid: 42, session: 2 } } };
const AUDIT = { ts_ms: NOW - 1000, kind: "mcp", device_id: DEVICE.id, tool: "runs.exec", caller: "client", status: 200, detail: "run motion-run exited 0" };
const RUN = { id: "motion-run", command: "echo", args: [LONG], cwd: ".", exit_code: 0, timed_out: false,
  duration_ms: 1, started_at_ms: NOW - 1000, stdout_bytes: 0, stderr_bytes: 0 };
const PLUGIN = { id: "motion-plugin", namespace: "motion", name: "Motion fixture", version: "1", state: "running", tools: 1, path: "/fixture/plugin", last_error: null };
const TOOLS = ["terminal.create", "terminal.list", "terminal.read", "terminal.screen", "plugins.list", "forward.list", "monitor.list", "memory.list"];
const TOKEN = { id: "motion-token", name: "Motion token", kind: "device", device_id: DEVICE.id, tools: ["terminal.read"], allow_destructive: false, created_at_ms: NOW - 1000 };
const streams = new Set();
const errors = [];
let opened = false;
let signedIn = false;
const answer = (body) => ({ result: { content: [{ type: "text", text: typeof body === "string" ? body : JSON.stringify(body) }] } });
function toolReply(name, args) {
  switch (name) {
    case "terminal.list": return answer(opened ? [{ id: "motion-session", kind: "pty", started_at_ms: NOW,
      holder: { kind: "console", token_id: null }, goal: { text: LONG, by: { kind: "console", token_id: null }, at_ms: NOW } }] : []);
    case "terminal.create": opened = true; return answer({ id: "motion-session" });
    case "terminal.resize": return answer({ cols: 80, rows: 24 });
    case "terminal.read": return answer({ data: "", cursor: args.cursor ?? 0 });
    case "terminal.screen": return answer({ lines: ["Motion fixture / 动态效果夹具"] });
    case "browser.list": return answer([{ id: "motion-page", title: "Fixture page", url: "https://example.test", type: "page" }]);
    case "browser.screenshot": return answer({ data: PNG, bytes: Buffer.from(PNG, "base64").length });
    case "screen.capture": return answer({ data: PNG, width: 1, height: 1 });
    case "files.list": return answer([{ name: "notes.txt", kind: "file", len: Buffer.byteLength(LONG + "\n") }]);
    case "files.read": return answer(LONG + "\n");
    case "runs.list": return answer([RUN]);
    case "runs.get": return answer(RUN);
    case "plugins.list": return answer([PLUGIN]);
    case "plugins.logs": return answer({ lines: [LONG] });
    case "memory.list": return answer({ ok: true, results: [{ id: "motion-memory", title: LONG, content: LONG, tags: ["fixture"] }] });
    case "monitor.list": return answer({ ok: true, interval_secs: 15, targets: [{ id: "motion-watch", host: "service.example.test", port: 19000, path: "/health",
      summary: { probes: 1, up: 1, down: 0, up_pct: 100, up_now: true, since_ms: NOW, drops: 0, last_status: 200 } }] });
    case "forward.list": return answer({ rules: [{ name: LONG, listen: "127.0.0.1:19001", target: "service.example.test:19000", via: "operator@gateway.example.test:22",
      state: { listening: true, since_ms: NOW, attempts: 1, connections: 0, last_error: null } }] });
    case "system.process_list": return answer([{ pid: 4242, name: "fixture-process", memory_kb: 1024 }]);
    case "state.get": return answer({ health: { status: "ok", version: DEVICE.version, pid: 42421, poisoned: false }, state: {},
      events: [{ id: 1, event: "fixture.state.read", tool: "state.get", decision: "allow", ok: true, transport: "console", ts_ms: NOW }] });
    default: throw new Error(`Unimplemented motion fixture tool: ${name}`);
  }
}
const server = createServer(async (req, res) => {
  try {
    const url = new URL(req.url, "http://127.0.0.1");
    const route = url.pathname;
    if (route === "/" || route === "/index.html") { res.writeHead(200, { "content-type": "text/html", "cache-control": "no-store" }); return res.end(HTML); }
    if (route === "/assets/icon.png" || route === "/favicon.ico") { res.writeHead(200, { "content-type": "image/png", "cache-control": "no-store" }); return res.end(FAVICON_PNG); }
    if (route === "/events" || /^\/terminal\/[^/]+\/[^/]+\/stream$/.test(route)) {
      res.writeHead(200, { "content-type": "text/event-stream", "cache-control": "no-store" });
      res.write(route === "/events" ? `event: snapshot\ndata: ${JSON.stringify({ lines: [AUDIT], running: [] })}\n\n`
        : `event: output\ndata: ${JSON.stringify({ data: "Motion fixture / 动态效果夹具\r\n", cursor: Buffer.byteLength("Motion fixture / 动态效果夹具\r\n") })}\n\n`);
      streams.add(res);
      const timer = setInterval(() => res.write(": ping\n\n"), 25000);
      req.on("close", () => { streams.delete(res); clearInterval(timer); });
      return;
    }
    let payload;
    if (route === "/auth" && req.method === "POST") { signedIn = true; payload = { token: "motion-fixture-token", expires_at_ms: NOW + 3600000 }; }
    else if (route === "/session") payload = signedIn ? { authenticated: true, email: "motion@gate.test", via: "token" } : { authenticated: false, access: { configured: false } };
    else if (route === "/health") payload = { status: "ok", devices: 1, online: 1, uptime_s: 60, version: "motion-fixture" };
    else if (route === "/fleet") payload = [DEVICE];
    else if (route === "/installer") payload = { available: false };
    else if (route === "/tokens") payload = [TOKEN];
    else if (route === "/denials" || /^\/fleet\/[^/]+\/connections$/.test(route)) payload = [];
    else if (route === "/tools") payload = TOOLS.map((name) => ({ name, destructive: name === "terminal.create" }));
    else if (/^\/fleet\/[^/]+\/tools$/.test(route)) payload = { tools: TOOLS.map((name) => ({ name, description: LONG, inputSchema: { type: "object", properties: {} }, source: "builtin", capability: name.split(".")[0] })) };
    else if (route === "/audit") payload = [AUDIT];
    else if (route === "/audit/day") payload = { client_calls: 1, failed: 0, console: 0, console_failed: 0, poll: 0, poll_failed: 0, unattributed: 0, day_start_ms: new Date(NOW).setHours(0, 0, 0, 0) };
    else if (route === "/audit/source") payload = { file: "audit.jsonl", bytes: 128, oldest_ts_ms: AUDIT.ts_ms, newest_ts_ms: AUDIT.ts_ms, max_bytes: 8388608, rotations: 0 };
    else if (route === "/journal/read") payload = { device: DEVICE.id, journal: false, oldest_seq: 0, last_seq: 0, rows: [], more: false, read_at_ms: NOW, unknown: false };
    else if (route === "/journal/day") payload = { device: DEVICE.id, journal: false, day_start_ms: Number(url.searchParams.get("from")), day_end_ms: Number(url.searchParams.get("to")), oldest_seq: 0, last_seq: 0, scanned: 0, more: false,
      calls: Object.fromEntries(["console", "panel", "mesh", "loopback", "unknown"].map((key) => [key, { calls: 0, failed: 0 }])), read_at_ms: NOW, unknown: false };
    else if (route === "/mcp" && req.method === "POST") {
      let body = "";
      for await (const chunk of req) body += chunk;
      const call = JSON.parse(body).request.params;
      payload = toolReply(call.name, call.arguments ?? {});
    } else throw new Error(`Unimplemented motion fixture request: ${req.method} ${route}`);
    res.writeHead(200, { "content-type": "application/json", "cache-control": "no-store" });
    res.end(JSON.stringify(payload));
  } catch (error) {
    errors.push(error.message);
    res.writeHead(500, { "content-type": "application/json" });
    res.end(JSON.stringify({ error: error.message }));
  }
});

/** Every element still MOVING for a readable length of time. 0.01ms is not motion. */
const moving = () => {
  const out = [];
  for (const el of document.querySelectorAll("body *")) {
    if (!el.getClientRects().length || !el.checkVisibility({ checkVisibilityCSS: true, checkOpacity: true })) continue;
    const s = getComputedStyle(el);
    for (const d of s.transitionDuration.split(",")) {
      const ms = parseFloat(d) * (d.includes("ms") ? 1 : 1000);
      if (ms > 1) { out.push(`${(el.className || el.tagName).toString().slice(0, 30)}:${d}`); break; }
    }
    // Keep the actual moving-duration predicate, including each duration in animation lists.
    const durations = s.animationDuration.split(",");
    if (s.animationName.split(",").some((name, i) => {
      const d = durations[i % durations.length];
      return name.trim() !== "none" && parseFloat(d) * (d.includes("ms") ? 1 : 1000) > 1;
    })) {
      out.push(`animation:${s.animationName} on ${(el.className || el.tagName).toString().slice(0, 26)}`);
    }
  }
  return out;
};

let context;
const measured = [];
try {
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const base = `http://127.0.0.1:${server.address().port}`;
  // Context option precedes page creation, navigation, and every measured UI state.
  context = await chromium.launchPersistentContext(path.join(OUT, "profile"), { executablePath: EXECUTABLE, headless: true, colorScheme: "light", forcedColors: "none", viewport: { width: 1440, height: 960 }, reducedMotion: "reduce" });
  const page = context.pages()[0];
  page.on("pageerror", (error) => errors.push(error.message));
  page.on("console", (message) => { if (message.type() === "error") errors.push(message.text()); });
  const measure = async (name, ready, surface, overlay = false) => {
    await page.locator(ready).first().waitFor({ state: "visible", timeout: 10000 });
    if (name === "screen" || name === "browser") {
      await page.waitForFunction((selector) => { const img = document.querySelector(selector); return img instanceof HTMLImageElement && img.complete && img.naturalWidth === 1; }, ready, { timeout: 10000 });
    }
    if (surface === "machine") await page.waitForFunction(() => {
      const health = document.querySelector('.self-view .self-health');
      return health?.textContent.includes('pid 42421') && health.querySelector('.chip')?.textContent.trim() === 'ok' && [...document.querySelectorAll('.self-view .self-event .self-what')].some((node) => node.textContent.includes('fixture.state.read state.get'));
    }, null, { timeout: 10000 });
    if (name === "terminal") await page.waitForFunction(() => document.querySelector('.term-host:not([hidden]) .xterm-rows')?.textContent.includes('Motion fixture / 动态效果夹具'), null, { timeout: 10000 });
    const facts = await page.evaluate(({ surface, overlay }) => {
      const main = document.querySelector("main#main, .login-wrap");
      const dialog = [...document.querySelectorAll('[role="dialog"]')].filter((el) => el.getClientRects().length).at(-1);
      if (!main || main.dataset.uiSurface !== surface || (overlay && !dialog)) throw new Error(`Wrong active motion surface: ${main?.dataset.uiSurface}, wanted ${surface}`);
      const visible = (node) => node.getClientRects().length > 0 && node.checkVisibility({ checkVisibilityCSS: true });
      const headings = [...main.querySelectorAll('h1')].filter(visible).map((node) => node.textContent.trim());
      const tools = [...main.querySelectorAll('.device-content .page-head[data-heading-level="2"] h2')].filter(visible).map((node) => node.textContent.trim());
      return { surface: main.dataset.uiSurface, density: main.dataset.uiDensity, reducedMotion: matchMedia("(prefers-reduced-motion: reduce)").matches, headings, tools, browserMode: !!main.querySelector('.browser-pane')?.checkVisibility({ checkVisibilityCSS: true }),
        dialog: dialog?.getAttribute("aria-label") || dialog?.querySelector("h2")?.textContent || null, hash: location.hash };
    }, { surface, overlay });
    if (!facts.reducedMotion) throw new Error(`${name}: context is not reduced motion`);
    const globalTitles = { signin: 'Sign in to the console', home: 'Overview', machines: 'Machines', activity: 'Activity', access: 'Access' };
    const toolTitles = { overview: 'Overview', terminal: 'Terminal', screen: facts.browserMode ? 'Browser' : 'Screen', files: 'Files', runs: 'Run history', tools: 'Tools', memory: 'Memory', machine: 'Machine operations' };
    const expectedHeading = name === 'picker' ? 'Choose a machine' : globalTitles[surface] ?? DEVICE.name;
    if (facts.headings.length !== 1 || facts.headings[0] !== expectedHeading) throw new Error(`${name}: wrong A3 page/machine heading ${JSON.stringify(facts.headings)}`);
    if (name !== 'picker' && toolTitles[surface] && (facts.tools.length !== 1 || facts.tools[0] !== toolTitles[surface])) throw new Error(`${name}: wrong A3 active tool h2 ${JSON.stringify(facts.tools)}`);
    const slow = await page.evaluate(moving);
    if (slow.length) throw new Error(`${name}: ${slow.length} element(s) still move under prefers-reduced-motion — ${slow.slice(0, 4).join(", ")}`);
    measured.push({ name, ...facts, moving: slow });
    await page.screenshot({ path: path.join(OUT, `${name}.png`) });
    console.log(`motion-gate: ${name}: active ${surface}, 0 elements moving under reduce`);
  };
  await page.goto(base, { waitUntil: "domcontentloaded" });
  await measure("signin", ".login input", "signin");
  await page.getByLabel("Operator token").fill("motion-fixture-token");
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await measure("home", ".machine-work-card", "home");
  await page.waitForFunction(() => document.querySelectorAll(".metric-value").length === 4 && [...document.querySelectorAll(".metric-value")].every((node) => /^\d+$/.test(node.textContent.trim())), null, { timeout: 10000 });
  await page.locator('nav.rail button[title="Device"]').click();
  await measure("picker", ".picker li", "terminal");
  await page.getByRole("button", { name: "Open DESKTOP-MOTION", exact: true }).click();
  const go = async (hash) => page.evaluate((value) => { location.hash = value; }, hash);
  const selectSection = async (scope, label) => {
    const tab = scope.getByRole("tablist", { name: "Connect sections", exact: true }).getByRole("tab", { name: label, exact: true });
    await tab.click();
    const id = await tab.getAttribute("aria-controls");
    if (!id || await tab.getAttribute("aria-selected") !== "true") throw new Error(`${label}: no selected owned connection panel`);
    const selector = `[role="tabpanel"][id=${JSON.stringify(id)}]`;
    await scope.locator(selector).waitFor({ state: "visible", timeout: 10000 });
    if (label === "Clients") {
      const client = scope.locator(selector).getByRole("tablist", { name: "Client", exact: true }).getByRole("tab", { selected: true });
      const clientId = await client.getAttribute("aria-controls");
      if (!clientId) throw new Error("Selected client has no owned panel");
      await scope.locator(`[role="tabpanel"][id=${JSON.stringify(clientId)}] pre.code`).waitFor({ state: "visible", timeout: 10000 });
    }
    return selector;
  };
  await go("#device/terminal");
  await page.getByRole("button", { name: "New session", exact: true }).click();
  await measure("terminal-new", '[role="dialog"] #connect-kind', "terminal", true);
  for (const kind of ["ssh", "serial", "pty"]) {
    await page.getByLabel("Kind", { exact: true }).selectOption(kind);
    await measure(`terminal-new-${kind}`, '[role="dialog"] #connect-kind', "terminal", true);
  }
  await page.getByRole("dialog", { name: "New terminal session" }).getByRole("button", { name: "Open", exact: true }).click();
  await measure("terminal", ".term-host:not([hidden]) .xterm", "terminal");
  for (const [name, hash, ready, surface] of [
    ["overview", "#device/overview", ".home-today", "overview"],
    ["machines", "#machines", ".fleet-inventory .fleet-row", "machines"],
    ["access", "#access", ".access-page .connect", "access"],
    ["activity", "#activity", ".timeline-row", "activity"],
    ["files", "#device/files", ".files-list", "files"],
    ["runs", "#device/runs", ".runs-row", "runs"],
    ["tools", "#device/tools", ".plugin-row", "tools"],
    ["memory", "#device/memory", ".memory-entry", "memory"],
    ["machine", "#device/machine", ".forward-row", "machine"]
  ]) {
    await go(hash);
    await measure(name, ready, surface);
  }
  await page.locator('.account > summary').focus();
  await page.keyboard.press("Enter");
  await measure("account", ".account[open] .account-pop", "machine");
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: "Connect a client", exact: true }).click();
  const connect = page.getByRole("dialog", { name: "Connect a client" });
  for (const tab of ["Endpoint", "Clients", "Tokens"]) {
    const selectedPanel = await selectSection(connect, tab);
    await measure(`connect-${tab.toLowerCase()}`, `[role="dialog"] ${selectedPanel}`, "machine", true);
  }
  await connect.locator(".token-row").getByRole("button", { name: "Revoke", exact: true }).click();
  await measure("token-revoke", '[role="dialog"] .modal-actions', "machine", true);
  await page.getByRole("dialog", { name: 'Revoke "Motion token"?' }).getByRole("button", { name: "Cancel", exact: true }).click();
  // The child Modal is gone before the parent's top-dialog registration necessarily commits.
  await page.waitForFunction(() => {
    const parent = document.querySelector('.connect-sheet')?.closest('.sheet[role="dialog"]');
    return !document.querySelector('.modal[role="dialog"]') && parent?.getAttribute('aria-modal') === 'true' && !parent.closest('[inert], [aria-hidden="true"]') && parent.contains(document.activeElement);
  }, null, { timeout: 10000 });
  await page.keyboard.press("Escape");
  await page.getByRole("dialog").waitFor({ state: "detached" });
  await go("#device/files");
  await page.getByRole("button", { name: "notes.txt", exact: true }).click();
  await measure("files-editor", "#files-editor", "files");
  await page.locator("#files-editor").fill(LONG + " changed");
  await page.locator(".files-viewer").getByRole("button", { name: "Close", exact: true }).click();
  await measure("files-discard", '[role="dialog"] .modal-actions', "files", true);
  await page.getByRole("dialog", { name: "Discard unsaved changes?" }).getByRole("button", { name: "Cancel", exact: true }).click();
  await go("#device/tools");
  await page.locator(".plugin-row").waitFor({ state: "visible" });
  await page.getByRole("button", { name: "Add plugin", exact: true }).first().click();
  await measure("plugin-add", '[role="dialog"] .add-plugin', "tools", true);
  await page.keyboard.press("Escape");
  await page.getByRole("dialog").waitFor({ state: "detached" });
  await page.locator(".plugin-actions summary").click();
  await page.locator(".plugin-actions").getByRole("button", { name: "Logs", exact: true }).click();
  await page.locator(".plugin-logs pre").getByText(LONG, { exact: true }).waitFor({ state: "visible" });
  await measure("plugin-logs", ".plugin-logs pre", "tools", true);
  await page.keyboard.press("Escape");
  await page.getByRole("dialog").waitFor({ state: "detached" });
  await page.locator(".plugin-actions").getByRole("button", { name: "Remove", exact: true }).click();
  await measure("plugin-remove", '[role="dialog"] .remove-plugin', "tools", true);
  await page.keyboard.press("Escape");
  await page.getByRole("dialog").waitFor({ state: "detached" });
  await page.locator(".tool-row").first().click();
  await measure("tool-schema", '[aria-label="Input schema"]', "tools", true);
  await page.keyboard.press("Escape");
  await page.getByRole("dialog").waitFor({ state: "detached" });
  await go("#device/screen");
  await page.getByRole("button", { name: "Capture screen", exact: true }).click();
  await measure("screen", ".screen-shot", "screen");
  await page.getByRole("button", { name: "Browser", exact: true }).click();
  await page.getByLabel("Page", { exact: true }).selectOption("motion-page");
  await page.getByRole("button", { name: "Refresh view", exact: true }).click();
  await measure("browser", ".browser-shot", "screen");
  // A3's separate evidence inspector is populated from a real selected record.
  await go("#activity");
  const record = page.locator(".timeline-row button.timeline-line").first();
  await record.click();
  const evidence = page.locator(".activity-inspector:not(.is-empty)");
  await evidence.locator('[aria-label="Selected evidence details"]').waitFor({ state: "visible", timeout: 10000 });
  if (await record.getAttribute("aria-expanded") !== "true" || !(await evidence.textContent()).includes("runs.exec")) throw new Error("Selected Activity record has no populated owned evidence");
  await measure("activity-evidence", ".activity-inspector:not(.is-empty)", "activity");
  await evidence.getByRole("button", { name: "Close evidence", exact: true }).click();
  if (!await record.evaluate((node) => node === document.activeElement)) throw new Error("Closing evidence lost its record trigger");
  await go("#access");
  for (const label of ["Endpoint", "Clients", "Tokens"]) {
    const selectedPanel = await selectSection(page.locator(".access-page"), label);
    await measure(`access-${label.toLowerCase()}`, `.access-page ${selectedPanel}`, "access");
  }
  await page.setViewportSize({ width: 390, height: 844 });
  await go("#home");
  await measure("home-phone", ".machine-work-card", "home");
  const navigation = page.getByRole("button", { name: "Navigation", exact: true });
  for (const close of ["button", "escape", "backdrop", "current-route"]) {
    await navigation.click();
    await page.waitForFunction(() => {
      const nav = document.querySelector('.workspace-sidebar'), trigger = document.querySelector('.workspace-nav-toggle');
      return trigger?.getAttribute('aria-expanded') === 'true' && document.querySelector('.topbar')?.inert === true && nav?.contains(document.activeElement);
    }, null, { timeout: 10000 });
    await measure(`navigation-${close}`, ".workspace-sidebar:not([hidden]) nav.rail", "home", true);
    if (close === "button") await page.locator(".workspace-nav-close").click();
    else if (close === "escape") await page.keyboard.press("Escape");
    else if (close === "backdrop") await page.locator(".workspace-nav-backdrop").click({ position: { x: 380, y: 300 } });
    else await page.getByRole("navigation", { name: "Destinations", exact: true }).getByRole("button", { name: "Overview", exact: true }).click();
    await page.waitForFunction(() => document.querySelector(".workspace-nav-toggle") === document.activeElement && document.querySelector(".workspace-nav-toggle").getAttribute("aria-expanded") === "false", null, { timeout: 10000 });
  }
  await go("#activity");
  await page.locator(".timeline-row button.timeline-line").first().click();
  await measure("activity-evidence-phone", '.activity-inspector.is-sheet [aria-label="Selected evidence details"]', "activity", true);
  await page.keyboard.press("Escape");
  const expected = ["signin", "home", "machines", "activity", "access", "overview", "terminal", "screen", "files", "runs", "tools", "memory", "machine"];
  const covered = new Set(measured.map((scene) => scene.surface));
  if (expected.some((surface) => !covered.has(surface))) throw new Error(`Missing A3 reduced-motion surfaces: ${expected.filter((surface) => !covered.has(surface)).join(", ")}`);
  if (errors.length) throw new Error(`Fixture/page errors: ${errors.join(" ;; ")}`);
  console.log(`motion-gate: PASS — ${measured.length} genuine active views/overlays, 0 elements moving under context reduce`);
  process.exitCode = 0;
} catch (error) {
  console.error(`motion-gate: FAIL — ${error.message}`);
  process.exitCode = 1;
} finally {
  try {
    await writeFile(path.join(OUT, "motion.json"), JSON.stringify({ inputs, passed: process.exitCode === 0, measured, errors }, null, 2) + "\n");
  } finally {
    try { if (context) await context.close(); } finally {
      for (const stream of streams) stream.end();
      server.closeAllConnections();
      if (server.listening) await new Promise((resolve) => server.close(resolve));
    }
  }
}
