// npm CLI first tests (coverage audit rows 13+14). The CLI is the SOLE
// install/update channel and runs PowerShell under SYSTEM/admin — its
// quoting and update-mutual-exclusion previously had zero coverage.
// bin/summrise.js exports the pure helpers (dispatch is require.main-guarded).
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
  probeLine,
  monitorsJson,
  asciiJson,
  targetLine,
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
  newestOf,
  componentUrl,
  componentKey,
  desktopTaskPs,
  desktopStartPs
} = require("../bin/summrise.js");

test("psq: PowerShell single-quote doubling (injection surface for SYSTEM task scripts)", () => {
  assert.equal(
    psq("C:\\Program Files\\Summrise\\a'b"),
    "C:\\Program Files\\Summrise\\a''b",
  );
  assert.equal(psq("/plain/path"), "/plain/path");
  assert.equal(psq(""), "");
  assert.equal(psq("'"), "''");
  assert.equal(psq("a'b'c"), "a''b''c");
});

test("componentKey: the manifest's keys and the FILE names are not the same strings", () => {
  // The manifest pins `playwright` / `electron` / `cloudflared`; the files are
  // summrise-playwright.zip / electron-win32-x64.zip / cloudflared.exe. Mapping them
  // is what lets setup verify a fetched component against the release manifest — and
  // a file this release does NOT pin must not borrow somebody else's pin.
  assert.equal(componentKey("summrise-playwright.zip"), "playwright");
  assert.equal(componentKey("electron-win32-x64.zip"), "electron");
  assert.equal(componentKey("cloudflared.exe"), "cloudflared");
  assert.equal(componentKey("fix-tunnel.ps1"), null);
});

test("componentUrl: a component comes from the release host, under the agent path", () => {
  // The shape setup now depends on: the package carries none of the big
  // binaries, so "not in package" must mean "fetch it from the host that
  // already serves it" -- not "give up", which left a migrated device with no
  // tunnel and no way for the console to reach it.
  const cf = componentUrl("cloudflared.exe");
  assert.match(cf, /^https:\/\/[^/]+\/summrise-agent\/cloudflared\.exe$/);
  assert.match(
    componentUrl("summrise-playwright.zip"),
    /\/summrise-agent\/summrise-playwright\.zip$/,
  );
  // The name is APPENDED under the agent path, so it cannot move the request to
  // another host — and every component resolves to the SAME host, which is the
  // property the fetch depends on.
  assert.equal(
    componentUrl("anything").split("/")[2],
    cf.split("/")[2],
    "every component must come from one host",
  );
  assert.ok(
    componentUrl("anything").indexOf("/summrise-agent/") > 0,
    "components live under the agent path",
  );
});

