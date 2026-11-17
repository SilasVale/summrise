// THE HOST'S READ-ONLY SELF-CHECK — the document a 9444 GET answers, pinned as a suite.
//
// Five things are load-bearing here, and only the first is about the happy path:
//   1. THE SHAPE IS THE CONTRACT (`plan.md:64-74`): the exact key set, the types, the values a temp
//      install can produce, and the hashes of the bytes that are actually on disk.
//   2. ABSENCE IS DATA, NOT A CRASH: a missing config, missing exe/asar/shell and an agent that never
//      answers all produce nulls/false in the same document — the handler must be able to answer 200
//      with an installation that is broken, and the 500 path is for the unexpected throw. A DENIED
//      config is not absence: it is present but unreadable, with the token unknown.
//   3. NO TOKEN VALUE: the only token-named key is `token_present`, and it is a boolean. The config
//      BODY is never copied into the document — the test writes a canary marker into it and asserts
//      the serialized document does not contain it. No test fixture holds a credential.
//   4. THE ROUTE IS HOST-OWNED: the policy crate's router returns `not-found` for it (measured), so a
//      future crate change that claims the name would make this file fail rather than silently
//      shadow a Rust decision.
//   5. THE ARCHIVE GOES THROUGH THE RAW READER (added after review found the live defect): Electron's
//      patched `fs` presents `<app.asar>` as a directory (`isFile()` false, `readFileSync` fails), so
//      a plain-`fs` implementation answers null for exactly the bytes the route exists to prove. The
//      test leaves the archive path as a directory, shows plain `fs` is blind to it, and requires the
//      document's asar bytes/hash to come from the injected raw reader — it fails if `fs` was used.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { createRequire } from "node:module";

import {
  SELF_CHECK_METHOD, SELF_CHECK_PATH, SELF_CHECK_SCHEMA, collectSelfCheck,
} from "../src/self-check.js";

const require = createRequire(import.meta.url);
const policy = require("../src/summrise_shell_policy.js");

const sha256 = (buf) => crypto.createHash("sha256").update(buf).digest("hex");

/** A throwaway install root shaped like the real one, with no config and no binaries yet: each test
 *  writes only what it is about. The canary is a body marker, never a token. */
const CANARY = "CANARY_CONFIG_BODY_2d9f";
function tempInstall(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "summrise-self-check-"));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const p = {
    root,
    config: path.join(root, "etc", "config.yaml"),
    exe: path.join(root, "desktop", "SummriseDesktop.exe"),
    asar: path.join(root, "desktop", "resources", "app.asar"),
    shell: path.join(root, "summrise-shell.exe"),
    current: path.join(root, "current.txt"),
    previous: path.join(root, "previous.txt"),
  };
  fs.mkdirSync(path.join(root, "etc"), { recursive: true });
  fs.mkdirSync(path.join(root, "desktop", "resources"), { recursive: true });
  return p;
}

function deps(p, over = {}) {
  return {
    installRoot: p.root,
    configPath: p.config,
    tokenPresent: false,
    agentBase: "http://127.0.0.1:18090",
    agentHealth: async () => false,
    appPath: p.asar,
    exePath: p.exe,
    electron: "33.4.11",
    appVersion: "0.2.0",
    shellPath: p.shell,
    currentPath: p.current,
    previousPath: p.previous,
    ...over,
  };
}

