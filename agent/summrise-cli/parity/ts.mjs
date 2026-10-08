#!/usr/bin/env node
// THE TYPESCRIPT SIDE OF THE PARITY CHECK.
//
// Prints, as JSON, the answer every pure decision in `agent/summrise-agent-npm/bin/summrise.js`
// gives for the corpus in `summrise-cli/src/bin/parity.rs`. `compare.mjs` runs both and diffs them,
// case by case.
//
// It requires the BUILT bin (the same artifact `npm test` uses) rather than the TypeScript source,
// because bin/summrise.js is what a device runs — the source and the emit are held together by the
// package's own `npm run build`, and the parity check must compare the two things that exist.
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const require = createRequire(import.meta.url);
const cli = require(join(here, "..", "..", "summrise-agent-npm", "bin", "summrise.js"));

const {
  psq,
  psArgv,
  parseAgentPort,
  parseDeviceToken,
  parseTargetArg,
  componentKey,
  componentUrl,
  componentFetchUrl,
  fmtDuration,
  targetLine,
  probeLine,
  monitorsJson,
  asciiJson,
  busyIsFresh,
  behindBy,
  isBehind,
  newestOf,
  updateWouldNotMove,
  rollbackVersionOk,
  updateBusyPath,
  busyMarkerPs,
  updateReceiptPs,
  statusReport,
  releaseMarkerVerdict,
  DESKTOP_AUMID,
  lnkIdentity,
  deskShortcutRepairPs,
  startDesktopPs,
  ensureDesktopPs,
  desktopTaskPs,
  desktopStartPs,
  firewallPs,
  bootTaskPs,
  migrateLayoutPs,
  uninstallRegBodyPs,
  uninstallVersionPs,
  playwrightProbePs,
  autostartArgv,
  BOOT_TASKS,
} = cli;

const cases = [];
const add = (id, value) => cases.push({ id, value });
const SEP = "\u001f";

// ── quoting ────────────────────────────────────────────────────────────────
for (const s of [
  "C:\\Program Files\\Summrise\\a'b",
  "/plain/path",
  "",
  "'",
  "a'b'c",
  "it''s",
]) {
  add(`psq:${s}`, psq(s));
}
const script =
  "[$(Get-Date -Format o)] update requested 1.2.322 -> 1.2.323 \" | Out-File 'D:\\Summrise\\logs\\summrise-update.log' -Append; Write-Output \"a<b & c|d ^ e%f\"";
add("ps_argv:script", psArgv(script).join(SEP));

// ── config parsing ─────────────────────────────────────────────────────────
for (const yaml of [
  "server:\n  host: \"0.0.0.0\"\n  port: 7740\n",
  "server:\n  port: 18080\n",
  "server:\n  host: \"127.0.0.1\"\n",
  "serial:\n  port: 1234\n",
  "server:\n  port: 0\n",
  "server:\n  port: 99999\n",
  "",
  "server:\n  port: \"7740\"  # a comment\n",
  "server:\r\n  port: 8080\r\n",
  "server:\n  port:7740\n",
]) {
  add(`parse_agent_port:${yaml.replace(/\n/g, "\\n").replace(/\r/g, "\\r")}`, parseAgentPort(yaml));
}
for (const yaml of [
  "server:\n  device_token: abc.DEF-123_x\n",
  "server:\n  device_token: \"tok\"\n",
  "serial:\n  device_token: nope\n",
  "server:\n  host: x\n",
  "",
]) {
  add(`parse_device_token:${yaml.replace(/\n/g, "\\n")}`, parseDeviceToken(yaml));
}
for (const arg of [
  "192.168.1.1:80/health",
  "192.168.1.1:80",
  "h:0",
  "h:70000",
  "no-port",
  "h:80/x:y",
  "",
  "  h:22  ",
]) {
  const t = parseTargetArg(arg);
  add(
    `parse_target_arg:${arg}`,
    t ? { host: t.host, port: t.port, path: t.path, id: t.id } : null,
  );
}

