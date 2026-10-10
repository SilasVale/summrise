// THE SHELL POLICY'S WASM, LOADED IN NODE — the artifact `main.js` requires, executed by a suite.
//
// WHAT THIS FILE IS FOR, AND WHAT IT DELIBERATELY IS NOT. It is NOT a second copy of the policy's cases:
// those are `agent/summrise-shell-policy/src/tests.rs` (55 `#[test]` functions), and a JavaScript copy
// would be a second oracle for one decision — which is how two oracles come to disagree. What NOTHING
// else in this repository can check is that the COMMITTED BYTES load and answer: the shell's main process
// imports electron and no suite here can start it, so a missing export, a stale `.wasm` or a glue/module
// mismatch would otherwise be discovered on the operator's device, after a release.
//
// SO IT PINS FIVE THINGS, and the second and fifth are the ones a real regression would trip:
//
//   1. THE SURFACE — the 57 names `main.ts` imports, exactly. A renamed or dropped export is a
//      `TypeError` in a shell nobody runs on this box.
//   2. **THE TWO MODULES MUST NOT OVERLAP, AND THAT IS THE LANDING'S OWN HARD-WON CONSTRAINT.**
//      wasm-bindgen exports every `#[wasm_bindgen]` item in the whole crate GRAPH, and the first build of
//      `summrise-shell-policy` DEPENDED on `summrise-url-policy` — so its glue came out with 75 exports,
//      the 18 url-policy names among them. That is not a duplicated name, it is a duplicated MODULE: each
//      glue compiles its own `.wasm`, so url-policy's `thread_local!` port state would exist TWICE and
//      `setAgentPort(7740)` through one glue would leave the other at 18080 — a custom-port install where
//      the DSH door admits the wrong origin and the main-window tripwire snaps the panel back. Silent,
//      device-only. The assertion at the bottom of this file is what stops that dependency coming back.
//   3. THE ONE CALL SITE PER DECISION — a smoke over the values the SHELL asks about (the menu table, the
//      browser-open plan, the control router, the watchdog, the tooltip), not a corpus.
//   4. THE BOUNDARY CASES a wasm parameter gets WRONG. Several of these channels are renderer-reachable
//      (`browser-session:open`, `embedded-browser:zoom`, `embedded-dsh:go`, `embedded-browser:place`) and
//      two are NETWORK-reachable (`req.url`, `req.headers.origin`). url-policy's first build TRAPPED with
//      `RuntimeError: memory access out of bounds` on a non-string, so the coercion is pinned HERE, where
//      the JavaScript is.
//   5. THE KILL SWITCH: the shell-policy glue must never export the url-policy names again (see 2).
import test from "node:test";
import assert from "node:assert/strict";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const policy = require("../src/summrise_shell_policy.js");

/** The 57 names the glue exports — sorted, and the WHOLE surface rather than only the ones `main.ts`
 *  names at a call site. TWO OF THEM HAVE NO CALLER IN `main.ts`: `fmtUptime` and `fmtClock` are reached
 *  from inside the module (by `refreshTrayHealth`) and are exported anyway, because their boundaries are
 *  pinned by the Rust cases and executing the SHIPPED bytes over them is worth more than a list one line
 *  shorter. The other fifty-five are named in `main.ts`'s import block. */
const EXPORTS = [
  "agentHostLabel",
  "appMenuJson",
  "aumidReport",
  "authorization",
  "autoLaunchPlan",
  "browserId",
  "cdpEndpoint",
  "cdpSelfCheckOk",
  "cdpSelfCheckOwnsPort",
  "cdpUserAgent",
  "cdpUserAgentOurs",
  "cdpWarningForeign",
  "cdpWarningNotResponding",
  "cdpWarningUnreachable",
  "controlIsPreflight",
  "controlPath",
  "controlQuery",
  "controlRoute",
  "corsAllowOrigin",
  "deviceToken",
  "dshHome",
  "dshPopupTarget",
  "dshTarget",
  "embeddedPopupTarget",
  "embeddedRecoverUrl",
  "fmtClock",
  "fmtUptime",
  "forbiddenFrame",
  "goBackwards",
  "harnessDoors",
  "isWaitPage",
  "nextRetryMs",
  "planBrowserOpen",
  "refreshTrayHealth",
  "resolveAgentPort",
  "resolveDshPort",
  "schtasksCreateArgs",
  "schtasksDeleteArgs",
  "schtasksEndArgs",
  "schtasksQueryArgs",
  "schtasksRunArgs",
  "shellConstants",
  "shouldRetryLoad",
  "shownUrl",
  "slotTooSmall",
  "statusIsAlive",
  "tokenCacheFresh",
  "trayIconName",
  "trayShouldWatch",
  "tripwireAllows",
  "tripwireLog",
  "usesAppUserModelId",
  "viewVisible",
  "watchdogLog",
  "watchdogShouldStart",
  "windowIconName",
  "zoomFactor",
];

