// THE WASM POLICY, LOADED IN NODE — the artifact `main.js` requires, executed by a suite.
//
// WHAT THIS FILE IS FOR, AND WHAT IT DELIBERATELY IS NOT. It is NOT a second copy of the policy's cases:
// those moved to `agent/summrise-url-policy/src/tests.rs` when `url-policy.ts` was deleted (11 `#[test]`
// functions, one per `test(...)` block of the old `.mjs`), and a JavaScript copy of them would be a second
// oracle for one decision — which is how two oracles come to disagree. What NOTHING else in this
// repository can check is that the COMMITTED BYTES load and answer: the shell's main process imports
// electron and no suite here can start it, so a missing export, a stale `.wasm` or a glue/module mismatch
// would otherwise be discovered on the operator's device, after a release.
//
// SO IT PINS FOUR THINGS, and the fourth is the one that would have caught a real regression:
//
//   1. THE SURFACE — the 18 names `main.ts` imports, exactly. A renamed or dropped export is a
//      `TypeError` in a shell nobody runs on this box (`test/ipc-door.test.mjs` reads main.ts as text and
//      cannot see the module at all).
//   2. THE ONE CALL SITE PER DECISION — a smoke over the values the SHELL asks about (the agent origin,
//      the load door, the DSH door, `server.port`), not a corpus.
//   3. THE REFUSAL SHAPE — `addDshPort` threw a real `Error` in the TypeScript, and `summrise update`
//      reads what it says; the wasm wrapper throws a real `Error` with the same message.
//   4. THE BOUNDARY CASES a wasm `&str` parameter gets WRONG. wasm-bindgen hands the argument to
//      `passStringToWasm0`, which reads `.length` off it: the first build of this crate TRAPPED with
//      `RuntimeError: memory access out of bounds` on `sanitizeBrowserUrl(42)`, where the TypeScript
//      answered `"about:blank"` — and a renderer reaches that function through `browser-session:open`.
//      The Rust cases cannot see this (they pass `&str`), so it is pinned HERE.
import test from "node:test";
import assert from "node:assert/strict";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const policy = require("../src/summrise_url_policy.js");

/** The 18 names `main.ts` imports from the glue — the whole exported surface, sorted. */
const EXPORTS = [
  "addDshPort",
  "agentBase",
  "certBypassAllowed",
  "clearExtraDshPorts",
  "controlOriginOk",
  "dshBase",
  "dshOrigins",
  "frameUrlOk",
  "getAgentPort",
  "getDshPort",
  "isBaseOrigin",
  "isDesktopSpaUrl",
  "isDshUrl",
  "isPrivateHost",
  "parseAgentPort",
  "sanitizeBrowserUrl",
  "setAgentPort",
  "setDshPort",
];

test("the committed wasm module loads in Node and exports the shell's surface", () => {
  assert.deepEqual(
    Object.keys(policy).sort(),
    EXPORTS,
    "the glue's exports must be exactly the names main.ts imports — the shell requires this file in an Electron main process no suite here can start",
  );
  for (const name of EXPORTS) {
    assert.equal(typeof policy[name], "function", `${name} must be callable`);
  }
});