// ── components ─────────────────────────────────────────────────────────────
for (const name of [
  "summrise-playwright.zip",
  "summrise-playwright-mcp.tgz",
  "electron-win32-x64.zip",
  "cloudflared.exe",
  "fix-tunnel.ps1",
  "",
]) {
  add(`component_key:${name}`, componentKey(name));
  add(`component_url:${name}`, componentUrl(name));
}
const pins = {
  cloudflared: { url: "https://mirror.test/summrise-agent/cloudflared.exe", sha256: "a".repeat(64) },
  playwright: { url: "   ", sha256: "b".repeat(64) },
  electron: { sha256: "c".repeat(64) },
};
for (const name of [
  "cloudflared.exe",
  "electron-win32-x64.zip",
  "summrise-playwright.zip",
  "fix-tunnel.ps1",
]) {
  add(`component_fetch_url:${name}`, componentFetchUrl(name, pins));
}
// The advice is a constant in the TypeScript source rather than an export, so it is read from the
// SOURCE the same way the oracle reads it — one string, one author.
add("stage_advice", require("node:fs").readFileSync(join(here, "..", "..", "summrise-agent-npm", "src", "summrise.ts"), "utf8").match(/run 'summrise setup' to stage it[^"]*/)[0]);

// ── durations, lines, JSON ─────────────────────────────────────────────────
for (const ms of [0, 999, 1000, 59_999, 60_000, 3_599_999, 3_600_000, 86_400_000, 200_000_000, -5]) {
  add(`fmt_duration:${ms}`, fmtDuration(ms));
}
const now = 1_789_000_000_000;
const targets = [
  { id: "a:22", summary: { up_now: true, since_ms: now - 1000, drops: 1 } },
  { id: "a:22", summary: { up_now: true, since_ms: now - 1000, drops: 2 } },
  { id: "a:22", summary: { up_now: true, since_ms: now - 1000, drops: 0 } },
  { id: "h:80/", summary: { up_now: false, since_ms: now - 1000, last_status: 200, last_expect_ok: false } },
  { id: "h:80/", summary: { up_now: true, since_ms: now - 1000, last_status: 200, last_expect_ok: true } },
  { id: "h:80/", summary: { up_now: true, since_ms: now - 1000, last_status: 200, last_expect_ok: null } },
  { id: "x:1", summary: { up_now: null, probes: 0 } },
  {
    id: "long-host-name.example.com:1234",
    summary: { up_now: true, since_ms: now - 90_000, up_pct: 99, latency: { avg: 9 }, drops: 3 },
    note: { text: "I rebooted it \u2014 ok" },
  },
];
targets.forEach((t, i) => add(`target_line:${i}`, targetLine(t, now, 30)));
const payload = {
  ok: true,
  interval_secs: 15,
  targets: [
    {
      id: "192.168.1.1:22", host: "192.168.1.1", port: 22, path: null, expect: null,
      summary: {
        probes: 12, up: 12, down: 0, up_pct: 100, up_now: true, since_ms: 111, drops: 0,
        latency: { min: 7, avg: 9, max: 16 }, last_status: null, last_expect_ok: null,
      },
      transitions: [{ at_ms: 100, up: true, lasted_ms: 39_000 }],
      series: [{ ts_ms: 1, ok: true, ms: 9 }],
    },
    {
      id: "h:80/", host: "h", port: 80, path: "/", expect: "OpenWrt",
      summary: {
        probes: 4, up: 2, down: 2, up_pct: 50, up_now: false, since_ms: 222, drops: 1,
        latency: null, last_status: 200, last_expect_ok: false,
      },
      transitions: [],
    },
  ],
};
add("monitors_json:all", asciiJson(monitorsJson({ device: "d1", askedAtMs: 1_789_000_000_000, payload, only: null })));
add("monitors_json:one", asciiJson(monitorsJson({ device: "d1", askedAtMs: 1, payload, only: "h:80/" })));
add("print_monitors_json", asciiJson(monitorsJson({ device: "d1", askedAtMs: 1_789_000_000_000, payload, only: null })));
const unknown = { targets: [{ id: "x:1", host: "x", port: 1, summary: { up_now: null, probes: 0 } }] };
add("monitors_json:unknown", asciiJson(monitorsJson({ device: "d1", askedAtMs: 1, payload: unknown, only: null })));
add("monitors_json:null", asciiJson(monitorsJson({ device: "d1", askedAtMs: 1, payload: null, only: null })));
for (const v of [
  { id: "h:22", text: "I rebooted it \u2014 not a fault" },
  { a: 1, b: true, c: null },
  {},
  { t: "\u4e2d\u6587 / \u65e5\u672c\u8a9e / \u00e9moji \ud83d\ude80" },
  [1, 2.5, "x", null, true],
  { nested: { deep: ["\ud83d\ude00", { k: "\u2014" }] } },
]) {
  add(`ascii_json:${JSON.stringify(v)}`, asciiJson(v));
  add(`ascii_json_pretty:${JSON.stringify(v)}`, asciiJson(v, 2));
}
add("probe_line", probeLine({ id: "h:80/", expect: "OpenWrt" }, { ok: false, status: 200, expect_ok: false, ms: 12 }, now));

// ── the update decision's facts ────────────────────────────────────────────
for (const [m, n] of [
  [1_700_000_000_000 - 9 * 60_000, 1_700_000_000_000],
  [1_700_000_000_000 - 11 * 60_000, 1_700_000_000_000],
  [1_700_000_000_000, 1_700_000_000_000],
  [1_700_000_000_000 - 10 * 60_000 - 1, 1_700_000_000_000],
]) {
  add(`busy_is_fresh:${m}:${n}`, busyIsFresh(m, n));
}
for (const [a, b] of [
  ["1.2.9", "1.2.12"],
  ["1.2.9", "1.2.10"],
  ["1.1.9", "1.2.0"],
  ["garbage", "1.2.0"],
  ["1.2.0", "garbage"],
  ["1.2.3", "1.2.3"],
  ["1.2.5", "1.2.1"],
]) {
  add(`behind_by:${a}:${b}`, behindBy(a, b));
  add(`is_behind:${a}:${b}`, isBehind(a, b));
  add(`newest_of:${a}:${b}`, newestOf(a, b));
}
for (const a of ["1.2.452", "garbage", ""]) {
  add(`newest_of:null:${a}`, newestOf(null, a));
  add(`newest_of:${a}:null`, newestOf(a, null));
}
add("newest_of:null:null", newestOf(null, null));
for (const [a, b] of [
  ["1.2.438", "1.2.438"],
  ["1.2.437", "1.2.438"],
  ["", "1.2.438"],
  ["1.2.438", ""],
]) {
  add(`update_would_not_move:${a}:${b}`, updateWouldNotMove(a, b));
}
for (const v of ["1.2.307", "0.0.1", "1.2", "1.2.3.4", "1.2.307/../../evil", "--clear", ""]) {
  add(`rollback_version_ok:${v}`, rollbackVersionOk(v));
}
add("update_busy_path", updateBusyPath());
add("busy_marker_ps", busyMarkerPs());
for (const [dq, from, to] of [
  ["D:\\Summrise", "1.2.321", "1.2.322"],
  ["D:\\it''s\\Summrise", "1.0.0", "1.0.1"],
]) {
  add(`update_receipt_ps:${dq}`, updateReceiptPs(dq, from, to).join("\n"));
}

// ── the status report's content ────────────────────────────────────────────
const base = (agent, release, latest, marker) => ({
  agentRunning: agent,
  installDir: "D:\\Summrise",
  exeExists: true,
  port: 18080,
  releaseVersion: release,
  updateMarkerMs: marker,
  updateMarkerUnreadable: false,
  packageVersion: "1.2.359",
  latestVersion: latest,
  nowMs: 1_700_000_000_000,
});
const N = 1_700_000_000_000;
for (const [id, facts] of [
  ["unknown-process", base(null, "1.2.359", "1.2.359", null)],
  ["running", base(true, "1.2.359", "1.2.359", null)],
  ["stopped", base(false, "1.2.359", "1.2.359", null)],
  ["stalled", base(true, "1.2.321", "1.2.322", N - 27 * 60_000)],
  ["in-flight", base(true, "1.2.321", "1.2.322", N - 30_000)],
  ["no-marker", base(false, null, "1.2.322", null)],
  ["behind", base(true, "1.2.340", "1.2.345", null)],
  ["current", base(true, "1.2.340", "1.2.340", null)],
  ["cdn-silent", base(true, "1.2.340", null, null)],
]) {
  add(`status_report:${id}`, statusReport(facts).join("\n"));
}
add(
  "status_report:marker-unreadable",
  statusReport({ ...base(true, "1.2.340", "1.2.340", null), updateMarkerUnreadable: true }).join("\n"),
);

const verdict = (c) => {
  const v = releaseMarkerVerdict(c);
  return { writePin: v.writePin, exitCode: v.exitCode, message: v.message };
};
add("release_marker_verdict:proven", verdict({ want: "1.2.322", saw: "1.2.322", ok: true }));
add("release_marker_verdict:unproven", verdict({ want: "1.2.322", saw: "1.2.321", ok: false, waitedMs: 90_000 }));
add("release_marker_verdict:blind", verdict({ want: "1.2.322", saw: null, ok: false, waitedMs: 90_000 }));

// ── the PowerShell it generates, byte for byte ─────────────────────────────
add("desktop_aumid", DESKTOP_AUMID);
{
  const a = lnkIdentity("D:\\Summrise\\components\\summrise-desktop-electron\\icon.ico");
  add("lnk_identity", `${a.id}|${a.icon}`);
}
add(
  "desk_shortcut_repair_ps",
  deskShortcutRepairPs(
    "D:\\Summrise\\scripts",
    "D:\\Summrise\\components\\summrise-desktop-electron",
    "Write-Host",
  ).join("\n"),
);
add(
  "start_desktop_ps",
  startDesktopPs("D:\\Summrise\\components\\summrise-desktop-electron", "C:\\ProgramData\\Summrise\\logs").join("\n"),
);
add(
  "ensure_desktop_ps",
  ensureDesktopPs("D:\\Summrise\\scripts", "C:\\ProgramData\\Summrise\\logs").join("\n"),
);
add(
  "desktop_task_ps",
  desktopTaskPs("C:\\Program Files\\Summrise", "C:\\ProgramData\\Summrise\\logs").join("\n"),
);
add("desktop_start_ps", desktopStartPs("C:\\Summrise").join("\n"));
add("firewall_ps", firewallPs(7740).join("\n"));
add("boot_task_ps:false", bootTaskPs("C:\\V\\summrise-agent.exe", "C:\\V\\etc\\config.yaml", false).join("\n"));
add("boot_task_ps:true", bootTaskPs("C:\\V\\summrise-agent.exe", "C:\\V\\etc\\config.yaml", true).join("\n"));
add("migrate_layout_ps", migrateLayoutPs("D:\\Summrise", "C:\\ProgramData\\Summrise").join("\n"));
add("uninstall_reg_body_ps", uninstallRegBodyPs("D:\\Summrise", "1.2.307").join("\n"));
add("uninstall_version_ps", uninstallVersionPs("C:\\Program Files\\Summrise", "1.2.307").join("\n"));
add("uninstall_reg_body_ps:empty", uninstallRegBodyPs("D:\\Summrise", "").join("\n"));
add("playwright_probe_ps", playwrightProbePs().join("\n"));
for (const t of BOOT_TASKS) {
  for (const a of ["on", "off"]) add(`autostart_argv:${t}:${a}`, autostartArgv(t, a).join(" "));
}

process.stdout.write(JSON.stringify({ cases }));