/** THE NAMES THAT BELONG TO THE OTHER MODULE, and the reason this list is here rather than in a comment.
 *  `main.ts` requires `./summrise_url_policy` for these; if any of them appears in THIS glue, the two
 *  crates have been linked and url-policy's port state exists twice. See the header, item 2. */
const URL_POLICY_ONLY = [
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

test("THE TWO GLUES DO NOT OVERLAP — a linked crate would duplicate the url policy's port state", () => {
  const linked = URL_POLICY_ONLY.filter((name) => typeof policy[name] !== "undefined");
  assert.deepEqual(
    linked,
    [],
    "summrise_shell_policy.js re-exports the url policy's surface, which means summrise-shell-policy depends on summrise-url-policy again. wasm-bindgen exports the whole crate GRAPH, so the two glues would each compile their OWN .wasm and url-policy's thread_local! port state would exist TWICE: `setAgentPort(7740)` through one module leaves the other at 18080, the DSH door admits the wrong origin, and the tripwire snaps the panel back — on a custom-port install only. REMOVE the dependency; pass the predicate's ANSWER into the shell-policy call instead.",
  );
  // The other side of the same fact: the glue main.ts requires for those names must still BE there.
  const urlPolicy = require("../src/summrise_url_policy.js");
  for (const name of URL_POLICY_ONLY) {
    assert.equal(typeof urlPolicy[name], "function", `${name} must still be exported by the url policy's glue`);
  }
});

test("the decisions the shell asks about answer from the committed bytes", () => {
  // ── boot ──────────────────────────────────────────────────────────────────────────────────────────
  assert.equal(policy.deviceToken("device_token: 0123456789abcdef"), "0123456789abcdef");
  assert.equal(policy.deviceToken("device_token: 0123456789ABCDEF"), undefined, "the pattern is lowercase-hex only");
  assert.equal(policy.deviceToken(""), undefined);
  assert.equal(policy.authorization("abc"), "Bearer abc");
  assert.equal(policy.authorization(""), undefined, "an empty token sends no header");
  assert.equal(policy.tokenCacheFresh(0, 1_759_123_456_789), false, "the cold sentinel is never fresh");
  assert.equal(policy.resolveAgentPort("7740", 9000), 7740, "the environment wins");
  assert.equal(policy.resolveAgentPort(undefined, 9000), 9000, "then the device's own config");
  assert.equal(policy.resolveAgentPort(undefined, undefined), 18080);
  assert.equal(policy.resolveDshPort(undefined), 18081);
  assert.equal(policy.trayIconName("win32"), "icon.ico");
  assert.equal(policy.trayIconName("linux"), "icon.png");
  assert.equal(policy.windowIconName(), "icon.png");
  assert.equal(policy.aumidReport("linux"), "(non-windows)");
  assert.equal(policy.agentHostLabel("http://127.0.0.1:18080"), "127.0.0.1:18080");

  // ── the menu table and the constants ───────────────────────────────────────────────────────────────
  const menu = JSON.parse(policy.appMenuJson("win32", "Summrise"));
  assert.deepEqual(menu.map((s) => s.label), ["File", "Edit", "View", "Session"]);
  assert.equal(menu[0].items[0].command, "new-pty");
  assert.equal(menu[0].items[4].separator, true);
  const mac = JSON.parse(policy.appMenuJson("darwin", "Summrise"));
  assert.equal(mac[0].label, "Summrise", "the Apple menu is labelled with app.name");
  const K = JSON.parse(policy.shellConstants());
  assert.equal(K.CDP_PORT, 9333, "the product contract agent/tests/mcp_autoselect_integration.rs binds");
  assert.equal(K.CTRL_PORT, 9444);
  assert.equal(K.AUTOSTART_TASK, "SummriseDesktop");
  assert.equal(K.AGENT_TASK, "SummriseAgent");
  for (const key of [
    "CDP_PORT", "CTRL_PORT", "MAX_BROWSER_WINDOWS", "MIN_SLOT_PX", "RETRY_START_MS", "RETRY_CAP_MS",
    "TRIPWIRE_BACKOFF_MS", "AUTO_START_AFTER_MISSES", "AUTO_START_MIN_GAP_MS", "TRAY_POLL_MS",
    "AGENT_PROBE_BOOT_MS", "AGENT_PROBE_TRAY_MS", "AGENT_PROBE_DEFAULT_MS", "STATUS_FETCH_MS",
    "HARNESS_FETCH_MS", "CDP_CHECK_MS", "MENU_FLUSH_MS", "CONTROL_PROBE_MS",
    "AUTOSTART_TASK", "AGENT_TASK", "SCHTASKS_TIMEOUT_MS", "CDP_UA_MARKER", "LOG_URL_UNITS",
    "CDP_UA_UNITS", "CORS_ALLOW_METHODS", "CORS_ALLOW_HEADERS", "CORS_VARY",
    "FORBIDDEN_ORIGIN", "NOT_FOUND", "FORBIDDEN_FRAME_ERROR", "ABOUT_BLANK",
    "DESKTOP_AUMID", "AUMID_SET_FAILED", "AUMID_NON_WINDOWS",
  ]) {
    assert.notEqual(K[key], undefined, `shellConstants() must carry ${key} — main.ts destructures it`);
  }

  // ── the browser sessions and the two views ─────────────────────────────────────────────────────────
  const existing = JSON.stringify([{ id: "browser-1", target: "https://a/", destroyed: false }]);
  assert.deepEqual(JSON.parse(policy.planBrowserOpen(existing, "https://a/", 8)), { reuse: "browser-1", evict: null });
  assert.deepEqual(JSON.parse(policy.planBrowserOpen(existing, "https://b/", 1)), { reuse: null, evict: "browser-1" });
  assert.equal(policy.browserId(1_759_123_456_789), "browser-1759123456789");
  assert.equal(policy.cdpEndpoint(9333), "http://127.0.0.1:9333");
  assert.equal(policy.slotTooSmall(null), true);
  assert.equal(policy.slotTooSmall({ width: 40, height: 400 }), true);
  assert.equal(policy.slotTooSmall({ width: 400, height: 50 }), false);
  assert.equal(policy.embeddedPopupTarget("about:blank"), undefined, "a refused popup is DROPPED, not blanked");
  assert.equal(policy.embeddedPopupTarget("https://ok.example/"), "https://ok.example/");
  assert.equal(policy.dshTarget("http://127.0.0.1:18081/a", true), "http://127.0.0.1:18081/a");
  assert.equal(policy.dshTarget("http://127.0.0.1:18080/panel/", false), "about:blank", "isDshUrl is the host's answer");
  assert.equal(policy.dshHome("http://127.0.0.1:18081"), "http://127.0.0.1:18081/");
  assert.equal(policy.zoomFactor(0), 1, "`Number(f) || 1` — a zoom of 0 is 100%");
  assert.equal(policy.zoomFactor(9), 3);
  assert.equal(policy.embeddedRecoverUrl(""), "about:blank");
  assert.equal(policy.shownUrl(undefined, "https://last/"), "https://last/");
  assert.equal(policy.viewVisible(true, false), false);
  assert.equal(policy.goBackwards(-1), true);
  // `Vec<u16>` crosses the boundary as a `Uint16Array`, which is what `for (const port of …)` iterates
  // over in main.ts — so the assertion says so rather than comparing it to an Array and hiding the type.
  assert.deepEqual(Array.from(policy.harnessDoors([{ local_port: 7801 }, { local_port: 0 }, { local_port: "18082" }])), [7801, 18082]);
  assert.deepEqual(Array.from(policy.harnessDoors("not an array")), []);

  // ── the control server ─────────────────────────────────────────────────────────────────────────────
  assert.equal(policy.controlPath("/api/shell/icon-status?x=1"), "/api/shell/icon-status");
  assert.equal(policy.controlPath(undefined), "/", "`req.url || \"/\"`");
  assert.equal(policy.controlQuery("/api/browser-session/open?url=https%3A%2F%2Fok.example%2Fa%3Fx%3D1", "url"), "https://ok.example/a?x=1");
  assert.equal(policy.controlIsPreflight("OPTIONS"), true);
  assert.equal(policy.controlRoute("POST", "/api/browser-session/open"), "browser-open");
  assert.equal(policy.controlRoute("GET", "/api/browser-session/open"), "not-found", "POST-only routes fall through");
  assert.equal(policy.controlRoute("POST", "/api/browser-session/list"), "browser-list", "the read routes have no method guard");
  assert.equal(policy.corsAllowOrigin(undefined), "*");

  // ── the lifecycle, the tripwire and the CDP self-check ─────────────────────────────────────────────
  assert.equal(policy.autoLaunchPlan(true, true), "nothing");
  assert.equal(policy.autoLaunchPlan(true, false), "create");
  assert.equal(policy.autoLaunchPlan(false, true), "remove");
  assert.deepEqual(policy.schtasksRunArgs("SummriseAgent"), ["/run", "/tn", "SummriseAgent"]);
  assert.ok(
    policy.schtasksCreateArgs("SummriseDesktop", "C:\\s\\start-desktop.ps1")[4].includes('\\"C:\\s\\start-desktop.ps1\\"'),
    "the /tr value keeps its OWN inner quotes — schtasks re-parses it as a command line",
  );
  assert.equal(policy.watchdogShouldStart(4, 0, 1_700_000_000_000), false);
  assert.equal(policy.watchdogShouldStart(5, 0, 1_700_000_000_000), true);
  assert.equal(policy.nextRetryMs(16_000), 30_000);
  assert.equal(policy.shouldRetryLoad(true, -3), false, "ERR_ABORTED must not retry");
  assert.equal(policy.shouldRetryLoad(true, -105), true);
  assert.equal(policy.isWaitPage("data:text/html,x"), true);
  assert.equal(policy.statusIsAlive(401), true, "a token-gated agent answering 401 IS alive");
  assert.equal(policy.statusIsAlive(undefined), false);
  assert.equal(policy.tripwireAllows("https://qq.com/", false), false);
  assert.equal(policy.tripwireAllows("about:blank", false), true);
  assert.equal(policy.tripwireAllows("http://127.0.0.1:18080/desktop/", true), true);
  assert.equal(policy.cdpUserAgentOurs("summrise-desktop-electron"), true);
  assert.equal(policy.cdpSelfCheckOwnsPort('{"User-Agent":"x summrise-desktop-electron"}'), true);
  assert.equal(policy.cdpUserAgent("{}"), "");

  // ── one poll's whole decision ──────────────────────────────────────────────────────────────────────
  const AT = 1_767_225_600_000;
  const poll = JSON.parse(policy.refreshTrayHealth(JSON.stringify({
    running: true,
    statusText: '{"release":"1.2.510","uptime_secs":204,"live_sessions":2,"cpu_pct":4.4,"mem_pct":12.6}',
    prev: { version: "", uptime: "", sessions: 0, cpu: null, mem: null },
    observedAt: AT,
    lastAnsweredAt: null,
    tzOffsetMin: 0,
  })));
  assert.equal(poll.health, "answered 00:00:00 \u00b7 v1.2.510, up 3m 24s, 2 sessions, CPU 4% \u00b7 MEM 13%");
  assert.equal(poll.lastAnsweredAt, AT);
  const down = JSON.parse(policy.refreshTrayHealth(JSON.stringify({
    running: false, statusText: "", prev: poll.facts, observedAt: AT, lastAnsweredAt: null, tzOffsetMin: 0,
  })));
  assert.equal(down.health, "reachability NOT VERIFIED \u00b7 no reply since launch");
  assert.equal(down.facts.version, "1.2.510", "a failed poll keeps what the last answer said");
  assert.equal(policy.trayShouldWatch(false, false, true, true), true);
  assert.equal(policy.trayShouldWatch(false, false, true, false), false);
});

test("the refusal is a real object with the SPA's shape, not a string", () => {
  const refusal = JSON.parse(policy.forbiddenFrame());
  assert.deepEqual(refusal, { ok: false, error: "forbidden frame" });
});

test("a non-string on a RENDERER-REACHABLE channel is COERCED, not trapped", () => {
  // `browser-session:open` (url), `embedded-browser:zoom` (factor), `embedded-dsh:go` (url) and
  // `embedded-browser:place` (bounds) all carry a value a renderer chose, and `req.url` /
  // `req.headers.origin` carry a value the NETWORK chose. url-policy's first build trapped the module on
  // exactly this; the coercions below are the same ones it needed.
  assert.equal(policy.dshTarget(42, true), "about:blank");
  assert.equal(policy.zoomFactor("2"), 2, "`Number(factor)` is JavaScript's conversion");
  assert.equal(policy.zoomFactor({}), 1, "`Number({})` is NaN, and `NaN || 1` is 1");
  assert.equal(policy.zoomFactor(null), 1);
  assert.equal(policy.slotTooSmall({ width: "40", height: "400" }), true, "`\"40\" < 50` is true in JavaScript too");
  assert.equal(policy.slotTooSmall("nonsense"), false, "a truthy non-object has no width or height, and `undefined < 50` is false — so it PLACES rather than hides, exactly as the TypeScript did");
  assert.equal(policy.controlPath(42), "/42");
  assert.equal(policy.corsAllowOrigin(7), "7");
  assert.equal(policy.deviceToken(42), undefined, "a non-string config is the empty document");
  assert.equal(policy.agentHostLabel(null), "null", "`String(null)` is `\"null\"`, and a base of it strips nothing");
  assert.deepEqual(Array.from(policy.harnessDoors([null, 5, { local_port: 7801 }])), [7801], "a null row is not a port");
  assert.equal(policy.trayShouldWatch(false, false, true, true), true);
  assert.equal(policy.isWaitPage(null), false);
});