test("the document is the pinned shape, from a temp install, with real hashes", async (t) => {
  const p = tempInstall(t);
  const configBody = `# ${CANARY}\nserver:\n  port: 18090\n`;
  const exeBytes = Buffer.from("MZ-temp-desktop-exe");
  const asarBytes = Buffer.from("temp-asar-bytes");
  const shellBytes = Buffer.from("temp-shell-bytes");
  fs.writeFileSync(p.config, configBody);
  fs.writeFileSync(p.exe, exeBytes);
  fs.writeFileSync(p.asar, asarBytes);
  fs.writeFileSync(p.shell, shellBytes);
  fs.writeFileSync(p.current, "2.0.9\n");
  fs.writeFileSync(p.previous, "2.0.8\n");

  const doc = await collectSelfCheck(deps(p, { tokenPresent: true, agentHealth: async () => true }));

  // The exact key set — a new fact is a contract change and must be deliberate.
  assert.deepEqual(Object.keys(doc).sort(),
    ["agent", "at", "config", "host", "install", "install_root", "ok", "route", "schema", "shell"]);
  assert.deepEqual(Object.keys(doc.config).sort(),
    ["bytes", "exists", "path", "readable", "token_present"]);
  assert.deepEqual(Object.keys(doc.agent).sort(), ["base", "health"]);
  assert.deepEqual(Object.keys(doc.host).sort(),
    ["app_version", "asar", "asar_bytes", "asar_sha256", "electron", "exe", "exe_bytes", "pid"]);
  assert.deepEqual(Object.keys(doc.shell).sort(), ["bytes", "path", "sha256"]);
  assert.deepEqual(Object.keys(doc.install).sort(), ["current", "previous"]);

  assert.equal(doc.ok, true);
  assert.equal(doc.schema, SELF_CHECK_SCHEMA);
  assert.equal(doc.schema, 1);
  assert.equal(doc.route, SELF_CHECK_PATH);
  assert.equal(new Date(doc.at).toISOString(), doc.at);
  assert.equal(doc.install_root, p.root);

  // exists/bytes come from stat only; token_present is the passed boolean, never derived here.
  assert.deepEqual(doc.config, {
    path: p.config, exists: true, bytes: Buffer.byteLength(configBody), readable: true,
    token_present: true,
  });
  assert.deepEqual(doc.agent, { base: "http://127.0.0.1:18090", health: true });

  assert.equal(doc.host.pid, process.pid);
  assert.equal(doc.host.exe, p.exe);
  assert.equal(doc.host.exe_bytes, exeBytes.length);
  assert.equal(doc.host.asar, p.asar);
  assert.equal(doc.host.asar_bytes, asarBytes.length);
  assert.equal(doc.host.asar_sha256, sha256(asarBytes));
  assert.equal(doc.host.electron, "33.4.11");
  assert.equal(doc.host.app_version, "0.2.0");

  assert.deepEqual(doc.shell, { path: p.shell, bytes: shellBytes.length, sha256: sha256(shellBytes) });
  assert.deepEqual(doc.install, { current: "2.0.9", previous: "2.0.8" });

  // The config body is never copied into the answer.
  assert.equal(JSON.stringify(doc).includes(CANARY), false);
});

test("missing config/files and a dead agent answer nulls/false, and never throw", async (t) => {
  const p = tempInstall(t);
  const doc = await collectSelfCheck(deps(p, {
    // agentResponds' failure mode: an error/timeout resolves false. A throw must not escape either.
    agentHealth: async () => { throw new Error("agent down"); },
  }));

  assert.equal(doc.ok, true);
  assert.deepEqual(doc.config, {
    path: p.config, exists: false, bytes: null, readable: false, token_present: false,
  });
  assert.deepEqual(doc.agent, { base: "http://127.0.0.1:18090", health: false });
  assert.equal(doc.host.exe_bytes, null);
  assert.equal(doc.host.asar_bytes, null);
  assert.equal(doc.host.asar_sha256, null);
  assert.equal(doc.shell.bytes, null);
  assert.equal(doc.shell.sha256, null);
  assert.deepEqual(doc.install, { current: null, previous: null });
});

test("a denied config is present but unreadable, and the token is unknown", async (t) => {
  const p = tempInstall(t);
  const errorWith = (code) => Object.assign(new Error(`${code}: permission denied`), { code });

  // Shape 1 — the stat itself is denied (Windows withholds FILE_READ_ATTRIBUTES). The file is there;
  // the host may not ask about it. A denial must never be reported as absence.
  const deniedStat = {
    statSync() { throw errorWith("EACCES"); },
    openSync() { throw new Error("a denied stat must not reach open"); },
    closeSync() { throw new Error("nothing was opened, so nothing may be closed"); },
  };
  const a = await collectSelfCheck(deps(p, { tokenPresent: true, configFs: deniedStat }));
  assert.deepEqual(a.config, {
    path: p.config, exists: true, bytes: null, readable: false, token_present: null,
  });

  // Shape 2 — the stat succeeds but the open is denied: the size is proved, the body is not, and a
  // token the host could read before says nothing about this file. bytes keep the stat size.
  const body = "server:\n  port: 18090\n";
  fs.writeFileSync(p.config, body);
  let closes = 0;
  const deniedOpen = {
    statSync() { return { isFile: () => true, size: Buffer.byteLength(body) }; },
    openSync() { throw errorWith("EACCES"); },
    closeSync() { closes += 1; },
  };
  const b = await collectSelfCheck(deps(p, { tokenPresent: true, configFs: deniedOpen }));
  assert.deepEqual(b.config, {
    path: p.config, exists: true, bytes: Buffer.byteLength(body), readable: false,
    token_present: null,
  });
  assert.equal(closes, 0);

  // An injected ENOENT is absence — the one state that answers token_present:false, never null.
  const missing = {
    statSync() { throw errorWith("ENOENT"); },
    openSync() { throw new Error("a missing file must not be opened"); },
    closeSync() { throw new Error("nothing was opened, so nothing may be closed"); },
  };
  const c = await collectSelfCheck(deps(p, { tokenPresent: true, configFs: missing }));
  assert.deepEqual(c.config, {
    path: p.config, exists: false, bytes: null, readable: false, token_present: false,
  });

  // The denied cell is always the five-key state, whatever the shape.
  for (const doc of [a, b, c]) {
    assert.deepEqual(Object.keys(doc.config).sort(),
      ["bytes", "exists", "path", "readable", "token_present"]);
    assert.equal(typeof doc.config.readable, "boolean");
  }
});

