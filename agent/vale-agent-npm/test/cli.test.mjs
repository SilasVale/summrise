// npm CLI first tests (coverage audit rows 13+14). The CLI is the SOLE
// install/update channel and runs PowerShell under SYSTEM/admin — its
// quoting and update-mutual-exclusion previously had zero coverage.
// bin/vale.js exports the pure helpers (dispatch is require.main-guarded).
import test from "node:test";
import assert from "node:assert/strict";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const fs = require("node:fs");
const {
  psq,
  busyIsFresh,
  deskShortcutRepairPs,
  playwrightProbePs,
  parseAgentPort,
  agentPort,
  parseDeviceToken,
  parseTargetArg,
  parseWatchArgs,
  probeLine,
  monitorsJson,
  asciiJson,
  waitDecision,
  waitLine,
  parseWaitArgs,
  stripAnsi,
  lastLine,
  lastOutputBefore,
  reportText,
  targetLine,
  transitionLines,
  fmtDuration,
  firewallPs,
  uninstallVersionPs,
  uninstallRegBodyPs,
  BOOT_TASKS,
  autostartArgv,
  bootTaskPs,
  migrateLayoutPs,
  startDesktopPs,
  rollbackVersionOk,
} = require("../bin/vale.js");

test("psq: PowerShell single-quote doubling (injection surface for SYSTEM task scripts)", () => {
  assert.equal(
    psq("C:\\Program Files\\Vale\\a'b"),
    "C:\\Program Files\\Vale\\a''b",
  );
  assert.equal(psq("/plain/path"), "/plain/path");
  assert.equal(psq(""), "");
  assert.equal(psq("'"), "''");
  assert.equal(psq("a'b'c"), "a''b''c");
});

test("busyIsFresh: the 10-minute update-exclusion window", () => {
  const now = 1_700_000_000_000;
  const MIN = 60_000;
  assert.equal(
    busyIsFresh(now - 9 * MIN, now),
    true,
    "9 min old = in-progress, refuse",
  );
  assert.equal(
    busyIsFresh(now - 11 * MIN, now),
    false,
    "11 min old = stale marker after reboot, proceed",
  );
  assert.equal(busyIsFresh(now, now), true, "brand-new = fresh");
  assert.equal(
    busyIsFresh(now - 10 * MIN - 1, now),
    false,
    "just past the window",
  );
});

test("deskShortcutRepairPs: stale-shortcut repair is repair-only + sunrise-pinned", () => {
  const lines = deskShortcutRepairPs(
    "D:\\Vale\\scripts",
    "D:\\Vale\\components\\vale-desktop-electron",
    "Write-Host",
  );
  const body = lines.join("\n");
  assert.match(body, /Vale\.lnk/, "touches the desktop Vale link");
  assert.match(body, /vale-desktop\.exe/, "detects the retired Tauri target");
  assert.match(body, /vale-tray\.exe/, "detects the retired tray target");
  assert.match(body, /icon\.ico/, "pins IconLocation to the sunrise ico");
  assert.match(
    body,
    /start-desktop\.ps1/,
    "repoints at the Electron onlogon path",
  );
  assert.match(body, /Write-Host/, "uses the caller sink for logging");
  assert.ok(
    !body.includes("Remove-Item -Recurse"),
    "never deletes directories, files only",
  );
});