test("desktopTaskPs / desktopStartPs: asking for the window, and answering with a FACT", () => {
  const task = desktopTaskPs("C:\\Program Files\\Summrise").join("\n");
  // The app's own shape, so `summrise desktop` and `summrise setup` cannot drift: logon plus a
  // 5-minute watchdog, launched through a .vbs so no console flashes, with the guard that
  // stops the watchdog stealing focus from whatever the operator is doing.
  assert.match(task, /New-ScheduledTaskTrigger -AtLogOn/);
  assert.match(task, /RepetitionInterval \(New-TimeSpan -Minutes 5\)/);
  assert.match(task, /Get-Process electron -ErrorAction SilentlyContinue\) \{ exit \}/);
  assert.match(task, /desktop-pulse\.vbs/);
  assert.match(task, /Register-ScheduledTask SummriseDesktop/);
  assert.match(task, /Start-ScheduledTask -TaskName SummriseDesktop/);

  const start = desktopStartPs("C:\\Summrise").join("\n");
  for (const word of ["already-running", "started", "not-started"]) {
    assert.ok(start.includes(word), `the command must be able to say ${word}`);
  }
  // ORDER IS THE PROPERTY: "already running" has to be decided BEFORE anything is started,
  // or asking for a window that is already there would launch a second shell and steal focus.
  assert.ok(
    start.indexOf("already-running") < start.indexOf("Start-ScheduledTask"),
    "the already-running check must come first",
  );
  // THE THREE THINGS THE DEVICE TAUGHT US, each of which broke this command in a different way:
  // (a) `-File` takes DOUBLE quotes — single quotes are literal at the cmd layer, and the device
  //     answered "unsupported path format" with the quotes still in the path;
  // (b) it must NOT re-register the task to start it — Register-ScheduledTask needs the
  //     interactive user's principal, and a service-account shell has no such mapping;
  // (c) IT MUST NOT BE A `-Command` STRING AT ALL. The CLI spawns with `shell: true`, so the
  //     command goes through cmd.exe, and cmd splits on `&` — the PowerShell call operator cut
  //     the command in half and the `$t` assignment never ran. A file has nothing to mangle.
  assert.ok(start.includes('-File "C:\\Summrise\\scripts\\register-desktop-task.ps1"'),
    "the register script must be passed with double quotes");
  assert.ok(!start.includes("-Command"), "the start script must not be a -Command string (cmd splits on &)");
  assert.ok(start.includes("if (-not $t)"), "re-registration must be conditional on the task being absent");
  assert.ok(start.includes("Start-ScheduledTask"), "and starting it is what actually happens");
  // AND THE CLI MUST ACTUALLY USE IT AS A FILE. This is the wiring half: the builder above can be
  // perfect while the caller still spawns `-Command`, which is exactly what shipped in 1.2.456.
  const built = readFileSync(new URL("../bin/summrise.js", import.meta.url), "utf8");
  assert.ok(built.includes("desktop-start.ps1"), "the CLI must write the start script to a file");
  assert.ok(!/"-Command",\s*desktopStartPs/.test(built),
    "the CLI must not spawn desktopStartPs() as a -Command string");
});

test("newestOf: the newer of two channels — and silence is never agreement", () => {
  assert.equal(newestOf("1.2.452", "1.2.453"), "1.2.453");
  assert.equal(newestOf("1.2.453", "1.2.452"), "1.2.453");
  // Across a release line the triple still decides — the check that matters
  // when one channel has moved on and the other has not.
  assert.equal(newestOf("1.2.9", "1.3.0"), "1.3.0");
  // ONE channel silent: the other still answers.
  assert.equal(newestOf(null, "1.2.452"), "1.2.452");
  assert.equal(newestOf("1.2.452", null), "1.2.452");
  // BOTH silent: unknown. This is the property the CDN-only read already had
  // and the reason it is a pure function now — a status that cannot check must
  // never render as "this device is current".
  assert.equal(newestOf(null, null), null);
  // An unparseable version is not a version.
  assert.equal(newestOf("garbage", "1.2.452"), "1.2.452");
  assert.equal(newestOf("1.2.452", "not-a-version"), "1.2.452");
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
    "D:\\Summrise\\scripts",
    "D:\\Summrise\\components\\summrise-desktop-electron",
    "Write-Host",
  );
  const body = lines.join("\n");
  assert.match(body, /Summrise\.lnk/, "touches the desktop Summrise link");
  assert.match(body, /summrise-desktop\.exe/, "detects the retired Tauri target");
  assert.match(body, /summrise-tray\.exe/, "detects the retired tray target");
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
  const { parseAgentPort } = require("../bin/summrise.js");
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
  const { agentPort } = require("../bin/summrise.js");
  assert.equal(agentPort("/definitely/not/here"), 18080, "missing config");
  const d = fs.mkdtempSync(path.join(os.tmpdir(), "summrise-port-"));
  fs.writeFileSync(path.join(d, "config.yaml"), "server:\n  port: 7740\n");
  assert.equal(agentPort(d), 7740);
  fs.rmSync(d, { recursive: true, force: true });
});

