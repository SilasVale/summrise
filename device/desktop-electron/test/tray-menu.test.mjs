import { test } from "node:test";
import assert from "node:assert/strict";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const { trayMenuRows } = require("../src/tray-menu.js");
const { fallbackShellPath, launchFallbackShell } = require("../src/fallback-shell.js");

test("the tray menu carries the two device pages, the two panes, the fallback, the version and the update door", () => {
  const rows = trayMenuRows({ version: "2.0.3", health: "running" });
  const labels = rows.map((row) => row.label).filter(Boolean);
  for (const label of ["Open transcript", "Open panel", "Browser", "Harness", "Open fallback shell (window only)", "Check for updates…"]) {
    assert.ok(labels.includes(label), `${label} missing — labels: ${labels.join(", ")}`);
  }
  // The rows the deleted surfaces used to own are GONE, not merely unused.
  assert.ok(!labels.includes("Open"), "there is no generic 'Open' row — the tray icon's click focuses the main window");
  assert.ok(!labels.includes("Open terminal"), "the terminal died with the SPA");
  const version = rows.find((row) => row.action === undefined && /^Version/.test(row.label));
  assert.equal(version.label, "Version 2.0.3");
  assert.equal(version.enabled, false, "the version row is a fact, not a button");
});

test("a tray with no version says so, and an empty health line leaves two separators", () => {
  const rows = trayMenuRows({ version: "", health: "" });
  const version = rows.find((row) => /^Version/.test(row.label));
  assert.equal(version.label, "Version (not reported)");
  assert.equal(
    rows.filter((row) => row.type === "separator").length,
    2,
    "an empty health line and its neighbours must collapse to two separators (never two in a row, never one at an end)",
  );
});

test("the actions the shell must implement are exactly these seven", () => {
  const actions = trayMenuRows({ version: "2.0.3", health: "running" })
    .map((row) => row.action)
    .filter(Boolean);
  assert.deepEqual(actions, ["transcript", "panel", "browser", "harness", "fallback", "update", "quit"]);

  // The installed shell is a sibling of the `desktop` directory that holds SummriseDesktop.exe.
  const execPath = "C:\\Program Files\\Summrise\\desktop\\SummriseDesktop.exe";
  const shellPath = fallbackShellPath(execPath, {}, (p) => p.endsWith("summrise-shell.exe"));
  assert.equal(shellPath, "C:\\Program Files\\Summrise\\summrise-shell.exe");

  // An explicit override wins over the sibling path.
  const override = "D:\\Summrise\\bin\\summrise-shell.exe";
  assert.equal(
    fallbackShellPath(execPath, { SUMMRISE_SHELL_EXE: override }, () => true),
    override,
  );

  // The trigger spawns the WINDOW-ONLY mode: --panel, detached, no stdio, no console window.
  let spawned = null;
  let spawnCalls = 0;
  let unrefCalls = 0;
  const fakeSpawn = (command, args, options) => {
    spawnCalls += 1;
    spawned = { command, args, options };
    return { pid: 42, unref() { unrefCalls += 1; } };
  };
  const launched = launchFallbackShell({
    execPath,
    env: {},
    exists: () => true,
    spawn: fakeSpawn,
  });
  assert.equal(launched.ok, true);
  assert.equal(launched.path, "C:\\Program Files\\Summrise\\summrise-shell.exe");
  assert.equal(launched.pid, 42);
  assert.equal(spawned.command, "C:\\Program Files\\Summrise\\summrise-shell.exe");
  assert.deepEqual(spawned.args, ["--panel"]);
  assert.equal(spawned.options.detached, true);
  assert.equal(spawned.options.stdio, "ignore");
  assert.equal(spawned.options.windowsHide, true);
  assert.equal(spawnCalls, 1);
  assert.equal(unrefCalls, 1, "the detached child is unreferenced exactly once");

  // A missing shell is a visible failure naming the path, not a silent click and not a spawn.
  spawnCalls = 0;
  const missing = launchFallbackShell({
    execPath,
    env: {},
    exists: () => false,
    spawn: fakeSpawn,
  });
  assert.equal(missing.ok, false);
  assert.match(missing.error, /C:\\Program Files\\Summrise\\summrise-shell\.exe/);
  assert.equal(spawnCalls, 0, "no process is started when the shell file is absent");
  assert.equal(unrefCalls, 1, "a refused launch neither spawns nor unreferences another child");

  // A missing explicit override names the override, not the sibling derivation.
  const missingOverride = launchFallbackShell({
    execPath,
    env: { SUMMRISE_SHELL_EXE: override },
    exists: () => false,
    spawn: fakeSpawn,
  });
  assert.equal(missingOverride.ok, false);
  assert.match(missingOverride.error, /SUMMRISE_SHELL_EXE/);
  assert.ok(missingOverride.error.includes(override), missingOverride.error);
  assert.equal(spawnCalls, 0);

  // Node reports a failed spawn asynchronously: no pid, then an 'error' event. The launch must call
  // that a failure (not {ok:true} followed by an uncaught exception) and must listen for the event.
  const events = [];
  let failedUnrefs = 0;
  const noPidSpawn = () => ({
    pid: undefined,
    unref() { failedUnrefs += 1; },
    on(event) { events.push(event); },
  });
  const failed = launchFallbackShell({
    execPath,
    env: {},
    exists: () => true,
    spawn: noPidSpawn,
  });
  assert.equal(failed.ok, false);
  assert.match(failed.error, /C:\\Program Files\\Summrise\\summrise-shell\.exe/);
  assert.deepEqual(events, ["error"], "an error listener is attached even when the child never got a pid");
  assert.equal(failedUnrefs, 0, "a child that never started is not unreferenced");

  // A child that DID get a pid can still emit 'error'; the listener must log it, never throw.
  const logged = [];
  const realConsoleError = console.error;
  const lateEvents = [];
  let lateUnrefs = 0;
  const lateSpawn = () => ({
    pid: 43,
    unref() { lateUnrefs += 1; },
    on(event, listener) {
      lateEvents.push(event);
      if (event === "error") listener(new Error("ENOENT: late spawn failure"));
    },
  });
  let late;
  console.error = (...args) => { logged.push(args.join(" ")); };
  try {
    late = launchFallbackShell({ execPath, env: {}, exists: () => true, spawn: lateSpawn });
  } finally {
    console.error = realConsoleError;
  }
  assert.equal(late.ok, true);
  assert.equal(late.pid, 43);
  assert.equal(lateUnrefs, 1);
  assert.deepEqual(lateEvents, ["error"]);
  assert.ok(
    logged.some((line) => line.includes("summrise-shell.exe")),
    `the late spawn error is logged with its path — logged: ${logged.join(" | ")}`,
  );
});