test("playwrightProbePs: waits for desktop CDP before forking headless", () => {
  const body = playwrightProbePs().join("\n");
  assert.match(body, /Test-Port 9333/, "probes the desktop CDP port");
  assert.match(
    body,
    /for \(\$i = 1/,
    "retries instead of a single check (boot race)",
  );
  assert.match(body, /Start-Sleep -Seconds 5/, "backs off between probes");
  assert.match(
    body,
    /--cdp-endpoint \$ep/,
    "attaches to the watched view when up",
  );
  assert.match(body, /--headless/, "keeps the private-chromium fallback");
  assert.match(
    body,
    /127\.0\.0\.1:9229,localhost:9229/,
    "keeps the anti-DNS-rebinding hosts",
  );
  assert.match(
    body,
    /--output-dir \$pwout/,
    "pins screenshots to the evidence dir",
  );
  assert.ok(
    ![...body].some((c) => c.charCodeAt(0) > 127),
    "ASCII-only (system-locale PS)",
  );
});

test("parseAgentPort: server.port only, strict", () => {
  const { parseAgentPort } = require("../bin/vale.js");
  assert.equal(
    parseAgentPort('server:\n  host: "0.0.0.0"\n  port: 7740\n'),
    7740,
  );
  assert.equal(parseAgentPort("server:\n  port: 18080\n"), 18080);
  assert.equal(
    parseAgentPort('server:\n  host: "127.0.0.1"\n'),
    null,
    "absent port",
  );
  assert.equal(
    parseAgentPort("serial:\n  port: 1234\n"),
    null,
    "non-server section ignored",
  );
  assert.equal(
    parseAgentPort("server:\n  port: 0\n"),
    null,
    "ephemeral rejected",
  );
  assert.equal(
    parseAgentPort("server:\n  port: 99999\n"),
    null,
    "out of range rejected",
  );
  assert.equal(parseAgentPort(""), null);
});

test("agentPort: reads dir config, defaults 18080", () => {
  const fs = require("node:fs");
  const os = require("node:os");
  const path = require("node:path");
  const { agentPort } = require("../bin/vale.js");
  assert.equal(agentPort("/definitely/not/here"), 18080, "missing config");
  const d = fs.mkdtempSync(path.join(os.tmpdir(), "vale-port-"));
  fs.writeFileSync(path.join(d, "config.yaml"), "server:\n  port: 7740\n");
  assert.equal(agentPort(d), 7740);
  fs.rmSync(d, { recursive: true, force: true });
});

test("firewallPs: idempotent Vale-scoped rule for the port", () => {
  const { firewallPs } = require("../bin/vale.js");
  const body = firewallPs(7740).join("\n");
  assert.match(
    body,
    /LocalPort \$fwPort/,
    "uses the variable, hardcodes nothing else",
  );
  assert.match(body, /\$fwPort = 7740/, "bakes the configured port");
  assert.match(body, /New-NetFirewallRule/, "creates the allow rule");
  assert.match(body, /Remove-NetFirewallRule/, "prunes stale own rules");
  assert.match(body, /'Vale Agent'/, "DisplayName-scoped, never foreign rules");
  assert.ok(
    ![...body].some((c) => c.charCodeAt(0) > 127),
    "ASCII-only (system-locale PS)",
  );
});

test("startDesktopPs: the electron launcher matches the migrated layout", () => {
  const { startDesktopPs } = require("../bin/vale.js");
  const body = startDesktopPs(
    "D:\\Vale\\components\\vale-desktop-electron",
  ).join("\n");
  assert.match(
    body,
    /\$dir = 'D:\\Vale\\components\\vale-desktop-electron'/,
    "pins the components dir",
  );
  assert.match(
    body,
    /Set-Location \$dir/,
    "cwd matters (electron resolves package.json main)",
  );
  assert.match(
    body,
    /node_modules\\electron\\dist\\electron\.exe/,
    "launches the boxed electron",
  );
  // second-instance (ValeDesktop pulse every 5 min) must NOT open a window —
  // the app's single-instance lock focuses the existing one. No -new flag here.
  assert.ok(
    !/Start-Process/i.test(body),
    "plain invocation (focus steal is the app's job)",
  );
  assert.ok(
    ![...body].some((c) => c.charCodeAt(0) > 127),
    "ASCII-only (system-locale PS)",
  );
});

test("writeReleaseMarker: fresh-install parity with the round-298 update marker", () => {
  const fs = require("node:fs");
  const os = require("node:os");
  const path = require("node:path");
  const { writeReleaseMarker } = require("../bin/vale.js");
  const d = fs.mkdtempSync(path.join(os.tmpdir(), "vale-relmark-"));
  try {
    // Writes the package.json version under etc/ (layout v2). Callers
    // (setup/update) always create etc/ during staging/migration first.
    fs.mkdirSync(path.join(d, "etc"), { recursive: true });
    writeReleaseMarker(d);
    const pkg = JSON.parse(
      fs.readFileSync(new URL("../package.json", import.meta.url), "utf8"),
    );
    assert.equal(
      fs.readFileSync(path.join(d, "etc", ".vale-release"), "utf8"),
      pkg.version,
    );
    // Idempotent (re-run overwrites with the same value).
    writeReleaseMarker(d);
    assert.equal(
      fs.readFileSync(path.join(d, "etc", ".vale-release"), "utf8"),
      pkg.version,
    );
  } finally {
    fs.rmSync(d, { recursive: true, force: true });
  }
});

test("writeReleaseMarker: missing dir stays silent (best-effort, never throws)", () => {
  const { writeReleaseMarker } = require("../bin/vale.js");
  assert.doesNotThrow(() => writeReleaseMarker("Z:\\definitely\\not\\here"));
});

test("uninstallVersionPs: $ok-gated DisplayVersion parity, UninstallString only when absent", () => {
  const { uninstallVersionPs } = require("../bin/vale.js");
  const body = uninstallVersionPs("C:\\Program Files\\Vale", "1.2.307").join(
    "\n",
  );
  assert.match(
    body,
    /\$ok -and '1\.2\.307'/,
    "gated on provable swap success like .vale-release",
  );
  assert.match(body, /DisplayVersion/, "moves the Add/Remove version");
  assert.match(body, /DisplayName/, "moves the display name with it");
  assert.match(body, /InstallLocation/, "records where the release lives");
  assert.match(
    body,
    /catch \{\}/,
    "best-effort: registry failure never fails the update",
  );
  assert.match(
    body,
    /Test-Path \$rk/,
    "creates the key for npm-only installs that lack one",
  );
  assert.ok(
    ![...body].some((c) => c.charCodeAt(0) > 127),
    "ASCII-only (system-locale PS)",
  );
});

test("uninstallRegBodyPs: shared setup/swap body, conditional elevated uninstall", () => {
  const { uninstallRegBodyPs } = require("../bin/vale.js");
  const body = uninstallRegBodyPs("D:\\Vale", "1.2.307").join("\n");
  assert.ok(
    !body.includes("$ok"),
    "ungated body (setup has no $ok; the swap wraps it)",
  );
  assert.match(body, /DisplayVersion/, "versions the entry");
  // UninstallString is conditional — an NSIS install owns its
  // $INSTDIR\uninstall.exe value and it must never be overwritten.
  assert.match(
    body,
    /Get-ItemProperty -Path \$rk -Name UninstallString/,
    "reads before writing",
  );
  const idxRead = body.indexOf(
    "Get-ItemProperty -Path $rk -Name UninstallString",
  );
  const idxWrite = body.indexOf("-Name UninstallString -Value", idxRead);
  assert.ok(
    idxRead >= 0 && idxWrite > idxRead,
    "write is guarded by the absence read",
  );
  // v2 path first, legacy fallback second.
  const idxComp = body.indexOf("components\\npm-global\\vale.cmd");
  const idxTools = body.indexOf("tools\\npm-global\\vale.cmd");
  assert.ok(
    idxComp >= 0 && idxTools > idxComp,
    "components first, tools legacy fallback",
  );
  // Control panel does not elevate: relaunch elevated or uninstall dies.
  assert.match(body, /-Verb RunAs/, "re-elevates (HKLM/schtasks need admin)");
  assert.match(body, /-Wait/, "control panel waits for completion");
  // Empty version = no-op (setup with an unreadable package.json).
  assert.deepEqual(
    uninstallRegBodyPs("D:\\Vale", ""),
    [],
    "empty ver writes nothing",
  );
  assert.ok(
    ![...body].some((c) => c.charCodeAt(0) > 127),
    "ASCII-only (system-locale PS)",
  );
});

test("autostartArgv: ENABLE/DISABLE both boot tasks, no credential-prompt flags", () => {
  const { autostartArgv, BOOT_TASKS } = require("../bin/vale.js");
  assert.deepEqual(
    [...BOOT_TASKS].sort(),
    ["ValeAgent", "ValeDesktop"],
    "both boot tasks covered",
  );
  for (const t of BOOT_TASKS) {
    assert.deepEqual(autostartArgv(t, "off"), [
      "schtasks",
      "/Change",
      "/TN",
      t,
      "/DISABLE",
    ]);
    assert.deepEqual(autostartArgv(t, "on"), [
      "schtasks",
      "/Change",
      "/TN",
      t,
      "/ENABLE",
    ]);
  }
  const all = BOOT_TASKS.flatMap((t) => [
    autostartArgv(t, "on").join(" "),
    autostartArgv(t, "off").join(" "),
  ]).join("\n");
  for (const banned of ["/RU", "/RP", "/RI", "/TR"]) {
    assert.ok(
      !all.includes(banned),
      `${banned} must never appear (it prompts for the account password and hangs)`,
    );
  }
});

test("bootTaskPs: explicit config argument, hardened SYSTEM task, optional kick", () => {
  const { bootTaskPs } = require("../bin/vale.js");
  const reg = bootTaskPs(
    "C:\\V\\vale-agent.exe",
    "C:\\V\\etc\\config.yaml",
    false,
  ).join("\n");
  assert.match(
    reg,
    /-Argument \('"'\s*\+\s*'C:\\V\\etc\\config\.yaml'\s*\+\s*'"'\)/,
    "-Argument is the config path",
  );
  assert.ok(
    !reg.includes("vale-agent.exe' + '\"'"),
    "the exe path must never be the argument",
  );
  assert.match(reg, /-UserId SYSTEM/, "SYSTEM principal");
  assert.match(reg, /ExecutionTimeLimit.*0/, "never kill the running task");
  assert.match(
    reg,
    /Register-ScheduledTask ValeAgent/,
    "re-registers with -Force semantics",
  );
  assert.ok(!reg.includes("Start-ScheduledTask"), "no kick without start=true");
  const kick = bootTaskPs(
    "C:\\V\\vale-agent.exe",
    "C:\\V\\etc\\config.yaml",
    true,
  ).join("\n");
  assert.match(
    kick,
    /Start-ScheduledTask ValeAgent/,
    "setup kicks the task once",
  );
  for (const banned of ["/RU", "/RP", "/RI", "/TR"]) {
    assert.ok(
      !reg.includes(` ${banned}`),
      `${banned} must never appear (password prompt hangs headless setup)`,
    );
  }
  assert.ok(
    ![...reg].some((c) => c.charCodeAt(0) > 127),
    "ASCII-only (system-locale PS)",
  );
});

test("migrateLayoutPs: mirrors paths.rs pairs, never clobbers, kills boxed node first, marker-gated", () => {
  const { migrateLayoutPs } = require("../bin/vale.js");
  const body = migrateLayoutPs("D:\\Vale", "C:\\ProgramData\\Vale").join("\n");
  for (const pair of [
    ["D:\\Vale\\config.yaml", "D:\\Vale\\etc\\config.yaml"],
    ["D:\\Vale\\vale-agent.hostname", "D:\\Vale\\etc\\vale-agent.hostname"],
    ["D:\\Vale\\.vale-release", "D:\\Vale\\etc\\.vale-release"],
    ["D:\\Vale\\tools\\node", "D:\\Vale\\components\\node"],
    ["D:\\Vale\\playwright", "D:\\Vale\\components\\playwright"],
    [
      "D:\\Vale\\vale-desktop-electron",
      "D:\\Vale\\components\\vale-desktop-electron",
    ],
    ["D:\\Vale\\start-desktop.ps1", "D:\\Vale\\scripts\\start-desktop.ps1"],
    ["D:\\Vale\\installer.log", "C:\\ProgramData\\Vale\\logs\\installer.log"],
    ["D:\\Vale\\pwout", "C:\\ProgramData\\Vale\\pwout"],
  ]) {
    assert.ok(
      body.includes(pair[0]) && body.includes(pair[1]),
      `migration covers ${pair[0]} -> ${pair[1]}`,
    );
  }
  assert.match(
    body,
    /-not \(Test-Path/,
    "every move is guarded (never clobbers staged output)",
  );
  assert.match(
    body,
    /CommandLine -like '\*.*playwright\*/,
    "boxed node processes are stopped before the tree moves",
  );
  // Marker aging (ADR 0008): whole block skips when done-marker present,
  // and the marker is written only when NO old->new pair is still pending.
  assert.ok(
    body.includes("$valeMg = (-not (Test-Path 'D:\\Vale\\etc\\.layout-v2'))"),
    "marker short-circuits re-runs (single-line guard)",
  );
  // 24 move lines + the node-kill line + the marker write all carry the guard.
  assert.equal(
    body.split("if ($valeMg").length - 1,
    26,
    "every statement carries the marker guard",
  );
  assert.ok(
    body.includes("if ($valeMg -and (-not (") &&
      body.includes("(Test-Path 'D:\\Vale\\config.yaml')"),
    "marker write gated on pending pairs",
  );
  assert.ok(
    body.includes(
      `New-Item -ItemType File -Force -Path 'D:\\Vale\\etc\\.layout-v2'`,
    ),
    "marker file itself",
  );
  assert.ok(
    ![...body].some((c) => c.charCodeAt(0) > 127),
    "ASCII-only (system-locale PS)",
  );
});

test("rollbackVersionOk: plain dotted triples only (URL interpolation gate)", () => {
  const { rollbackVersionOk } = require("../bin/vale.js");
  assert.equal(rollbackVersionOk("1.2.307"), true);
  assert.equal(rollbackVersionOk("0.0.1"), true);
  assert.equal(rollbackVersionOk("1.2"), false, "two parts");
  assert.equal(rollbackVersionOk("1.2.3.4"), false, "four parts");
  assert.equal(rollbackVersionOk("1.2.307/../../evil"), false, "path escape");
  assert.equal(rollbackVersionOk("--clear"), false, "flag is not a version");
  assert.equal(rollbackVersionOk(""), false);
});

// SOLID Round-17 (contract completion): boxedVersions/writeBoxedVersions
// carry an "exported: unit-tested shape" comment but had ZERO pins — the
// boxed-component manifest (/api/status surfaces it) is the version-lock
// supervision for playwright + cloudflared. Staged temp dirs only; the
// cloudflared --version probe never fires here (no staged binary).
test("boxedVersions: empty trees → all unknown, ISO updated stamp", () => {
  const { boxedVersions } = require("../bin/vale.js");
  const fs = require("node:fs");
  const os = require("node:os");
  const path = require("node:path");
  const dir = fs.mkdtempSync(
    path.join(os.tmpdir(), `vale-boxed-empty-${process.pid}-`),
  );
  try {
    const m = boxedVersions(dir, dir);
    assert.ok(!Number.isNaN(Date.parse(m.updated)), "machine-readable stamp");
    assert.equal(m.playwright_mcp.version, "unknown");
    assert.equal(m.playwright_mcp.sha256, "unknown");
    assert.equal(m.playwright_core.version, "unknown");
    assert.equal(m.cloudflared.version, "unknown");
    assert.equal(m.cloudflared.sha256, "unknown");
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("boxedVersions: staged versions read, zip hashed exactly", () => {
  const { boxedVersions } = require("../bin/vale.js");
  const fs = require("node:fs");
  const os = require("node:os");
  const path = require("node:path");
  const crypto = require("node:crypto");
  const dir = fs.mkdtempSync(
    path.join(os.tmpdir(), `vale-boxed-full-${process.pid}-`),
  );
  try {
    const mcpPkg = path.join(
      dir,
      "components",
      "playwright",
      "node_modules",
      "@playwright",
      "mcp",
    );
    fs.mkdirSync(mcpPkg, { recursive: true });
    fs.writeFileSync(path.join(mcpPkg, "package.json"), '{"version":"9.9.9"}');
    const zipBytes = Buffer.from("fake-playwright-zip-bytes");
    fs.writeFileSync(path.join(dir, "vale-playwright.zip"), zipBytes);
    const m = boxedVersions(dir, dir);
    assert.equal(m.playwright_mcp.version, "9.9.9");
    assert.equal(
      m.playwright_mcp.sha256,
      crypto.createHash("sha256").update(zipBytes).digest("hex"),
    );
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("boxedVersions: >300MB blob → unknown sha without reading it", () => {
  const { boxedVersions } = require("../bin/vale.js");
  const fs = require("node:fs");
  const os = require("node:os");
  const path = require("node:path");
  const dir = fs.mkdtempSync(
    path.join(os.tmpdir(), `vale-boxed-big-${process.pid}-`),
  );
  try {
    // Sparse file: stat reports 301MB instantly, disk use stays ~nil.
    const big = path.join(dir, "vale-playwright.zip");
    fs.writeFileSync(big, "x");
    fs.truncateSync(big, 301 * 1024 * 1024);
    const t0 = Date.now();
    const m = boxedVersions(dir, dir);
    assert.equal(
      m.playwright_mcp.sha256,
      "unknown",
      "oversize guard, not a hash",
    );
    assert.ok(Date.now() - t0 < 5000, "must not read 301MB");
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("writeBoxedVersions: writes a parseable manifest; hostile dirs stay silent", () => {
  const { writeBoxedVersions } = require("../bin/vale.js");
  const fs = require("node:fs");
  const os = require("node:os");
  const path = require("node:path");
  const dir = fs.mkdtempSync(
    path.join(os.tmpdir(), `vale-boxed-write-${process.pid}-`),
  );
  try {
    writeBoxedVersions(dir, dir); // etc/ auto-created, never throws
    const back = JSON.parse(
      fs.readFileSync(path.join(dir, "etc", "boxed-versions.json"), "utf8"),
    );
    for (const k of [
      "updated",
      "playwright_mcp",
      "playwright_core",
      "cloudflared",
    ]) {
      assert.ok(back[k] !== undefined, `manifest carries ${k}`);
    }
    // A file where a directory is expected: best-effort skip, never throws.
    const file = path.join(dir, "blocker");
    fs.writeFileSync(file, "x");
    writeBoxedVersions(file, dir);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

// ── The update that left no trace ───────────────────────────────────────────
//
// INCIDENT: a device update was issued through the sanctioned flow and the
// connection dropped, which is the DOCUMENTED behaviour of a successful swap
// ("the terminal connection DROPS for ~10 s mid-update"). It was therefore read
// as "the update started". It had not: on the device there was no
// update-busy marker, no staged vale-agent.new.exe, no scripts\vale-update.ps1,
// and no `update start` line in vale-update.log — the command never reached the
// device at all, and the transport failure was indistinguishable from the
// success signal. Only the RELEASE MARKER, still reading the old version, told
// the truth, and the operator had to go and read four things by hand to learn it.
//
// `vale status` is the command a person runs to ask "where is this device". It
// reported RUNNING / install dir / panel URL and NOTHING about the release or a
// pending swap, so it could not answer the only question that mattered.
//
// These tests pin the report's CONTENT, because the failure mode is an
// omission: a `status` that prints three plausible lines while leaving out the
// release version looks perfectly healthy.
test("statusReport: an UNREADABLE process list is not 'STOPPED'", () => {
  const { statusReport } = require("../bin/vale.js");
  // `agentRunning: null` means the process list could not be READ. Reporting STOPPED
  // there is a claim of ABSENCE from a failed read — and for the agent itself that is
  // the worst possible answer ("your device is down" when nobody looked). Fourth
  // instance of this shape; the field follows the rule its neighbours state.
  const unknown = statusReport({
    agentRunning: null,
    installDir: "D:\\Vale",
    exeExists: true,
    port: 18080,
    releaseVersion: "1.2.359",
    updateMarkerMs: null,
    updateMarkerUnreadable: false,
    packageVersion: "1.2.359",
    latestVersion: "1.2.359",
    nowMs: 1_700_000_000_000,
  }).join("\n");
  assert.match(
    unknown,
    /UNKNOWN/,
    "an unreadable process list is reported as UNKNOWN",
  );
  assert.match(
    unknown,
    /not a verdict/,
    "and says so, rather than implying STOPPED",
  );
  assert.doesNotMatch(
    unknown,
    /status: STOPPED/,
    "must not claim the agent is stopped when the probe never answered",
  );

  // The two real answers must still be exact.
  const facts = {
    installDir: "D:\\Vale",
    exeExists: true,
    port: 18080,
    releaseVersion: "1.2.359",
    updateMarkerMs: null,
    updateMarkerUnreadable: false,
    packageVersion: "1.2.359",
    latestVersion: "1.2.359",
    nowMs: 1_700_000_000_000,
  };
  assert.match(
    statusReport({ ...facts, agentRunning: true }).join("\n"),
    /status: RUNNING/,
  );
  assert.match(
    statusReport({ ...facts, agentRunning: false }).join("\n"),
    /status: STOPPED/,
  );
});

test("statusReport: reports the running release and a FAILED update, not just RUNNING", () => {
  const { statusReport } = require("../bin/vale.js");
  const now = 1_700_000_000_000;

  // (a) An update was attempted and never finished: the marker survives with
  //     its original mtime. This is THE diagnostic the incident lacked.
  const stalled = statusReport({
    agentRunning: true,
    installDir: "D:\\Vale",
    exeExists: true,
    port: 18080,
    releaseVersion: "1.2.321",
    updateMarkerMs: now - 27 * 60_000,
    packageVersion: "1.2.322",
    nowMs: now,
  }).join("\n");
  assert.match(
    stalled,
    /1\.2\.321/,
    "must name the version the device is actually running",
  );
  assert.match(
    stalled,
    /did not finish|DID NOT FINISH/i,
    "a marker past the freshness window means an update STARTED AND DID NOT FINISH — status must say so, not stay silent",
  );
  // The DRIFT line specifically — the one that answers "am I up to date?".
  // Pinned separately from the bare version numbers above, because those also
  // match the `this CLI:` line and would keep passing if the drift line were
  // dropped in a refactor. That is the whole failure mode under test: a status
  // that looks informative while omitting the fact that matters.
  assert.match(
    stalled,
    /device runs 1\.2\.321.*1\.2\.322/,
    "must state the DRIFT (device runs X, this CLI is Y), not merely print both numbers somewhere",
  );

  // (b) Nothing in flight and the device matches this CLI: say so plainly.
  const current = statusReport({
    agentRunning: true,
    installDir: "D:\\Vale",
    exeExists: true,
    port: 18080,
    releaseVersion: "1.2.322",
    updateMarkerMs: null,
    packageVersion: "1.2.322",
    nowMs: now,
  }).join("\n");
  assert.match(current, /1\.2\.322/);
  assert.ok(
    !/DID NOT FINISH/i.test(current),
    "an absent marker must NOT be reported as a failed update — absent is not the same as broken",
  );

  // (c) An update IS in flight (fresh marker): distinct from both above.
  const inFlight = statusReport({
    agentRunning: true,
    installDir: "D:\\Vale",
    exeExists: true,
    port: 18080,
    releaseVersion: "1.2.321",
    updateMarkerMs: now - 30_000,
    packageVersion: "1.2.322",
    nowMs: now,
  }).join("\n");
  assert.match(
    inFlight,
    /in flight|IN FLIGHT/i,
    "a fresh marker means a swap is running right now",
  );

  // (d) An install with no release marker at all (pre-round-298 or a fresh box)
  //     says "unknown" rather than inventing a version.
  const unknown = statusReport({
    agentRunning: false,
    installDir: "D:\\Vale",
    exeExists: false,
    port: 18080,
    releaseVersion: null,
    updateMarkerMs: null,
    packageVersion: "1.2.322",
    nowMs: now,
  }).join("\n");
  assert.match(unknown, /STOPPED/);
  assert.match(
    unknown,
    /unknown/i,
    "no marker => unknown, never a fabricated version",
  );
});

// The receipt that makes "the command never ran" provable from the log alone.
test("updateReceiptPs: appends the INTENT before the handoff, to the same log the swap writes", () => {
  const { updateReceiptPs } = require("../bin/vale.js");
  const lines = updateReceiptPs("D:\\Vale", "1.2.321", "1.2.322");
  const body = lines.join("\n");
  assert.match(
    body,
    /vale-update\.log/,
    "same file the swap script appends to — one timeline",
  );
  assert.match(body, /1\.2\.321/, "records the version being replaced");
  assert.match(body, /1\.2\.322/, "records the version being installed");
  assert.match(
    body,
    /-Append/,
    "appends; must never truncate the swap's own log",
  );
  assert.match(
    body,
    /requested/i,
    "the word that distinguishes it from the swap's own 'update start'",
  );
  // The swap script's first line is `update start`. The receipt must be a
  // DIFFERENT marker, or the two become indistinguishable and the whole point
  // (did the CLI run? did the swap run?) is lost.
  assert.ok(
    !/update start/.test(body),
    "must not reuse the swap's 'update start' marker",
  );
});

// The receipt is only worth anything if it lands in the SAME file the swap
// appends to. A receipt written to a different path is worse than none: the
// log would look like the swap never started, which is the false conclusion
// this whole change exists to prevent. Pinned against the swap's own
// construction rather than against a literal, so moving one moves the test.
test("updateReceiptPs: the sink is byte-identical to the swap script's log sink", () => {
  const { updateReceiptPs, psq } = require("../bin/vale.js");
  const probe = [
    "D:\\Vale",
    "D:\\ProgramData\\Vale",
    "C:\\Program Files\\Vale",
  ];
  for (const dataDir of probe) {
    // Mirrors the `const log = ...` line in update(), verbatim.
    const swapLog = `Out-File '${dataDir.replace(/'/g, "''")}\\logs\\vale-update.log' -Append`;
    const receipt = updateReceiptPs(psq(dataDir), "1.0.0", "1.0.1")[0];
    const receiptLog = receipt.slice(receipt.indexOf("Out-File"));
    assert.equal(receiptLog, swapLog, `sinks must agree for ${dataDir}`);
  }
  // A quote in the data dir must be doubled, exactly as the swap does it —
  // otherwise the single-quoted PS literal breaks and the receipt never lands.
  const quoted = updateReceiptPs(psq("D:\\it's\\Vale"), "1.0.0", "1.0.1")[0];
  assert.match(
    quoted,
    /it''s/,
    "embedded quote doubled for the PS single-quoted literal",
  );
});

// ── A version marker must be EARNED, not asserted ───────────────────────────
//
// `etc\.vale-release` is the device's ONLY local version truth: agent_update
// reads it as `local` and answers up_to_date when the remote is not newer, and
// /api/status serves it as `release` — the field the panel, the tray and the
// console fleet card all display.
//
// `vale rollback` wrote it UNCONDITIONALLY after `vale update` returned status 0.
// But status 0 means the WMI handoff was ACCEPTED — a process was created — not
// that the swap succeeded; everything that decides success (the fail-closed
// migration gate, the copy retry, the $ok-gated marker write, the task restart)
// happens afterwards inside a process nobody reads. So a rollback whose swap
// died left a marker claiming a version the device is NOT running: every UI
// lies, and once the pin is cleared agent_update sees the fake version, decides
// it is current, and the device is stuck on the old release permanently.
//
// The swap script itself already gates the same write on a provable copy
// (`if ($ok -and ...)`). The CLI did not. These tests pin the read-back.
test("awaitReleaseMarker: only a MARKER THAT SHOWS THE TARGET counts as success", async () => {
  const { awaitReleaseMarker } = require("../bin/vale.js");
  const noSleep = async () => {};

  // The swap wrote the target version: success, immediately.
  let r = await awaitReleaseMarker({
    want: "1.2.322",
    timeoutMs: 5000,
    intervalMs: 10,
    read: () => "1.2.322",
    sleep: noSleep,
    now: () => 0,
  });
  assert.equal(r.ok, true, "marker already at target => success");
  assert.equal(r.saw, "1.2.322");

  // The marker never moves off the OLD version: the swap failed. This is the
  // incident shape — and the caller must NOT write the pin or claim the version.
  let t = 0;
  r = await awaitReleaseMarker({
    want: "1.2.322",
    timeoutMs: 100,
    intervalMs: 10,
    read: () => "1.2.321",
    sleep: noSleep,
    now: () => (t += 50),
  });
  assert.equal(
    r.ok,
    false,
    "a marker that never reaches the target is NOT success",
  );
  assert.equal(
    r.saw,
    "1.2.321",
    "reports what it actually saw, for the message",
  );

  // The marker arrives late but within the budget: still success. The swap kills
  // the agent and restarts the task, so a delay is normal, not a failure.
  let n = 0;
  t = 0;
  r = await awaitReleaseMarker({
    want: "1.2.322",
    timeoutMs: 5000,
    intervalMs: 10,
    read: () => (++n < 3 ? "1.2.321" : "1.2.322"),
    sleep: noSleep,
    now: () => (t += 50),
  });
  assert.equal(
    r.ok,
    true,
    "a marker that arrives within the budget is success",
  );

  // Unreadable marker (absent, or a torn write) is NOT success and NOT a crash.
  t = 0;
  r = await awaitReleaseMarker({
    want: "1.2.322",
    timeoutMs: 100,
    intervalMs: 10,
    read: () => {
      throw new Error("ENOENT");
    },
    sleep: noSleep,
    now: () => (t += 50),
  });
  assert.equal(
    r.ok,
    false,
    "a missing marker is a failure, never a silent pass",
  );
  assert.equal(
    r.saw,
    null,
    "absence is reported as null, not as a fabricated version",
  );

  // Whitespace/newline around the marker is not a mismatch (Set-Content -NoNewline
  // is used, but a hand-edited or pre-v2 marker may carry a trailing newline).
  r = await awaitReleaseMarker({
    want: "1.2.322",
    timeoutMs: 100,
    intervalMs: 10,
    read: () => "1.2.322\r\n",
    sleep: noSleep,
    now: () => 0,
  });
  assert.equal(r.ok, true, "the marker is compared trimmed");
});

test("releaseMarkerVerdict: the pin is written ONLY on a proven swap", () => {
  const { releaseMarkerVerdict } = require("../bin/vale.js");
  // Proven: write the pin, say so.
  let v = releaseMarkerVerdict({ want: "1.2.322", saw: "1.2.322", ok: true });
  assert.equal(v.writePin, true);
  assert.equal(v.exitCode, 0);
  // Unproven: do NOT write the pin, do NOT report success, and say what is real.
  v = releaseMarkerVerdict({
    want: "1.2.322",
    saw: "1.2.321",
    ok: false,
    waitedMs: 90_000,
  });
  assert.equal(
    v.writePin,
    false,
    "an unproven swap must not pin the device to a version it is not running",
  );
  assert.equal(
    v.exitCode,
    1,
    "the caller must see a non-zero exit so scripts can react",
  );
  assert.match(
    v.message,
    /1\.2\.321/,
    "names the version the device is ACTUALLY on",
  );
  assert.match(
    v.message,
    /NOT pinned|not pinned/i,
    "states plainly that the pin was not written",
  );
  // THE BOUND, NOT A CONCLUSION. `ok: false` is a TIMEOUT: a slow-but-successful swap
  // looks the same as a failed one, so the message must say what was observed and over
  // how long — it used to assert "the swap did NOT take" for every case, including the
  // one where the marker could not be read at all (`saw === null`).
  assert.match(
    v.message,
    /within 90s/,
    "names the read-back window it actually waited",
  );
  assert.doesNotMatch(
    v.message,
    /did NOT take/i,
    "must not assert a conclusion a timeout cannot support",
  );

  // `saw === null` is the weakest evidence of the three and must not read as a verdict.
  const blind = releaseMarkerVerdict({
    want: "1.2.322",
    saw: null,
    ok: false,
    waitedMs: 90_000,
  });
  assert.match(
    blind.message,
    /empty or unreadable/,
    "an unreadable marker is named as such",
  );
  assert.doesNotMatch(
    blind.message,
    /device is on/,
    "and must not claim a version was seen",
  );
});

// The ORIGINAL bug was a WIRING bug: `rollback()` wrote the marker
// unconditionally at its call site. Every pure helper can be perfect while the
// caller ignores it — and mutation testing proved exactly that, because
// restoring the original bug (`if (false)`) left the whole suite GREEN. The
// call site does real I/O (spawnSync, process.exit) and cannot be driven from
// `node --test`, so it is pinned STRUCTURALLY, the same way this repo pins
// `module_map` and the run-id credential rule.
//
// LIMIT, stated rather than implied: this is a source scan. It asserts the
// gate exists and that the CLI never writes the release marker itself; it does
// not execute the branch.
test("rollback: the pin is gated on the verdict, and the CLI never writes the release marker", () => {
  const fs = require("node:fs");
  const path = require("node:path");
  const { fileURLToPath } = require("node:url");
  const here = path.dirname(fileURLToPath(import.meta.url));
  const src = fs.readFileSync(path.join(here, "..", "bin", "vale.js"), "utf8");

  // (a) The marker is written by the SWAP SCRIPT (gated on $ok), never by the
  //     CLI. A CLI write is the regression: it erases the swap's proof.
  assert.ok(
    !/writeFileSync\(path\.join\(ETC_DIR,\s*"\.vale-release"\)/.test(src),
    "the CLI must NOT write etc\\.vale-release — only the swap script may, and only from a provable copy",
  );

  // (b) The rollback body consults the read-back verdict before pinning.
  const start = src.indexOf("async rollback(args)");
  assert.ok(start > 0, "rollback found in the compiled CLI");
  const body = src.slice(start, src.indexOf("\n    },", start));
  assert.match(
    body,
    /awaitReleaseMarker\(/,
    "rollback must READ THE MARKER BACK rather than trust the handoff",
  );
  assert.match(
    body,
    /releaseMarkerVerdict\(/,
    "rollback must derive its outcome from the verdict",
  );
  assert.match(
    body,
    /if \(!verdict\.writePin\)/,
    "the pin write must be GATED on the verdict",
  );
  assert.match(
    body,
    /process\.exit\(verdict\.exitCode\)/,
    "an unproven swap must exit non-zero so scripts can react",
  );

  // (c) A throw while staging must release the in-progress marker, or the next
  //     update refuses for ten minutes citing an update that never started.
  const stage = src.slice(
    src.indexOf("const BUSYM = updateBusyPath()"),
    src.indexOf("Invoke-CimMethod"),
  );
  assert.match(stage, /catch \(e\)/, "the staging region is guarded");
  assert.match(
    stage,
    /unlinkSync\(BUSYM\)/,
    "a staging failure releases the in-progress marker",
  );
});

// ── No shell may sit between the CLI and PowerShell ─────────────────────────
//
// INCIDENT (found on d1 during the 1.2.323 update, by reading the log the
// receipt itself wrote): the receipt came out as
//   "update requested 1.2.322 - (CLI reached the device...)"
// — the arrow and the TARGET VERSION were gone — and a stray ZERO-BYTE FILE
// named `1.2.323` appeared in the working directory.
//
// Cause: `ps()` built a command STRING and ran it with `shell: true`, so cmd.exe
// re-parsed it before PowerShell saw it. cmd has no `\"` escape — a quote is a
// TOGGLE — so the string-ended-quoted region early and the `>` in `1.2.322 ->
// 1.2.323` became a REDIRECTION OPERATOR. The target version was written to a
// file name instead of the log.
//
// The unit test I wrote for the receipt could not see this: it asserted the
// string `updateReceiptPs` GENERATES, and that string was correct. What was
// wrong was what ARRIVED. This is the "generate vs land" gap, and the fix is
// structural — pass argv, never a command line.
test("psArgv: the script is ONE argv element, so no shell can re-parse it", () => {
  const { psArgv } = require("../bin/vale.js");
  const script =
    `"[$(Get-Date -Format o)] update requested 1.2.322 -> 1.2.323 " | Out-File 'D:\\Vale\\logs\\vale-update.log' -Append; ` +
    `Write-Output "a<b & c|d ^ e%f"`;
  const argv = psArgv(script);

  assert.deepEqual(argv.slice(0, 2), ["-NoProfile", "-Command"]);
  assert.equal(argv.length, 3, "the whole script is exactly one argument");
  assert.equal(
    argv[2],
    script,
    "and it is passed VERBATIM — no quoting, no escaping of any kind",
  );

  // The characters cmd.exe treats as operators must survive untouched. Each of
  // these broke, or would have broken, the shell form.
  for (const ch of [">", "<", "|", "&", "^", "%", '"']) {
    assert.ok(argv[2].includes(ch), `script still carries ${ch}`);
  }
  assert.ok(
    !argv.some((a) => a.includes('\\"')),
    "no cmd-style quote escaping may appear anywhere — that escaping is what made `>` an operator",
  );
});

test("psArgv: every ps() script is passed as argv, and ps() never shells out", () => {
  const fs = require("node:fs");
  const path = require("node:path");
  const { fileURLToPath } = require("node:url");
  const here = path.dirname(fileURLToPath(import.meta.url));
  const src = fs.readFileSync(path.join(here, "..", "bin", "vale.js"), "utf8");

  // Structural, because `ps()` needs a real Windows PowerShell to execute. The
  // regression is a one-line revert to the string form, so the scan is the
  // right pin — same approach as the rollback call-site pin.
  const m = /function ps\(script\) \{([\s\S]*?)\n\}/.exec(src);
  assert.ok(m, "ps() found in the compiled CLI");
  assert.match(
    m[1],
    /spawnSync\)\("powershell", psArgv\(script\)/,
    "ps() spawns powershell with argv (the compiled form is `(0, child_process_1.spawnSync)(...)`)",
  );
  assert.ok(
    !/sh\(`powershell/.test(m[1]),
    "ps() must NOT build a command string — that is the defect: cmd.exe re-parses it and `>` becomes a redirection",
  );
  assert.ok(!/shell:\s*true/.test(m[1]), "ps() must not use a shell");
});

// THE INGRESS ADDRESS AND THE LISTEN ADDRESS ARE CHOSEN IN TWO LANGUAGES, SO
// NOTHING IN EITHER ONE CAN SEE THE OTHER. They disagreed: this CLI wrote
// `http://127.0.0.2:<port>` into etc\tunnel.yml while the agent's own
// provisioning (`agent/src/tunnel.rs`) writes 127.0.0.1, keeps a helper whose
// comment says it exists to "reach the agent where it actually listens", and
// calls 127.0.0.2 "a dead address (502)". One file, two writers, two answers —
// and the LIVE DEVICE settles which is right: `netstat` on d1 shows the listener
// on 127.0.0.1:18080, and d1's own tunnel.yml says `service: http://127.0.0.1:18080`.
//
// This test is the pin across the language boundary — the same shape as the
// gateway's code-viewer mirror check, and for the same reason: a test on one copy
// can only ever compare copies.
test("the tunnel ingress names the address the agent actually listens on", () => {
  const src = fs.readFileSync(
    new URL("../src/vale.ts", import.meta.url),
    "utf8",
  );
  const built = fs.readFileSync(
    new URL("../bin/vale.js", import.meta.url),
    "utf8",
  );
  for (const [name, text] of [
    ["src", src],
    ["bin", built],
  ]) {
    assert.ok(
      /service: http:\/\/127\.0\.0\.1:/.test(text),
      `${name}: the tunnel ingress must be 127.0.0.1 — that is where d1's agent ` +
        `listens (netstat: 127.0.0.1:18080) and what the agent's own writer puts ` +
        `in the same file. 127.0.0.2 is a socket nobody holds.`,
    );
    assert.ok(
      !/service: http:\/\/127\.0\.0\.2:/.test(text),
      `${name}: 127.0.0.2 is back — the agent calls it a dead address (502)`,
    );
    assert.ok(
      /allow-remote-config: false/.test(text),
      `${name}: the writer must keep allow-remote-config: false. cloudflared ` +
        `prefers a REMOTE config when one exists, so dropping this re-enables a ` +
        `stale remote ingress pointing at a dead address "no matter what ` +
        `tunnel.yml says" (tunnel.rs). The agent writes it; this CLI did not.`,
    );
  }
});

test("the agent's own default host agrees with the ingress", () => {
  // The third spelling. `ServerConfig::default()` said 127.0.0.2 while the
  // shipped config.yaml, the agent's tunnel writer and the live device all say
  // 127.0.0.1 — a default that disagreed with the file it exists to replace.
  const core = fs.readFileSync(
    new URL("../../vale-command-core/src/config.rs", import.meta.url),
    "utf8",
  );
  assert.ok(
    /host:\s*"127\.0\.0\.1"\.into\(\)/.test(core),
    "ServerConfig::default must bind the same address the tunnel ingress names",
  );
  assert.ok(
    !/host:\s*"127\.0\.0\.2"\.into\(\)/.test(core),
    "the 127.0.0.2 default is back — it disagrees with config.yaml, with " +
      "tunnel.rs's ingress and with the live device",
  );
});

// ONE UPDATE LOCK, ONE STALENESS WINDOW — PINNED ACROSS THE LANGUAGE BOUNDARY.
//
// The marker PATH agreed between the two sides; the WINDOW did not. The CLI
// reclaimed an abandoned marker after ten minutes while the agent refused for an
// hour, so at eleven minutes the CLI OVERWROTE a marker the agent still honoured
// — and a CLI update could then run alongside a console-launched one, which is
// the interleaved `Copy-Item` on `*.new` (a half-written exe reported "ok") that
// the marker exists to prevent. The operator docs stated the ten-minute rule
// only, so the hour was invisible to whoever read them.
//
// Neither language can see the other's number, which is why this test reads
// both files.
test("the update staleness window is the same on both sides of the lock", () => {
  const ts = fs.readFileSync(
    new URL("../src/vale.ts", import.meta.url),
    "utf8",
  );
  const cliMs = /return nowMs - mtimeMs < (\d+) \* 60 \* 1000;/.exec(ts);
  assert.ok(
    cliMs,
    "busyIsFresh's window must stay a literal this pin can read",
  );

  const rust = fs.readFileSync(
    new URL("../../src/plugins/update/tools.rs", import.meta.url),
    "utf8",
  );
  const rustSecs = /const BUSY_STALE_SECS: u64 = (\d+);/.exec(rust);
  assert.ok(rustSecs, "BUSY_STALE_SECS must stay a literal this pin can read");

  assert.equal(
    Number(cliMs[1]) * 60,
    Number(rustSecs[1]),
    `the two sides disagree about when an abandoned update marker may be ` +
      `reclaimed: the CLI says ${cliMs[1]} minutes, the agent says ` +
      `${rustSecs[1]} seconds. One lock with two rules means whichever is ` +
      `shorter steals the marker from the longer one — and the two updates ` +
      `interleave Copy-Item on *.new.`,
  );
});

// THE DELIVERY GAP — the single most repeated finding in this project's log.
// Every round records a device found many releases behind the CDN (five, six,
// once three in one round) and every time the ONLY thing that noticed was a
// human looking. `status` answered "what is this device running" and never "is
// that current", and those two questions are answered by different machines.
const { statusReport, behindBy } = require("../bin/vale.js");
const DRIFT_BASE = {
  agentRunning: true,
  installDir: "D:\\Vale",
  exeExists: true,
  port: 18080,
  releaseVersion: "1.2.340",
  updateMarkerMs: null,
  packageVersion: "1.2.340",
  nowMs: 1_700_000_000_000,
};

test("delivery drift: NAMES the gap when the device is behind the CDN", () => {
  const out = statusReport({ ...DRIFT_BASE, latestVersion: "1.2.345" }).join(
    "\n",
  );
  assert.match(out, /THIS DEVICE IS BEHIND by 5 releases/);
  assert.ok(out.includes("1.2.345"), out);
});

test("delivery drift: says CURRENT only when it actually compared", () => {
  const out = statusReport({ ...DRIFT_BASE, latestVersion: "1.2.340" }).join(
    "\n",
  );
  assert.match(out, /this device is current/);
  assert.ok(!/BEHIND/.test(out), out);
});

test("delivery drift: an unreadable CDN is NOT agreement", () => {
  // The failure mode this whole log is about: silence that reads like "fine".
  // `status` runs when something is already wrong, so the one thing it must not
  // do is imply the device is current because the check failed.
  const out = statusReport({ ...DRIFT_BASE, latestVersion: null }).join("\n");
  assert.match(out, /could NOT be checked/);
  assert.match(out, /says nothing about whether the device is current/);
  assert.ok(!/is current\)/.test(out), out);
});

test("delivery drift: counts patches within a minor, refuses a count across one", () => {
  assert.equal(behindBy("1.2.9", "1.2.12"), "3 releases");
  assert.equal(behindBy("1.2.9", "1.2.10"), "1 release");
  // A cross-minor jump is a different operation (the CDN prunes last-5-per-minor
  // and `vale rollback` refuses it), so it is not "N releases".
  assert.equal(behindBy("1.1.9", "1.2.0"), "a release line, not a patch count");
  assert.equal(behindBy("garbage", "1.2.0"), "an unknown number of releases");
});

// ── the contract list's ROW 1, which had no instrument ──────────────────────────
//
// `docs/agents/iteration-loop.md` carries a four-row contract list headed "check before
// touching a promise". Its FIRST row is the one with the widest blast radius:
//
//   | published tgz + devices in the field | CLI verbs/args, install layout, boot-task
//     arguments | rename or remove a verb/arg | dual-accept + rollback point + device
//     regression |
//
// Round 253 measured how that promise is kept: **this file's 36 tests cover HELPERS and
// PowerShell generators** (`busyIsFresh`, `writeReleaseMarker`, `uninstallVersionPs`,
// `autostartArgv`, `rollbackVersionOk`, `psq`, `parseAgentPort` …) **and ZERO of them touch
// the verb dispatch.** So nothing anywhere would fail if a verb were renamed — which is
// the exact break the contract list names.
//
// SAFETY, learned the hard way in round 246: this assertion runs the CLI **with NO
// ARGUMENTS ONLY**. `vale` with no args prints its verb list and exits 0 — measured. Every
// documented verb (`setup`, `update`, `uninstall`, `rollback`) MUTATES the machine, so an
// assertion that "checked" a verb by invoking it would install, swap or delete something.
// When the direct verification is destructive, find the part of it that is a measurement.
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { join, dirname } from "node:path";

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = join(HERE, "..", "..", "..");

test("every CLI verb the root guide promises is one the CLI prints", () => {
  // The verbs the guide advertises, read as data.
  const guide = readFileSync(join(REPO, "AGENTS.md"), "utf8");
  const promised = [
    ...new Set([...guide.matchAll(/^vale ([a-z]+)/gm)].map((m) => m[1])),
  ].sort();
  // The guide is no longer a verb catalogue (it names what an operator needs and nothing
  // more), so the bar is "the verbs it DOES name are real" — the CLI's own usage line below
  // is the contract, and it is checked in full.
  assert.ok(
    promised.length >= 2,
    `expected the root guide to advertise several CLI verbs, found ${promised.length} — if the ` +
      `mention syntax changed, fix THIS extractor rather than deleting the test (ADR 0011's ` +
      `deletion criterion)`,
  );

  // NO ARGUMENTS. That prints the verb list and changes nothing.
  const out = execFileSync(process.execPath, [join(HERE, "..", "bin", "vale.js")], {
    encoding: "utf8",
    timeout: 30_000,
  });

  const missing = promised.filter((v) => !new RegExp(`(^|[^a-z-])${v}([^a-z-]|$)`, "m").test(out));
  assert.deepEqual(
    missing,
    [],
    `these verbs are promised by the root guide but the CLI does not print them: ${missing.join(", ")}. ` +
      `The contract list in docs/agents/iteration-loop.md names this as a promise to published tgz ` +
      `holders and "devices in the field", with the break being "rename or remove a verb/arg" — so a ` +
      `verb disappearing is a device-breaking change that needs dual-accept, a rollback point and a ` +
      `device regression, not just a doc edit. CLI printed: ${out.split("\n")[0]}`,
  );
});


test("parseWatchArgs: --once in any position, and one target at most", () => {
  assert.deepEqual(parseWatchArgs([]), { once: false, only: null });
  assert.deepEqual(parseWatchArgs(["--once"]), { once: true, only: null });
  const withTarget = parseWatchArgs(["192.168.1.1:80/", "--once"]);
  assert.equal(withTarget.once, true);
  assert.equal(withTarget.only.id, "192.168.1.1:80/");
  // A typo in the flag position is still a typo, not a target.
  assert.ok(parseWatchArgs(["not-a-target"]).error);
  assert.ok(parseWatchArgs(["a:22", "b:22"]).error);
});

test("targetLine: one drop is 'drop', several are 'drops'", () => {
  const now = 1_789_000_000_000;
  const at = (drops) => targetLine({ id: "a:22", summary: { up_now: true, since_ms: now - 1000, drops } }, now);
  assert.match(at(1), /1 drop(?!s)/);
  assert.match(at(2), /2 drops/);
  assert.doesNotMatch(at(0), /drop/);
});


// ── vale report: the block an operator pastes ────────────────────────────────
test("reportText: assembles status, sessions, restarts and watched targets", () => {
  // LOCAL time on purpose (the reader stands next to the device). Dates are built locally
  // here too: a Date.UTC fixture made this test timezone-dependent.
  const now = new Date(2026, 8, 14, 23, 20, 5).getTime();
  const lines = reportText({
    // The REAL /api/status shape: `version` is the crate (1.0.x) and `release` the npm
    // release. A fixture with only `version` is how the report shipped claiming a drift of
    // a hundred versions on a device that was current.
    status: {
      ok: true,
      version: "1.0.145",
      release: "1.2.384",
      uptime_secs: 11_520,
      last_boot: "run journal: previous run DID NOT EXIT CLEANLY — REPLACED by a restart (an update swap or a task restart killed it mid-flight); started 2062s ago, last heartbeat 21s before this start, survived 2041s",
      last_boot_kind: "replaced",
      cpu_pct: 12,
      mem_pct: 61,
      live_sessions: 2,
      pending_approvals: 1,
    },
    monitors: {
      interval_secs: 15,
      targets: [
        { id: "192.168.1.1:22", summary: { up_now: true, since_ms: now - 60_000, latency: { avg: 9 }, up_pct: 100, drops: 0 } },
        {
          id: "192.168.1.1:80/",
          summary: { up_now: false, since_ms: now - 360_000, latency: null, up_pct: 50, drops: 2, last_status: 500 },
          transitions: [{ at_ms: now - 360_000, up: false, lasted_ms: 17_000 }],
        },
      ],
    },
    // The LIVE list (terminal_list), which is the only surface that knows what is running —
    // and `live_sessions` from /api/status, which must agree with it. The audit trail was the
    // wrong source: it keeps sessions killed by an agent restart marked `opened` (38 of them on
    // d1 for a device with one), which the first fix counted.
    sessions: {
      sessions: [
        { id: "term-1", kind: "pty" },
        { id: "term-2", kind: "ssh" },
      ],
    },
    boots: { summary: { window_secs: 86_400, boots: 3, crashes: 1 } },
    nowMs: now,
    cliVersion: "1.2.384",
    hostLabel: "d1",
  });
  const text = lines.join("\n");
  assert.match(text, /Vale report — d1 — 2026-09-14/);
  assert.match(text, /release 1\.2\.384, CLI 1\.2\.384, agent up 3h 12m/);
  assert.doesNotMatch(text, /DRIFT/); // same version: no drift claimed
  // The previous boot is the KIND plus the first clause — not the whole run-journal sentence.
  assert.match(text, /previous boot: replaced — previous run DID NOT EXIT CLEANLY/);
  assert.doesNotMatch(text, /survived 2041s/);
  assert.match(text, /CPU 12%, mem 61%, 2 session\(s\), 1 awaiting approval/);
  assert.match(text, /sessions: 1 pty, 1 ssh/);
  assert.doesNotMatch(text, /kind unknown/);
  // A LIVE LIST that disagrees with /api/status says so instead of picking a winner.
  const disagreeing = reportText({
    status: { ok: true, release: "1.2.385", uptime_secs: 60, live_sessions: 3 },
    monitors: null,
    sessions: { sessions: [{ id: "a", kind: "pty" }] },
    boots: null,
    nowMs: now,
    cliVersion: "1.2.385",
    hostLabel: "d1",
  }).join("\n");
  assert.match(disagreeing, /sessions: 1 pty \(status says 3\)/);
  // …and with no live list at all, the count it does have, with no invented breakdown.
  const countOnly = reportText({
    status: { ok: true, release: "1.2.385", uptime_secs: 60, live_sessions: 2 },
    monitors: null, sessions: null, boots: null,
    nowMs: now, cliVersion: "1.2.385", hostLabel: "d1",
  }).join("\n");
  assert.match(countOnly, /sessions: 2 \(kinds not read\)/);
  assert.match(text, /restarts in the last 24h: 3 \(1 crash-like\)/);
  assert.match(text, /watching 2 target\(s\), probed every 15s — 1 down:/);
  assert.match(text, /DOWN 192\.168\.1\.1:80\/\s+down 6m\s+HTTP 500/);
  // The DOWN target brings its outage log; the UP one does not (noise).
  assert.match(text, /went down\s+\(previous state lasted 17s\)/);
  assert.equal((text.match(/previous state lasted/g) || []).length, 1);
});
test("reportText: a route that did not answer reads as NOT READ, never as a value", () => {
  const text = reportText({
    status: { __error: "device unreachable on 127.0.0.1:18080" },
    monitors: null,
    sessions: null,
    boots: null,
    nowMs: new Date(2026, 8, 14, 23, 20, 5).getTime(),
    cliVersion: "1.2.383",
    hostLabel: "d1",
  }).join("\n");
  assert.match(text, /agent status: NOT READ/);
  assert.match(text, /watched targets: NOT READ/);
  // No invented numbers for what was not read.
  assert.doesNotMatch(text, /release 1\.|CPU|mem /);
  // A device that reported no npm release says so, and does not dress the crate version up as one.
  const noRelease = reportText({
    status: { ok: true, version: "1.0.145", uptime_secs: 60 },
    monitors: null, sessions: null, boots: null,
    nowMs: new Date(2026, 8, 14, 23, 20, 5).getTime(), cliVersion: "1.2.384", hostLabel: "d1",
  }).join("\n");
  assert.match(noRelease, /release NOT REPORTED \(crate 1\.0\.145\)/);
  assert.doesNotMatch(noRelease, /DRIFT/);
});

test("reportText: a version drift is named, and an empty watch list says how to start one", () => {
  const text = reportText({
    status: { ok: true, version: "1.0.145", release: "1.2.400", uptime_secs: 60 },
    monitors: { interval_secs: 15, targets: [] },
    sessions: null,
    boots: null,
    nowMs: new Date(2026, 8, 14, 23, 20, 5).getTime(),
    cliVersion: "1.2.383",
    hostLabel: "d1",
  }).join("\n");
  assert.match(text, /CLI 1\.2\.383 \(DRIFT\)/);
  assert.match(text, /watching nothing \(vale monitor add <host:port>\)/);
});


test("targetLine: a content check that failed says so, next to the code that looked fine", () => {
  const now = 1_789_000_000_000;
  const line = targetLine(
    { id: "h:80/", summary: { up_now: false, since_ms: now - 1000, last_status: 200, last_expect_ok: false } },
    now,
  );
  assert.match(line, /HTTP 200/);
  assert.match(line, /no match/); // 200 AND wrong: the status alone would have called this healthy
  const ok = targetLine(
    { id: "h:80/", summary: { up_now: true, since_ms: now - 1000, last_status: 200, last_expect_ok: true } },
    now,
  );
  assert.match(ok, /matches/);
  // No content check: neither word appears.
  const plain = targetLine({ id: "h:80/", summary: { up_now: true, since_ms: now - 1000, last_status: 200, last_expect_ok: null } }, now);
  assert.doesNotMatch(plain, /match/);
});


test("probeLine: a DOWN probe says WHICH way it failed", () => {
  const now = 1_789_000_000_000;
  // Up, plain TCP: state and latency, no invented status.
  assert.equal(
    probeLine({ id: "h:22" }, { ok: true, status: null, ms: 9, expect_ok: null }, now),
    "UP   h:22  9ms",
  );
  // Up with a status: the number an operator asked for.
  assert.match(probeLine({ id: "h:80/" }, { ok: true, status: 200, ms: 11, expect_ok: null }, now), /HTTP 200/);
  // 500: down, with the code — the case a TCP connect cannot see.
  const bad = probeLine({ id: "h:80/" }, { ok: false, status: 500, ms: 12, expect_ok: null }, now);
  assert.match(bad, /^DOWN/);
  assert.match(bad, /HTTP 500/);
  // 200 with the wrong body: down, and the line names the text it wanted.
  const miss = probeLine(
    { id: "h:80/", expect: "OpenWrt" },
    { ok: false, status: 200, ms: 11, expect_ok: false },
    now,
  );
  assert.match(miss, /^DOWN/);
  assert.match(miss, /HTTP 200/);
  assert.match(miss, /no match for "OpenWrt"/);
  // A match is stated too, so a healthy content check is visible rather than merely absent.
  assert.match(
    probeLine({ id: "h:80/", expect: "OpenWrt" }, { ok: true, status: 200, ms: 5, expect_ok: true }, now),
    /matches "OpenWrt"/,
  );
  // No answer at all: no status, no latency, and the line says so.
  assert.equal(probeLine({ id: "h:9" }, { ok: false, status: null, ms: null, expect_ok: null }, now), "DOWN h:9  no answer");
});


// ── what the console said when it happened (the join) ───────────────────────
test("stripAnsi/lastLine: a report line, not a transcript", () => {
  // OSC (the window-title escape npm emits) and CSI colour both go; the text stays.
  assert.equal(stripAnsi("\u001b]0;npm i something\u0007done"), "done");
  assert.equal(stripAnsi("\u001b[31mred\u001b[0m"), "red");
  assert.equal(lastLine("one\ntwo\r\n\n   \nlast line here\n"), "last line here");
  // A trailing blank line is not "the last line": an operator wants the content.
  assert.equal(lastLine("content\n\n\n"), "content");
  assert.equal(lastLine(""), "");
  assert.equal(lastLine("\u001b[2J\u001b[H"), "");
  // Long lines are clipped, because this lands inside a report line.
  const long = lastLine("x".repeat(300));
  assert.equal(long.length, 120);
  assert.match(long, /\.\.\.$/);
});

test("lastOutputBefore: the newest output AT OR BEFORE the moment, never after", () => {
  const recs = [
    { kind: "command/start", ts_ms: 1000, command: "x" },
    { kind: "output", ts_ms: 1500, text: "first" },
    { kind: "output", ts_ms: 2500, text: "second" },
    { kind: "output", ts_ms: 9000, text: "AFTER the moment" },
    { kind: "output", ts_ms: 500, text: "" }, // no text: cannot be a console line
    { kind: "output" }, // no stamp: dropped rather than placed by guess
  ];
  assert.equal(lastOutputBefore(recs, 3000).text, "second");
  assert.equal(lastOutputBefore(recs, 1500).text, "first");
  assert.equal(lastOutputBefore(recs, 10), null, "nothing before the moment is null, not the first");
  assert.equal(lastOutputBefore([], 1000), null);
});

test("reportText: a DOWN target carries the console line from just before it dropped", () => {
  const now = new Date(2026, 8, 14, 23, 20, 5).getTime();
  const downAt = now - 60_000;
  const text = reportText({
    status: { ok: true, release: "1.2.389", uptime_secs: 60 },
    monitors: {
      interval_secs: 15,
      targets: [
        {
          id: "127.0.0.1:45993",
          summary: { up_now: false, since_ms: downAt, drops: 1, up_pct: 50, latency: null, last_status: null },
          transitions: [{ at_ms: downAt, up: false, lasted_ms: 39_000 }],
        },
      ],
    },
    sessions: null,
    boots: null,
    nowMs: now,
    cliVersion: "1.2.389",
    hostLabel: "d1",
    consoleNear: { "127.0.0.1:45993": { sid: "term-98cf04-0", tsMs: downAt - 1500, line: "ifconfig br-lan down" } },
  }).join("\n");
  assert.match(text, /went down\s+\(previous state lasted 39s\)/);
  assert.match(text, /console \d\d:\d\d:\d\d \(term-98cf04-0\): ifconfig br-lan down/);

  // No join found: the report says nothing about the console rather than guessing at it.
  const bare = reportText({
    status: { ok: true, release: "1.2.389", uptime_secs: 60 },
    monitors: {
      interval_secs: 15,
      targets: [
        {
          id: "127.0.0.1:45993",
          summary: { up_now: false, since_ms: downAt, drops: 1, up_pct: 50, latency: null, last_status: null },
          transitions: [{ at_ms: downAt, up: false, lasted_ms: 39_000 }],
        },
      ],
    },
    sessions: null, boots: null, nowMs: now, cliVersion: "1.2.389", hostLabel: "d1",
  }).join("\n");
  assert.doesNotMatch(bare, /console /);
});


// ── machine-readable output ─────────────────────────────────────────────────
test("monitorsJson: the device's numbers verbatim, with only what the CLI knows added", () => {
  const payload = {
    ok: true,
    interval_secs: 15,
    targets: [
      {
        id: "192.168.1.1:22",
        host: "192.168.1.1",
        port: 22,
        path: null,
        expect: null,
        summary: { probes: 12, up: 12, down: 0, up_pct: 100, up_now: true, since_ms: 111, drops: 0, latency: { min: 7, avg: 9, max: 16 }, last_status: null, last_expect_ok: null },
        transitions: [{ at_ms: 100, up: true, lasted_ms: 39_000 }],
        series: [{ ts_ms: 1, ok: true, ms: 9 }],
      },
      {
        id: "h:80/",
        host: "h",
        port: 80,
        path: "/",
        expect: "OpenWrt",
        summary: { probes: 4, up: 2, down: 2, up_pct: 50, up_now: false, since_ms: 222, drops: 1, latency: null, last_status: 200, last_expect_ok: false },
        transitions: [],
      },
    ],
  };
  const all = monitorsJson({ device: "d1", askedAtMs: 1_789_000_000_000, payload, only: null });
  assert.equal(all.device, "d1");
  assert.equal(all.asked_at_ms, 1_789_000_000_000);
  assert.equal(all.interval_secs, 15);
  assert.equal(all.targets.length, 2);
  // The numbers are the device's, under names a script can branch on.
  assert.deepEqual(all.targets[0], {
    id: "192.168.1.1:22",
    host: "192.168.1.1",
    port: 22,
    path: null,
    expect: null,
    up: true,
    up_pct: 100,
    since_ms: 111,
    drops: 0,
    latency_ms: 9,
    last_status: null,
    last_expect_ok: null,
    probes: 12,
    note: null, // no note attached: null, never a missing key
    transitions: [{ at_ms: 100, up: true, lasted_ms: 39_000 }],
  });
  assert.equal(all.targets[1].last_expect_ok, false);
  assert.equal(all.targets[1].last_status, 200);
  // A filtered view (watch --once <target> --json) is the same shape, one entry.
  const one = monitorsJson({ device: "d1", askedAtMs: 1, payload, only: "h:80/" });
  assert.equal(one.targets.length, 1);
  assert.equal(one.targets[0].id, "h:80/");
  // Nothing read yet is null, NOT false: a script must be able to tell "not known" from "down".
  const unknown = monitorsJson({
    device: "d1",
    askedAtMs: 1,
    payload: { targets: [{ id: "x:1", host: "x", port: 1, summary: { up_now: null, probes: 0 } }] },
    only: null,
  });
  assert.equal(unknown.targets[0].up, null);
  assert.equal(unknown.targets[0].up_pct, null);
  assert.equal(unknown.targets[0].transitions.length, 0);
  // A payload with no targets at all is an empty list, not a crash.
  assert.deepEqual(monitorsJson({ device: "d1", askedAtMs: 1, payload: null }).targets, []);
});


test("the operator's note rides on the line it explains", () => {
  const now = 1_789_000_000_000;
  // A DOWN line with the human's reason: without it an intentional reboot reads as a fault.
  const line = targetLine(
    { id: "h:22", note: { text: "I rebooted it", at_ms: now - 5000 }, summary: { up_now: false, since_ms: now - 4000, drops: 1 } },
    now,
  );
  assert.match(line, /^DOWN/);
  assert.match(line, /— I rebooted it$/);
  // No note: nothing appended (not an em dash with nothing after it).
  const bare = targetLine({ id: "h:22", summary: { up_now: true, since_ms: now - 1000 } }, now);
  assert.doesNotMatch(bare, /—/);
  // And the JSON carries it as data, with its stamp.
  const j = monitorsJson({
    device: "d1",
    askedAtMs: now,
    payload: { targets: [{ id: "h:22", host: "h", port: 22, note: { text: "I rebooted it", at_ms: now - 5000 }, summary: { up_now: false } }] },
    only: null,
  });
  assert.deepEqual(j.targets[0].note, { text: "I rebooted it", at_ms: now - 5000 });
  assert.equal(
    monitorsJson({ device: "d1", askedAtMs: now, payload: { targets: [{ id: "h:22", host: "h", port: 22, summary: {} }] }, only: null })
      .targets[0].note,
    null,
    "no note is null, never a missing key",
  );
});

test("reportText: a DOWN target shows the note above the console line", () => {
  const now = new Date(2026, 8, 15, 1, 40, 0).getTime();
  const text = reportText({
    status: { ok: true, release: "1.2.392", uptime_secs: 60 },
    monitors: {
      interval_secs: 15,
      targets: [
        {
          id: "h:22",
          note: { text: "I rebooted it", at_ms: now - 90_000 },
          summary: { up_now: false, since_ms: now - 60_000, drops: 1, up_pct: 50, latency: null, last_status: null },
          transitions: [{ at_ms: now - 60_000, up: false, lasted_ms: 39_000 }],
        },
      ],
    },
    sessions: null, boots: null, nowMs: now, cliVersion: "1.2.392", hostLabel: "d1",
  }).join("\n");
  assert.match(text, /went down\s+\(previous state lasted 39s\)/);
  assert.match(text, /note: I rebooted it/);
});


test("asciiJson: text that goes through a command line must survive it", () => {
  // The bug this exists for: a note with an em dash was stored as mojibake, because `curl -d` takes
  // its argument in the process code page on Windows.
  const body = asciiJson({ id: "h:22", text: "I rebooted it — not a fault" });
  // 1. Pure ASCII: no code page can mangle what has no high bytes.
  // eslint-disable-next-line no-control-regex
  assert.doesNotMatch(body, /[\u0080-\uffff]/);
  // 2. The escape is the standard \uXXXX form, so the device's JSON parser restores the text.
  assert.match(body, /\\u2014/);
  assert.deepEqual(JSON.parse(body), { id: "h:22", text: "I rebooted it — not a fault" });
  // 3. Structure and other types are untouched.
  assert.equal(asciiJson({ a: 1, b: true, c: null }), '{"a":1,"b":true,"c":null}');
  assert.deepEqual(JSON.parse(asciiJson({ t: "中文 / 日本語 / émoji 🚀" })), { t: "中文 / 日本語 / émoji 🚀" });
  // 4. An empty body stays empty (no stray braces).
  assert.equal(asciiJson({}), "{}");
  // 5. The same rule serves the OUTPUT side: a pipe is an encoding boundary too (PowerShell decodes
  //    a child's stdout with the console code page, which turned piped `--json` into mojibake on d1
  //    while the same text printed directly read fine).
  const out = asciiJson({ device: "d1", targets: [{ note: { text: "我重启的 — ok 🚀" } }] }, 2);
  // eslint-disable-next-line no-control-regex
  assert.doesNotMatch(out, /[\u0080-\uffff]/);
  assert.equal(JSON.parse(out).targets[0].note.text, "我重启的 — ok 🚀");
  assert.match(out, /\n  "device"/, "the indent is kept, so a human can still read it");
});


test("the --json paths actually USE the escaping helper (a helper test is not a wiring test)", () => {
  // Both encoding bugs of this round had the same shape: the helper was right and the CALL SITE
  // was not. `asciiJson`'s own test passed while the two `--json` printers still called
  // `JSON.stringify` directly, so the pipe stayed broken. This reads the shipped file and asserts
  // the wiring, which is the only place the difference is visible.
  const shipped = readFileSync(new URL("../bin/vale.js", import.meta.url), "utf8");
  const wired = shipped.split("asciiJson(monitorsJson(").length - 1;
  assert.equal(wired, 2, "both --json printers must go through asciiJson (see asciiJson's comment)");
  // …and the request body path, for the same reason.
  assert.match(shipped, /-d", asciiJson\(body\)/);
});


// ── waiting for a state ─────────────────────────────────────────────────────
test("waitDecision: the boundaries, including the one that would hang", () => {
  // Already there: one probe and out.
  assert.equal(waitDecision({ ok: true, wantUp: true, elapsedMs: 0, timeoutMs: 180_000 }), "met");
  assert.equal(waitDecision({ ok: false, wantUp: false, elapsedMs: 0, timeoutMs: 180_000 }), "met");
  // Not there yet: keep waiting while there is time.
  assert.equal(waitDecision({ ok: false, wantUp: true, elapsedMs: 1000, timeoutMs: 180_000 }), "waiting");
  // The exact boundary gives up — `>=`, not `>`, or `--timeout 0` would never finish.
  assert.equal(waitDecision({ ok: false, wantUp: true, elapsedMs: 180_000, timeoutMs: 180_000 }), "timed-out");
  assert.equal(waitDecision({ ok: false, wantUp: true, elapsedMs: 0, timeoutMs: 0 }), "timed-out");
  // An unknown state (no probe yet) is never "met", in either direction.
  assert.equal(waitDecision({ ok: null, wantUp: true, elapsedMs: 0, timeoutMs: 10 }), "waiting");
  assert.equal(waitDecision({ ok: undefined, wantUp: false, elapsedMs: 0, timeoutMs: 0 }), "timed-out");
});

test("parseWaitArgs: one place decides what wait means", () => {
  const a = parseWaitArgs(["wait", "192.168.1.1:22"]);
  assert.equal(a.target.id, "192.168.1.1:22");
  assert.equal(a.wantUp, true); // default: wait for it to come UP
  assert.equal(a.timeoutMs, 180_000);
  assert.equal(a.json, false);
  const b = parseWaitArgs(["wait", "h:80/", "--down", "--timeout", "30", "--json"]);
  assert.equal(b.wantUp, false);
  assert.equal(b.timeoutMs, 30_000);
  assert.equal(b.json, true);
  // Refusals, each with the reason a person needs.
  assert.match(parseWaitArgs(["wait", "h:22", "--up", "--down"]).error, /usage/);
  assert.match(parseWaitArgs(["wait", "h:22", "--timeout", "soon"]).error, /--timeout takes seconds/);
  assert.match(parseWaitArgs(["wait", "h:22", "--timeout", "999999"]).error, /--timeout takes seconds/);
  assert.match(parseWaitArgs(["wait"]).error, /usage/);
  assert.match(parseWaitArgs(["wait", "nonsense"]).error, /usage/);
});

test("waitLine: says what it saw, how long, and what it is waiting for", () => {
  const human = waitLine({ elapsedMs: 4000, probe: { ok: false, status: 500 }, wantUp: true, id: "h:80/" });
  assert.match(human, /^\s+4s\s+down\s+HTTP 500\s+\(waiting for up\)$/);
  const json = JSON.parse(waitLine({ elapsedMs: 4000, probe: { ok: true, status: 200, ms: 7 }, wantUp: true, id: "h:80/", json: true }));
  assert.deepEqual(json, { id: "h:80/", waited_ms: 4000, ok: true, status: 200, expect_ok: null, ms: 7, state: "up" });
  // A probe with no status (a TCP check) says null, not a missing key.
  const tcp = JSON.parse(waitLine({ elapsedMs: 0, probe: { ok: true, ms: 3 }, wantUp: true, id: "h:22", json: true }));
  assert.equal(tcp.status, null);
  assert.equal(tcp.state, "up");
});


test("stop and restart mark the run as deliberate before killing it", () => {
  // The run journal cannot see WHO ended the process, so a supervisor must say so first — otherwise
  // `vale restart` reads as "crashed" whenever the revival is slower than the heartbeat window.
  // This pins the WIRING (the helper is in the device route; a helper test cannot see a call site).
  const shipped = readFileSync(new URL("../bin/vale.js", import.meta.url), "utf8");
  const marks = shipped.split("markDeliberateStop()").length - 1;
  // 1 definition + 2 call sites (stop, restart).
  assert.equal(marks, 3, "stop and restart must both mark the run before ending it");
  assert.match(shipped, /\/api\/run\/mark-exit/);
});