test("the running archive is read through the raw reader, never Electron's patched fs", async (t) => {
  const p = tempInstall(t);
  // Electron's patched fs presents the archive as a DIRECTORY: `statSync().isFile()` is false (bytes
  // would be null) and `readFileSync` fails. Leaving the asar path as a directory reproduces that
  // state without Electron.
  fs.mkdirSync(p.asar);
  const exeBytes = Buffer.from("exe");
  const shellBytes = Buffer.from("shell");
  fs.writeFileSync(p.exe, exeBytes);
  fs.writeFileSync(p.shell, shellBytes);

  const archiveBytes = Buffer.from("asar-archive-bytes-through-the-raw-reader");
  const calls = [];
  const archiveFs = {
    statSync(q) { calls.push(["stat", q]); return { isFile: () => true, size: archiveBytes.length }; },
    readFileSync(q) { calls.push(["read", q]); return archiveBytes; },
  };

  const doc = await collectSelfCheck(deps(p, { archiveFs }));

  // The archive facts came from the raw reader, and from it alone, in this order.
  assert.deepEqual(calls, [["stat", p.asar], ["read", p.asar]]);
  assert.equal(doc.host.asar, p.asar);
  assert.equal(doc.host.asar_bytes, archiveBytes.length);
  assert.equal(doc.host.asar_sha256, sha256(archiveBytes));

  // The other paths keep the normal fs: the exe and shell really are stat'd/read on disk.
  assert.equal(doc.host.exe_bytes, exeBytes.length);
  assert.deepEqual(doc.shell, { path: p.shell, bytes: shellBytes.length, sha256: sha256(shellBytes) });

  // The simulated patched fs really is blind to the archive — so without the raw reader the document
  // answers null. That is the live defect this test exists to keep from coming back.
  assert.equal(fs.statSync(p.asar).isFile(), false);
  assert.throws(() => fs.readFileSync(p.asar), /EISDIR/);
  const patched = await collectSelfCheck(deps(p));
  assert.equal(patched.host.asar_bytes, null);
  assert.equal(patched.host.asar_sha256, null);
});

test("no token value can reach the document: the only token-named key is the boolean", async (t) => {
  const p = tempInstall(t);
  fs.writeFileSync(p.config, `# ${CANARY}\nserver:\n  port: 18090\n`);
  fs.writeFileSync(p.current, "2.0.9\n");
  fs.writeFileSync(p.previous, "2.0.8\n");

  const doc = await collectSelfCheck(deps(p, { tokenPresent: true }));
  const json = JSON.stringify(doc);

  const tokenKeys = [];
  (function walk(node) {
    for (const [k, v] of Object.entries(node)) {
      if (/token/i.test(k)) tokenKeys.push(k);
      if (v && typeof v === "object") walk(v);
    }
  })(doc);
  assert.deepEqual(tokenKeys, ["token_present"]);
  assert.equal(typeof doc.config.token_present, "boolean");
  assert.equal(doc.config.token_present, true);
  assert.equal(/"token"\s*:/i.test(json), false);
  assert.equal(json.includes(CANARY), false);
  assert.equal(json.includes("Bearer"), false);
});

test("the route is host-owned: GET only, and the policy crate does not claim it", () => {
  assert.equal(SELF_CHECK_PATH, "/api/shell/self-check");
  assert.equal(SELF_CHECK_METHOD, "GET");
  assert.equal(SELF_CHECK_SCHEMA, 1);
  // Measured through the committed wasm: the crate's table has no such route, so the host branch is
  // the only thing that answers. If the crate ever claimed it, this fails and the branch is re-thought.
  assert.equal(policy.controlRoute("GET", SELF_CHECK_PATH), "not-found");
  assert.equal(policy.controlRoute("POST", SELF_CHECK_PATH), "not-found");
  // The query is stripped before the branch sees the pathname (the host's `controlPath` contract).
  assert.equal(policy.controlPath(`${SELF_CHECK_PATH}?probe=1`), SELF_CHECK_PATH);
});