test("the decisions the shell asks about answer from the committed bytes", () => {
  // The agent's origin and the one load door.
  assert.equal(policy.getAgentPort(), 18080, "the canonical port is the default");
  assert.equal(policy.agentBase(), "http://127.0.0.1:18080");
  assert.equal(policy.isBaseOrigin("http://127.0.0.1:18080/desktop/"), true);
  assert.equal(
    policy.isBaseOrigin("http://127.0.0.1:18080@evil.com/x"),
    false,
    "IPC audit #1: the userinfo prefix must not pass",
  );
  assert.equal(policy.frameUrlOk("http://127.0.0.1:18080/panel/"), true);
  assert.equal(policy.isDesktopSpaUrl("http://127.0.0.1:18080/desktop/settings"), true);
  assert.equal(policy.isDesktopSpaUrl("http://127.0.0.1:18080/desktopx"), false, "/desktop is a path segment");
  assert.equal(policy.sanitizeBrowserUrl("file:///C:/Windows/win.ini"), "about:blank");
  assert.equal(policy.sanitizeBrowserUrl("https://ok.example/a?x=1"), "https://ok.example/a?x=1");
  assert.equal(policy.certBypassAllowed("https://192.168.1.1:8000/?Role=Gpon"), true, "the ONT's self-signed UI");
  assert.equal(policy.certBypassAllowed("https://example.com/"), false, "the public internet stays validated");
  assert.equal(policy.isPrivateHost("192.168.1.1.evil.com"), false, "the suffix lookalike");
  // The control API's origin veto (port 9444) — including the lookalike the old inline regex admitted.
  assert.equal(policy.controlOriginOk("http://127.0.0.1:9444"), true);
  assert.equal(policy.controlOriginOk("http://127.0.0.1.evil.com"), false);
  assert.equal(policy.controlOriginOk(undefined), true, "curl sends no Origin, deliberately allowed");
  // `server.port` out of the device's own config.yaml.
  assert.equal(policy.parseAgentPort('server:\n  host: "0.0.0.0"\n  port: 7740\n'), 7740);
  // AND THIS IS THE ONE ANSWER SPELLED DIFFERENTLY, stated where it is visible: the TypeScript answered
  // `null` and this export answers `undefined` (the generated `.d.ts` says `number | undefined` instead
  // of `any`, which is what bought the change). Both are falsy and the shell's only use is
  // `if (port) return port;` in main.ts, so nothing can observe it — but a reader comparing the two
  // implementations side by side should not have to find that out by accident.
  assert.equal(policy.parseAgentPort("serial:\n  port: 1234\n"), undefined, "another section's port is not the agent's");
  assert.equal(policy.parseAgentPort("server:\n  port: 0\n"), undefined, "an out-of-range port is refused, not defaulted");
  // The DSH view's door, and the second host's forward.
  assert.equal(policy.getDshPort(), 18081, "the DSH is not the agent");
  assert.equal(policy.dshBase(), "http://127.0.0.1:18081");
  assert.equal(policy.isDshUrl("http://127.0.0.1:18081/api/remote.mux"), true);
  assert.equal(policy.isDshUrl("http://127.0.0.1:18080/panel/"), false, "the DSH view must not load the panel");
  try {
    policy.addDshPort(7801);
    assert.deepEqual(policy.dshOrigins(), ["http://127.0.0.1:18081", "http://127.0.0.1:7801"]);
    assert.equal(policy.isDshUrl("http://127.0.0.1:7801/"), true);
  } finally {
    policy.clearExtraDshPorts();
  }
  assert.deepEqual(policy.dshOrigins(), ["http://127.0.0.1:18081"], "the list is not append-only");
});

test("addDshPort throws a real Error, with the TypeScript's message", () => {
  assert.throws(
    () => policy.addDshPort(0),
    (err) => err instanceof Error && err.message === "not a port: 0",
    "0 is not a port, and the refusal is an Error a caller can print",
  );
  assert.throws(() => policy.addDshPort(70000), /not a port: 70000/, "and neither is 70000");
  assert.equal(policy.isDshUrl("http://127.0.0.1:70000/"), false);
});

test("a non-string argument is COERCED, not trapped (the stage-n preload audit)", () => {
  // `browser-session:open` is a renderer-reachable channel whose value reaches loadTarget() ->
  // sanitizeBrowserUrl(). The TypeScript coerced with `String(url || "about:blank")` for exactly this
  // reason; a wasm `&str` parameter would read `.length` off the number and TRAP the module.
  assert.equal(policy.sanitizeBrowserUrl(42), "about:blank");
  assert.equal(policy.sanitizeBrowserUrl({}), "about:blank");
  // `String(["https://ok.example/a"])` IS that string, so this is the answer the TypeScript gave too
  // (the differential that compared the two implementations over 927 inputs is where that was checked,
  // not assumed): the coercion is JavaScript's, not a "non-strings fail closed" rule of its own.
  assert.equal(policy.sanitizeBrowserUrl(["https://ok.example/a"]), "https://ok.example/a");
  assert.equal(policy.sanitizeBrowserUrl(null), "about:blank");
  assert.equal(policy.isPrivateHost(42), false);
  assert.equal(policy.isBaseOrigin(42), false);
  assert.equal(policy.isDshUrl([]), false);
  assert.equal(policy.controlOriginOk([]), false, "[] is TRUTHY, so the TypeScript parsed its empty string and refused it");
  assert.equal(policy.parseAgentPort(42), undefined);
  // The port setters keep `Number.isInteger`'s typeof gate: a numeric string is NOT a port, exactly as
  // the TypeScript ignored it.
  try {
    policy.setAgentPort("7740");
    assert.equal(policy.getAgentPort(), 18080, "a string is not a port");
    policy.setAgentPort(7740);
    assert.equal(policy.getAgentPort(), 7740);
    policy.setAgentPort(NaN);
    policy.setAgentPort(99999);
    assert.equal(policy.getAgentPort(), 7740, "and neither is NaN or an out-of-range number");
  } finally {
    policy.setAgentPort(18080);
    policy.setDshPort(18081);
  }
  assert.equal(policy.agentBase(), "http://127.0.0.1:18080", "default restored");
});