test("firewallPs: idempotent Summrise-scoped rule for the port", () => {
  const { firewallPs } = require("../bin/summrise.js");
  const body = firewallPs(7740).join("\n");
  assert.match(
    body,
    /LocalPort \$fwPort/,
    "uses the variable, hardcodes nothing else",
  );
  assert.match(body, /\$fwPort = 7740/, "bakes the configured port");
  assert.match(body, /New-NetFirewallRule/, "creates the allow rule");
  assert.match(body, /Remove-NetFirewallRule/, "prunes stale own rules");
  assert.match(body, /'Summrise Agent'/, "DisplayName-scoped, never foreign rules");
  assert.ok(
    ![...body].some((c) => c.charCodeAt(0) > 127),
    "ASCII-only (system-locale PS)",
  );
});

test("startDesktopPs: the electron launcher matches the migrated layout", () => {
  const { startDesktopPs } = require("../bin/summrise.js");
  const body = startDesktopPs(
    "D:\\Summrise\\components\\summrise-desktop-electron",
  ).join("\n");
  assert.match(
    body,
    /\$dir = 'D:\\Summrise\\components\\summrise-desktop-electron'/,
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
  // second-instance (SummriseDesktop pulse every 5 min) must NOT open a window —
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
  const { writeReleaseMarker } = require("../bin/summrise.js");
  const d = fs.mkdtempSync(path.join(os.tmpdir(), "summrise-relmark-"));
  try {
    // Writes the package.json version under etc/ (layout v2). Callers
    // (setup/update) always create etc/ during staging/migration first.
    fs.mkdirSync(path.join(d, "etc"), { recursive: true });
    writeReleaseMarker(d);
    const pkg = JSON.parse(
      fs.readFileSync(new URL("../package.json", import.meta.url), "utf8"),
    );
    assert.equal(
      fs.readFileSync(path.join(d, "etc", ".summrise-release"), "utf8"),
      pkg.version,
    );
    // Idempotent (re-run overwrites with the same value).
    writeReleaseMarker(d);
    assert.equal(
      fs.readFileSync(path.join(d, "etc", ".summrise-release"), "utf8"),
      pkg.version,
    );
  } finally {
    fs.rmSync(d, { recursive: true, force: true });
  }
});

test("writeReleaseMarker: missing dir stays silent (best-effort, never throws)", () => {
  const { writeReleaseMarker } = require("../bin/summrise.js");
  assert.doesNotThrow(() => writeReleaseMarker("Z:\\definitely\\not\\here"));
});

test("uninstallVersionPs: $ok-gated DisplayVersion parity, UninstallString only when absent", () => {
  const { uninstallVersionPs } = require("../bin/summrise.js");
  const body = uninstallVersionPs("C:\\Program Files\\Summrise", "1.2.307").join(
    "\n",
  );
  assert.match(
    body,
    /\$ok -and '1\.2\.307'/,
    "gated on provable swap success like .summrise-release",
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
  const { uninstallRegBodyPs } = require("../bin/summrise.js");
  const body = uninstallRegBodyPs("D:\\Summrise", "1.2.307").join("\n");
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
  const idxComp = body.indexOf("components\\npm-global\\summrise.cmd");
  const idxTools = body.indexOf("tools\\npm-global\\summrise.cmd");
  assert.ok(
    idxComp >= 0 && idxTools > idxComp,
    "components first, tools legacy fallback",
  );
  // Control panel does not elevate: relaunch elevated or uninstall dies.
  assert.match(body, /-Verb RunAs/, "re-elevates (HKLM/schtasks need admin)");
  assert.match(body, /-Wait/, "control panel waits for completion");
  // Empty version = no-op (setup with an unreadable package.json).
  assert.deepEqual(
    uninstallRegBodyPs("D:\\Summrise", ""),
    [],
    "empty ver writes nothing",
  );
  assert.ok(
    ![...body].some((c) => c.charCodeAt(0) > 127),
    "ASCII-only (system-locale PS)",
  );
});

test("autostartArgv: ENABLE/DISABLE both boot tasks, no credential-prompt flags", () => {
  const { autostartArgv, BOOT_TASKS } = require("../bin/summrise.js");
  assert.deepEqual(
    [...BOOT_TASKS].sort(),
    ["SummriseAgent", "SummriseDesktop"],
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
  const { bootTaskPs } = require("../bin/summrise.js");
  const reg = bootTaskPs(
    "C:\\V\\summrise-agent.exe",
    "C:\\V\\etc\\config.yaml",
    false,
  ).join("\n");
  assert.match(
    reg,
    /-Argument \('"'\s*\+\s*'C:\\V\\etc\\config\.yaml'\s*\+\s*'"'\)/,
    "-Argument is the config path",
  );
  assert.ok(
    !reg.includes("summrise-agent.exe' + '\"'"),
    "the exe path must never be the argument",
  );
  assert.match(reg, /-UserId SYSTEM/, "SYSTEM principal");
  assert.match(reg, /ExecutionTimeLimit.*0/, "never kill the running task");
  assert.match(
    reg,
    /Register-ScheduledTask SummriseAgent/,
    "re-registers with -Force semantics",
  );
  assert.ok(!reg.includes("Start-ScheduledTask"), "no kick without start=true");
  const kick = bootTaskPs(
    "C:\\V\\summrise-agent.exe",
    "C:\\V\\etc\\config.yaml",
    true,
  ).join("\n");
  assert.match(
    kick,
    /Start-ScheduledTask SummriseAgent/,
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
  const { migrateLayoutPs } = require("../bin/summrise.js");
  const body = migrateLayoutPs("D:\\Summrise", "C:\\ProgramData\\Summrise").join("\n");
  for (const pair of [
    ["D:\\Summrise\\config.yaml", "D:\\Summrise\\etc\\config.yaml"],
    ["D:\\Summrise\\summrise-agent.hostname", "D:\\Summrise\\etc\\summrise-agent.hostname"],
    ["D:\\Summrise\\.summrise-release", "D:\\Summrise\\etc\\.summrise-release"],
    ["D:\\Summrise\\tools\\node", "D:\\Summrise\\components\\node"],
    ["D:\\Summrise\\playwright", "D:\\Summrise\\components\\playwright"],
    [
      "D:\\Summrise\\summrise-desktop-electron",
      "D:\\Summrise\\components\\summrise-desktop-electron",
    ],
    ["D:\\Summrise\\start-desktop.ps1", "D:\\Summrise\\scripts\\start-desktop.ps1"],
    ["D:\\Summrise\\installer.log", "C:\\ProgramData\\Summrise\\logs\\installer.log"],
    ["D:\\Summrise\\pwout", "C:\\ProgramData\\Summrise\\pwout"],
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
    body.includes("$summriseMg = (-not (Test-Path 'D:\\Summrise\\etc\\.layout-v2'))"),
    "marker short-circuits re-runs (single-line guard)",
  );
  // 24 move lines + the node-kill line + the marker write all carry the guard.
  assert.equal(
    body.split("if ($summriseMg").length - 1,
    26,
    "every statement carries the marker guard",
  );
  assert.ok(
    body.includes("if ($summriseMg -and (-not (") &&
      body.includes("(Test-Path 'D:\\Summrise\\config.yaml')"),
    "marker write gated on pending pairs",
  );
  assert.ok(
    body.includes(
      `New-Item -ItemType File -Force -Path 'D:\\Summrise\\etc\\.layout-v2'`,
    ),
    "marker file itself",
  );
  assert.ok(
    ![...body].some((c) => c.charCodeAt(0) > 127),
    "ASCII-only (system-locale PS)",
  );
});

test("rollbackVersionOk: plain dotted triples only (URL interpolation gate)", () => {
  const { rollbackVersionOk } = require("../bin/summrise.js");
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
  const { boxedVersions } = require("../bin/summrise.js");
  const fs = require("node:fs");
  const os = require("node:os");
  const path = require("node:path");
  const dir = fs.mkdtempSync(
    path.join(os.tmpdir(), `summrise-boxed-empty-${process.pid}-`),
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
  const { boxedVersions } = require("../bin/summrise.js");
  const fs = require("node:fs");
  const os = require("node:os");
  const path = require("node:path");
  const crypto = require("node:crypto");
  const dir = fs.mkdtempSync(
    path.join(os.tmpdir(), `summrise-boxed-full-${process.pid}-`),
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
    fs.writeFileSync(path.join(dir, "summrise-playwright.zip"), zipBytes);
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
  const { boxedVersions } = require("../bin/summrise.js");
  const fs = require("node:fs");
  const os = require("node:os");
  const path = require("node:path");
  const dir = fs.mkdtempSync(
    path.join(os.tmpdir(), `summrise-boxed-big-${process.pid}-`),
  );
  try {
    // Sparse file: stat reports 301MB instantly, disk use stays ~nil.
    const big = path.join(dir, "summrise-playwright.zip");
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
  const { writeBoxedVersions } = require("../bin/summrise.js");
  const fs = require("node:fs");
  const os = require("node:os");
  const path = require("node:path");
  const dir = fs.mkdtempSync(
    path.join(os.tmpdir(), `summrise-boxed-write-${process.pid}-`),
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
// update-busy marker, no staged summrise-agent.new.exe, no scripts\summrise-update.ps1,
// and no `update start` line in summrise-update.log — the command never reached the
// device at all, and the transport failure was indistinguishable from the
// success signal. Only the RELEASE MARKER, still reading the old version, told
// the truth, and the operator had to go and read four things by hand to learn it.
//
// `summrise status` is the command a person runs to ask "where is this device". It
// reported RUNNING / install dir / panel URL and NOTHING about the release or a
// pending swap, so it could not answer the only question that mattered.
//
// These tests pin the report's CONTENT, because the failure mode is an
// omission: a `status` that prints three plausible lines while leaving out the
// release version looks perfectly healthy.
test("statusReport: an UNREADABLE process list is not 'STOPPED'", () => {
  const { statusReport } = require("../bin/summrise.js");
  // `agentRunning: null` means the process list could not be READ. Reporting STOPPED
  // there is a claim of ABSENCE from a failed read — and for the agent itself that is
  // the worst possible answer ("your device is down" when nobody looked). Fourth
  // instance of this shape; the field follows the rule its neighbours state.
  const unknown = statusReport({
    agentRunning: null,
    installDir: "D:\\Summrise",
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
    installDir: "D:\\Summrise",
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
  const { statusReport } = require("../bin/summrise.js");
  const now = 1_700_000_000_000;

  // (a) An update was attempted and never finished: the marker survives with
  //     its original mtime. This is THE diagnostic the incident lacked.
  const stalled = statusReport({
    agentRunning: true,
    installDir: "D:\\Summrise",
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
    installDir: "D:\\Summrise",
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
    installDir: "D:\\Summrise",
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
    installDir: "D:\\Summrise",
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
  const { updateReceiptPs } = require("../bin/summrise.js");
  const lines = updateReceiptPs("D:\\Summrise", "1.2.321", "1.2.322");
  const body = lines.join("\n");
  assert.match(
    body,
    /summrise-update\.log/,
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
  const { updateReceiptPs, psq } = require("../bin/summrise.js");
  const probe = [
    "D:\\Summrise",
    "D:\\ProgramData\\Summrise",
    "C:\\Program Files\\Summrise",
  ];
  for (const dataDir of probe) {
    // Mirrors the `const log = ...` line in update(), verbatim.
    const swapLog = `Out-File '${dataDir.replace(/'/g, "''")}\\logs\\summrise-update.log' -Append`;
    const receipt = updateReceiptPs(psq(dataDir), "1.0.0", "1.0.1")[0];
    const receiptLog = receipt.slice(receipt.indexOf("Out-File"));
    assert.equal(receiptLog, swapLog, `sinks must agree for ${dataDir}`);
  }
  // A quote in the data dir must be doubled, exactly as the swap does it —
  // otherwise the single-quoted PS literal breaks and the receipt never lands.
  const quoted = updateReceiptPs(psq("D:\\it's\\Summrise"), "1.0.0", "1.0.1")[0];
  assert.match(
    quoted,
    /it''s/,
    "embedded quote doubled for the PS single-quoted literal",
  );
});

// ── A version marker must be EARNED, not asserted ───────────────────────────
//
// `etc\.summrise-release` is the device's ONLY local version truth: agent_update
// reads it as `local` and answers up_to_date when the remote is not newer, and
// /api/status serves it as `release` — the field the panel, the tray and the
// console fleet card all display.
//
// `summrise rollback` wrote it UNCONDITIONALLY after `summrise update` returned status 0.
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
  const { awaitReleaseMarker } = require("../bin/summrise.js");
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
  const { releaseMarkerVerdict } = require("../bin/summrise.js");
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
  const src = fs.readFileSync(path.join(here, "..", "bin", "summrise.js"), "utf8");

  // (a) The marker is written by the SWAP SCRIPT (gated on $ok), never by the
  //     CLI. A CLI write is the regression: it erases the swap's proof.
  assert.ok(
    !/writeFileSync\(path\.join\(ETC_DIR,\s*"\.summrise-release"\)/.test(src),
    "the CLI must NOT write etc\\.summrise-release — only the swap script may, and only from a provable copy",
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
  const { psArgv } = require("../bin/summrise.js");
  const script =
    `"[$(Get-Date -Format o)] update requested 1.2.322 -> 1.2.323 " | Out-File 'D:\\Summrise\\logs\\summrise-update.log' -Append; ` +
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
  const src = fs.readFileSync(path.join(here, "..", "bin", "summrise.js"), "utf8");

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
    new URL("../src/summrise.ts", import.meta.url),
    "utf8",
  );
  const built = fs.readFileSync(
    new URL("../bin/summrise.js", import.meta.url),
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
    new URL("../../summrise-command-core/src/config.rs", import.meta.url),
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
    new URL("../src/summrise.ts", import.meta.url),
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
const { statusReport, behindBy, isBehind } = require("../bin/summrise.js");
const DRIFT_BASE = {
  agentRunning: true,
  installDir: "D:\\Summrise",
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
  // AND THE SAME FACT AS A BOOLEAN (round 202). The update guard needs "is the CDN ahead of me", and its first version
  // asked `behindBy(...)` — whose answer is a PHRASE, truthy for "0 releases" and "-1 releases" alike, so it would have
  // refused every update including the correct one. These cases are the ones that caught it.
  assert.equal(isBehind("1.2.439", "1.2.440"), true);
  assert.equal(isBehind("1.2.440", "1.2.440"), false);
  assert.equal(isBehind("1.2.440", "1.2.439"), false);
  assert.equal(isBehind("1.2.440", "1.3.0"), false);
  assert.equal(isBehind("", "1.2.440"), false);
  assert.equal(isBehind("garbage", "1.2.440"), false);
  // A cross-minor jump is a different operation (the CDN prunes last-5-per-minor
  // and `summrise rollback` refuses it), so it is not "N releases".
  assert.equal(behindBy("1.1.9", "1.2.0"), "a release line, not a patch count");
  assert.equal(behindBy("garbage", "1.2.0"), "an unknown number of releases");
});

// ── the contract list's ROW 1, which had no instrument ──────────────────────────
//
// THE PROMISE, stated here because its original home is gone: a published tgz is installed on
// devices in the field, so the CLI's VERBS, ARGS, install layout and boot-task arguments are a
// device-facing contract — renaming or removing one needs dual-accept, a rollback point and a
// device regression, not just a doc edit. (This used to cite a four-row contract list in
// `docs/agents/iteration-loop.md`; that file was pruned with the iteration journal in 2bb98183 and
// this citation was left pointing at nothing. The rule outlives the document, so it is written
// out rather than referenced.)
//
// Round 253 measured how that promise is kept: **this file's 36 tests cover HELPERS and
// PowerShell generators** (`busyIsFresh`, `writeReleaseMarker`, `uninstallVersionPs`,
// `autostartArgv`, `rollbackVersionOk`, `psq`, `parseAgentPort` …) **and ZERO of them touch
// the verb dispatch.** So nothing anywhere would fail if a verb were renamed — which is
// the exact break the contract list names.
//
// SAFETY, learned the hard way in round 246: this assertion runs the CLI **with NO
// ARGUMENTS ONLY**. `summrise` with no args prints its verb list and exits 0 — measured. Every
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
    ...new Set([...guide.matchAll(/^summrise ([a-z]+)/gm)].map((m) => m[1])),
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
  const out = execFileSync(process.execPath, [join(HERE, "..", "bin", "summrise.js")], {
    encoding: "utf8",
    timeout: 30_000,
  });

  const missing = promised.filter((v) => !new RegExp(`(^|[^a-z-])${v}([^a-z-]|$)`, "m").test(out));
  assert.deepEqual(
    missing,
    [],
    `these verbs are promised by the root guide but the CLI does not print them: ${missing.join(", ")}. ` +
      `A published tgz lives on devices in the field, so the verb list is a device-facing promise: ` +
      `a verb disappearing needs dual-accept, a rollback point and a device regression, not just a ` +
      `doc edit. CLI printed: ${out.split("\n")[0]}`,
  );
});



test("targetLine: one drop is 'drop', several are 'drops'", () => {
  const now = 1_789_000_000_000;
  const at = (drops) => targetLine({ id: "a:22", summary: { up_now: true, since_ms: now - 1000, drops } }, now);
  assert.match(at(1), /1 drop(?!s)/);
  assert.match(at(2), /2 drops/);
  assert.doesNotMatch(at(0), /drop/);
});


// ── summrise report: the block an operator pastes ────────────────────────────────



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




// ── what the console said when it happened (the join) ───────────────────────




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


test("the --json path actually USES the escaping helper (a helper test is not a wiring test)", () => {
  // Both encoding bugs of this round had the same shape: the helper was right and the CALL SITE
  // was not. `asciiJson`'s own test passed while the two `--json` printers still called
  // `JSON.stringify` directly, so the pipe stayed broken. This reads the shipped file and asserts
  // the wiring, which is the only place the difference is visible.
  const shipped = readFileSync(new URL("../bin/summrise.js", import.meta.url), "utf8");
  const wired = shipped.split("asciiJson(monitorsJson(").length - 1;
  assert.equal(wired, 1, "the remaining --json printer must go through asciiJson (watch was pruned)");
  // …and the request body path, for the same reason.
  assert.match(shipped, /-d", asciiJson\(body\)/);
});


// ── waiting for a state ─────────────────────────────────────────────────────




test("stop and restart mark the run as deliberate before killing it", () => {
  // The run journal cannot see WHO ended the process, so a supervisor must say so first — otherwise
  // `summrise restart` reads as "crashed" whenever the revival is slower than the heartbeat window.
  // This pins the WIRING (the helper is in the device route; a helper test cannot see a call site).
  const shipped = readFileSync(new URL("../bin/summrise.js", import.meta.url), "utf8");
  // ASSERT THE CALL SHAPES, not an occurrence count: counting broke the moment a third caller
  // appeared, and a count cannot tell a call site from a comment.
  assert.equal(
    (shipped.match(/markDeliberateStop\(\);/g) || []).length,
    2,
    "stop and restart must both mark the run before ending it",
  );
  assert.match(shipped, /\/api\/run\/mark-exit/);
  // …and the UPDATE path marks its own swap with the reason that makes the next start say
  // "replaced" instead of "crashed" (its timing cannot be told from a crash: the gaps overlap).
  assert.match(shipped, /markDeliberateStop\("update"\)/, "summrise update must mark its swap as deliberate");
  assert.match(shipped, /reason/, "the marker carries the reason the verdict depends on");
});


// ── the console, read the same way by `report` and `wait` ───────────────────






// ── following a LIVE console ────────────────────────────────────────────────



test("the help cannot promise a verb that does not exist", () => {
  // The round-25 prune deleted `report` and `watch` while the hand-written usage line still
  // advertised them — a usage line is a contract. It is derived from `commands` now, and this pins
  // that: every name printed comes from the dispatcher's own table, and nothing else is printed.
  const shipped = readFileSync(new URL("../bin/summrise.js", import.meta.url), "utf8");
  assert.match(shipped, /summrise <" \+ Object\.keys\(commands\)\.join\("\|"\) \+ ">/);
  assert.doesNotMatch(shipped, /summrise <setup\|status\|report/);
});
