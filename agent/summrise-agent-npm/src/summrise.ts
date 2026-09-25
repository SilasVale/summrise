#!/usr/bin/env node
/**
 * summrise CLI — DSH-style management for the Summrise Agent.
 *
 * The agent is a headless auto-start service; management lives here (CLI)
 * and in the web panel (http://127.0.0.1:18080/panel/, desktop shortcut
 * created by setup). The native tray was retired 2026-08-22.
 */
import { spawn, spawnSync } from "child_process";
import * as crypto from "crypto";
import * as fs from "fs";
import * as os from "os";
import * as path from "path";

const EXE_SRC = path.join(__dirname, "..", "summrise-agent.exe");

// C1 (2026-08-28): the registry is the single source of truth for the install
// dir. Resolution: $env:SUMMRISE_AGENT_DIR → HKLM\SOFTWARE\Summrise\Agent\InstallDir
// → default. No legacy directory probing — the installer/setup always write
// the registry, and all commands use this one DIR.
function resolveDir() {
  if (process.env.SUMMRISE_AGENT_DIR) return process.env.SUMMRISE_AGENT_DIR;
  try {
    const out = spawnSync(
      "reg",
      ["query", "HKLM\\SOFTWARE\\Summrise\\Agent", "/v", "InstallDir"],
      { encoding: "utf8" },
    );
    if (out.status === 0 && out.stdout) {
      const m = /REG_SZ\s+(.+)/.exec(
        out.stdout.split(/\r?\n/).find((l) => l.includes("InstallDir")) || "",
      );
      if (m && m[1].trim()) return m[1].trim();
    }
  } catch {
    /* fall through */
  }
  return "C:\\Program Files\\Summrise";
}

const DIR = resolveDir();
const EXE_DST = path.join(DIR, "summrise-agent.exe");
// Layout v2 (ADR 0008): the install ROOT keeps only the service exe (+ its
// transient .new/.old) and the NSIS uninstaller. Everything else lives in
// one of these — leaf names unchanged (Electron packaging + task arguments
// are rename-sensitive; only the parent moves).
const ETC_DIR = path.join(DIR, "etc");
const COMPONENTS_DIR = path.join(DIR, "components");
const SCRIPTS_DIR = path.join(DIR, "scripts");
// DataDir mirrors resolveDir (registry DataDir, else %ProgramData%\Summrise) —
// runtime logs + evidence live there, never in program files.
function resolveDataDir() {
  try {
    const out = spawnSync(
      "reg",
      ["query", "HKLM\\SOFTWARE\\Summrise\\Agent", "/v", "DataDir"],
      { encoding: "utf8" },
    );
    if (out.status === 0 && out.stdout) {
      const m = /REG_SZ\s+(.+)/.exec(
        out.stdout.split(/\r?\n/).find((l) => l.includes("DataDir")) || "",
      );
      if (m && m[1].trim()) return m[1].trim();
    }
  } catch {
    /* fall through */
  }
  return path.join(process.env.ProgramData || "C:\\ProgramData", "Summrise");
}
/**
 * Write one HKLM value and VERIFY IT. The registry is the single source of truth for
 * path resolution — `paths.rs` on the Rust side and `resolveDataDir()` here both read it
 * — so a silent failure means the agent and this CLI can disagree about where the install
 * IS, and `summrise status` then prints the default dir and "panel: (not installed)" for an
 * install that succeeded somewhere else.
 *
 * It used to be `spawnSync("reg", [...], { stdio: "ignore" })` inside a `try` that could
 * never fire: spawnSync does NOT throw on a failing program, and stdio ignored suppressed
 * the only evidence.
 */
function regWrite(name: string, value: string): boolean {
  const r = spawnSync(
    "reg",
    [
      "add",
      "HKLM\\SOFTWARE\\Summrise\\Agent",
      "/v",
      name,
      "/t",
      "REG_SZ",
      "/d",
      value,
      "/f",
    ],
    { encoding: "utf8" },
  );
  if (r.status !== 0) {
    console.error(
      `setup: WARNING -- could not record ${name} in HKLM\\SOFTWARE\\Summrise\\Agent` +
        (r.stderr ? ` (${String(r.stderr).trim()})` : "") +
        " -- path resolution will fall back to the default",
    );
    return false;
  }
  return true;
}

const DATA_DIR = resolveDataDir();
const LOGS_DIR = path.join(DATA_DIR, "logs");
const CFG_FILE = path.join(ETC_DIR, "config.yaml");
const HOSTNAME_FILE = path.join(ETC_DIR, "summrise-agent.hostname");
const DESK_DIR = path.join(COMPONENTS_DIR, "summrise-desktop-electron");
const PW_DIR = path.join(COMPONENTS_DIR, "playwright");
const TASK = "SummriseAgent";
// Gateway API base — where the console endpoints live (tunnel-token /
// register). Overridable for staging.
const API_BASE = process.env.SUMMRISE_API_BASE || "https://api.saisi.online";

// POST JSON to the gateway, return parsed JSON or throw with the error text.
function apiPost(pathname, body) {
  const res = spawnSync(
    "curl",
    [
      "-sS",
      "-m",
      "30",
      "-X",
      "POST",
      "-H",
      "content-type: application/json",
      "-d",
      JSON.stringify(body),
      API_BASE + pathname,
    ],
    { encoding: "utf8" },
  );
  if (res.status !== 0)
    throw new Error("gateway unreachable: " + (res.stderr || "").trim());
  const out = (res.stdout || "").trim();
  try {
    return JSON.parse(out);
  } catch {
    throw new Error("gateway bad response: " + out.slice(0, 120));
  }
}

// ── `sh()` IS `cmd.exe /d /s /c "<string>"`, AND DOUBLE QUOTES DO NOT STOP `%` ──
//
// Quoting a value stops cmd from reading `&`, `|`, `>` and `^` as OPERATORS (that is
// the `psArgv` incident below). It does NOT stop PERCENT EXPANSION: cmd expands
// `%NAME%` inside a quoted region exactly as it does outside one, so an interpolated
// PATH that contains a `%` is still a variable reference rather than data. `%` is
// legal in an NTFS name, and the install/data dirs here are chosen by the operator,
// so this is reachable rather than theoretical.
//
// MEASURED on d1 (2026-09-25) through this exact `spawnSync(cmd, {shell:true})` path;
// each arrow is the child's own stdout:
//   `echo [%CMDCMDLINE%]`                 -> [C:\WINDOWS\system32\cmd.exe /d /s /c "echo [%CMDCMDLINE%]"]
//   `echo ["C:\%ProgramFiles%\Summrise"]` -> ["C:\C:\Program Files\Summrise"]   (rewritten)
//   shell  `powershell ... "Write-Output '%ProgramFiles%'"` -> C:\Program Files  (value lost)
//   argv   `powershell ...  Write-Output '%ProgramFiles%'`  -> %ProgramFiles%    (verbatim)
//
// AND THE BATCH-FILE ESCAPE IS THE WRONG FIX: cmd's `%%` -> `%` rule belongs to batch
// FILES. On a `/d /s /c` command line that same measurement prints `[100%%]` for
// `echo [100%%]` — the doubling is NOT collapsed, so "escaping" a value with `%%`
// would put a DOUBLED sign into the path, which is worse than the defect. What
// remains is argv (no cmd in the path at all — `ps()` below hands PowerShell its
// script directly) or a value that provably cannot contain `%`. A site that keeps a
// cmd line anyway says which of those it is in a `cmd-% <kind>: <reason>` comment,
// and test/cli.test.mjs fails any interpolating site that does not.
//
// `rmdir`, `taskkill`, `sc`, `schtasks` and `reg` cannot take argv (the first is a
// cmd BUILTIN — there is no rmdir.exe), so a path handed to one of those must reach
// cmd as a `%NAME%` reference rather than as text.
function sh(cmd, opts = {}) {
  return spawnSync(cmd, { shell: true, stdio: "inherit", ...opts });
}
/**
 * The argv for a one-shot PowerShell script — NO SHELL, and that is the point.
 *
 * `shell: true` routed this through cmd.exe, where `"` is a quote TOGGLE and
 * `\"` is not an escape at all. So cmd re-parsed the argument before PowerShell
 * ever saw it, and any character it treats specially became an OPERATOR.
 *
 * Observed on d1: the round-7 update receipt contains `1.2.322 -> 1.2.323`.
 * cmd saw the `>` and performed a REDIRECTION — the log line landed as
 * "update requested 1.2.322 - (CLI reached the device...)" (arrow and TARGET
 * VERSION gone, exactly the half that says what is being installed) and a stray
 * zero-byte file named `1.2.323` appeared in the working directory. The
 * receipt's whole purpose is to distinguish "the CLI ran but the swap did not"
 * from "nothing ran"; a receipt that cannot name its target is half a receipt.
 *
 * Passing argv straight to CreateProcess means PowerShell receives the script
 * VERBATIM and no quoting layer sits between the two. This is also what every
 * other spawn in this file already does (`spawnSync("reg", [...])`) — `ps()` was
 * the only one that shelled out.
 */
export function psArgv(script: string): string[] {
  return ["-NoProfile", "-Command", script];
}

function ps(script) {
  // npm audit #7: results were discarded — a failed Register-ScheduledTask
  // printed SUCCESS anyway. Return the spawn result.
  return spawnSync("powershell", psArgv(script), { stdio: "inherit" });
}
// Run a PowerShell script from a temp .ps1 FILE instead of -Command. The
// layout-migration script is ~10KB — past cmd.exe's 8191-char command-line
// limit — so `ps()` (which shells through cmd) failed it with "命令行太长"
// and the migration silently no-op'd. A file path is short, so the script
// length is then unbounded. This is a DIRECT child spawn (not the WMI
// handoff where -ExecutionPolicy Bypass dies silently on d1), so Bypass is
// safe here and sidesteps any Restricted execution policy on the box.
function psFile(script: string): { status: number | null } {
  const tmpDir = process.env.TEMP || process.env.TMP || "C:\\Windows\\Temp";
  const tmp = path.join(tmpDir, `summrise-mig-${process.pid}.ps1`);
  try {
    fs.writeFileSync(tmp, script, "utf8");
    return spawnSync(
      "powershell",
      ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", tmp],
      { encoding: "utf8" },
    );
  } catch (e: any) {
    console.log("setup: psFile failed (" + (e?.message || e) + ")");
    return { status: 1 };
  } finally {
    try {
      fs.unlinkSync(tmp);
    } catch {
      /* best-effort */
    }
  }
}
// npm audit #6: setup interpolated RAW paths into PS single-quote literals;
// an apostrophe in a path (O'Brien) unbalanced the literal and the script
// PARSE-failed invisibly. Shared doubling helper.
// exported: unit-tested in test/cli.test.mjs (SYSTEM-context PS quoting = injection surface)
export const psq = (x: string) => String(x).replace(/'/g, "''");
// stage-brand: healed desktop shortcut. A 2026-09-01 Summrise.lnk on devices
// points at the RETIRED Tauri summrise-desktop.exe (embedded stale icon) —
// double-clicking it launches the dead app instead of the Electron shell.
// Repair (only if the link already exists — headless installs must not
// sprout desktop icons): repoint at start-desktop.ps1 (the SummriseDesktop
// onlogon path) with IconLocation pinned to the sunrise icon.ico, and
// remove the retired Tauri/tray orphans nothing ships anymore.
// Best-effort + logged, never fatal. `sink` is a PS output pipe
// (e.g. Write-Host, or the update log pipe). Single-quoted PS literals
// only (npm audit #6) except the double quotes the .lnk Arguments path
// needs — callers passing through -Command "..." must backslash-escape
// them (see setup step 7); the update swap script runs from a file.
// exported: unit-tested in test/cli.test.mjs.
export function deskShortcutRepairPs(
  scriptsQ: string,
  deskDirQ: string,
  sink: string,
): string[] {
  return [
    `$dLnk = Join-Path $env:PUBLIC 'Desktop\\Summrise.lnk'`,
    `$dIco = '${deskDirQ}\\icon.ico'`,
    `$dPs1 = '${scriptsQ}\\start-desktop.ps1'`,
    `$dNeed = $false`,
    `if (Test-Path $dLnk) {`,
    `  try { $dEx = (New-Object -ComObject WScript.Shell).CreateShortcut($dLnk); if (($dEx.TargetPath -like '*summrise-desktop.exe') -or ($dEx.TargetPath -like '*summrise-tray.exe') -or (-not (Test-Path $dEx.TargetPath))) { $dNeed = $true } } catch { $dNeed = $true }`,
    `}`,
    `if ($dNeed -and (Test-Path $dIco) -and (Test-Path $dPs1)) {`,
    `  try { $dWs = New-Object -ComObject WScript.Shell; $dSc = $dWs.CreateShortcut($dLnk); $dSc.TargetPath = Join-Path $env:SystemRoot 'System32\\WindowsPowerShell\\v1.0\\powershell.exe'; $dSc.Arguments = '-NoProfile -ExecutionPolicy Bypass -File "' + $dPs1 + '"'; $dSc.WorkingDirectory = '${deskDirQ}'; $dSc.IconLocation = $dIco + ',0'; $dSc.Save(); 'desk: Summrise.lnk repointed to electron shell' | ${sink} } catch { ('desk: Summrise.lnk repair failed: ' + $_.Exception.Message) | ${sink} }`,
    `}`,
    `foreach ($dRx in @('summrise-desktop.exe','summrise-tray.exe')) { $dRp = '${deskDirQ}\\' + $dRx; if (Test-Path $dRp) { try { Remove-Item -Force -ErrorAction Stop $dRp; ('desk: removed retired ' + $dRx) | ${sink} } catch { ('desk: retired ' + $dRx + ' locked, kept') | ${sink} } } }`,
  ];
}
// exported: the start-desktop.ps1 launcher (the SummriseDesktop onlogon task +
// desktop Summrise.lnk both call it). Launches the Electron shell from
// components\summrise-desktop-electron\ with the working directory set there
// (electron . resolves src/main.js via package.json main). The script is
// intentionally tiny + ASCII-only (system-locale PS). unit-tested.
export function startDesktopPs(deskDirQ: string): string[] {
  return [
    `$dir = '${deskDirQ}'`,
    `Set-Location $dir`,
    `& "$dir\\node_modules\\electron\\dist\\electron.exe" .`,
  ];
}

/** The desktop shell's watchdog pair + its task, as ONE PowerShell script.
 *
 *  WHY IT IS A FILE AND NOT AN INLINE -Command: it has to write two other files whose
 *  contents contain quotes, and every attempt to do that inside one command string turns
 *  into quoting hell — the first version of this shipped as a hand-written file for that
 *  reason. `summrise setup` writes it and `summrise desktop` runs it, so a machine that
 *  never ran setup can still get a working task by asking for the window. Idempotent:
 *  ensure-desktop.ps1 exits when electron is already alive, so the 5-minute trigger never
 *  steals focus. unit-tested. */
export function desktopTaskPs(installQ: string): string[] {
  return [
    "# written by `summrise setup` / `summrise desktop` -- the desktop shell's task + watchdog.",
    "$ErrorActionPreference = 'Stop'",
    `$q = '${installQ}'`,
    `$en = Join-Path $q 'scripts\\ensure-desktop.ps1'`,
    `$vb = Join-Path $q 'scripts\\desktop-pulse.vbs'`,
    `Set-Content -Path $en -Value ('if (Get-Process electron -ErrorAction SilentlyContinue) { exit }; & powershell -NoProfile -ExecutionPolicy Bypass -File "' + $q + '\\scripts\\start-desktop.ps1"') -Force`,
    `Set-Content -Path $vb -Value ('CreateObject("WScript.Shell").Run "powershell -NoProfile -ExecutionPolicy Bypass -File " & Chr(34) & "' + $en + '" & Chr(34), 0, False') -Force`,
    `$da = New-ScheduledTaskAction -Execute 'wscript.exe' -Argument ('"' + $vb + '"') -WorkingDirectory $q`,
    `$dt1 = New-ScheduledTaskTrigger -AtLogOn`,
    `$dw1 = New-ScheduledTaskTrigger -Once -At (Get-Date).AddMinutes(3) -RepetitionInterval (New-TimeSpan -Minutes 5)`,
    `$pr = New-ScheduledTaskPrincipal -UserId ('{0}\\{1}' -f $env:USERDOMAIN, $env:USERNAME) -LogonType Interactive -RunLevel Highest`,
    `$st = New-ScheduledTaskSettingsSet -ExecutionTimeLimit (New-TimeSpan -Minutes 10) -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable -MultipleInstances IgnoreNew`,
    `Register-ScheduledTask SummriseDesktop -Action $da -Trigger @($dt1,$dw1) -Principal $pr -Settings $st -Force | Out-Null`,
    `Start-ScheduledTask -TaskName SummriseDesktop`,
  ];
}

/** The desktop shell's START script, as a file — NOT a `-Command` string.
 *
 *  WHY A FILE, and this is the third thing the device taught us: the CLI spawns with
 *  `shell: true`, so every command goes through cmd.exe, and **cmd treats `&` as a command
 *  separator**. The PowerShell call operator `&` therefore split the command in half, the
 *  `$t` assignment never ran, and the command fell through to re-registering the task —
 *  which then failed on the service account's principal. One metacharacter, two unrelated
 *  looking failures. `powershell -File "<path>"` has no metacharacters to mangle, and the
 *  operator can READ what the command does.
 *
 *  Prints ONE WORD the CLI can trust: `already-running`, `started`, or `not-started`.
 *  Pure; the CLI writes it and runs it. unit-tested. */
export function desktopStartPs(installQ: string): string[] {
  return [
    "# written by `summrise desktop` -- ask for the window, or say it is already there.",
    "$ErrorActionPreference = 'Stop'",
    "if (Get-Process electron -ErrorAction SilentlyContinue) { Write-Output 'already-running'; exit 0 }",
    "# DO NOT RE-REGISTER THE TASK TO START IT. Register-ScheduledTask needs the interactive",
    "# user's principal as DOMAIN\\user, and a shell running as a service account has no such",
    '# mapping -- the device answered "No mapping between account names and security IDs was',
    '# done ... UserId" from a PTY running as systemprofile, while the task itself was Ready.',
    "# The task already carries the right principal -- `summrise setup` creates it -- so starting",
    "# it is all that is needed.",
    "$t = Get-ScheduledTask -TaskName SummriseDesktop -ErrorAction SilentlyContinue",
    `if (-not $t) { & powershell -NoProfile -ExecutionPolicy Bypass -File "${installQ}\\scripts\\register-desktop-task.ps1" }`,
    "Start-ScheduledTask -TaskName SummriseDesktop -ErrorAction SilentlyContinue",
    "Start-Sleep -Seconds 3",
    "if (Get-Process electron -ErrorAction SilentlyContinue) { Write-Output 'started' } else { Write-Output 'not-started'; exit 1 }",
  ];
}
// exported: agent bind port plumbing (custom-port installs). server.port
// out of <dir>/config.yaml (first `port:` under top-level `server:`),
// canonical 18080 when absent/invalid/missing (fresh installs have no
// config yet — the agent writes defaults on first boot). Pure core
// unit-tested; agentPort() is the thin fs wrapper. unit-tested in
// test/cli.test.mjs.
export function parseAgentPort(yamlText: string): number | null {
  let inServer = false;
  for (const raw of String(yamlText || "").split(/\r?\n/)) {
    const line = raw.replace(/\s+$/, "");
    if (/^\S/.test(line)) inServer = /^server\s*:/.test(line);
    if (!inServer) continue;
    const m = /^\s*port\s*:\s*"?(\d{1,5})"?\s*(#.*)?$/.exec(line);
    if (m) {
      const n = Number(m[1]);
      return Number.isInteger(n) && n > 0 && n < 65536 ? n : null;
    }
  }
  return null;
}
// The device token, read from etc\config.yaml the same way the port is: a line
// scan, no YAML dependency. Absent means "cannot talk to the device API", which
// the callers report as a state rather than as a crash.
export function parseDeviceToken(yamlText) {
  let inServer = false;
  for (const raw of String(yamlText || "").split(/\r?\n/)) {
    const line = raw.replace(/\s+$/, "");
    if (/^\S/.test(line)) inServer = /^server\s*:/.test(line);
    if (!inServer) continue;
    const m = /^\s*device_token\s*:\s*"?([A-Za-z0-9._-]+)"?\s*(#.*)?$/.exec(
      line,
    );
    if (m) return m[1];
  }
  return null;
}
function deviceToken(dir) {
  try {
    return parseDeviceToken(
      fs.readFileSync(path.join(dir, "config.yaml"), "utf8"),
    );
  } catch {
    return null;
  }
}
// `host:port[/path]` -> the target id the device uses. A path is part of the
// identity, so `192.168.1.1:80/` and `192.168.1.1:80` are two different checks.
export function parseTargetArg(arg) {
  const s = String(arg || "").trim();
  const m = /^([^\s/:]+):(\d{1,5})(\/.*)?$/.exec(s);
  if (!m) return null;
  const port = Number(m[2]);
  if (!Number.isInteger(port) || port < 1 || port > 65535) return null;
  return {
    host: m[1],
    port,
    path: m[3] || "",
    id: `${m[1]}:${port}${m[3] || ""}`,
  };
}
/**
 * JSON that survives the command line.
 *
 * `deviceApi` hands its body to `curl -d <string>`, and on Windows that argument is encoded in the
 * process's ANSI code page — NOT UTF-8. So a note typed with an em dash reached the device as
 * mojibake: the CLI echoed back what it had been GIVEN ("— I rebooted it"), while the stored copy
 * read `鈥?` everywhere the device's own data was shown (list, report, --json). Caught on d1 by
 * writing a note with a dash in it and reading it back from the device.
 *
 * Escaping every non-ASCII character as \uXXXX makes the body pure ASCII, which no code page can
 * mangle, and `JSON.parse` on the device restores the original text exactly.
 */
export function asciiJson(value, indent?) {
  return JSON.stringify(value, null, indent).replace(
    /[\u007f-\uffff]/g,
    (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"),
  );
}

// The device's own API on loopback, with the token from etc\config.yaml.
export function deviceApi(method, pathname, body?) {
  const dir = ETC_DIR;
  const token = deviceToken(dir);
  if (!token)
    return {
      ok: false,
      error: "no device token in " + path.join(dir, "config.yaml"),
    };
  const port = agentPort(dir);
  const args = [
    "-sS",
    "-m",
    "15",
    "-X",
    method,
    "-H",
    "Authorization: Bearer " + token,
  ];
  if (body !== undefined) {
    args.push("-H", "content-type: application/json", "-d", asciiJson(body));
  }
  args.push(`http://127.0.0.1:${port}${pathname}`);
  const r = spawnSync("curl", args, { encoding: "utf8", timeout: 20000 });
  if (r.error || r.status !== 0) {
    return {
      ok: false,
      error:
        `device unreachable on 127.0.0.1:${port}` +
        (r.error ? ` (${r.error.message})` : ""),
    };
  }
  try {
    return { ok: true, body: JSON.parse(String(r.stdout || "").trim()) };
  } catch {
    return { ok: false, error: "device sent something that is not JSON" };
  }
}
/**
 * A duration in the shapes the rest of the panel uses: `45s`, `4m`, `1h 04m`, `2d 4h`.
 *
 * IT USED TO CARRY FOURTEEN LINES ABOUT `summrise report`, a verb pruned in round 25 — including "THE
 * RENDER IS PURE (`reportText`)", naming a function that occurs nowhere else in this file. The doc
 * described a command an operator cannot run, on a function whose actual job went unsaid. Two rules from
 * that block are worth keeping, and this is not the place they belong: a server that did not answer is
 * printed as NOT READ, never as an invented value, and a target that is down brings its outage log.
 */
export function fmtDuration(ms) {
  const s = Math.max(0, Math.floor(Number(ms || 0) / 1000));
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  const h = Math.floor(s / 3600);
  return h >= 24
    ? `${Math.floor(h / 24)}d ${h % 24}h`
    : `${h}h ${String(Math.floor((s % 3600) / 60)).padStart(2, "0")}m`;
}

export function targetLine(t, nowMs, width = 30) {
  const s = t.summary || {};
  const up = s.up_now === true;
  const state = up ? "up" : s.up_now === false ? "down" : "no readings";
  const id = String(t.id || "");
  const since = s.since_ms ? ` ${fmtDuration(nowMs - s.since_ms)}` : "";
  const status =
    s.last_status === null || s.last_status === undefined
      ? ""
      : `  HTTP ${s.last_status}`;
  // A content check that did not find its text is the case a status code cannot express, so it
  // is printed as its own word next to the code.
  const match =
    s.last_expect_ok === false
      ? "  no match"
      : s.last_expect_ok === true
        ? "  matches"
        : "";
  const lat = s.latency ? `  ${s.latency.avg}ms avg` : "";
  const pct =
    s.up_pct === null || s.up_pct === undefined ? "" : `  ${s.up_pct}% up`;
  const drops = s.drops
    ? `  ${s.drops} ${s.drops === 1 ? "drop" : "drops"}`
    : "";
  // The operator's own words belong on the line they explain, not in a separate view.
  const note = t.note && t.note.text ? `  — ${t.note.text}` : "";
  return `${up ? "UP  " : s.up_now === false ? "DOWN" : "?   "} ${id.padEnd(width)} ${state}${since}${status}${match}${lat}${pct}${drops}${note}`;
}
// ── MACHINE-READABLE OUTPUT ────────────────────────────────────────────────
// `summrise monitor list --json` prints the DEVICE'S OWN ANSWER,
// projected rather than re-derived: the summaries, the transitions and the samples are the ones
// `/api/monitors` returns, so a script and the panel (and the AI) can never disagree about whether
// a target is up. The CLI adds only what the device cannot know — which device answered, and when
// this script asked — under names that say so.
//
// A STABLE SHAPE, not a passthrough: `{device, asked_at_ms, interval_secs, targets:[…]}`. Anything
// a script needs to branch on is in it; anything that is presentation (the coloured line, the
// outage wording) is not.
export function monitorsJson({ device, askedAtMs, payload, only }) {
  const targets = ((payload && payload.targets) || [])
    .filter((t) => !only || t.id === only)
    .map((t) => ({
      // EVERY field is present in every row, as `null` when the device did not send it: a
      // field that vanishes from the JSON is a field a script cannot tell from `false`, and
      // `JSON.stringify` drops `undefined` keys silently (which is how the first version of
      // this shipped `up_pct` missing for a target that had never been probed).
      id: t.id === undefined ? null : t.id,
      host: t.host === undefined ? null : t.host,
      port: t.port === undefined ? null : t.port,
      path: t.path === undefined ? null : t.path,
      expect: t.expect === undefined ? null : t.expect,
      // The device's numbers, verbatim. `up: null` means "not read yet" and is NOT false.
      up: t.summary && t.summary.up_now !== undefined ? t.summary.up_now : null,
      up_pct:
        t.summary && t.summary.up_pct !== undefined ? t.summary.up_pct : null,
      since_ms:
        t.summary && t.summary.since_ms !== undefined
          ? t.summary.since_ms
          : null,
      drops:
        t.summary && t.summary.drops !== undefined ? t.summary.drops : null,
      latency_ms: t.summary && t.summary.latency ? t.summary.latency.avg : null,
      last_status:
        t.summary && t.summary.last_status !== undefined
          ? t.summary.last_status
          : null,
      last_expect_ok:
        t.summary && t.summary.last_expect_ok !== undefined
          ? t.summary.last_expect_ok
          : null,
      probes:
        t.summary && t.summary.probes !== undefined ? t.summary.probes : null,
      transitions: (t.transitions || []).map((x) => ({
        at_ms: x.at_ms,
        up: x.up,
        lasted_ms: x.lasted_ms,
      })),
    }));
  return {
    device: device || "",
    asked_at_ms: askedAtMs,
    interval_secs: (payload && payload.interval_secs) || null,
    targets,
  };
}

// ONE PROBE'S ANSWER, as a line — the terminal's version of what the device's `monitor_probe`
// returns. Pure, because the wording is the feature: a DOWN probe says WHICH way it failed
// (no connection / status 500 / 200 without the expected text), not merely that it failed.
export function probeLine(target, probe, nowMs) {
  const id = String((target && target.id) || "");
  const ok = probe && probe.ok === true;
  const state = ok ? "UP  " : "DOWN";
  const bits = [];
  if (probe && probe.status !== null && probe.status !== undefined)
    bits.push(`HTTP ${probe.status}`);
  // A content check is the one failure a status code cannot express, so it is named.
  if (target && target.expect) {
    if (probe && probe.expect_ok === false)
      bits.push(`no match for "${target.expect}"`);
    else if (probe && probe.expect_ok === true)
      bits.push(`matches "${target.expect}"`);
    else bits.push(`could not read the body to look for "${target.expect}"`);
  }
  if (probe && typeof probe.ms === "number") bits.push(`${probe.ms}ms`);
  if (!ok && bits.length === 0) bits.push("no answer");
  return `${state} ${id}${bits.length ? "  " + bits.join("  ") : ""}`;
}

// The transitions a target has been through, newest first — the outage log an
// operator pastes into a report.
export function agentPort(dir: string): number {
  try {
    return (
      parseAgentPort(fs.readFileSync(path.join(dir, "config.yaml"), "utf8")) ??
      18080
    );
  } catch {
    return 18080;
  }
}
// exported: idempotent Windows firewall inbound rule for the agent port
// (LAN clients are dropped at the firewall otherwise, even bound 0.0.0.0).
// ASCII-only PS, plain statements (WMI/session-0 rule). Prunes our own
// stale-port rules, never foreign ones (DisplayName-scoped). unit-tested.
export function firewallPs(port: number): string[] {
  return [
    `$fwPort = ${port};`,
    `foreach ($fr in @(Get-NetFirewallRule -DisplayName 'Summrise Agent' -ErrorAction SilentlyContinue)) { try { $fp = @(Get-NetFirewallPortFilter -AssociatedNetFirewallRule $fr | Select-Object -ExpandProperty LocalPort); if ($fp -notcontains "$fwPort") { Remove-NetFirewallRule -Name $fr.Name -Confirm:$false -ErrorAction SilentlyContinue } } catch {} }`,
    `if (-not (Get-NetFirewallRule -DisplayName 'Summrise Agent' -ErrorAction SilentlyContinue | Where-Object { @(Get-NetFirewallPortFilter -AssociatedNetFirewallRule $PSItem | Select-Object -ExpandProperty LocalPort) -contains "$fwPort" })) { New-NetFirewallRule -DisplayName 'Summrise Agent' -Direction Inbound -LocalPort $fwPort -Protocol TCP -Action Allow | Out-Null }`,
  ];
}
// exported: SummriseAgent boot-task registration (SYSTEM, hardened). The task's
// -Argument is the EXPLICIT config path (layout v2: etc\config.yaml) — never
// the exe path (Rust takes argv[1] as the config FILE; an exe path fails
// YAML parse and quarantines the install). Shared by setup (fresh install)
// and the update swap (repoint, fail-closed — the repoint runs BEFORE any
// swap, and a config-path argument boots old AND new agents alike, so a
// repoint failure aborts the update with the old version still running).
// `start` appends the kick for setup; the swap omits it (it restarts the
// task itself after the swap). ASCII-only PS. unit-tested.
export function bootTaskPs(
  exeQ: string,
  cfgQ: string,
  start = false,
): string[] {
  const lines = [
    `$action = New-ScheduledTaskAction -Execute '${exeQ}' -Argument ('"' + '${cfgQ}' + '"')`,
    "$boot = New-ScheduledTaskTrigger -AtStartup",
    "$watch = New-ScheduledTaskTrigger -Once -At (Get-Date).AddMinutes(3) -RepetitionInterval (New-TimeSpan -Minutes 5)",
    "$principal = New-ScheduledTaskPrincipal -UserId SYSTEM -LogonType ServiceAccount -RunLevel Highest",
    "$settings = New-ScheduledTaskSettingsSet -ExecutionTimeLimit (New-TimeSpan -Seconds 0) -RestartCount 8 -RestartInterval (New-TimeSpan -Minutes 1) -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable",
    "Register-ScheduledTask SummriseAgent -Action $action -Trigger @($boot,$watch) -Principal $principal -Settings $settings -Force | Out-Null",
  ];
  if (start) lines.push("Start-ScheduledTask SummriseAgent");
  return lines;
}
// exported: layout-v2 one-time migration (ADR 0008) for the setup/update
// paths. Mirrors paths.rs migration_moves EXACTLY (same pairs — both sides
// pinned by tests; drift strands upgraded devices). Moves only when the
// target is missing (never clobbers staged .new output); dirs merge
// children. Best-effort per item; callers GATE on etc\config.yaml +
// hostname afterwards (fail-closed). Marker aging (same contract as
// paths.rs): skip entirely when etc\.layout-v2 exists; write it only when
// nothing is pending (old home exists, new missing). q = escaped DIR,
// dq = escaped DataDir. ASCII-only PS. unit-tested.
export function migrateLayoutPs(q: string, dq: string): string[] {
  const mvf = (oldRel: string, newAbs: string) =>
    `if ($summriseMg -and (Test-Path '${q}\\${oldRel}') -and (-not (Test-Path '${newAbs}'))) { try { New-Item -ItemType Directory -Force -Path (Split-Path '${newAbs}') | Out-Null; Move-Item -Force -Path '${q}\\${oldRel}' -Destination '${newAbs}' -ErrorAction Stop } catch {} }`;
  const mvd = (oldRel: string, newAbs: string) =>
    `if ($summriseMg -and (Test-Path '${q}\\${oldRel}')) { try { if (-not (Test-Path '${newAbs}')) { New-Item -ItemType Directory -Force -Path (Split-Path '${newAbs}') | Out-Null; Move-Item -Path '${q}\\${oldRel}' -Destination '${newAbs}' -ErrorAction Stop } else { Get-ChildItem -Force '${q}\\${oldRel}' | ForEach-Object { if (-not (Test-Path (Join-Path '${newAbs}' $_.Name))) { Move-Item -Force -Path $_.FullName -Destination (Join-Path '${newAbs}' $_.Name) -ErrorAction SilentlyContinue } } } } catch {} }`;
  const etc = `${q}\\etc`;
  const comp = `${q}\\components`;
  const scr = `${q}\\scripts`;
  const logs = `${dq}\\logs`;
  // (oldRel, newAbs, kind) pairs — 'f' files move atomically, 'd' dirs
  // merge children. The pending check covers EVERY pair exactly once with
  // paths.rs' rule (old && !new); a merged dir whose target EXISTS is not
  // pending even with locked leftovers behind them (garbage for uninstall).
  const moves: Array<[string, string, "f" | "d"]> = [
    ["config.yaml", `${etc}\\config.yaml`, "f"],
    ["summrise-agent.hostname", `${etc}\\summrise-agent.hostname`, "f"],
    ["tunnel.yml", `${etc}\\tunnel.yml`, "f"],
    [".summrise-release", `${etc}\\.summrise-release`, "f"],
    ["boxed-versions.json", `${etc}\\boxed-versions.json`, "f"],
    ["tools\\node", `${comp}\\node`, "d"],
    ["tools\\npm-global", `${comp}\\npm-global`, "d"],
    ["tools\\cloudflared.exe", `${comp}\\cloudflared.exe`, "f"],
    ["playwright", `${comp}\\playwright`, "d"],
    ["summrise-desktop-electron", `${comp}\\summrise-desktop-electron`, "d"],
    ["ensure-desktop.ps1", `${scr}\\ensure-desktop.ps1`, "f"],
    ["desktop-pulse.vbs", `${scr}\\desktop-pulse.vbs`, "f"],
    ["start-desktop.ps1", `${scr}\\start-desktop.ps1`, "f"],
    ["summrise-online-setup.ps1", `${scr}\\summrise-online-setup.ps1`, "f"],
    ["fix-tunnel.ps1", `${scr}\\fix-tunnel.ps1`, "f"],
    ["playwright\\run-hidden.vbs", `${scr}\\run-hidden.vbs`, "f"],
    ["playwright\\playwright-probe.ps1", `${scr}\\playwright-probe.ps1`, "f"],
    ["shell-integration", `${scr}\\shell-integration`, "d"],
    ["installer.log", `${logs}\\installer.log`, "f"],
    ["install-result.txt", `${logs}\\install-result.txt`, "f"],
    ["summrise-update.log", `${logs}\\summrise-update.log`, "f"],
    ["agent.log", `${logs}\\agent.log`, "f"],
    ["startup.log", `${logs}\\startup.log`, "f"],
    ["pwout", `${dq}\\pwout`, "d"],
  ];
  const pending = moves
    .map(
      ([o, n]) => `((Test-Path '${q}\\${o}') -and (-not (Test-Path '${n}')))`,
    )
    .join(" -or ");
  // All statements stay SINGLE-LINE (setup passes them joined with "; "
  // through -Command): the marker guard is precomputed into $summriseMg and
  // every line carries it, instead of wrapping the block in braces.
  return [
    `$summriseMg = (-not (Test-Path '${etc}\\.layout-v2'))`,
    // A running boxed node locks the playwright tree — stop DIR-local ones
    // first (setup precedent; the updater itself runs from npm-global,
    // which never matches the playwright filter).
    `if ($summriseMg) { Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'node.exe' -and $_.CommandLine -like '*${q}*playwright*' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue } }`,
    ...moves.map(([o, n, k]) => (k === "d" ? mvd(o, n) : mvf(o, n))),
    `if ($summriseMg -and (-not (${pending}))) { try { New-Item -ItemType Directory -Force -Path '${etc}' | Out-Null; New-Item -ItemType File -Force -Path '${etc}\\.layout-v2' | Out-Null } catch {} }`,
  ];
}
// exported: Add/Remove-Programs version parity. The NSIS installer writes
// DisplayVersion once at install time, but `summrise update` swaps the exe
// out-of-band — without this the control-panel entry shows the ORIGINAL
// version forever and misleads troubleshooting. Same round-298 discipline
// as .summrise-release: the caller splices these lines right after the marker
// write, gated on $ok (a failed swap must not move the version), wrapped
// in try/catch (best-effort — a registry failure must never fail the
// update). Creates the key when missing (npm-only installs never had one)
// but never fabricates UninstallString (NSIS owns it; npm uninstall is
// `summrise uninstall`). q = single-quote-escaped install dir, ver = release
// version. ASCII-only PS. unit-tested.
// exported: Add/Remove-Programs entry body (shared by setup + the update
// swap). Writes DisplayVersion/DisplayName/InstallLocation/Publisher always;
// writes UninstallString ONLY when absent — NSIS installs own theirs
// ($INSTDIR\uninstall.exe) and it must never be overwritten. The fallback
// value relaunches summrise.cmd ELEVATED (control panel does not elevate for
// us; without RunAs the uninstall dies on HKLM/schtasks with access
// denied), preferring components\npm-global (layout v2) then the legacy
// tools\ path. Best-effort try/catch throughout — a registry failure must
// never fail install/update. Empty ver = no-op. ASCII-only PS. unit-tested.
export function uninstallRegBodyPs(q: string, ver: string): string[] {
  if (!ver) return [];
  return [
    `try {`,
    `  $rk = 'HKLM:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\SummriseAgent'`,
    `  if (-not (Test-Path $rk)) { New-Item -Path $rk -Force | Out-Null }`,
    `  Set-ItemProperty -Path $rk -Name DisplayVersion -Value '${ver}' -ErrorAction Stop`,
    `  Set-ItemProperty -Path $rk -Name DisplayName -Value 'Summrise Agent ${ver}' -ErrorAction Stop`,
    `  Set-ItemProperty -Path $rk -Name InstallLocation -Value '${q}' -ErrorAction Stop`,
    `  Set-ItemProperty -Path $rk -Name Publisher -Value 'Summrise' -ErrorAction Stop`,
    `  $uv = '${q}\\components\\npm-global\\summrise.cmd'`,
    `  if (-not (Test-Path $uv)) { $uv = '${q}\\tools\\npm-global\\summrise.cmd' }`,
    `  if ((Test-Path $uv) -and (-not (Get-ItemProperty -Path $rk -Name UninstallString -ErrorAction SilentlyContinue))) { Set-ItemProperty -Path $rk -Name UninstallString -Value ('powershell.exe -NoProfile -ExecutionPolicy Bypass -Command "Start-Process -FilePath ''' + $uv + ''' -ArgumentList ''uninstall'' -Verb RunAs -Wait"') -ErrorAction Stop }`,
    `} catch {}`,
  ];
}
export function uninstallVersionPs(q: string, ver: string): string[] {
  if (!ver) return [];
  return [`if ($ok -and '${ver}') {`, ...uninstallRegBodyPs(q, ver), `}`];
}
// exported: autostart (boot) switch for the two scheduled tasks. SummriseAgent
// (SYSTEM service task) + SummriseDesktop (logon shell task) ARE the autostart
// surface — `summrise stop` only Ends the running instance and the 5-min
// watchdog revives it, so stop != opting out of autostart. This flips the
// task ENABLED flag itself. /ENABLE|/DISABLE need no credentials, unlike
// trigger edits which prompt for the /ru password interactively (and hang
// the caller) — never add /RI /RU /RP /TR here. unit-tested.
export const BOOT_TASKS = ["SummriseAgent", "SummriseDesktop"];
export function autostartArgv(task: string, action: "on" | "off"): string[] {
  return [
    "schtasks",
    "/Change",
    "/TN",
    task,
    action === "on" ? "/ENABLE" : "/DISABLE",
  ];
}
// exported: the SummrisePlaywright probe launcher (playwright-probe.ps1).
// Probe order matches the agent's preferred_cdp_endpoint(): 9333
// (Electron DESKTOP embedded view — what the user watches) when up, else
// private --headless. --output-dir pins MCP screenshots where the Evidence
// drawer lists them (install\pwout). ASCII-only, plain -NoProfile -File
// (the repo rule: -ExecutionPolicy Bypass / -EncodedCommand die silently
// under WMI/session-0 launches). unit-tested in test/cli.test.mjs.
export function playwrightProbePs(): string[] {
  return [
    "param([string]$node, [string]$cli)",
    "$ErrorActionPreference = 'Continue'",
    "$pwout = Join-Path (Split-Path $node -Parent) '..\\pwout'",
    "if (!(Test-Path $pwout)) { New-Item -ItemType Directory -Path $pwout -Force | Out-Null }",
    "$ep = ''",
    "function Test-Port([int]$port) {",
    "  try {",
    "    $c = New-Object System.Net.Sockets.TcpClient",
    "    $iar = $c.BeginConnect('127.0.0.1', $port, $null, $null)",
    "    if ($iar.AsyncWaitHandle.WaitOne(1500)) { return $c.Connected }",
    "    $c.Close()",
    "  } catch { }",
    "  return $false",
    "}",
    // Boot race: the task can fire before the desktop's CDP is up (Electron
    // starts at logon, later than the task). A single check then forks a
    // private headless chromium nobody sees (device-caught: detached 9229
    // serving about:blank while the user watched the embedded view). Wait
    // up to ~60s for 9333 before falling back to headless.
    "for ($i = 1; $i -le 12; $i++) { if (Test-Port 9333) { $ep = 'http://127.0.0.1:9333'; break }; Start-Sleep -Seconds 5 }",
    "if ($ep) {",
    "  & $node $cli --port 9229 --host 127.0.0.1 --cdp-endpoint $ep --output-dir $pwout --ignore-https-errors --allowed-hosts '127.0.0.1:9229,localhost:9229'",
    "} else {",
    "  & $node $cli --port 9229 --browser chromium --host 127.0.0.1 --headless --output-dir $pwout --ignore-https-errors --allowed-hosts '127.0.0.1:9229,localhost:9229'",
    "}",
  ];
}
// exported: the update mutual-exclusion window (npm audit #10 seam), unit-tested.
//
// TEN MINUTES, AND IT MUST EQUAL THE AGENT'S `BUSY_STALE_SECS`. It did not: this
// reclaimed at ten minutes while `agent/src/plugins/update/tools.rs` refused for
// an hour, so at eleven minutes this side OVERWROTE a marker the agent still
// honoured and a CLI update could start alongside a console-launched one —
// interleaving `Copy-Item` on `*.new`, the half-written-exe hazard this marker
// exists to prevent (npm audit #10). The agent now uses ten minutes too, and
// `test/cli.test.mjs` pins the two numbers against each other, because no test
// inside either language can see the other's.
export function busyIsFresh(mtimeMs: number, nowMs: number): boolean {
  return nowMs - mtimeMs < 10 * 60 * 1000;
}

// ── The update receipt and the status report ────────────────────────────────
//
// INCIDENT (round 17 → 18). A device update was issued through the sanctioned
// flow; the connection dropped, which is the DOCUMENTED behaviour of a
// SUCCESSFUL swap ("the terminal connection DROPS for ~10 s mid-update"). It was
// therefore read as "the swap started". It had not: the device held no
// update-busy marker, no staged summrise-agent.new.exe, no scripts\summrise-update.ps1
// and no `update start` line in summrise-update.log — the command never reached the
// device, and a TRANSPORT failure was indistinguishable from the success signal.
// Learning the truth meant reading four things by hand.
//
// Two seams close that, and they are different seams on purpose:
//   * `updateReceiptPs` — written by the CLI BEFORE the handoff, into the SAME
//     log the swap script appends to. So one file answers both questions:
//     "update requested" present + "update start" absent ⇒ the CLI ran and the
//     swap never launched; NEITHER present ⇒ the command never ran at all.
//     Without it both cases look identical (an empty log) — the incident.
//   * `statusReport` — the question "where is this device, and is a swap still
//     pending" in ONE command, instead of four hand reads.
//
// The marker's three states are deliberately distinguished, because they mean
// different things and only one of them is an error:
//   absent       → nothing pending (a finished swap clears it)
//   fresh (<10m) → a swap is running right now
//   stale (≥10m) → an update STARTED AND DID NOT FINISH — the incident's shape
export interface StatusFacts {
  /** `null` when the process list could not be READ — which is NOT "stopped". The
   *  neighbours follow the same rule ("null is NOT 'up to date'"). */
  agentRunning: boolean | null;
  installDir: string;
  exeExists: boolean;
  port: number;
  /** Contents of etc\.summrise-release, trimmed; null when absent/unreadable. */
  releaseVersion: string | null;
  /** mtime of the update-busy marker, or null when there is no marker. */
  updateMarkerMs: number | null;
  /** The marker EXISTS but could not be read (EACCES/EBUSY). `updateMarkerMs === null`
   *  then means "could not look", NOT "nothing in flight" — the same rule the fields
   *  around it follow, and the reason `rollback --clear` had to be fixed too. */
  updateMarkerUnreadable: boolean;
  /** This CLI's own version — what `summrise update` would install. */
  packageVersion: string;
  /** The newest release the CDN advertises, or null when it could not be read.
   *  `null` is NOT "up to date" — see the drift line. */
  latestVersion: string | null;
  nowMs: number;
}

/**
 * The newest version the release CDN advertises, or `null` when it cannot be
 * read. Never throws and never guesses — the caller renders `null` as "could not
 * be checked", because a silent failure here is indistinguishable from "current".
 *
 * Uses `curl` (what `summrise rollback` already uses for its HEAD check) rather than
 * a Node HTTP client: it inherits the proxy and TLS store the rest of the CLI
 * relies on, and a 3 s cap keeps `status` from hanging on a bad network.
 */
export function latestCdnVersion(): string | null {
  const base = (
    process.env.SUMMRISE_CDN || "https://agent.saisi.online"
  ).replace(/\/+$/, "");
  const r = spawnSync("curl", ["-s", "-m", "3", `${base}/api/version`], {
    encoding: "utf8",
    timeout: 5000,
  });
  if (r.status !== 0 || !r.stdout) return null;
  try {
    const j = JSON.parse(r.stdout);
    const v = j && typeof j.version === "string" ? j.version.trim() : "";
    return v || null;
  } catch {
    // A body that is not JSON is NOT a version — the same rule the gateway's
    // tool path had to learn.
    return null;
  }
}

/**
 * The registry's own answer to "what is latest", asked in the smallest way npm
 * offers: `/-/package/<name>/dist-tags` is a few bytes, where the packument is a
 * document. Same 3 s cap and the same rule as the CDN read — a failure is
 * `null`, which the report renders as "could NOT be checked", never as
 * agreement.
 */
export function latestNpmVersion(): string | null {
  const r = spawnSync(
    "curl",
    [
      "-s",
      "-m",
      "3",
      "https://registry.npmjs.org/-/package/summrise-agent/dist-tags",
    ],
    { encoding: "utf8", timeout: 5000 },
  );
  if (r.status !== 0 || !r.stdout) return null;
  try {
    const j = JSON.parse(r.stdout);
    const v = j && typeof j.latest === "string" ? j.latest.trim() : "";
    // A dist-tag can point at a prerelease; the device's update path only
    // understands x.y.z, and a tag it cannot parse is not a version.
    return /^\d+\.\d+\.\d+$/.test(v) ? v : null;
  } catch {
    return null;
  }
}

/** The newer of two release versions; either may be unknown. */
export function newestOf(a: string | null, b: string | null): string | null {
  if (!a) return b;
  if (!b) return a;
  const ta = versionTriple(a);
  const tb = versionTriple(b);
  if (!ta) return b;
  if (!tb) return a;
  for (let i = 0; i < 3; i++) {
    if (ta[i] !== tb[i]) return ta[i] > tb[i] ? a : b;
  }
  return a;
}

/**
 * What "latest" means now that there are TWO channels: the newer of the CDN's
 * `version.json` and the registry's dist-tag. `null` only when NEITHER
 * answered — one silent channel must not be reported as agreement, which is the
 * property the CDN-only version already had.
 */
export function latestReleaseVersion(): string | null {
  return newestOf(latestCdnVersion(), latestNpmVersion());
}

/** The release host this CLI already trusts for version checks. */
export function cdnBase(): string {
  return (process.env.SUMMRISE_CDN || "https://agent.saisi.online").replace(
    /\/+$/,
    "",
  );
}

/** Where a boxed component lives on that host. Pure — the tests pin it. */
export function componentUrl(name: string): string {
  return `${cdnBase()}/summrise-agent/${name}`;
}

/**
 * The address to FETCH a boxed component from: the release manifest's own `url` for it, or
 * `componentUrl`'s derived route when the manifest carries none.
 *
 * WHY THE MANIFEST IS THE AUTHOR. Every release since the pins existed has published a `url`
 * beside each component's `sha256` in version.json, and this CLI read the digest while the
 * address was retyped here — one fact, two authors. They agree today, which is why reading the
 * manifest is a DRIFT GUARD rather than a fix: index/src/index.js rebuilds the published url
 * against the origin of the request that asked for `/api/version`, so the address it hands back
 * is the one this function would have derived — under $SUMMRISE_CDN too, where both move
 * together. A release whose url and route disagreed (a renamed bundle, the round-115 shape)
 * now fetches what it published; a silent manifest (an older release, or no network) keeps the
 * derived route, which is what every release before this one did.
 *
 * Pure — the tests pin it. The digest check beside it is unchanged.
 */
export function componentFetchUrl(
  name: string,
  pins: Record<string, { url?: string; sha256?: string }>,
): string {
  const key = componentKey(name);
  const u = key ? (pins[key] || {}).url : "";
  return typeof u === "string" && /^https?:\/\/\S+$/.test(u.trim())
    ? u.trim()
    : componentUrl(name);
}

/** The release manifest's key for a component's FILE name (they are not the same string). */
export function componentKey(fileName: string): string | null {
  if (fileName.startsWith("summrise-playwright")) return "playwright";
  if (fileName.startsWith("electron-")) return "electron";
  if (fileName.startsWith("cloudflared")) return "cloudflared";
  return null; // a file this release does not pin (e.g. fix-tunnel.ps1)
}

/** The manifest's component pins, or {} when it carries none (an older release). */
export function componentPins(): Record<
  string,
  { url?: string; sha256?: string }
> {
  const r = spawnSync("curl", ["-s", "-m", "5", `${cdnBase()}/api/version`], {
    encoding: "utf8",
    timeout: 8000,
  });
  if (r.status !== 0 || !r.stdout) return {};
  try {
    const j = JSON.parse(r.stdout);
    const c = j && typeof j.components === "object" ? j.components : null;
    return c || {};
  } catch {
    return {};
  }
}

/** sha256 of a file. Read whole: the electron runtime is 115 MB and node's heap is fine with it. */
export function sha256File(p: string): string {
  return crypto.createHash("sha256").update(fs.readFileSync(p)).digest("hex");
}

/**
 * A boxed component's local path: the copy inside the package if it is there,
 * otherwise the release host's route for it, downloaded to a temp file.
 *
 * WHY THIS EXISTS (2026-09-23). The npm package carries NONE of the big
 * binaries on purpose (cloudflared 54 MB, playwright 31 MB, electron 234 MB), so
 * "not in the package" used to mean "you do not get it, ever" — setup printed a
 * line and moved on. The same shape for cloudflared left a freshly migrated
 * device with no tunnel, and a device with no tunnel is INVISIBLE TO THE
 * CONSOLE while looking perfectly healthy from inside: an hour of that is what
 * this function is for. The release host serves all three, and for cloudflared
 * it proxies a PINNED upstream asset, which the agent's own sha256 pin then
 * checks again on the path it uses.
 *
 * `null` when neither source works: every component here is optional to the
 * AGENT, and a failed fetch must not fail the install.
 *
 * THE FIRST ARM IS A SEAM, NOT A LIVE PATH — measured, because a reader deserves to know which: no
 * component is in `package.json`'s `files[]` or in `required-in-tgz.txt`, and `tar tzf` on the published
 * tarball confirms none of the three is there. So today the release host is the ONLY source, and the
 * `existsSync` above exists so that a future release which DOES box one gets it from the package without
 * touching this function. The test beside it pins the fact, so boxing one is a deliberate act that also
 * updates this sentence.
 */
export function resolveComponent(name: string, pkgPath: string): string | null {
  if (fs.existsSync(pkgPath)) return pkgPath;
  const dest = path.join(
    fs.mkdtempSync(path.join(os.tmpdir(), "summrise-comp-")),
    name,
  );
  // -f: an HTTP error is a FAILURE, not a 404 page written to disk (the
  // HTML-polluted download that round-54 exists to remember).
  //
  // THE MANIFEST IS READ ONCE, BEFORE THE FETCH, because it is the author of the DIGEST (below)
  // and now of the ADDRESS too (componentFetchUrl). Same read, same object, same order of
  // effects: an unreachable manifest yields `{}`, the fetch falls back to the derived route, and
  // the digest check then says "fetched WITHOUT a manifest pin — not verified", exactly as it
  // did when the address was derived unconditionally.
  const pins = componentPins();
  const r = spawnSync(
    "curl",
    ["-fsSL", "-m", "300", "-o", dest, componentFetchUrl(name, pins)],
    { encoding: "utf8", timeout: 320000 },
  );
  if (r.status !== 0 || !fs.existsSync(dest) || fs.statSync(dest).size === 0) {
    return null;
  }
  // VERIFY WHAT WAS FETCHED (grilling Q4). Until this existed, a worker serving
  // different bytes would have been staged without complaint — the components are
  // executed or loaded, so "the host said so" is not an anchor. The manifest that
  // /api/version serves carries a sha256 per component (index/components.json,
  // copied in at publish time); a pin that does not match REFUSES the component,
  // because the failure it prevents is a device running bytes nobody published.
  // No pin at all (an older release) is a warning, not a refusal: it must not make
  // an install impossible.
  //
  // `pins` is the read from above — ONE manifest read per component fetch, exactly as before; the
  // check itself is unchanged.
  const key = componentKey(name);
  const pin = key ? (pins[key] || {}).sha256 || "" : "";
  if (pin) {
    const got = sha256File(dest);
    if (got !== pin) {
      console.error(
        `setup: REFUSING ${name} — sha256 ${got.slice(0, 12)}… does not match the release manifest's ${pin.slice(0, 12)}… (the host served different bytes than it published)`,
      );
      return null;
    }
    console.log(
      `setup: ${name} verified against the release manifest (${pin.slice(0, 12)}…)`,
    );
  } else if (key) {
    console.log(`setup: ${name} fetched WITHOUT a manifest pin — not verified`);
  }
  console.log(
    `setup: ${name} fetched from the release host (not in the package)`,
  );
  return dest;
}

/**
 * Ensure the desktop shell's electron RUNTIME. The shell's sources are ~90 KB
 * and ship in the package; the runtime is 234 MB and deliberately does not.
 *
 * WHY IT IS FETCHED AND NOT `npx electron`: the release host serves a PINNED
 * upstream proxy of the official build (v33.4.11 when this was written), so a
 * device behind the GFW needs no GitHub access and no ELECTRON_MIRROR — the two
 * things that made the hand-install on 2026-09-23 need a mirror variable at all.
 * `npx electron` would also mean rewriting the launcher, which calls
 * `<shell>\node_modules\electron\dist\electron.exe` directly.
 *
 * The zip's ROOT is the dist contents (`LICENSES.chromium.html` is its first
 * entry — measured through the route, not assumed), so it expands into `dist`.
 * Idempotent, and non-fatal: an install without a window still installs an agent.
 */
export function ensureElectron(): string | null {
  const elDir = path.join(DESK_DIR, "node_modules", "electron");
  const dist = path.join(elDir, "dist");
  if (fs.existsSync(path.join(dist, "electron.exe"))) return dist;
  const zip = resolveComponent(
    "electron-win32-x64.zip",
    path.join(__dirname, "..", "electron-win32-x64.zip"),
  );
  if (!zip) return null;
  fs.mkdirSync(dist, { recursive: true });
  // argv via `ps()`, not a cmd string: BOTH values are paths, and cmd expands `%NAME%`
  // even inside the double quotes the old form carried (see `sh()`).
  ps(
    `Expand-Archive -Force -Path '${psq(zip)}' -DestinationPath '${psq(dist)}'`,
  );
  if (!fs.existsSync(path.join(dist, "electron.exe"))) return null;
  try {
    // The electron package's own lookup file; the launcher does not need it, but
    // anything that resolves the package the npm way will.
    fs.writeFileSync(path.join(elDir, "path.txt"), "electron.exe");
  } catch {
    /* best-effort */
  }
  return dist;
}

export function statusReport(f: StatusFacts): string[] {
  const out: string[] = [];
  out.push(
    f.agentRunning === null
      ? "status: UNKNOWN -- the process list could not be read (tasklist failed); this is not a verdict"
      : f.agentRunning
        ? "status: RUNNING"
        : "status: STOPPED",
  );
  out.push("install dir: " + f.installDir);
  out.push(
    "panel: " +
      (f.exeExists ? `http://127.0.0.1:${f.port}/panel/` : "(not installed)"),
  );
  // A device without a release marker is not "on some version" — it is a device
  // whose version is UNKNOWN (a fresh box, or an install predating the marker).
  // Printing this CLI's version here would be a fabricated fact.
  out.push(
    "release: " +
      (f.releaseVersion ? f.releaseVersion : "unknown (no release marker)"),
  );
  out.push("this CLI: " + f.packageVersion);

  if (f.updateMarkerMs === null) {
    out.push(
      f.updateMarkerUnreadable
        ? "update: state UNKNOWN -- the busy marker exists but could not be read (permissions or a lock); do not assume no update is running"
        : "update: none in flight",
    );
  } else if (busyIsFresh(f.updateMarkerMs, f.nowMs)) {
    const secs = Math.max(0, Math.round((f.nowMs - f.updateMarkerMs) / 1000));
    out.push(
      `update: IN FLIGHT (marker ${secs}s old -- a swap is running now; the connection drops for ~10s)`,
    );
  } else {
    const mins = Math.round((f.nowMs - f.updateMarkerMs) / 60_000);
    out.push(
      `update: a previous update STARTED AND DID NOT FINISH (marker ${mins} min old). ` +
        `Check the log tail, then re-run 'summrise update' -- a stale marker is safe to overwrite.`,
    );
  }

  // The drift line: what a human actually wants from `status` after an update.
  // Only claimed when the running version is KNOWN — otherwise the comparison
  // would be against a guess.
  if (f.releaseVersion && f.releaseVersion !== f.packageVersion) {
    out.push(
      `update: device runs ${f.releaseVersion}, this CLI is ${f.packageVersion} -- ` +
        `run 'summrise update' to swap, then 'summrise status' again to confirm.`,
    );
  }

  // THE DELIVERY GAP, WHICH NOTHING ELSE IN THIS REPO CHECKS.
  //
  // `release` answers "what is this device running"; it did NOT answer "is that
  // current", and the two questions are answered by different machines. Every
  // round of this project's log records a device found MANY RELEASES BEHIND the
  // CDN — five, six, once three in a single round — and each time the only thing
  // that noticed was a human looking. This closes it where a human already
  // looks: the one command run after every update.
  //
  // `null` IS NOT "UP TO DATE". If the CDN could not be read the line says so
  // instead of staying silent, because silence here reads exactly like
  // agreement — the failure mode this whole log is about.
  // `== null` COVERS `undefined` TOO, AND THAT IS THE POINT. The first version of
  // this tested `=== null`, and a caller that simply OMITS the field — an older
  // edition, a test fixture — fell through to the drift branch with `undefined`
  // and crashed reading `.split` of nothing. Missing and null are both "we do not
  // know what the CDN has"; only a STRING is a comparison. (The panel learned the
  // same distinction the hard way in round 36: `undefined` is not `null`.)
  if (f.latestVersion == null) {
    out.push(
      "latest: could NOT be checked (the release CDN did not answer) -- this says nothing about whether the device is current",
    );
  } else if (f.releaseVersion && f.releaseVersion !== f.latestVersion) {
    out.push(
      `latest: ${f.latestVersion} is on the CDN -- THIS DEVICE IS BEHIND by ${behindBy(f.releaseVersion, f.latestVersion)}; run 'summrise update'`,
    );
  } else if (f.releaseVersion) {
    out.push(`latest: ${f.latestVersion} (this device is current)`);
  }
  return out;
}

/**
 * How far behind, in patch releases, WITHIN THE SAME MINOR. Returns a phrase,
 * never a fabricated number: `1.2.9` → `1.2.12` is "3 releases", but a MINOR or
 * MAJOR difference is not a count of anything a reader can act on, so it is
 * stated as such. The last-5-per-minor CDN prune means a cross-minor jump is a
 * different operation anyway (`summrise rollback` refuses it for the same reason).
 */
/** ONE PARSE, TWO PRESENTATIONS (round 202). The comparison between two x.y.z strings was written out once, for the
 *  sentence `status` prints. The update guard needed the same fact as a BOOLEAN, and its first version tested the
 *  sentence's truthiness — where "0 releases" and "-1 releases" are both TRUTHY, so it would have refused every update,
 *  including the correct one. Caught by asking the artefact before shipping it. */
function versionTriple(v: string): number[] | null {
  const t = String(v || "")
    .split(".")
    .map(Number);
  return t.length === 3 && t.every((n) => Number.isFinite(n)) ? t : null;
}

/** WOULD THIS UPDATE MOVE THE DEVICE AT ALL? The parity fact, and it needs NO NETWORK: the CLI stamps
 *  `<install>/.summrise-release` with ITS OWN version, and the device's `agent_update` reads that file as
 *  the local version — so when the device is already on the version this CLI carries, the stamp equals
 *  what is there, the swap installs the same build, and nothing moves. That is round 201's measured
 *  defect ("update requested 1.2.438 -> 1.2.438", "copy ok=True", release unchanged), which is why the
 *  guard below exists at all. THE GUARD THAT EXISTS CANNOT SEE IT: it compares the CLI against the
 *  RELEASE CHANNEL, and when the CDN is unreadable `latest` is empty and the whole check is skipped, so a
 *  network blip re-opens the defect. This one compares two facts already on the machine. */
export function updateWouldNotMove(
  fromVersion: string,
  selfVersion: string,
): boolean {
  return Boolean(fromVersion) && fromVersion === selfVersion;
}

/** Is `latest` ahead of `device` on the SAME release line? */
export function isBehind(device: string, latest: string): boolean {
  const a = versionTriple(device);
  const b = versionTriple(latest);
  if (!a || !b) return false;
  if (a[0] !== b[0] || a[1] !== b[1]) return false;
  return b[2] > a[2];
}

export function behindBy(device: string, latest: string): string {
  const a = versionTriple(device);
  const b = versionTriple(latest);
  if (!a || !b) {
    return "an unknown number of releases";
  }
  if (a[0] !== b[0] || a[1] !== b[1])
    return "a release line, not a patch count";
  const n = b[2] - a[2];
  return n === 1 ? "1 release" : `${n} releases`;
}

/**
 * The CLI's pre-handoff receipt, appended to the swap's own log.
 *
 * Takes the resolved DATA dir and rebuilds the same `<data>\logs\summrise-update.log`
 * path the swap script appends to — NOT a `..` relative guess, because
 * `DATA_DIR` can be a registry-remapped location that is not `DIR`, and a
 * receipt written to a different file than the swap writes is worse than none:
 * it would look like the swap never started.
 *
 * Deliberately NOT the swap's `update start` wording: the value of the receipt
 * is that its presence-without-`update start` proves the CLI ran and the swap
 * did not, so the two markers must stay distinguishable in the file.
 */
export function updateReceiptPs(
  dataDirQ: string,
  fromVersion: string,
  toVersion: string,
): string[] {
  const log = `Out-File '${dataDirQ}\\logs\\summrise-update.log' -Append`;
  const line =
    `"[$(Get-Date -Format o)] update requested ${fromVersion} -> ${toVersion} ` +
    `(CLI reached the device; the swap has not started yet)"`;
  return [`${line} | ${log}`];
}

/**
 * The update mutual-exclusion marker — ONE owner for the path.
 *
 * Three readers now depend on it agreeing: the mutual-exclusion check in
 * `setup()`, the guard in `update()`, and `statusReport`'s "is a swap pending"
 * line. If they ever computed the path differently, `status` would confidently
 * report "none in flight" while an update was refusing to start because a
 * marker it could not see was in the way.
 *
 * A SURVIVING STALE MARKER means the update started and never finished: the
 * swap script clears it on its own known-failure paths (task repoint, migration
 * gate) and after a successful restart, so one still sitting there is an update
 * that died before cleanup. `statusReport` deliberately reuses `busyIsFresh` —
 * the SAME predicate `update()` refuses on — so "stale" cannot mean one thing
 * to the guard and another to the report.
 */
export function updateBusyPath(): string {
  return path.join(
    process.env.ProgramData || "C:\\ProgramData",
    "SummriseAgent",
    "update-busy",
  );
}

/**
 * The SAME marker as the PowerShell text the swap script embeds, DERIVED from `updateBusyPath()` rather
 * than written out by hand — which is what this file did, three times, inside the generated script. The
 * agent fixed exactly this on its own side and says why (tools.rs, above BUSY_MARKER_REL): "It used to be
 * spelled out TWICE: the Rust acquirer built <ProgramData>\SummriseAgent\update-busy from PathBuf joins
 * while the generated PowerShell swap script carried the same location as two hand-written string
 * literals. A drift between the two is invisible until an update actually runs — and then the swap
 * releases a file the agent never created, the marker survives, and every later update is refused for up
 * to an hour." The CLI's own doc above claims ONE owner for this path; this is what makes that true.
 */
export function busyMarkerPs(): string {
  const root = process.env.ProgramData || "C:\\ProgramData";
  const rel = path.relative(root, updateBusyPath()).split(path.sep).join("\\");
  return `(Join-Path $env:ProgramData '${rel}')`;
}

// ── A version marker must be EARNED ─────────────────────────────────────────
//
// `etc\.summrise-release` is the device's ONLY local version truth: agent_update
// reads it as `local` and answers up_to_date when the remote is not newer, and
// /api/status serves it as `release` — the field the panel, the tray and the
// console fleet card all display.
//
// `summrise rollback` wrote it UNCONDITIONALLY once `summrise update` returned status 0.
// But status 0 means the WMI handoff was ACCEPTED — a process was created — not
// that the swap succeeded. Everything that decides success (the fail-closed
// migration gate, the copy retry, the `$ok`-gated marker write, the task
// restart) happens AFTERWARDS inside a WMI-parented process nobody reads. So a
// rollback whose swap died left a marker claiming a version the device was not
// running: every UI lies, and once the pin is cleared agent_update sees the fake
// version, decides it is current, and the device is stuck on the old release.
//
// The swap script always gated the same write on a provable copy
// (`if ($ok -and '<ver>')`); the CLI simply never did. The fix is to read the
// marker BACK: only a marker that shows the target version is evidence.
export interface ReleaseMarkerCheck {
  ok: boolean;
  /** The version actually read, trimmed; null when absent or unreadable. */
  saw: string | null;
  /** How long the poll actually waited before giving up. Only meaningful when
   *  `ok` is false: a marker that never arrived is a TIMEOUT, which is weaker
   *  evidence than a read that returned the wrong version. */
  waitedMs: number;
}

/**
 * Poll `etc\.summrise-release` until it shows `want`, or the budget expires.
 *
 * Bounded on purpose: the swap kills the agent and restarts a scheduled task, so
 * a delay is NORMAL — but a marker that never arrives is a failed swap and must
 * be reported as one. `read`/`sleep`/`now` are injected so the behaviour is
 * testable without real timers or a device.
 */
export async function awaitReleaseMarker(o: {
  want: string;
  timeoutMs: number;
  intervalMs: number;
  read: () => string;
  sleep: (ms: number) => Promise<void>;
  now: () => number;
}): Promise<ReleaseMarkerCheck> {
  const start = o.now();
  const deadline = start + o.timeoutMs;
  let saw: string | null = null;
  for (;;) {
    try {
      const v = String(o.read()).trim();
      saw = v || null;
      if (saw === o.want) return { ok: true, saw, waitedMs: o.now() - start };
    } catch {
      saw = null; // absent / unreadable is NOT success and NOT a crash
    }
    if (o.now() >= deadline)
      return { ok: false, saw, waitedMs: o.now() - start };
    await o.sleep(o.intervalMs);
  }
}

/**
 * What to do about a rollback whose swap could not be proven.
 *
 * Separated from the I/O so the DECISION is assertable: the pin is the device's
 * protection against being auto-upgraded back, and the marker is what every UI
 * believes — writing either on an unproven swap is how a device ends up
 * misreporting its own version and refusing the update that would fix it.
 */
export function releaseMarkerVerdict(
  c: ReleaseMarkerCheck & {
    want: string;
    verb?: "rollback" | "update";
    from?: string;
  },
): {
  writePin: boolean;
  exitCode: number;
  message: string;
} {
  // WHOSE FAILURE IS THIS? The message is printed by TWO callers — `rollback`, which staged a release in
  // order to PIN it, and `update`, which staged one to INSTALL it — and it said "rollback:" with
  // "re-run `summrise rollback <want>`" for both. On the update path that advice is worse than useless:
  // it names the version that just failed to install, so following it asks the device to pin a release
  // it is not running. The verb decides the sentence now.
  const verb = c.verb ?? "rollback";
  if (c.ok) {
    return {
      writePin: true,
      exitCode: 0,
      message: `${verb}: pinned to ${c.want} -- auto-upgrade refused until 'summrise rollback --clear' or a forced agent_update`,
    };
  }
  return {
    writePin: false,
    exitCode: 1,
    // WHAT WAS OBSERVED, NOT WHAT IT MEANS. `ok: false` is a TIMEOUT: either the swap
    // failed, or it is still running (it kills and restarts the agent), or the marker
    // could not be read at all — and `saw === null` is the weakest of the three. The old
    // wording asserted "the swap did NOT take" for all of them.
    message:
      `${verb}: no release marker showing ${c.want} within ${
        Number.isFinite(c.waitedMs)
          ? Math.round(c.waitedMs / 1000) + "s"
          : "the read-back window"
      } ` +
      `(last read: ${c.saw ?? "empty or unreadable"}). ` +
      `NOT pinned (a pin would claim a version this device may not be running) and no release ` +
      `marker written. ` +
      (verb === "update"
        ? `Check the update log and \`summrise status\`, then re-run \`summrise update\`. If it fails ` +
          `the same way, \`summrise rollback ${c.from ?? "<the version it was on>"}\` pins the release ` +
          `this device is ACTUALLY running — which is not the one that failed to install.`
        : `Check the update log and \`summrise status\`, then re-run 'summrise rollback ${c.want}'.`),
  };
}

// P2-4 (low-cost boxed pinning): setup/update record the boxed-component
// versions next to the install dir (boxed-versions.json — playwright-mcp /
// playwright-core / cloudflared version+sha; "unknown" + timestamp when the
// upstream version is unavailable). The agent echoes the file in /api/status
// (and /api/plugins/status). NEVER fail-closed: every probe is best-effort
// and the write itself is wrapped — a failure only logs, never blocks the
// install/update.
// exported: unit-tested shape (pure path probing + guarded FS).
export interface BoxedManifest {
  updated: string;
  playwright_mcp: { version: string; sha256: string };
  playwright_core: { version: string; sha256: string };
  cloudflared: { version: string; sha256: string };
}
export function boxedVersions(
  installDir: string,
  pkgDir: string,
): BoxedManifest {
  const pkgVer = (p: string): string => {
    try {
      const j = JSON.parse(fs.readFileSync(p, "utf8"));
      return typeof j?.version === "string" && j.version
        ? j.version
        : "unknown";
    } catch {
      return "unknown";
    }
  };
  const shaOf = (p: string): string => {
    try {
      const st = fs.statSync(p);
      if (!st.isFile() || st.size > 300 * 1024 * 1024) return "unknown";
      return crypto
        .createHash("sha256")
        .update(fs.readFileSync(p))
        .digest("hex");
    } catch {
      return "unknown";
    }
  };
  // Layout v2: callers (setup/update) always run after staging/migration, so
  // the components\ homes exist — no legacy fallback (single semantic).
  const cfBin = fs.existsSync(
    path.join(installDir, "components", "cloudflared.exe"),
  )
    ? path.join(installDir, "components", "cloudflared.exe")
    : path.join(pkgDir, "cloudflared.exe");
  let cfVer = "unknown";
  try {
    if (fs.existsSync(cfBin)) {
      const r = spawnSync(cfBin, ["--version"], {
        encoding: "utf8",
        timeout: 15000,
      });
      const line = ((r.stdout || "") + (r.stderr || ""))
        .split(/\r?\n/)[0]
        .trim();
      if (line) cfVer = line.slice(0, 120);
    }
  } catch {
    /* best-effort */
  }
  const pwRoot = path.join(installDir, "components", "playwright");
  return {
    updated: new Date().toISOString(),
    playwright_mcp: {
      version: pkgVer(
        path.join(pwRoot, "node_modules", "@playwright", "mcp", "package.json"),
      ),
      sha256: shaOf(path.join(pkgDir, "summrise-playwright.zip")),
    },
    playwright_core: {
      version: pkgVer(
        path.join(pwRoot, "node_modules", "playwright-core", "package.json"),
      ),
      sha256: "unknown",
    },
    cloudflared: {
      version: cfVer,
      sha256: fs.existsSync(cfBin) ? shaOf(cfBin) : "unknown",
    },
  };
}
// exported: best-effort writer for the P2-4 manifest (never throws).
export function writeBoxedVersions(installDir: string, pkgDir: string): void {
  try {
    fs.mkdirSync(path.join(installDir, "etc"), { recursive: true });
    fs.writeFileSync(
      path.join(installDir, "etc", "boxed-versions.json"),
      JSON.stringify(boxedVersions(installDir, pkgDir), null, 2),
    );
  } catch (e: any) {
    console.log(
      "boxed-versions: manifest write skipped (" + (e?.message || e) + ")",
    );
  }
}

// round-298 parity: record this package's release version next to the install
// dir as `.summrise-release` — the file agent_update reads as the LOCAL version
// (fallback: Cargo 1.0.x, which never changes, so remote always looks newer
// and every agent_update call re-downloads + swaps). `summrise update` writes it
// from the swap script after a provable copy; `summrise setup` (fresh install)
// copies THIS package's exe, so the provable-success point is right after the
// boot task registers — the caller invokes this only once setup succeeded.
// Best-effort, never fail-closed (a marker failure must not block install).
export function writeReleaseMarker(installDir: string): void {
  try {
    const v = String(require("../package.json").version || "");
    if (!v) return;
    // No mkdir: callers (setup/update) always run after staging/migration,
    // so etc\ exists — a missing dir stays a silent best-effort skip.
    fs.writeFileSync(
      path.join(installDir, "etc", ".summrise-release"),
      v,
      "utf8",
    );
  } catch {
    /* best-effort */
  }
}

/**
 * Stage the Electron desktop shell sources (main/preload/url-policy +
 * icons) into components\summrise-desktop-electron. setup writes them in place;
 * update writes `*.new` so the swap script can atomically replace them. The
 * two flows used to each inline this block.
 */
/** @returns how many files were actually staged — 0 means the sources were not
 *  present, which the caller MUST NOT report as "staged". */
function stageDesktopShell(installDir: string, suffix: "" | ".new"): number {
  const DESK_SRC = path.join(
    __dirname,
    "..",
    "summrise-desktop-electron",
    "src",
  );
  if (!fs.existsSync(DESK_SRC)) return 0;
  const desDst = path.join(
    installDir,
    "components",
    "summrise-desktop-electron",
    "src",
  );
  fs.mkdirSync(desDst, { recursive: true });
  let staged = 0;
  for (const f of ["main.js", "preload.js", "url-policy.js"]) {
    const s = path.join(DESK_SRC, f);
    if (fs.existsSync(s)) {
      fs.copyFileSync(s, path.join(desDst, f + suffix));
      staged += 1;
    }
  }
  // icon.png/.ico go next to src/ (Electron loads from ../icon.png;
  // Windows Tray requires the .ico).
  for (const icon of ["icon.png", "icon.ico"]) {
    const iconSrc = path.join(
      __dirname,
      "..",
      "summrise-desktop-electron",
      icon,
    );
    if (fs.existsSync(iconSrc))
      fs.copyFileSync(
        iconSrc,
        path.join(installDir, "components", "summrise-desktop-electron", icon),
      );
  }
  // Fresh-install desktop fix: the shell is launched as `electron .`
  // (start-desktop.ps1), which resolves its entry ONLY via package.json
  // "main". stageDesktopShell used to ship src/*.js + icons but NO
  // package.json — so `electron .` had nothing to load, AND the installer's
  // Electron step (gated on Test-Path package.json) was skipped entirely,
  // leaving the desktop shell dead on every fresh box. Write the minimal
  // manifest here (setup + update paths; idempotent, not held open by the
  // running shell).
  const shellDir = path.join(
    installDir,
    "components",
    "summrise-desktop-electron",
  );
  try {
    fs.writeFileSync(
      path.join(shellDir, "package.json"),
      JSON.stringify(
        {
          name: "summrise-desktop-electron",
          version: "0.2.0",
          main: "src/main.js",
          private: true,
        },
        null,
        2,
      ),
      "utf8",
    );
  } catch {
    /* best-effort — a failed write must not break staging */
  }
  return staged;
}

/**
 * Is a Windows process running? THREE answers, because two of them are not the same.
 *
 * `tasklist` failing (missing, refused, erroring) leaves stdout EMPTY, and every caller
 * here used to read that as "not running" — a claim of ABSENCE from a failed READ. That
 * is the fourth instance of this shape in this file (`statusReport`, `rollback status`,
 * `autostart status`), and it matters most for the AGENT itself: `summrise status` reporting
 * STOPPED for a device that is serving is the worst possible answer.
 */
function processRunning(image: string): "yes" | "no" | "unknown" {
  // No shell: `shell: true` concatenates argv into one cmd.exe string, so the unquoted
  // filter "IMAGENAME eq ..." was split at its spaces, tasklist rejected it, and the
  // probe always answered "no" even with the process running. (IMAGENAME also takes no
  // wildcard — the old `summrise-agent*` never matched.)
  const r = spawnSync("tasklist", ["/FI", `IMAGENAME eq ${image}`], {
    encoding: "utf8",
  });
  if (r.error || r.status === null || r.status !== 0) return "unknown";
  return String(r.stdout || "")
    .toLowerCase()
    .includes(image.toLowerCase())
    ? "yes"
    : "no";
}

function svc(action) {
  // RETURN the status. It used to be discarded, which is why `stop` printed "stopped"
  // and exited 0 for a missing task or an access-denied, and why `start`/`restart` were
  // silent either way. `autostart` already checks; this is the same pattern.
  //
  // argv, NOT an `sh()` command string: both `${action}` and `${TASK}` used to be
  // interpolated UNQUOTED into a line cmd.exe parses, which is the door the `psArgv()`
  // incident comment above describes — a value carrying a space, `&`, `|`, `>` or `<`
  // is re-parsed as an OPERATOR before schtasks ever sees it, and `TASK` is a
  // literal only until somebody makes the task name configurable.
  // schtasks needs no shell feature (no pipe, no `&&`, no redirection) and `/Query`
  // below already spawns it this way, so the door closes with no behaviour change:
  // the same program receives the same argv.
  return spawnSync("schtasks", [`/${action}`, "/TN", TASK], {
    stdio: "inherit",
  });
}

/**
 * The scheduled task's State (`Running`, `Ready`, `Disabled`, …) or **null when it could not be read**.
 *
 * THREE STATES, NOT ONE. `autostart` states the rule and names the shape — "a failed READ reported as
 * evidence of ABSENCE" — which it calls the THIRD instance, after `statusReport` and `rollback status`.
 * Here `null` means "I could not ask", and callers report that as itself instead of as a verdict.
 *
 * `LOCALE` is why this asks `Get-ScheduledTask` rather than parsing `schtasks /Query`: the latter's headers
 * are localized, which is the reason `autostart` uses this call too.
 */
function taskState(name: string): string | null {
  const r = spawnSync(
    "powershell",
    [
      "-NoProfile",
      "-Command",
      `(Get-ScheduledTask -TaskName '${name}' -ErrorAction SilentlyContinue | Select-Object -ExpandProperty State)`,
    ],
    { encoding: "utf8" },
  );
  const err = r && (r.error as Error | undefined);
  if (!r || err || r.status !== 0) return null; // spawn failed, or the read itself failed
  const s = String(r.stdout || "").trim();
  return s === "" ? null : s; // empty = the task is not there, which is also "no answer to Running?"
}

// Shared tunnel bootstrap: login (token or interactive) → create tunnel →
// DNS route → write tunnel.yml. Used by `summrise setup --tunnel` and
// `summrise tunnel install`.
function initTunnel(hostname, regKey) {
  const cf = path.join(COMPONENTS_DIR, "cloudflared.exe");
  const cfg = path.join(ETC_DIR, "tunnel.yml");
  if (!fs.existsSync(cf)) {
    console.error("tunnel: cloudflared.exe not staged at", cf);
    // NOT "REINSTALL THE PACKAGE". The npm package carries NO boxed components BY DESIGN (that is what
    // keeps it ~6.7 MB), so a reinstall cannot stage this and the advice sent the operator in a circle.
    // `setup` is what stages it: resolveComponent fetches it from the RELEASE HOST and the agent's own
    // sha256 pin checks it again on the path it uses. Its doc records what this costs when nobody does —
    // "a device with no tunnel is INVISIBLE TO THE CONSOLE while looking perfectly healthy from inside."
    console.error(
      "  run 'summrise setup' to stage it -- the component comes from the release host, not from the npm package.",
    );
    process.exit(1);
  }
  const host = hostname || "d1.agent.saisi.online";
  // Login: (1) reg-key → gateway tunnel-token exchange (FULLY automatic —
  // the console's stored Cloudflare credential, no env var, no browser), or
  // (2) CLOUDFLARE_API_TOKEN env, or (3) interactive cloudflared browser
  // login as last resort.
  let token = process.env.CLOUDFLARE_API_TOKEN || "";
  if (!token && regKey) {
    console.log(
      "tunnel: exchanging registration key for the Cloudflare API token...",
    );
    try {
      const r = apiPost("/api/install/tunnel-token", { key: regKey });
      if (r && r.apiToken) {
        token = r.apiToken;
        console.log("tunnel: key exchanged (consumed once)");
      } else
        console.log(
          "tunnel: tunnel-token exchange failed (" +
            (r && r.error ? r.error : "no token") +
            ") -- falling back",
        );
    } catch (e) {
      console.log(
        "tunnel: exchange unavailable (" + e.message + ") -- falling back",
      );
    }
  }
  const r1 = spawnSync(
    cf,
    token ? ["tunnel", "login", "--token", token] : ["tunnel", "login"],
    { stdio: "inherit" },
  );
  if (r1.status !== 0) {
    console.error("tunnel: cloudflare login failed");
    process.exit(1);
  }
  const name = "summrise-agent-" + host.split(".")[0];
  spawnSync(cf, ["tunnel", "create", name], { stdio: "inherit" });
  const list =
    spawnSync(cf, ["tunnel", "list", "--name", name], { encoding: "utf8" })
      .stdout || "";
  const m = /([0-9a-fA-F]{8}-[0-9a-fA-F-]{27})/.exec(list);
  const tunnelId = m ? m[1] : null;
  if (!tunnelId) {
    console.error("tunnel: could not determine tunnel id");
    process.exit(1);
  }
  const r3 = spawnSync(cf, ["tunnel", "route", "dns", name, host], {
    stdio: "inherit",
  });
  if (r3.status !== 0) {
    console.error("tunnel: dns route failed");
    process.exit(1);
  }
  const cred = path.join(
    process.env.USERPROFILE || "",
    ".cloudflared",
    tunnelId + ".json",
  );
  // Ingress follows the agent's configured bind port (custom ports 502
  // otherwise); DIR/config.yaml may not exist on fresh installs → default.
  const tunPort = agentPort(ETC_DIR);
  // THE INGRESS MUST NAME THE ADDRESS THE AGENT LISTENS ON, AND IT IS 127.0.0.1.
  //
  // This wrote `http://127.0.0.2:<port>` — and the agent's own provisioning
  // (`agent/src/tunnel.rs`) writes `127.0.0.1`, keeping a helper
  // (`ingress_service`) whose comment says it exists to "reach the agent where it
  // actually listens", and calling 127.0.0.2 "a dead address (502)". Two writers,
  // two answers, one file — and the LIVE DEVICE settles it: `netstat` shows the
  // listener on `127.0.0.1:18080`, and d1's own `etc\tunnel.yml` (written by the
  // agent) says `service: http://127.0.0.1:18080`. So this writer would have
  // repointed a working tunnel at a socket nobody holds.
  //
  // `allow-remote-config: false` IS NOT OPTIONAL EITHER, and it was missing here.
  // The agent writes it deliberately: cloudflared prefers a REMOTE config when one
  // exists, so a stale remote ingress keeps proxying to a dead address "no matter
  // what tunnel.yml says" (tunnel.rs). This writer silently re-enabled that.
  fs.writeFileSync(
    cfg,
    [
      "tunnel: " + tunnelId,
      "credentials-file: " + cred,
      "allow-remote-config: false",
      "ingress:",
      "  - hostname: " + host,
      "    service: http://127.0.0.1:" + tunPort,
      "  - service: http_status:404",
      "",
    ].join("\n"),
  );
  console.log(
    "tunnel: installed -- tunnel.yml written, agent spawns it on boot",
  );
  console.log("  hostname:", host);
}

// exported: rollback version gate — a plain dotted triple only (the tgz URL
// interpolates it; anything else could escape the /summrise-agent/ prefix). unit-tested.
export function rollbackVersionOk(v: string): boolean {
  return /^\d+\.\d+\.\d+$/.test(v);
}
/**
 * TELL THE AGENT IT IS ABOUT TO BE STOPPED ON PURPOSE, before anything kills it.
 *
 * `summrise restart` and `summrise stop` end the process from OUTSIDE, so it writes no clean-exit marker —
 * and the run journal then reports the operator's own action as `crashed` whenever the revival
 * takes longer than the heartbeat window. The device has one place that knows how to mark it
 * (`runstate::mark_deliberate_stop`, behind `POST /api/run/mark-exit`), so the CLI asks rather than
 * writing the file itself: one rule, one implementation.
 *
 * BEST-EFFORT BY DESIGN: it runs BEFORE the kill, and a device whose agent is already gone must
 * still be stoppable. A failure is a WARNING, never a refusal to stop.
 */
function markDeliberateStop(reason: "stop" | "update" = "stop"): void {
  // THE REASON DECIDES THE VERDICT. "update" makes the next start report `replaced` (the swap was
  // deliberate and normal) while "stop" reports `clean-exit`. Neither can be inferred from timing:
  // d1's boot history shows an update swap with a 61 s heartbeat gap while the boot task revives a
  // real crash within ~60 s — the durations overlap, so an unmarked update lands as a CRASH.
  const r = deviceApi("POST", "/api/run/mark-exit", { reason });
  if (!r.ok) {
    console.error(
      `  (note: could not mark this stop as deliberate -- ${r.error}; the next start may report a crash)`,
    );
    return;
  }
  if (r.body && r.body.marked === false) {
    // Nothing to mark: no run on record, or it already said it exited. Not an error.
    return;
  }
}

const commands = {
  // `summrise setup` = PURE LOCAL install (no key, no tunnel, no cloud). The
  // gateway/tunnel are OPTIONAL extras configured LATER via the Settings
  // page or the optional flags below:
  //   --reg-key <key>   register the device with the gateway console now
  //   --tunnel <host>   also provision the (free) cloudflared tunnel
  setup(args) {
    const regOk: boolean[] = [];
    let i = args.indexOf("--reg-key");
    let regKey = i >= 0 ? args[i + 1] : process.env.SUMMRISE_REG_KEY;
    let ti = args.indexOf("--tunnel");
    let tunnelHost = ti >= 0 ? args[ti + 1] : "";
    let wantTunnel =
      args.includes("--tunnel") || !!process.env.CLOUDFLARE_API_TOKEN;
    // Device hostname for self-register: explicit --hostname, else default
    // d1.agent.saisi.online. Written to summrise-agent.hostname (the agent's
    // self-register reads it at boot).
    const hi = args.indexOf("--hostname");
    const deviceHost =
      hi >= 0
        ? args[hi + 1]
        : process.env.SUMMRISE_HOSTNAME || "d1.agent.saisi.online";
    // review #1 (HIGH): the hostname write ran BEFORE the mkdirSync below —
    // on a FRESH machine DIR doesn't exist yet → ENOENT throw → setup died
    // having installed nothing. Ensure the dir first.
    fs.mkdirSync(ETC_DIR, { recursive: true });
    fs.mkdirSync(COMPONENTS_DIR, { recursive: true });
    fs.mkdirSync(SCRIPTS_DIR, { recursive: true });
    fs.mkdirSync(LOGS_DIR, { recursive: true });
    fs.writeFileSync(HOSTNAME_FILE, deviceHost);
    // No key required for a local install — key/tunnel are optional extras.
    if (regKey) {
      // WAS: "setup: registering device with the gateway (--reg-key)" — FALSE on this
      // path. `regKey` has exactly ONE consumer, the Cloudflare token exchange inside
      // `initTunnel`, and that runs only under `--tunnel`. So `summrise setup --reg-key K`
      // without `--tunnel` printed that line and never used K at all; the device still
      // appeared in the console, but via the AGENT's own token-based self-register on
      // first boot, which needs no key — so the operator credited the key.
      //
      // It cannot be fixed by registering here: the device token is minted by the agent
      // on first boot, and the gateway's POST /api/register wants {key, name, hostname,
      // token}. So the line says what the key is actually for.
      console.log(
        wantTunnel
          ? "setup: --reg-key will be exchanged for the tunnel token (--tunnel)"
          : "setup: --reg-key noted, but WITHOUT --tunnel it is not used — the device registers itself on first start with its own token. Pass --tunnel to use the key, or add the gateway later in the Settings page.",
      );
    } else {
      // WAS: "setup: LOCAL install (no cloud)." — FALSE, and privacy-relevant.
      // The CLI does not write config.yaml at all; the AGENT creates it on first boot
      // from its embedded default, which sets platform.console_url to the public
      // console, and main.rs then POSTs {name, hostname, token} to
      // /api/devices/self-register at boot and every 6h. So a no-key install DOES
      // contact the cloud and appear in the console — which is what the very next lines
      // already say ("device registers on start"), so the operator was told both things
      // at once. The URL is NOT repeated here on purpose: it lives in the agent's
      // embedded default, and a second copy in a CLI message is a copy that drifts.
      console.log(
        "setup: no key or tunnel configured — the device will still self-register with the console URL in its config on start.",
      );
      console.log(
        "setup: to keep it purely local, clear platform.console_url in config.yaml (unset = no cloud), or point it at your own gateway.",
      );
    }
    fs.mkdirSync(DIR, { recursive: true });
    // Layout-v2 migration (ADR 0008): a re-setup on a pre-v2 device moves
    // the old root paths into their v2 homes before anything stages.
    // Idempotent (fresh installs no-op). Runs here — before the residue
    // cleanup below, which targets the NEW homes.
    try {
      const mig = psFile(migrateLayoutPs(psq(DIR), psq(DATA_DIR)).join("\r\n"));
      if (!mig || mig.status !== 0)
        console.log("setup: layout migration had warnings (continuing)");
    } catch {
      console.log("setup: layout migration skipped (continuing)");
    }
    // ---- idempotent reinstall: clean every legacy residue BEFORE writing
    // anything (a re-run of `summrise setup` must leave a pristine install).
    // 1. Stop any running summrise processes (a live agent locks its exe and the
    //    copy below would fail).
    console.log("setup: stopping existing summrise processes...");
    // Best-effort cleanup of things that usually do NOT exist, so send BOTH
    // streams to NUL. `2>NUL` alone left sc/reg/schtasks messages on STDOUT:
    // every install printed scary fake errors ("[SC] OpenService 失败 1060",
    // "错误: 系统找不到指定的文件。") that were really "nothing to clean".
    sh("cmd /c schtasks /End /TN SummriseAgent >NUL 2>&1");
    sh("taskkill /F /IM summrise-agent.exe 2>NUL");
    sh("taskkill /F /IM summrise-desktop.exe 2>NUL");
    sh("taskkill /F /IM summrise-tray.exe 2>NUL");
    // 2. Remove legacy scheduled tasks (SummrisePlaywright from old installs,
    //    SummriseAgentTray) — SummriseAgent is re-registered below with -Force.
    sh("cmd /c schtasks /Delete /TN SummriseAgentTray /F >NUL 2>&1");
    sh("cmd /c schtasks /Delete /TN SummrisePlaywright /F >NUL 2>&1");
    // 3. Remove the legacy Cloudflared Windows service + EventLog source
    //    (installed by the retired setup.ps1; the agent-supervised model
    //    installs no service).
    sh("sc stop Cloudflared >NUL 2>&1");
    sh("sc delete Cloudflared >NUL 2>&1");
    sh(
      "reg delete HKLM\\SYSTEM\\CurrentControlSet\\Services\\EventLog\\Application\\Cloudflared /f >NUL 2>&1",
    );
    // 4. Stale update-busy marker (a crashed update would lock updates).
    const BUSY = path.join(
      process.env.ProgramData || "C:\\ProgramData",
      "SummriseAgent",
      "update-busy",
    );
    // `ps()` (argv) rather than `sh()`: BUSY is a PATH under %ProgramData%, and cmd
    // expands `%NAME%` in a value even when the value is quoted (see `sh()`).
    ps(`Remove-Item -Force -ErrorAction SilentlyContinue '${psq(BUSY)}'`);
    // 5. Refresh the BOXED playwright bundle: delete the old tree first so a
    //    removed package/version never leaves stale files behind.
    //    round-163: kill the runner/bridge node processes FIRST — a running
    //    node.exe holds its image file locked, the Remove-Item/Expand-Archive
    //    pair silently skipped it, and the device was left with a playwright
    //    dir WITHOUT node.exe (bridge could never spawn again; observed d1).
    // argv (`ps()`), because DIR is the install dir: an operator-chosen PATH, so cmd
    // could find a `%NAME%` in it — and this script's `|` and `{ }` are only data
    // because no shell parses the line at all.
    ps(
      `Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'node.exe' -and $_.CommandLine -like '*${psq(DIR)}*playwright*' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }`,
    );
    ps(
      `Remove-Item -Recurse -Force -ErrorAction SilentlyContinue '${psq(PW_DIR)}'`,
    );
    // 6. Legacy install dirs from retired installers (C:\summrise-agent /
    //    D:\summrise-agent). If the registry now points at a DIFFERENT dir and a
    //    legacy dir exists, it is a residue of the old channel — remove it
    //    (the data that matters lives in %ProgramData%\Summrise; the old dirs
    //    held programs + config only).
    for (const legacy of ["C:\\summrise-agent", "D:\\summrise-agent"]) {
      if (legacy !== DIR && fs.existsSync(legacy)) {
        console.log("setup: removing legacy install dir", legacy);
        // cmd-% literal: `legacy` is one of the two STRING LITERALS in the array on the
        // `for` line above, so this value cannot carry a `%` — reviewed, left on the cmd
        // line (the pin re-reads that array and checks it is literals all the way down).
        sh(
          `powershell -NoProfile -Command "Remove-Item -Recurse -Force -ErrorAction SilentlyContinue '${psq(legacy)}'"`,
        );
      }
    }
    // 7. stage-brand: heal a stale desktop shortcut (a 2026-09-01 Summrise.lnk
    //    launches the RETIRED Tauri exe with the old embedded icon) + drop
    //    the retired orphans. Repair-only (helper checks link existence).
    //    Backslash-escape the .lnk Arguments double quotes for -Command.
    console.log("setup: reconciling desktop shortcut (retired-exe repair)...");
    // cmd-% residual: THE ONE SITE THIS ROUND DID NOT FIX, and it is not "safe" — both
    // values are paths (SCRIPTS_DIR, DESK_DIR). It is left because the same line depends
    // on the cmd-level `\"` escaping above, which is its own defect (cmd does not unescape
    // `\"`: the quoted region ends at the first `"` and the call works by accident), and
    // argv-ing this call means deleting that `.replace()` in the same change. Named rather
    // than silently exempted: the pin allows exactly ONE such site and fails a second.
    sh(
      `powershell -NoProfile -Command "${deskShortcutRepairPs(psq(SCRIPTS_DIR), psq(DESK_DIR), "Write-Host").join("; ").replace(/"/g, '\\"')}"`,
    );
    // C1: write the registry single source of truth (InstallDir; DataDir
    // defaults to %ProgramData%\Summrise). Everything else reads it back.
    // `regOk` is collected here and summarised at the end of setup: best-effort, but the
    // operator should be told once, with the consequence, rather than not at all.
    regOk.push(regWrite("InstallDir", DIR));
    // ...AND ECHO THE RESOLVED ONE, NOT THE DEFAULT. `resolveDataDir()` is registry-first and
    // `DATA_DIR` is its answer, so a device whose data dir was remapped has that path here. This used to
    // WRITE THE LITERAL DEFAULT back to the registry — while the tree below was created at `DATA_DIR` —
    // so the new tree landed at the remapped path and the registry then named the default, which is the
    // path the AGENT reads. The comment two lines up says "DataDir defaults to %ProgramData%\Summrise",
    // i.e. exactly what the code did not do: a default applies when nothing is set, not over a remap.
    regOk.push(regWrite("DataDir", DATA_DIR));
    // Pre-create the data dir tree (sessions/memory/logs — C1 separation).
    const DATA = DATA_DIR;
    for (const sub of ["sessions", "memory", "logs"]) {
      fs.mkdirSync(path.join(DATA, sub), { recursive: true });
    }
    if (!fs.existsSync(EXE_SRC)) {
      console.error("setup: summrise-agent.exe missing from package:", EXE_SRC);
      process.exit(1);
    }
    // The just-killed agent's file handle can lag a beat (and AV may scan the
    // fresh exe), so a bare copyFileSync raced EBUSY on reinstall — the update
    // swap retries this 12x; setup never did. Retry, re-killing a respawned
    // instance between attempts.
    {
      let copied = false;
      for (let i = 0; i < 12; i++) {
        try {
          fs.copyFileSync(EXE_SRC, EXE_DST);
          copied = true;
          break;
        } catch (e: any) {
          if (
            e?.code !== "EBUSY" &&
            e?.code !== "EPERM" &&
            e?.code !== "EACCES"
          ) {
            console.error("setup: exe copy failed:", e?.message || e);
            process.exit(1);
          }
          spawnSync(
            "cmd",
            ["/c", "taskkill", "/F", "/IM", "summrise-agent.exe"],
            {
              stdio: "ignore",
            },
          );
          try {
            Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 700);
          } catch {
            /* best-effort sleep */
          }
        }
      }
      if (!copied) {
        console.error(
          "setup: FATAL -- could not replace summrise-agent.exe (locked). Close any running Summrise agent and re-run the installer.",
        );
        process.exit(1);
      }
    }
    // B2: stage the boxed playwright bundle (node_modules ONLY — node.exe is
    // NOT bundled; the system node detected below runs it). Single small
    // artifact in the npm package, Summrise version-locked.
    const PW_ZIP = resolveComponent(
      "summrise-playwright.zip",
      path.join(__dirname, "..", "summrise-playwright.zip"),
    );
    if (PW_ZIP) {
      const pwDir = PW_DIR;
      fs.mkdirSync(pwDir, { recursive: true });
      // argv (`ps()`): PW_ZIP and COMPONENTS_DIR are paths, and cmd expands `%NAME%`
      // inside quotes (see `sh()`).
      ps(
        `Expand-Archive -Force -Path '${psq(PW_ZIP)}' -DestinationPath '${psq(COMPONENTS_DIR)}'`,
      );
      // round-163: the whole point of the bundle is node.exe — VERIFY it
      // landed (a silently-missing copy killed the bridge forever on d1).
      // One retry, then fail loudly: a half-staged bundle is worse than none.
      if (!fs.existsSync(path.join(pwDir, "node.exe"))) {
        console.log("setup: node.exe missing after expand -- retrying once");
        ps(
          `Expand-Archive -Force -Path '${psq(PW_ZIP)}' -DestinationPath '${psq(COMPONENTS_DIR)}'`,
        );
      }
      // "node_modules verified" was in the message while only node.exe was checked —
      // and node_modules is where the entry point the agent actually runs lives, so a
      // half-expanded bundle passed the check and failed at first use.
      const pwCli = path.join(
        pwDir,
        "node_modules",
        "@playwright",
        "mcp",
        "cli.js",
      );
      if (fs.existsSync(path.join(pwDir, "node.exe")) && fs.existsSync(pwCli)) {
        console.log(
          "setup: playwright bundle staged (node.exe + node_modules/@playwright/mcp verified)",
        );
      } else {
        // NEVER fatal. The browser bundle is an OPTIONAL component, but this
        // branch hard-failed (exit 1) the whole agent install the moment
        // 1.2.311 started shipping the zip — its trigger is AV quarantining
        // playwright\node.exe (documented on d1). Drop the half-staged tree so
        // the agent cleanly sees "no bundle" and carry on: the agent core
        // does not need playwright.
        console.error(
          "setup: WARNING -- playwright bundle staged WITHOUT node.exe (AV/lock interference?); browser tools stay disabled, agent install continues.",
        );
        try {
          fs.rmSync(pwDir, { recursive: true, force: true });
        } catch {
          /* best-effort */
        }
      }
    } else {
      console.error(
        "setup: WARNING -- the playwright bundle could not be obtained (not in the package, and the release host did not serve it); browser tools stay disabled.",
      );
    }
    // Node runtime: the device has node (npm works), but the agent runs as
    // SYSTEM which may not see the user PATH — resolve the ABSOLUTE node path
    // now and record it in the registry so the agent can spawn it.
    const nodeWhich = spawnSync("where", ["node"], { encoding: "utf8" });
    const nodePath =
      nodeWhich.status === 0 && nodeWhich.stdout
        ? nodeWhich.stdout.split(/\r?\n/)[0].trim()
        : "";
    if (nodePath) {
      try {
        regOk.push(regWrite("NodePath", nodePath));
        console.log("setup: system node detected:", nodePath);
      } catch {
        /* non-fatal */
      }
    } else {
      console.log(
        "setup: WARNING -- node not found in PATH (browser tools need node)",
      );
    }
    // C2: stage the boxed cloudflared binary into components/ (optional — local
    // mode works without it; only used when the user opts into public access).
    const CF_SRC = resolveComponent(
      "cloudflared.exe",
      path.join(__dirname, "..", "cloudflared.exe"),
    );
    if (CF_SRC) {
      fs.mkdirSync(COMPONENTS_DIR, { recursive: true });
      fs.copyFileSync(CF_SRC, path.join(COMPONENTS_DIR, "cloudflared.exe"));
      console.log(
        "setup: cloudflared staged (tunnel optional -- `summrise tunnel install` to enable)",
      );
    } else {
      // This branch used to be SILENT, and that silence is what the 2026-09-23
      // migration cost an hour of: a device with no cloudflared has no tunnel,
      // and a device with no tunnel cannot be reached from the console at all
      // while `summrise status` reports it healthy from inside.
      console.error(
        "setup: WARNING -- cloudflared could NOT be staged (not in the package, and the release host did not serve it); this device will be unreachable from the console until the binary can be fetched.",
      );
    }
    // P2-4: record the boxed-component versions (never fail-closed).
    writeBoxedVersions(DIR, path.join(__dirname, ".."));
    // round-330: Tauri summrise-desktop staging removed (retired).
    // stage-l: stage the Electron shell sources (main/preload) so the desktop
    // app picks up menu/command features on a fresh install too.
    const deskStaged = stageDesktopShell(DIR, "");
    // The claim follows the FACT. `stageDesktopShell` returns early when the sources
    // are not present, and this printed "sources staged" regardless — so a package that
    // did not ship them claimed they were staged.
    console.log(
      deskStaged > 0
        ? `setup: summrise-desktop-electron sources staged (${deskStaged} files)`
        : "setup: summrise-desktop-electron sources NOT staged -- the package did not ship them; the desktop shell will not update",
    );
    // …and the runtime those sources need. Sources without a runtime is a shell
    // that cannot start: no window, and (before this) nothing in the output said
    // why — the operator saw only an absent window.
    const elDist = ensureElectron();
    console.log(
      elDist
        ? "setup: electron runtime ready for the desktop shell"
        : "setup: WARNING -- the electron runtime could not be obtained (not in the package, and the release host did not serve it); start-desktop.ps1 will not open a window until it can be fetched.",
    );
    // ── Q9: leave a `summrise` COMMAND behind ──────────────────────────────────
    // `npx summrise-agent setup` installs the service and nothing puts the CLI on
    // PATH, so the very commands this output recommends (`summrise update`,
    // `summrise status`) are command-not-found — measured on d1 on 2026-09-23.
    // Installing THIS version by name keeps it deterministic and uses npm's own
    // shims rather than hand-written ones; when npm cannot, say the command.
    {
      const selfVer = String(require("../package.json").version || "");
      const pfx = spawnSync("npm", ["prefix", "-g"], {
        encoding: "utf8",
        shell: true,
      });
      const pre = pfx.status === 0 ? String(pfx.stdout || "").trim() : "";
      if (selfVer && pre) {
        // `shell: true` is REQUIRED here (npm on Windows is a `.cmd` shim, which a bare
        // spawnSync cannot execute) — and that shell JOINS THIS ARGV INTO ONE cmd.exe
        // LINE, the shape the `processRunning` comment above records for the tasklist
        // filter. `pre` is a PATH (npm's global prefix), so it is DOUBLE-QUOTED for the
        // cmd layer: unquoted, a prefix like `C:\Program Files\nodejs` was split at its
        // space and npm installed the CLI somewhere else while reporting success.
        //
        // QUOTING IS NOT ENOUGH FOR `pre`, because cmd expands `%NAME%` inside quotes —
        // so the path does not appear in this command line at all. `%SUMMRISE_NPM_PREFIX%`
        // does, and the value travels in the environment below. MEASURED on d1, through
        // npm.cmd ITSELF (the target here is a `.cmd` BATCH shim, and a batch file re-parses
        // its own arguments, so the /c line was not the only parser to clear):
        //   `npm config get prefix --prefix "%SUMMRISE_PROBE2%"`          -> C:\%ProgramFiles%\Summrise
        //   `npm config get prefix --prefix "C:\%ProgramFiles%\Summrise"` -> C:\C:\Program Files\Summrise
        // The reference is expanded ONCE and what it produced is not re-scanned, so a prefix
        // literally named `C:\%ProgramFiles%\Summrise` arrives as exactly that, while the raw
        // interpolation of the same value does not; the quotes still hold an `&` inside one
        // argument. The pin requires every `%NAME%` on a cmd line to be defined in the same
        // call's `env`.
        //
        // cmd-% version: `${selfVer}` is package.json's own version — a semver, read from
        // the file the tests read too, so it cannot carry a `%` (and it must be quoted for
        // the cmd layer regardless, or npm reads an empty spec).
        const inst = spawnSync(
          "npm",
          [
            "i",
            "-g",
            `"summrise-agent@${selfVer}"`,
            "--prefix",
            `"%SUMMRISE_NPM_PREFIX%"`,
            "--registry=https://registry.npmjs.org/",
            "--no-audit",
            "--no-fund",
          ],
          {
            encoding: "utf8",
            shell: true,
            timeout: 300000,
            env: { ...process.env, SUMMRISE_NPM_PREFIX: pre },
          },
        );
        console.log(
          inst.status === 0
            ? `setup: ${selfVer} installed globally -- \`summrise status\` works from any shell`
            : `setup: WARNING -- could not install the CLI globally; run: npm i -g summrise-agent@${selfVer}`,
        );
      } else {
        console.log(
          `setup: WARNING -- could not resolve npm's global prefix; run: npm i -g summrise-agent@${selfVer || "<version>"}`,
        );
      }
    }
    // ── the desktop shell's task: ONLOGON + a 5-minute watchdog ────────────────
    // `summrise autostart` has always said "the desktop task comes from the
    // installer", and the NSIS installer is RETIRED — so NOTHING created it. Measured
    // on d1: the window vanished with the console that launched it and
    // SummriseDesktop did not exist at all. The shape is the app's own, written to a
    // .ps1 so it can be read rather than escaped into one unreadable line:
    // ensure-desktop.ps1 exits when electron is already alive (the watchdog never
    // steals focus), the .vbs wrapper runs it with no console flash, and the 5-minute
    // repetition is what reborns a dead shell.
    try {
      const reg = path.join(SCRIPTS_DIR, "register-desktop-task.ps1");
      fs.writeFileSync(reg, desktopTaskPs(DIR).join("\r\n") + "\r\n");
      // THE STATUS WAS DISCARDED AND THE SUCCESS LINE WAS UNCONDITIONAL, which is the defect the AGENT
      // task's check ten lines below names in its own comment: "audit #7: used to claim success
      // regardless". Worse, the `catch` below could not fire for this — `sh()` RETURNS, it does not
      // throw — so the only thing it ever caught was `writeFileSync`.
      //
      // ONE CALL, and its status is READ. The path is an ARGV ELEMENT now, and that is the fix for
      // both quoting attempts that came before it: the single-quoted version this shipped with
      // reached PowerShell with the quotes INCLUDED and died on the device with "unsupported path
      // format" for a path that was perfectly valid, and the double-quoted version that replaced it
      // still handed a PATH to cmd — where `%NAME%` expands inside the quotes (see `sh()`). argv
      // needs no quotes at all: PowerShell receives the path verbatim.
      //
      // A WARNING, NOT FATAL, unlike the agent task: headless installs have no SummriseDesktop, and the
      // registration script itself exits quietly when electron is already alive.
      const reg302 = spawnSync(
        "powershell",
        ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", reg],
        { stdio: "pipe" },
      );
      if (reg302 && reg302.status === 0) {
        console.log(
          "setup: SummriseDesktop registered (logon + a 5-minute watchdog) and started",
        );
      } else {
        console.log(
          `setup: WARNING -- registering SummriseDesktop failed (status ${reg302 ? reg302.status : "spawn error"}); ` +
            `run: powershell -File "${SCRIPTS_DIR}\\register-desktop-task.ps1"`,
        );
      }
    } catch {
      console.log(
        `setup: WARNING -- could not register SummriseDesktop; run: powershell -File "${SCRIPTS_DIR}\\register-desktop-task.ps1"`,
      );
    }
    // Layout v2: write the start-desktop.ps1 launcher into scripts\ (the
    // SummriseDesktop onlogon task + desktop Summrise.lnk both call it). Was never
    // written before — a real gap that left the shell unlaunchable.
    fs.mkdirSync(SCRIPTS_DIR, { recursive: true });
    fs.writeFileSync(
      path.join(SCRIPTS_DIR, "start-desktop.ps1"),
      startDesktopPs(psq(DESK_DIR)).join("\r\n"),
      "utf8",
    );
    console.log("setup: scripts\\start-desktop.ps1 written");
    // Register boot-start task (SYSTEM) and kick it once; the agent's own
    // first-run flow registers the device with the console using the key.
    //
    // round-118: this used to be a raw `schtasks /Create /SC ONSTART`, which
    // inherits Task Scheduler defaults — a 72h execution limit that silently
    // kills the agent after 3 days (device goes dark until reboot), plus no
    // restart-on-failure. Register via ScheduledTask cmdlets with the full
    // hardening set instead (mirrors deploy/summrise-agent-setup.ps1):
    //   - ExecutionTimeLimit 0        never kill the running task
    //   - RestartOnFailure 8 x 1min   scheduler retries after a crash
    //   - battery-safe + StartWhenAvailable
    //   - 5-min repetition watchdog   IgnoreNew = no-op while running;
    //                                 restarts within <=5 min if dead
    //   - explicit config -Argument   layout v2 (never the exe path)
    const regRes = ps(bootTaskPs(psq(EXE_DST), psq(CFG_FILE), true).join("; "));
    if (!regRes || regRes.status !== 0) {
      console.error(
        "setup: FATAL -- task registration failed (audit #7: used to claim success regardless).",
      );
      process.exit(1);
    }
    // round-298 parity: a FRESH install must carry the release marker — without it
    // agent_update compares against the Cargo 1.0.x fallback and re-downloads + swaps
    // on every call.
    //
    // WRITTEN HERE, and that placement IS the fix: it used to sit ~20 lines ABOVE this
    // point while its comment claimed "after the exe copy + task registration (setup
    // provably succeeded)". The registration exits 1 on failure, so an aborted setup
    // left the marker claiming a version on an install with NO boot task — and
    // `summrise status` reported "(this device is current)" for a device that never starts.
    // A marker must not be written before the thing it attests to.
    writeReleaseMarker(DIR);
    // Control-panel entry for npm-path installs too (the NSIS writer owns
    // UninstallString on its installs; the helper never overwrites one).
    // Best-effort — a registry failure must not fail the install.
    try {
      const pkgVer = String(require("../package.json").version || "");
      const ureg = ps(uninstallRegBodyPs(psq(DIR), pkgVer).join("; "));
      console.log(
        "setup: control-panel uninstall entry" +
          (ureg && ureg.status === 0
            ? " ensured"
            : " (ensure failed -- uninstall via `summrise uninstall`)"),
      );
    } catch {
      console.log(
        "setup: control-panel entry skipped (uninstall via `summrise uninstall`)",
      );
    }
    console.log("setup: installed to", DIR);
    // One summary rather than N scattered warnings, and it names the CONSEQUENCE
    // (path resolution disagrees) instead of just the failed call.
    if (regOk.includes(false)) {
      console.error(
        "setup: WARNING -- the registry entry for this install is INCOMPLETE (" +
          regOk.filter((ok) => !ok).length +
          " of " +
          regOk.length +
          " writes failed). The agent resolves its paths from the registry, so it may look in the DEFAULT location instead of " +
          DIR +
          ". Re-run `summrise setup` elevated if that matters.",
      );
    }
    console.log(
      "setup: device registers on start -- check the console Devices list",
    );
    // Inbound firewall for the agent port (idempotent; inert when bound to
    // loopback, required for LAN clients otherwise). Best-effort, never
    // fail-closed — a locked-down box keeps working locally regardless.
    try {
      const fwPort = agentPort(ETC_DIR);
      const fw = ps(firewallPs(fwPort).join("; "));
      console.log(
        "setup: firewall inbound TCP " +
          fwPort +
          (fw && fw.status === 0
            ? " ensured"
            : " (ensure failed -- LAN clients may be blocked)"),
      );
    } catch {
      console.log(
        "setup: firewall ensure skipped (LAN clients may be blocked)",
      );
    }
    // Optional: provision the tunnel in the same command (no second step).
    if (wantTunnel) {
      console.log("setup: provisioning cloudflare tunnel...");
      initTunnel(tunnelHost, regKey);
    } else {
      console.log(
        "setup: no tunnel configured (local mode). Enable later with `summrise tunnel install <hostname>`.",
      );
    }
  },

  // ── monitor ────────────────────────────────────────────────────────────
  // The device's reachability instrument, in the terminal the operator already
  // works in. The panel draws the same numbers; this runs where the ssh session
  // is, needs no browser, and runs its live view until Ctrl+C.
  // async: `wait` blocks on probes (the dispatcher already awaits every command).
  async monitor(args) {
    const sub = String(args[0] || "list").toLowerCase();
    // `--expect <text>`: the page must CONTAIN this text, or the watch counts it as down — the
    // difference between a working UI and a login page that answers 200.
    const expectAt = args.indexOf("--expect");
    const expect = expectAt >= 0 ? String(args[expectAt + 1] || "") : "";
    const positional = args.filter(
      (a, i) => i > 0 && a !== "--expect" && i !== expectAt + 1,
    );
    if (expectAt >= 0 && !expect) {
      console.error(
        'usage: summrise monitor add <host:port[/path]> --expect "<text>"',
      );
      process.exit(1);
    }
    if (sub === "add" || sub === "rm" || sub === "remove") {
      const t = parseTargetArg(positional[0]);
      if (!t) {
        console.error(
          "usage: summrise monitor add <host:port[/path]>   (e.g. 192.168.1.1:80/ or 192.168.1.1:22)",
        );
        process.exit(1);
      }
      if (sub === "add") {
        const r = deviceApi("POST", "/api/monitors/add", {
          host: t.host,
          port: t.port,
          path: t.path,
          expect,
        });
        if (!r.ok) {
          console.error(`monitor add: ${r.error}`);
          process.exit(1);
        }
        if (r.body && r.body.ok === false) {
          console.error(`monitor add: ${r.body.error}`);
          process.exit(1);
        }
        // Probe once so the operator sees a reading instead of "no readings yet"
        // for one 15 s cycle — the same courtesy the panel gives.
        deviceApi("POST", "/api/monitors/probe", { id: t.id });
        console.log(
          `watching ${t.id}${t.path ? " (HTTP GET, status code recorded)" : " (TCP connect)"}` +
            (expect ? ` requiring the body to contain "${expect}"` : ""),
        );
        return;
      }
      const r = deviceApi("POST", "/api/monitors/remove", { id: t.id });
      if (!r.ok) {
        console.error(`monitor rm: ${r.error}`);
        process.exit(1);
      }
      console.log(
        r.body && r.body.removed
          ? `stopped watching ${t.id}`
          : `monitor rm: ${t.id} was not being watched`,
      );
      return;
    }
    if (sub === "probe") {
      // CHECK NOW. The device probes every 15 s on its own timer; this is for the moment after
      // a reboot or a config change, when waiting one cycle is the difference between "it is
      // back" and "I am still guessing". The AI has had this ability since it existed; the
      // operator's own terminal did not.
      const t = parseTargetArg(positional[0]);
      if (!t) {
        console.error("usage: summrise monitor probe <host:port[/path]>");
        process.exit(1);
      }
      const r = deviceApi("POST", "/api/tools/monitor_probe", { id: t.id });
      const payload = r.ok ? r.body : null;
      if (!r.ok) {
        console.error(`monitor probe: ${r.error}`);
        process.exit(1);
      }
      if (!payload || payload.ok !== true) {
        // The device refuses to probe what it is not watching (one rule, one place). The
        // CLI passes that on WITH the way out, instead of quietly adding a watch.
        const why =
          (payload && payload.error) || "the device refused the probe";
        console.error(`monitor probe: ${why}`);
        console.error(`  to watch it: summrise monitor add ${t.id}`);
        process.exit(1);
      }
      const probe = payload.result && payload.result.probe;
      // The device sends the CRITERION with the answer (`expect`), so "no match" can name the
      // text it looked for — an unreadable verdict is a verdict nobody can act on.
      console.log(
        probeLine(
          {
            id: t.id,
            expect: (payload.result && payload.result.expect) || null,
          },
          probe,
          Date.now(),
        ),
      );
      return;
    }
    if (sub !== "list") {
      console.error(
        "usage: summrise monitor [list [--json] | add <host:port[/path]> [--expect <text>] | probe <host:port[/path]> | rm <host:port[/path]>]",
      );
      process.exit(1);
    }
    const r = deviceApi("GET", "/api/monitors");
    if (!r.ok) {
      console.error(`monitor: ${r.error}`);
      process.exit(1);
    }
    if (args.includes("--json")) {
      // For a script: the device's numbers, one object, no colour and no prose. A device that
      // could not be reached is a NON-ZERO EXIT with the reason on stderr, never a JSON body
      // pretending everything is fine.
      const os = require("os");
      // ASCII-escaped for the same reason the request body is (see `asciiJson`): a PIPE is an
      // encoding boundary too. PowerShell decodes a child's output with the console code page
      // (CP936 on d1), so raw UTF-8 through a pipe came back as mojibake while the same text
      // printed directly read fine — the second face of this bug.
      console.log(
        asciiJson(
          monitorsJson({
            device: os.hostname(),
            askedAtMs: Date.now(),
            payload: r.body,
            only: null,
          }),
          2,
        ),
      );
      return;
    }
    const targets = (r.body && r.body.targets) || [];
    if (targets.length === 0) {
      console.log(
        "watching nothing. `summrise monitor add 192.168.1.1:22` starts one;",
      );
      console.log(
        "add a path to check a web UI instead of a port: `summrise monitor add 192.168.1.1:80/`",
      );
      return;
    }
    const now = Date.now();
    console.log(
      `watching ${targets.length} target(s), probed every ${r.body.interval_secs || 15}s:`,
    );
    for (const t of targets) console.log(targetLine(t, now));
  },
  // Live view: redraw in place every few seconds until Ctrl+C. `summrise monitor
  // <host:port[/path]>` narrows it to one target and adds its outage log.
  // `summrise desktop` = put the window back, or say it is already there.
  //
  // WHY IT EXISTS. The shell is normally started by the SummriseDesktop task, but the LAUNCHER
  // dies with the console that started it — measured twice on 2026-09-23, both times arriving
  // as "the electron window is gone again" — and until now there was no command to ask for it, only a .ps1 path
  // an operator had to know. Idempotent by construction (the task's ensure-desktop.ps1 exits
  // when electron is alive, so a running shell is never disturbed), and it distinguishes the
  // three outcomes. "ok" without saying WHICH is how an operator ends up staring at a desktop.
  desktop() {
    const reg = path.join(SCRIPTS_DIR, "register-desktop-task.ps1");
    fs.mkdirSync(SCRIPTS_DIR, { recursive: true });
    if (!fs.existsSync(reg)) {
      // A machine that never ran setup still gets a working task: the same script setup
      // writes, from the same builder, so the two cannot drift.
      fs.writeFileSync(reg, desktopTaskPs(DIR).join("\r\n") + "\r\n");
    }
    // THE START SCRIPT GOES IN A FILE, AND ONLY IT DECIDES WHETHER TO REGISTER. Spawning with
    // `shell: true` means cmd.exe, and **cmd splits a command on `&`** — the PowerShell call
    // operator in the middle of this logic cut the command in half on the device, so the `$t`
    // assignment never ran and it fell through to re-registering a task that already existed,
    // which then failed on the service account's principal. Two failures, one metacharacter.
    const start = path.join(SCRIPTS_DIR, "desktop-start.ps1");
    fs.writeFileSync(start, desktopStartPs(DIR).join("\r\n") + "\r\n");
    // The register script is WRITTEN here (a machine that never ran setup still gets a working
    // task, from the same builder setup uses) but NOT run: desktop-start.ps1 runs it only when
    // the task is genuinely missing.
    // ARGV, NO SHELL — the path is the reason. `shell: true` JOINS an args array into one cmd.exe
    // line and warns about it (DEP0190: "the arguments are not escaped, only concatenated" — the
    // warning the operator saw on the device the first time this command worked), and that line is
    // then re-parsed a second time: `%NAME%` expands even inside the double quotes the old
    // one-string form carried (see `sh()`), and this path lives under the install dir, which the
    // operator chooses. Passed as argv, the path reaches PowerShell VERBATIM as one element and
    // DEP0190 goes with the shell instead of being worked around.
    const r = spawnSync(
      "powershell",
      ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", start],
      { encoding: "utf8" },
    );
    const out = String(r.stdout || "").trim();
    if (out.endsWith("already-running")) {
      console.log(
        "desktop: the Summrise shell is ALREADY RUNNING (look for the window, or the tray icon)",
      );
      return;
    }
    if (out.endsWith("started")) {
      console.log(
        "desktop: started -- the window should be on your desktop now",
      );
      return;
    }
    console.error(
      `desktop: the shell did NOT come up. Try it directly to see why: powershell -File "${path.join(SCRIPTS_DIR, "start-desktop.ps1")}"`,
    );
    process.exit(1);
  },
  status() {
    // THE EXIT CODE IS 0 EVEN WHEN THE REPORT SAYS "UNKNOWN", AND THAT IS THE CONVENTION.
    //
    // `status` is asked for a REPORT; it produces one, and the verdict lives in the text. `tunnel`
    // exits 1 on the same unreadable process list (`:3655-3662`) because the thing IT was asked for —
    // a running tunnel — did not happen. The two are not inconsistent; they answer different
    // questions, and the difference is what the caller asked for rather than how bad the news is.
    //
    // A script that needs a VERDICT must not read this exit code. `monitor list --json` states its own
    // contract at `:2424-2426`: "A device that could not be reached is a NON-ZERO EXIT with the reason
    // on stderr, never a JSON body pretending everything is fine."
    //
    // NOT via shell: `shell: true` concatenates argv into one cmd.exe string,
    // so the unquoted filter "IMAGENAME eq …" was split at its spaces, tasklist
    // rejected it, and this ALWAYS printed STOPPED even with the agent running.
    // (IMAGENAME also takes no wildcard — the old `summrise-agent*` never matched.)
    const agentState = processRunning("summrise-agent.exe");
    // The report answers "where is this device, and is a swap still pending" —
    // the question that cost four hand reads (and one wrong conclusion) in the
    // incident. Gathering the facts is thin I/O; the SHAPE lives in the pure
    // `statusReport`, which is where the tests can reach it.
    let releaseVersion: string | null = null;
    try {
      const v = fs
        .readFileSync(path.join(ETC_DIR, ".summrise-release"), "utf8")
        .trim();
      if (v) releaseVersion = v;
    } catch {
      /* absent => unknown, never fabricated */
    }
    let updateMarkerMs: number | null = null;
    // Only ENOENT means "no marker". Any OTHER stat error (EACCES/EBUSY) used to land
    // here too and was reported as "update: none in flight" — a claim about an update
    // that may be running right now. This is the sibling of the `rollback --clear` fix:
    // the same catch treated a failure to READ as evidence of ABSENCE.
    let updateMarkerUnreadable = false;
    try {
      updateMarkerMs = fs.statSync(updateBusyPath()).mtimeMs;
    } catch (e: any) {
      if (e && e.code !== "ENOENT") updateMarkerUnreadable = true;
    }
    let packageVersion = "";
    try {
      packageVersion = String(require("../package.json").version || "");
    } catch {
      /* best-effort */
    }
    // What the TWO channels advertise, so `status` can answer "is this device
    // current" — the question every round of this project's log answered by
    // hand. Bounded HARD (3 s each): `status` is the command an operator runs
    // when something is already wrong, and a status that hangs on a network
    // blip is worse than one that says it could not check. `null` only when
    // BOTH are silent, which the report renders as "could NOT be checked" —
    // never as agreement.
    const latestVersion = latestReleaseVersion();

    for (const line of statusReport({
      agentRunning: agentState === "unknown" ? null : agentState === "yes",
      installDir: DIR,
      exeExists: fs.existsSync(EXE_DST),
      port: agentPort(ETC_DIR),
      releaseVersion,
      updateMarkerMs,
      updateMarkerUnreadable,
      packageVersion,
      latestVersion,
      nowMs: Date.now(),
    })) {
      console.log(line);
    }
  },

  start() {
    // CHECK IT, LIKE `stop` DOES. The comment on `svc` above says the return value exists because
    // discarding it made `start`/`restart` "silent either way" — and two of the four call sites still
    // discarded it, including this one, which is step 2 of every documented journey. An operator whose
    // agent never came up got exit 0 and no sentence at all.
    const r = svc("Run");
    if (r && r.status !== 0) {
      console.error(
        `summrise start: schtasks /Run failed (status ${r.status}) -- the agent is NOT running`,
      );
      process.exit(1);
    }
  },

  stop() {
    // Say WHY before the process goes: see markDeliberateStop.
    markDeliberateStop();
    const r = svc("End");
    if (r && r.status !== 0) {
      // A NON-ZERO `/End` IS NOT EVIDENCE THE AGENT IS STILL RUNNING, and the mirror of this mistake is
      // already documented twice in this file. `restart` says it about the identical condition: "a task
      // that was not running has nothing to end, so a non-zero /End is the ordinary case". `svc`'s own
      // comment records the ORIGINAL bug, which was the opposite — "`stop` printed 'stopped' and exited 0
      // for a missing task or an access-denied". So the exit code answers neither question.
      //
      // ASK INSTEAD. `autostart` reads `Get-ScheduledTask ... -ExpandProperty State` and its comment names
      // this exact shape — "a failed READ reported as evidence of ABSENCE" — which it calls the THIRD
      // instance. This is the fourth, from the other side: an ordinary read taken as evidence of PRESENCE,
      // with "the agent may still be running" printed for a task that had already stopped.
      const st = taskState(TASK);
      if (st === null) {
        // Could not read it — say that, rather than guessing either way (the three-state rule).
        console.error(
          `summrise stop: schtasks /End failed (status ${r.status}) and the task's state could not be read`,
        );
        process.exit(1);
      }
      if (st === "Running") {
        console.error(
          `summrise stop: schtasks /End failed (status ${r.status}) and the task is still Running`,
        );
        process.exit(1);
      }
      console.error(
        `summrise stop: schtasks /End returned ${r.status}, but the task is not Running -- nothing to stop`,
      );
    }
    console.log(
      "stopped -- revives via 'summrise start' or the 5-min watchdog ('summrise autostart off' opts out of autostart)",
    );
  },

  restart() {
    markDeliberateStop();
    // THE STOP IS REPORTED BUT NOT FATAL: a task that was not running has nothing to end, so a non-zero
    // `/End` is the ordinary case for "restart a stopped agent", and the operator asked for the START.
    const end = svc("End");
    if (end && end.status !== 0) {
      console.error(
        `summrise restart: schtasks /End failed (status ${end.status}) -- the old process may still be running`,
      );
    }
    sh("timeout /t 2 >nul");
    const run = svc("Run");
    if (run && run.status !== 0) {
      console.error(
        `summrise restart: schtasks /Run failed (status ${run.status}) -- the agent is NOT running`,
      );
      process.exit(1);
    }
  },

  // Boot switch: `summrise autostart on|off|status` (default status). Flips the
  // ENABLED flag on BOTH boot tasks (service + desktop shell) — this is the
  // only real "don't start at boot" control; `summrise stop` is one-shot and
  // the watchdog revives it. Missing task = skipped with a note (never
  // fatal — headless installs have no SummriseDesktop). State read via
  // Get-ScheduledTask (locale-independent enum, unlike schtasks /Query
  // headers which localize).
  autostart(args) {
    const sub = String(args[0] || "status").toLowerCase();
    if (sub === "status") {
      for (const t of BOOT_TASKS) {
        const r = spawnSync(
          "powershell",
          [
            "-NoProfile",
            "-Command",
            `(Get-ScheduledTask -TaskName '${t}' -ErrorAction SilentlyContinue | Select-Object -ExpandProperty State -ErrorAction SilentlyContinue)`,
          ],
          { encoding: "utf8" },
        );
        // THREE STATES, NOT ONE. Every failure to READ the state landed on the same
        // output as a genuinely absent task: a missing powershell (spawn error), a
        // `Get-ScheduledTask` that threw on permissions, and "the task is not there" all
        // printed "(not installed)" — so an operator asking "is autostart on?" was sent
        // to re-run setup by an access-denied. This is the THIRD instance of the shape
        // fixed in `statusReport` and `rollback status`: a failed READ reported as
        // evidence of ABSENCE.
        const err = r && (r.error as Error | undefined);
        const st = r ? r.status : null;
        const s = String((r && r.stdout) || "").trim();
        if (err || st === null || st !== 0) {
          const why = err
            ? err.message
            : String((r && r.stderr) || "").trim() || `exit ${st}`;
          console.log(`${t}: state UNKNOWN -- could not read it (${why})`);
        } else {
          console.log(`${t}: ${s || "(not installed)"}`);
        }
      }
      return;
    }
    if (sub !== "on" && sub !== "off") {
      console.error("usage: summrise autostart <on|off|status>");
      process.exit(1);
    }
    let failed = false;
    let skipped = 0;
    for (const t of BOOT_TASKS) {
      // ASK WHETHER THE TASK EXISTS FIRST. The comment above this loop has said
      // "Missing task = skipped with a note (never fatal -- headless installs have no
      // SummriseDesktop)" all along, and the code below it made a missing task FATAL — so a
      // headless install could not turn autostart off at all, and the advice printed with
      // it ("run summrise setup first") cannot help, because `summrise setup` never registers
      // SummriseDesktop (only the NSIS online installer does).
      const exists =
        spawnSync("schtasks", ["/Query", "/TN", t], { encoding: "utf8" })
          .status === 0;
      if (!exists) {
        console.log(
          `autostart: ${t} not installed -- skipped (headless install)`,
        );
        skipped += 1;
        continue;
      }
      const argv = autostartArgv(t, sub);
      const r = spawnSync(argv[0], argv.slice(1), { stdio: "inherit" });
      if (!r || r.status !== 0) {
        console.error(
          `autostart: ${t} ${sub} FAILED on an existing task (status ${r ? r.status : "?"}) -- the change did not take`,
        );
        failed = true;
      } else {
        console.log(`autostart: ${t} ${sub === "on" ? "enabled" : "disabled"}`);
      }
    }
    if (skipped === BOOT_TASKS.length) {
      console.error(
        "autostart: no boot tasks found -- nothing to switch. `summrise setup` registers the agent task; the desktop task comes from the installer.",
      );
      process.exit(1);
    }
    // Only claim the durable outcome when every EXISTING task actually changed.
    if (sub === "off" && !failed)
      console.log(
        "autostart: off -- tasks stay disabled across reboot until 'summrise autostart on'",
      );
    if (failed) process.exit(1);
  },

  async update() {
    // THE CLI CANNOT DELIVER A VERSION IT DOES NOT CARRY (round 201, measured on the operator's device):
    // "update requested 1.2.438 -> 1.2.438", "copy ok=True", and the release unchanged — while `summrise status` kept saying
    // "THIS DEVICE IS BEHIND ... run 'summrise update'", i.e. the product pointed at a command that could not help. The reason
    // is the parity marker below: `<install>/.summrise-release` is stamped with THIS PACKAGE'S version, and the device's
    // `agent_update` reads that file as its LOCAL version. An older CLI therefore stamps the install with the version it
    // already has, the swap installs the same build, and nothing moves. Refuse before touching the device, and say the
    // thing that works — the release flow has always been two steps.
    const latest = latestReleaseVersion();
    const selfVersion = String(require("../package.json").version || "");
    if (latest && selfVersion && isBehind(selfVersion, latest)) {
      console.error(
        `update: this CLI is ${selfVersion} and the release channel has ${latest}.` +
          "\n  Updating from here would stamp the device with " +
          selfVersion +
          " and change nothing, because the device reads <install>/.summrise-release (written by this package) as its version." +
          "\n  Install the new CLI first, then update again:" +
          "\n    npm i -g summrise-agent" +
          "\n    summrise update",
      );
      process.exit(1);
    }
    // AND THE SAME REFUSAL WITHOUT THE NETWORK. The guard above cannot fire when the CDN is
    // unreadable, and the defect it exists for is exactly what happens then: an update that stamps the
    // install with a version it already has, swaps in the same build, and reports success while
    // `status` keeps saying the device is behind.
    let deviceNow = "";
    try {
      deviceNow = fs
        .readFileSync(path.join(ETC_DIR, ".summrise-release"), "utf8")
        .trim();
    } catch {
      // No marker is not a no-op; the update below writes one.
    }
    if (updateWouldNotMove(deviceNow, selfVersion)) {
      console.error(
        `update: this CLI is ${selfVersion} and the device is ALREADY on ${deviceNow}.` +
          "\n  Updating from here would stamp the device with the version it already has and change nothing," +
          "\n  because the device reads <install>/.summrise-release (written by this package) as its version." +
          "\n  If the device is meant to be newer, this CLI is too old to deliver it: install the new one first." +
          "\n    npm i -g summrise-agent" +
          "\n    summrise update",
      );
      process.exit(1);
    }
    // npm audit #10: no mutual exclusion — two updates (or setup racing a
    // swap) interleave Copy-Item on *.new, leaving a half-written exe "ok".
    // setup REMOVES the marker; update now CREATES it (refuse if <10 min
    // old); the swap script clears it after restart.
    // MEDIUM npm audit: use 'wx' exclusive-create so two racing updaters
    // cannot BOTH pass the freshness check — the second openSync throws
    // EEXIST and the WMI swap is never launched concurrently.
    const BUSYM = updateBusyPath();
    try {
      fs.mkdirSync(path.dirname(BUSYM), { recursive: true });
      const fd = fs.openSync(BUSYM, "wx");
      fs.writeSync(fd, String(Date.now()));
      fs.closeSync(fd);
    } catch (e: any) {
      if (e?.code === "EEXIST") {
        // Exists → check freshness (the owner may have died mid-swap).
        try {
          const st = fs.statSync(BUSYM);
          if (busyIsFresh(st.mtimeMs, Date.now())) {
            console.error(
              "update: another update looks in progress (" +
                BUSYM +
                " <10 min old) -- wait, or delete the marker after a mid-swap reboot",
            );
            process.exit(1);
          }
          // Stale marker — overwrite it.
          fs.writeFileSync(BUSYM, String(Date.now()));
        } catch {
          console.error(
            "update: another update looks in progress (cannot stat " +
              BUSYM +
              ")",
          );
          process.exit(1);
        }
      } else {
        throw e; // a real FS error — do not proceed
      }
    }
    // THE RECEIPT, before anything irreversible happens. Everything past this
    // point ends with the agent being killed — which is the DOCUMENTED success
    // signal ("the connection drops for ~10 s"), and therefore indistinguishable
    // from a transport failure that never ran this command at all. One line in
    // the log the swap itself appends to is what separates the two cases for
    // whoever reads the device afterwards.
    let fromVersion = "";
    try {
      fromVersion = fs
        .readFileSync(path.join(ETC_DIR, ".summrise-release"), "utf8")
        .trim();
    } catch {
      /* unknown */
    }
    let toVersion = "";
    try {
      toVersion = String(require("../package.json").version || "");
    } catch {
      /* best-effort */
    }
    console.log(
      `update: ${fromVersion || "unknown"} -> ${toVersion || "unknown"} -- staging, the connection will drop`,
    );
    // `ps()` RETURNS its spawn result, it does not throw (npm audit #7 made it return
    // precisely because a discarded result printed success anyway). So this try/catch
    // could never fire and a failed receipt write was SILENT — and the receipt is the
    // ONE artifact that separates "the CLI ran but the swap did not" from "nothing ran
    // at all" (the round-17 incident). Silent here means the log looks like the command
    // never reached the device. Best-effort, so it warns rather than aborting.
    const receipt = ps(
      updateReceiptPs(
        psq(DATA_DIR),
        fromVersion || "unknown",
        toVersion || "unknown",
      ).join("; "),
    );
    // MARK THE SWAP AS DELIBERATE, at the same instant as the receipt and for the same reason:
    // past this point the agent is killed from outside and writes no goodbye of its own, so the
    // next start has to GUESS — and the guess is wrong whenever the swap is slow (d1 measured a
    // 61 s gap, the boot task revives a crash in ~60 s, so the two overlap and genuine updates were
    // filed as CRASHES). With the mark, the next start reads `reason=update` and reports `replaced`.
    markDeliberateStop("update");
    if (!receipt || receipt.status !== 0 || receipt.error) {
      console.error(
        "update: WARNING -- could not write the receipt to summrise-update.log (" +
          ((receipt && receipt.error && receipt.error.message) ||
            `exit ${receipt ? receipt.status : "?"}`) +
          "); if this swap fails, the log will not distinguish it from a command that never arrived",
      );
    }
    // Swap the exe in-place: stop -> replace (with retry; the running agent
    // locks its own file) -> start.
    //
    // THE MARKER IS ALREADY CREATED at this point, and the only things that
    // release it are the WMI-failure handler below and the swap script's own
    // cleanup. Staging in between is NOT guarded by either: a full disk, an
    // antivirus lock or a permission error on the copy throws straight out to
    // the top level, leaving the marker behind — and the NEXT `summrise update`
    // then refuses for ten minutes citing an update that never started, while
    // the operator sees only a stack trace. Its neighbours
    // (`writeBoxedVersions`, `writeReleaseMarker`) are best-effort for the same
    // reason; this is the one region where a throw strands a LOCK.
    try {
      if (!fs.existsSync(EXE_SRC)) {
        console.error("exe missing from package:", EXE_SRC);
        throw new Error("exe missing from package: " + EXE_SRC);
      }
      fs.mkdirSync(DIR, { recursive: true });
      fs.copyFileSync(EXE_SRC, path.join(DIR, "summrise-agent.new.exe"));
      // stage-l: ship the Electron desktop shell's main/preload alongside —
      // the desktop app (components\summrise-desktop-electron) loads these sources;
      // without the sync, new menu/command features never reach the device.
      // (setup writes in place; update stages *.new for the atomic swap.)
      stageDesktopShell(DIR, ".new");
    } catch (e: any) {
      try {
        fs.unlinkSync(BUSYM);
      } catch {
        /* never created, or already gone */
      }
      console.error(
        "update: staging failed before the swap (" +
          (e && e.message ? e.message : e) +
          ")",
      );
      console.error(
        "update: nothing was swapped and the in-progress marker was released -- safe to re-run",
      );
      process.exit(1);
    }
    // P2-4: refresh the boxed-component manifest from the staged package +
    // the current install dir (best-effort, never fail-closed).
    writeBoxedVersions(DIR, path.join(__dirname, ".."));
    const q = DIR.replace(/'/g, "''");
    const qd = DATA_DIR.replace(/'/g, "''");
    const log = `Out-File '${qd}\\logs\\summrise-update.log' -Append`;
    // round-143: write the run-hidden.vbs wrapper next to node.exe, so the
    // SummrisePlaywright scheduled task can launch node.exe without flashing a
    // visible cmd window. Idempotent — overwrites any existing copy.
    // Layout v2: launchers live in scripts\, the runtime stays in
    // components\playwright\. The old-layout gate keeps migrating devices
    // refreshed too (migration carries the files over regardless).
    const pwDir = PW_DIR;
    const vbsPath = path.join(SCRIPTS_DIR, "run-hidden.vbs");
    // round-246 (browser-display audit C3) + round-257 + round-263:
    // ONE-BROWSER — the AI must drive the SAME browser the user watches:
    // the Electron desktop embedded WebContentsView (CDP 9333). The
    // SummrisePlaywright task used to launch playwright-mcp with --headless (a
    // PRIVATE chromium nobody sees). The task now goes through a probe
    // launcher that attaches to the DESKTOP view (9333) and falls back to a
    // private headless only when the desktop is down (agent restart window).
    // The bridge chromium (9223) tier was removed in round-263.
    const probePath = path.join(SCRIPTS_DIR, "playwright-probe.ps1");
    fs.mkdirSync(SCRIPTS_DIR, { recursive: true });
    if (fs.existsSync(pwDir) || fs.existsSync(path.join(DIR, "playwright"))) {
      // round-143: ASCII-only VBS (no em-dash, no Unicode). VBScript on
      // Windows uses the system locale; non-ASCII in comments corrupts the
      // file and causes "unterminated string constant" (800A0409). Use chr(34)
      // to produce literal double-quotes without string-escaping issues.
      fs.writeFileSync(
        vbsPath,
        [
          "Dim sh,cmd,i",
          'Set sh=CreateObject("WScript.Shell")',
          'cmd=chr(34) & WScript.Arguments(0) & chr(34) & " " & chr(34) & WScript.Arguments(1) & chr(34)',
          "For i=2 To WScript.Arguments.Count-1",
          '  cmd=cmd & " " & WScript.Arguments(i)',
          "Next",
          "sh.Run cmd,0,False",
        ].join("\r\n"),
      );
      // round-246 (C3) + round-257 + round-263: the probe launcher
      // (playwrightProbePs, unit-tested).
      fs.writeFileSync(probePath, playwrightProbePs().join("\r\n"));
    }
    // Layout v2: write the start-desktop.ps1 launcher into scripts\ (the
    // SummriseDesktop onlogon task + desktop Summrise.lnk both call it). Was never
    // written before — a real gap that left the shell unlaunchable. Written
    // unconditionally: on a pre-v2 device components\ appears only after
    // the swap script's migration, and a headless install simply never
    // calls the launcher (shortcut repair is Test-Path guarded anyway).
    fs.mkdirSync(SCRIPTS_DIR, { recursive: true });
    fs.writeFileSync(
      path.join(SCRIPTS_DIR, "start-desktop.ps1"),
      startDesktopPs(psq(DESK_DIR)).join("\r\n"),
      "utf8",
    );
    // round-298: record the release version on a PROVABLY successful swap
    // so agent_update (which reads <install>/.summrise-release as its local
    // version) reports up_to_date instead of re-swapping every call.
    let relVer = "";
    try {
      relVer = String(require("../package.json").version || "");
    } catch {
      /* best-effort */
    }
    const script = [
      `"[$(Get-Date -Format o)] update start" | ${log}`,
      // Layout v2 FIRST: repoint the boot task at the explicit config path
      // BEFORE touching anything (fail-closed — a config-path argument boots
      // old AND new agents alike, so aborting here leaves the old version
      // running untouched). Without this the moved config strands the boot.
      `try { ${bootTaskPs(`${q}\\summrise-agent.exe`, `${q}\\etc\\config.yaml`, false).join("; ")} } catch { "[$(Get-Date -Format o)] task repoint FAILED: $($_.Exception.Message)" | ${log}; try { Remove-Item -Force (${busyMarkerPs()}) } catch {}; exit 1 }`,
      `"[$(Get-Date -Format o)] task repointed at etc\\config.yaml" | ${log}`,
      // Layout-v2 migration (ADR 0008): move pre-v2 root paths into their
      // v2 homes. Best-effort per item; the gate below is fail-closed.
      ...migrateLayoutPs(q, qd),
      // Fail-closed gate: the new agent reads ONLY the v2 homes. A missing
      // config/hostname here means migration failed — do NOT swap (the old
      // exe keeps running the old layout until the next update).
      `if ((-not (Test-Path '${q}\\etc\\config.yaml')) -or (-not (Test-Path '${q}\\etc\\summrise-agent.hostname'))) { "[$(Get-Date -Format o)] migration gate FAILED (etc\\config.yaml/hostname missing) -- aborting, old version keeps running" | ${log}; try { Remove-Item -Force (${busyMarkerPs()}) } catch {}; exit 1 }`,
      // A running exe cannot be overwritten on Windows — stop the service
      // first (task end + process kill), THEN swap with retry.
      "try { Stop-ScheduledTask SummriseAgent -ErrorAction Stop } catch {}",
      "Get-Process summrise-agent -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue",
      "Start-Sleep -Milliseconds 1500",
      "$ok=$false",
      `foreach($i in 1..12){ try { Copy-Item -Force -ErrorAction Stop '${q}\\summrise-agent.new.exe' '${q}\\summrise-agent.exe'; $ok=$true; break } catch { Start-Sleep -Milliseconds 800 } }`,
      `"[$(Get-Date -Format o)] copy ok=$ok" | ${log}`,
      // round-298: .summrise-release is only written when the copy provably
      // completed (a failed swap keeps the device on the OLD exe — the
      // marker must not lie). The marker is what agent_update compares.
      `if ($ok -and '${relVer}') { Set-Content -Path '${q}\\etc\\.summrise-release' -Value '${relVer}' -NoNewline -ErrorAction SilentlyContinue }`,
      // Add/Remove-Programs parity (same $ok gate — a failed swap must not
      // move the displayed version either).
      ...uninstallVersionPs(q, relVer),
      `Remove-Item -Force -ErrorAction SilentlyContinue '${q}\\summrise-agent.new.exe'`,
      // stage-l: swap the desktop shell sources (main/preload) with retry —
      // the running Electron may hold them briefly.
      `foreach($df in @('main.js','preload.js','url-policy.js')){ $ds='${q}\\components\\summrise-desktop-electron\\src\\'+$df+'.new'; if (Test-Path $ds) { $ok2=$false; foreach($i in 1..8){ try { Copy-Item -Force -ErrorAction Stop $ds ('${q}\\components\\summrise-desktop-electron\\src\\'+$df); $ok2=$true; break } catch { Start-Sleep -Milliseconds 500 } }; Remove-Item -Force -ErrorAction SilentlyContinue $ds; "[$(Get-Date -Format o)] desk $df ok=$ok2" | ${log} } }`,
      // NEVER leave the device dark: even a failed swap must bring the task
      // back up (it will run the old exe until the next update).
      `try { Start-ScheduledTask SummriseAgent -ErrorAction Stop } catch { schtasks /Run /TN SummriseAgent }`,
      `"[$(Get-Date -Format o)] task restarted" | ${log}`,
      `try { Remove-Item -Force (${busyMarkerPs()}) } catch {}`,
      // Custom-port installs: the firewall rule must track the configured
      // bind port (baked at update time from the live config.yaml — the
      // swap itself runs from a static file and cannot read it).
      ...firewallPs(agentPort(ETC_DIR)),
      // stage-n: restart the Electron shell so newly-synced main/preload
      // sources take effect. The shell is INDEPENDENT of the SummriseAgent task —
      // it probes the configured port and loads /desktop/. Kill + relaunch
      // via start-desktop.ps1 (the same path SummriseDesktop onlogon uses); if
      // the task/script is missing (non-desktop install), skip silently.
      `$deskDir = '${q}\\components\\summrise-desktop-electron'`,
      `$deskStart = '${q}\\scripts\\start-desktop.ps1'`,
      // stage-n: harden the SHELL supervisor itself — SummriseDesktop gains a
      // 5-minute repetition trigger so a dead electron is reborn within
      // ≤5 min (previously only started at logon: "the watchdog died" left
      // d1 dark on 2026-09-25). Two field-test lessons encoded here:
      //  - `schtasks /Change /RI 5` prompts for the /ru password interactively
      //    (hung the PTY) and PS-array invocation mangles cmd-style args —
      //    use the ScheduledTasks cmdlets (SYSTEM runs them without prompt,
      //    mirroring SummriseAgent's proven -Once + -RepetitionInterval pattern).
      //  - the pulse must NOT start a second electron while one is alive
      //    (second-instance focuses the window = focus steal every 5 min) —
      //    the guarded ensure-desktop.ps1 checks Get-Process first, and the
      //    wscript wrapper runs it with no console flash.
      `$en1 = '${q}\\scripts\\ensure-desktop.ps1'`,
      `$vb1 = '${q}\\scripts\\desktop-pulse.vbs'`,
      `Set-Content -Path $en1 -Value 'if (Get-Process electron -ErrorAction SilentlyContinue) { exit }; & powershell -NoProfile -ExecutionPolicy Bypass -File "${q}\\scripts\\start-desktop.ps1"' -Force`,
      `Set-Content -Path $vb1 -Value 'CreateObject("WScript.Shell").Run "powershell -NoProfile -ExecutionPolicy Bypass -File " & Chr(34) & "${q}\\scripts\\ensure-desktop.ps1" & Chr(34), 0, False' -Force`,
      `if ($null -ne (Get-ScheduledTask -TaskName 'SummriseDesktop' -ErrorAction SilentlyContinue)) {`,
      `  $da = New-ScheduledTaskAction -Execute 'wscript.exe' -Argument ('"' + $vb1 + '"') -WorkingDirectory '${q}'`,
      `  $dt1 = New-ScheduledTaskTrigger -AtLogOn`,
      `  $dw1 = New-ScheduledTaskTrigger -Once -At (Get-Date).AddMinutes(3) -RepetitionInterval (New-TimeSpan -Minutes 5)`,
      `  Set-ScheduledTask -TaskName 'SummriseDesktop' -Action $da -Trigger @($dt1, $dw1) | Out-Null`,
      `  "[$(Get-Date -Format o)] desk: SummriseDesktop hardened (guarded 5-min pulse)" | ${log}`,
      `}`,
      `if ((Test-Path $deskDir) -and (Test-Path $deskStart)) {`,
      `  "[$(Get-Date -Format o)] desk: restarting electron shell" | ${log}`,
      `  Get-Process electron -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue`,
      `  Start-Sleep -Milliseconds 1500`,
      `  $deskTask = Get-ScheduledTask -TaskName 'SummriseDesktop' -ErrorAction SilentlyContinue`,
      // Start-ScheduledTask works for Ready AND Running tasks (Running is a
      // no-op) — use it unconditionally; the WMI-hosted swap process must not
      // spawn electron as its own child (it would be reaped with us; proven
      // twice on d1).
      `  if ($deskTask) { Start-ScheduledTask -TaskName 'SummriseDesktop' -ErrorAction SilentlyContinue }`,
      // no SummriseDesktop task (headless install): `cmd start` detaches the
      // shell from the swap host so it is not reaped with it.
      `  else { & cmd /c start /min "" powershell -NoProfile -ExecutionPolicy Bypass -File "$deskStart" }`,
      `  "[$(Get-Date -Format o)] desk: electron restart initiated" | ${log}`,
      `} else { "[$(Get-Date -Format o)] desk: no electron shell (skipped)" | ${log} }`,
      // stage-brand: heal a stale desktop shortcut (Summrise.lnk -> retired
      // Tauri exe) + drop the retired orphans. Repair-only, best-effort.
      ...deskShortcutRepairPs(
        `${q}\\scripts`,
        `${q}\\components\\summrise-desktop-electron`,
        log,
      ),
      // round-143: re-register SummrisePlaywright via the wscript/VBS wrapper so
      // node.exe no longer allocates a visible console. Idempotent — task may
      // not exist (older install paths), so wrap in try/catch.
      `$pwVbs = '${q}\\scripts\\run-hidden.vbs'`,
      `$pwProbe = '${q}\\scripts\\playwright-probe.ps1'`,
      `if ((Test-Path $pwVbs) -and (Test-Path $pwProbe)) {`,
      `  $pwNode = '${q}\\components\\playwright\\node.exe'`,
      `  $pwCli  = '${q}\\components\\playwright\\node_modules\\@playwright\\mcp\\cli.js'`,
      `  if ((Test-Path $pwNode) -and (Test-Path $pwCli)) {`, // parens: bare -and is a param parse error
      // Read the CURRENT task's UserId BEFORE unregistering — we need to know
      // who the task runs as (Administrator), but $env:USERNAME returns
      // "SYSTEM" when spawned via WMI, and Win32_ComputerSystem.UserName is
      // empty from session 0. The existing task's Principal is the most
      // reliable source. Fall back to the local user if the task doesn't exist.
      `    $oldTask = Get-ScheduledTask -TaskName 'SummrisePlaywright' -ErrorAction SilentlyContinue`,
      `    $pwUser = if ($oldTask) { $oldTask.Principal.UserId } else { (Get-CimInstance Win32_ComputerSystem -ErrorAction SilentlyContinue).UserName -replace '^.*\\\\', '' }`,
      `    try { Unregister-ScheduledTask -TaskName 'SummrisePlaywright' -Confirm:$false -ErrorAction SilentlyContinue } catch {}`,
      // round-246 (C3) + round-263: route through the probe launcher — it
      // attaches to the Electron desktop view (CDP 9333) when up, so AI
      // actions on 9229 drive the SAME browser the user watches (no more
      // invisible private headless).
      `    $pwPs = Join-Path $env:SystemRoot 'System32\\WindowsPowerShell\\v1.0\\powershell.exe'`,
      `    $pwArgs = '"' + $pwVbs + '" "' + $pwPs + '" -NoProfile -File "' + $pwProbe + '" "' + $pwNode + '" "' + $pwCli + '"'`,
      `    $pwAction = New-ScheduledTaskAction -Execute (Join-Path $env:SystemRoot 'System32\\wscript.exe') -Argument $pwArgs`,
      `    $pwBoot = New-ScheduledTaskTrigger -AtLogOn`,
      `    $pwWatch = New-ScheduledTaskTrigger -Once -At (Get-Date).AddMinutes(1) -RepetitionInterval (New-TimeSpan -Minutes 5)`,
      `    $pwSettings = New-ScheduledTaskSettingsSet -ExecutionTimeLimit (New-TimeSpan -Seconds 0) -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable`,
      `    Register-ScheduledTask -TaskName 'SummrisePlaywright' -Action $pwAction -Trigger @($pwBoot, $pwWatch) -Principal (New-ScheduledTaskPrincipal -UserId $pwUser -LogonType Interactive -RunLevel Limited) -Settings $pwSettings -Force | Out-Null`,
      `    Start-ScheduledTask -TaskName 'SummrisePlaywright' | Out-Null`,
      `    "[$(Get-Date -Format o)] SummrisePlaywright re-registered (probe launcher, user=$pwUser)" | ${log}`,
      `  }`,
      `}`,
      // Layout v2: the swap script itself is transient — delete it last
      // (the Rust agent_update twin already self-deletes; this one lingered
      // at the install root forever).
      `try { Remove-Item -LiteralPath $PSCommandPath -Force -ErrorAction Stop } catch {}`,
    ].join("\r\n");
    fs.mkdirSync(SCRIPTS_DIR, { recursive: true });
    fs.writeFileSync(path.join(SCRIPTS_DIR, "summrise-update.ps1"), script);
    // Launch the swap via WMI Win32_Process.Create: the child is parented by
    // WmiPrvSE, outside any caller job, so it survives this CLI (and the
    // agent it kills) dying — node's detached spawn does NOT (observed d1).
    //
    // Flags matter on this path: children created via WMI with
    // `-ExecutionPolicy Bypass` or `-EncodedCommand` in their command line
    // die silently before running anything (d1, no Defender ASR events —
    // cause unconfirmed). Plain `powershell -NoProfile -File` works. That
    // requires script execution to be allowed, so lift Restricted here once;
    // RemoteSigned is Microsoft's recommended default for automation hosts.
    spawnSync(
      "powershell",
      [
        "-NoProfile",
        "-Command",
        "if((Get-ExecutionPolicy) -eq 'Restricted'){ Set-ExecutionPolicy RemoteSigned -Scope LocalMachine -Force }",
      ],
      { stdio: "ignore", timeout: 30000 },
    );
    const ps1 = path.join(SCRIPTS_DIR, "summrise-update.ps1");
    // stage-n npm audit LOW: DIR can contain characters that break the
    // inner PS double-quote literal (backslash, quote). Escape for the
    // inner -File arg; the outer WMI literal is already escaped on L562.
    const ps1Safe = ps1.replace(/\\/g, "\\\\").replace(/"/g, '\\"');
    const inner = `powershell -NoProfile -File "${ps1Safe}"`;
    const wmi = `Invoke-CimMethod -ClassName Win32_Process -MethodName Create -Arguments @{CommandLine='${inner.replace(/'/g, "''")}'} | ConvertTo-Json -Compress`;
    const r = spawnSync("powershell", ["-NoProfile", "-Command", wmi], {
      stdio: ["ignore", "pipe", "pipe"],
      timeout: 20000,
    });
    // review #2 (HIGH): Win32_Process.Create reports success via
    // ReturnValue=0, but the old stdio:"inherit" printed the object and
    // checked only powershell's own exit code — a Create that returned 9
    // (path not found) / 21 still printed "swap launched" and the update
    // was a silent no-op (exactly incident #1's class). Parse the code.
    let retval: number | null = null;
    try {
      const j = JSON.parse((r.stdout || Buffer.from("")).toString().trim());
      retval = typeof j?.ReturnValue === "number" ? j.ReturnValue : null;
    } catch {
      /* fall through to the status/retval guard below */
    }
    if (r.status !== 0 || retval !== 0) {
      try {
        fs.unlinkSync(BUSYM);
      } catch {
        /* never created */
      }
      console.error(
        `update: WMI handoff failed (ps status ${r.status}, ReturnValue ${retval ?? "?"})` +
          (r.stderr ? " — " + r.stderr.toString().trim() : ""),
      );
      process.exit(1);
    }
    // ASK THE DEVICE, DO NOT TRUST THE HANDOFF. `ReturnValue=0` means a process was
    // created; every decision that matters (the fail-closed migration gate, the 12x
    // copy retry, the task restart) happens after, in a WmiPrvSE-parented script whose
    // exit code nobody reads — so a swap that fails one second later looked identical
    // to one that worked. `rollback` already solves this by reading the release marker
    // back; `update` reported the HANDOFF instead and this is the only reason the two
    // commands disagreed about whether an update took.
    //
    // The read is a FILE, not the network, so the dropped connection does not matter.
    // A successful swap finishes in ~10s; the bound only elapses on failure, and it is
    // the same bound `rollback` uses.
    console.log("update: swap launched -- waiting for the device to confirm");
    const markerFile = path.join(ETC_DIR, ".summrise-release");
    const check = await awaitReleaseMarker({
      want: toVersion,
      timeoutMs: 90_000,
      intervalMs: 2_000,
      read: () => fs.readFileSync(markerFile, "utf8"),
      sleep: (ms) => new Promise((r) => setTimeout(r, ms)),
      now: () => Date.now(),
    });
    const verdict = releaseMarkerVerdict({
      ...check,
      want: toVersion,
      // THE VERB MATTERS: this is an INSTALL that failed, so the advice names the previous version as
      // the thing to pin — not the one that just failed to install.
      verb: "update",
      from: fromVersion,
    });
    if (verdict.writePin) {
      console.log(
        `update: ${fromVersion || "?"} -> ${toVersion} COMPLETE (the device reported the new release)`,
      );
    } else {
      // Not a pin decision here — `update` never writes one — but the same verdict
      // logic answers "did it take", which is the question the operator asked.
      console.error(verdict.message);
      process.exit(1);
    }
  },

  // `summrise rollback <x.y.z> | --clear` — pin the device to a CDN-retained
  // release and prevent the agent_update auto-upgrade from undoing it.
  // Mechanics (the swap itself reuses the installed package's `update`):
  //  1. HEAD-check the pinned tgz (CDN keeps last-5-per-minor — a pruned
  //     version fails HERE with a clear message, not as an npm 404 storm).
  //  2. npm install -g --prefix <components\npm-global> <tgz> — replaces
  //     the package whose bin/summrise.js + exe the swap below uses.
  //  3. <npm-global>\summrise.cmd update — runs the TARGET version's own swap
  //     (its staged exe IS the rollback build). Old (pre-v2) scripts write
  //     .summrise-release to the install ROOT; steps 4/5 heal that: sync the
  //     marker into etc\ and delete the root leftover so agent_update can
  //     never read a split-brain version.
  //  4/5. write etc\.rollback-pin = <ver>, sync etc\.summrise-release.
  // agent_update (Rust) refuses any non-matching version while the pin
  // exists; force:true (or a real Rust-side upgrade) clears it — same for
  // a later `summrise rollback --clear`. A human `summrise update` deliberately
  // does NOT clear the pin: the update flow swaps whatever npm-global
  // holds, so after rollback that IS the pinned version (no-op), and the
  // pin stays authoritative for the auto path.
  async rollback(args) {
    const NPM_GLOBAL = path.join(COMPONENTS_DIR, "npm-global");
    const PIN = path.join(ETC_DIR, ".rollback-pin");
    const val = String(args[0] || "");
    if (val === "--clear") {
      // A FAILED DELETE IS NOT AN ABSENT PIN. `rmSync(force)` ignores only ENOENT;
      // EPERM/EACCES/EBUSY/EISDIR landed in this same catch, so a pin that SURVIVED was
      // reported as "nothing to clear" and the command returned 0 — while
      // `rollback status` still said "pinned" and agent_update kept refusing every
      // release, which is the state that governs auto-updates.
      let cur: string | null = null;
      try {
        cur = fs.readFileSync(PIN, "utf8").trim();
      } catch (e: any) {
        if (e && e.code !== "ENOENT") {
          console.error(
            `rollback: could not READ ${PIN} (${e.code || e.message}) -- not assuming it is absent`,
          );
          process.exitCode = 1;
          return;
        }
      }
      try {
        fs.rmSync(PIN, { force: true });
      } catch (e: any) {
        // fall through to the read-back below, which is what decides.
      }
      if (fs.existsSync(PIN)) {
        console.error(
          `rollback: FAILED to clear ${PIN} -- the pin is still in place and agent_update keeps refusing releases`,
        );
        process.exitCode = 1;
      } else if (cur !== null) {
        console.log(
          `rollback: pin cleared (was ${cur || "?"}) -- agent_update tracks the release channel again`,
        );
      } else {
        console.log("rollback: no pin present (nothing to clear)");
      }
      return;
    }
    if (val === "status") {
      try {
        console.log("rollback: pinned to", fs.readFileSync(PIN, "utf8").trim());
      } catch (e: any) {
        // "not pinned" is a claim about ABSENCE; a read error is not evidence of it.
        if (e && e.code !== "ENOENT") {
          console.error(
            `rollback: could not read ${PIN} (${e.code || e.message}) -- pin state UNKNOWN`,
          );
          process.exitCode = 1;
        } else {
          console.log("rollback: not pinned (tracks the release channel)");
        }
      }
      return;
    }
    if (!rollbackVersionOk(val)) {
      console.error("usage: summrise rollback <x.y.z> | status | --clear");
      process.exit(1);
    }
    const base = (
      process.env.SUMMRISE_CDN || "https://agent.saisi.online"
    ).replace(/\/+$/, "");
    const url = `${base}/summrise-agent/summrise-agent-${val}.tgz`;
    const head = spawnSync(
      "curl",
      [
        "-s",
        "-o",
        "/dev/null",
        "-w",
        "%{http_code}",
        "-m",
        "30",
        "--head",
        url,
      ],
      { encoding: "utf8", timeout: 40000 },
    );
    const code = String(head.stdout || "").trim();
    if (head.status !== 0 || code !== "200") {
      console.error(
        `rollback: ${val} is not on the release CDN (HTTP ${code || "?"}) -- the last-5-per-minor prune removed it;` +
          " pick a retained version (see https://agent.saisi.online/summrise-agent/version.json for the current line)",
      );
      process.exit(1);
    }
    console.log(
      `rollback: installing summrise-agent ${val} into ${NPM_GLOBAL} ...`,
    );
    const npmCmd = process.platform === "win32" ? "npm.cmd" : "npm";
    const inst = spawnSync(
      npmCmd,
      ["install", "-g", "--prefix", NPM_GLOBAL, url],
      { stdio: "inherit", timeout: 300000 },
    );
    // `status !== 0` is the FAIL-CLOSED test here — unlike the reversed checks fixed
    // elsewhere in this file, a spawn failure or timeout yields `status: null`, which is
    // also `!== 0`, so it aborts. Only the MESSAGE conflated the two, which matters
    // because "npm install failed" sends an operator to look at the registry while
    // "npm could not be run" sends them to look at PATH.
    if (inst.error || inst.status !== 0) {
      console.error(
        inst.error
          ? `rollback: could not RUN npm (${inst.error.message}) -- device left untouched`
          : `rollback: npm install failed (exit ${inst.status}) -- device left untouched`,
      );
      process.exit(1);
    }
    const summriseCmd = path.join(NPM_GLOBAL, "summrise.cmd");
    if (!fs.existsSync(summriseCmd)) {
      console.error(
        "rollback: summrise.cmd missing after install (broken package?) -- aborting before any swap",
      );
      process.exit(1);
    }
    console.log(`rollback: swapping in ${val} (connection drops ~10s) ...`);
    const upd = spawnSync(summriseCmd, ["update"], {
      stdio: "inherit",
      timeout: 120000,
    });
    if (upd.status !== 0) {
      console.error(
        "rollback: swap failed -- pin NOT written, device still runs the previous release",
      );
      process.exit(1);
    }
    // status 0 means the HANDOFF was accepted, not that the swap succeeded (the
    // script that decides that runs afterwards, parented by WmiPrvSE, with
    // nobody reading its exit code). So ASK THE DEVICE: read the marker back and
    // require it to show the version we just staged. Only then is the pin — and
    // the claim to every UI — earned.
    const markerFile = path.join(ETC_DIR, ".summrise-release");
    const check = await awaitReleaseMarker({
      want: val,
      timeoutMs: 90_000,
      intervalMs: 2_000,
      read: () => fs.readFileSync(markerFile, "utf8"),
      sleep: (ms) => new Promise((r) => setTimeout(r, ms)),
      now: () => Date.now(),
    });
    const verdict = releaseMarkerVerdict({ ...check, want: val });
    if (!verdict.writePin) {
      console.error(verdict.message);
      process.exit(verdict.exitCode);
    }
    // TWO OPERATIONS, TWO VERDICTS. They shared one try, so a CLEANUP failure
    // (rmSync) printed "pin write failed ... agent_update is NOT blocked" — the exact
    // inverse of the truth: the pin HAD been written and agent_update WAS blocked. It
    // also swallowed the success line, because that was inside the same try.
    let pinned = false;
    try {
      fs.mkdirSync(ETC_DIR, { recursive: true });
      fs.writeFileSync(PIN, val);
      pinned = true;
    } catch (e: any) {
      console.error(
        "rollback: WARNING -- pin write failed (" +
          e.message +
          "); device runs " +
          val +
          " but agent_update is NOT blocked",
      );
    }
    if (pinned) {
      // Decided on a READ-BACK, like `--clear` and the marker check: the write not
      // throwing is not the same as the pin being in place.
      let back: string | null = null;
      try {
        back = fs.readFileSync(PIN, "utf8").trim();
      } catch {
        /* reported below */
      }
      if (back !== val) {
        console.error(
          `rollback: WARNING -- wrote ${PIN} but read back ${back ?? "nothing"}; agent_update may not be blocked`,
        );
      } else {
        console.log(verdict.message);
      }
      // Heal a pre-v2 swap's split-brain marker (old CLI wrote ROOT .summrise-release;
      // the agent reads etc\). Root leftover is garbage. Cleanup only — it can never
      // change whether the pin exists, so it gets its OWN message.
      // NOTE: etc\.summrise-release is NOT written here — the swap script wrote it from a
      // provable copy, and overwriting it would erase that proof.
      try {
        fs.rmSync(path.join(DIR, ".summrise-release"), { force: true });
      } catch (e: any) {
        console.error(
          `rollback: note -- the pin is in place, but the pre-v2 root marker could not be removed (${e.message}); it is inert`,
        );
      }
    }
  },

  // The ONLY uninstall path (NSIS installer is retired — npm CLI is the
  // single install/update channel). Stops the agent, removes the scheduled
  // tasks, deletes the program dir + registry keys. The DATA dir
  // (%ProgramData%\Summrise) is KEPT by default (sessions/memory survive);
  // pass --purge-data to delete it too.
  uninstall(args) {
    const purge = args.includes("--purge-data");
    // DATA_DIR, not a local recomputation: the registry-first resolver is the single
    // source of truth (setup already uses it), and a second copy here meant a
    // registry-remapped install had the WRONG directory purged — and named in
    // "data kept at" — while the command reported success.
    const DATA = DATA_DIR;
    // HIGH npm audit: verify DIR is actually a Summrise install dir before
    // recursively deleting — an attacker who controls SUMMRISE_AGENT_DIR (env
    // var) or the registry key could point it at D:\Windows or C:\.
    if (
      !fs.existsSync(path.join(DIR, "summrise-agent.exe")) &&
      !fs.existsSync(path.join(ETC_DIR, "summrise-agent.hostname")) &&
      !fs.existsSync(path.join(DIR, "summrise-agent.hostname"))
    ) {
      console.error(
        "uninstall: REFUSE — " +
          DIR +
          " does not look like a Summrise install dir (no summrise-agent.exe/hostname). Set SUMMRISE_AGENT_DIR to the correct path.",
      );
      process.exit(1);
    }
    console.log("uninstall: stopping SummriseAgent...");
    sh("cmd /c schtasks /End /TN SummriseAgent 2>NUL");
    sh("taskkill /F /IM summrise-agent.exe 2>NUL");
    sh("taskkill /F /IM summrise-desktop.exe 2>NUL");
    // npm audit #11: electron survived uninstall (dead SPA window); the
    // update-hardened SummriseDesktop 5-min pulse kept firing against the deleted dir.
    sh("taskkill /F /IM electron.exe 2>NUL");
    sh("cmd /c schtasks /End /TN SummriseDesktop 2>NUL");
    sh("cmd /c schtasks /Delete /TN SummriseDesktop /F 2>NUL");
    // kill bundled playwright node + boxed cloudflared (best effort)
    // Match ANY node.exe whose command line mentions a summrise playwright
    // bundle (covers the current install dir AND legacy dirs like
    // D:\summrise-agent\playwright that a fresh uninstall must clear too).
    sh(
      `powershell -NoProfile -Command "Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'node.exe' -and ($_.CommandLine -like '*summrise-agent*playwright*' -or $_.CommandLine -like '*summrise-command*playwright*') } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }"`,
    );
    sh("taskkill /F /IM cloudflared.exe 2>NUL");
    sh("cmd /c schtasks /Delete /TN SummriseAgent /F 2>NUL");
    sh("cmd /c schtasks /Delete /TN SummrisePlaywright /F 2>NUL");
    // legacy service cleanup (best effort)
    sh("sc stop Cloudflared 2>NUL");
    sh("sc delete Cloudflared 2>NUL");
    sh(
      "reg delete HKLM\\SYSTEM\\CurrentControlSet\\Services\\EventLog\\Application\\Cloudflared /f 2>NUL",
    );
    // program dir + registry.
    //
    // NOT `rmdir /s /q "<DIR>"`: `rmdir` is a cmd BUILTIN — there is no rmdir.exe to hand argv
    // to — so a PATH given to it is text that cmd re-parses, and `%NAME%` expands even inside
    // those quotes (see `sh()`). DIR is the install dir, i.e. operator-chosen. `ps()` runs the
    // same deletion (Remove-Item -Recurse -Force, the form the legacy loop below already uses
    // because it also copes with locked/read-only files) with the path as argv. The VERIFY below
    // is what decides, which is why silence on failure is acceptable here.
    ps(
      `Remove-Item -LiteralPath '${psq(DIR)}' -Recurse -Force -ErrorAction SilentlyContinue`,
    );
    // Legacy install dirs from retired installers (C:\summrise-agent /
    // D:\summrise-agent) — uninstall must leave NO residue anywhere.
    for (const legacy of ["C:\\summrise-agent", "D:\\summrise-agent"]) {
      if (legacy !== DIR && fs.existsSync(legacy)) {
        console.log("uninstall: removing legacy install dir", legacy);
        // PowerShell Remove-Item -Recurse -Force handles locked/read-only
        // files better than rmdir; retry once after a short wait.
        //
        // cmd-% literal: `legacy` is one of the two STRING LITERALS in the array on the `for`
        // line above, so this value cannot carry a `%` — the pin re-reads that array.
        sh(
          `powershell -NoProfile -Command "Remove-Item -LiteralPath '${psq(legacy)}' -Recurse -Force -ErrorAction SilentlyContinue; Start-Sleep -Milliseconds 500; Remove-Item -LiteralPath '${psq(legacy)}' -Recurse -Force -ErrorAction SilentlyContinue"`,
        );
        if (fs.existsSync(legacy)) {
          console.log(
            "uninstall: WARNING -- legacy dir still present:",
            legacy,
          );
        }
      }
    }
    sh("reg delete HKLM\\SOFTWARE\\Summrise\\Agent /f 2>NUL");
    // VERIFY, because `sh()` discards its result at every one of its call sites — so a
    // locked file, an AV hold or a denied HKLM write produced "removed" with the thing
    // still there, and exit 0. The legacy-dir loop just above is the pattern.
    const survivors: string[] = [];
    if (fs.existsSync(DIR)) survivors.push(`install dir ${DIR}`);
    const regLeft = spawnSync(
      "reg",
      ["query", "HKLM\\SOFTWARE\\Summrise\\Agent"],
      {
        encoding: "utf8",
      },
    );
    if (regLeft.status === 0)
      survivors.push("registry key HKLM\\SOFTWARE\\Summrise\\Agent");
    if (survivors.length) {
      // ONE FAILURE CLASS, ONE TREATMENT. The data-dir twin forty lines below exits 1 for exactly this
      // ("FAILED to purge the data dir -- … is still present"), and this branch warned and exited 0 —
      // while the comment directly above states the rule it was breaking: `sh()` discards its result
      // "at every one of its call sites", so a locked file, an AV hold or a denied HKLM write "produced
      // 'removed' with the thing still there, AND EXIT 0".
      //
      // The program dir and the registry key are what `uninstall` was asked to remove. A warning that
      // exits 0 is the same false verdict the data-dir branch already refuses.
      console.error(
        "uninstall: FAILED -- still present after removal:",
        survivors.join(", "),
      );
      console.error(
        "uninstall: a locked file or a permission problem; re-run after stopping the agent.",
      );
      process.exitCode = 1;
    } else {
      console.log("uninstall: program dir + registry removed");
    }
    if (purge) {
      // Same as the program dir above: `rmdir` is a builtin, so its path would be text cmd
      // re-parses (`%NAME%` expands inside quotes), and DATA is a path the operator can remap.
      ps(
        `Remove-Item -LiteralPath '${psq(DATA)}' -Recurse -Force -ErrorAction SilentlyContinue`,
      );
      // A failed removal used to print "data dir purged" anyway.
      if (fs.existsSync(DATA)) {
        console.error(
          `uninstall: FAILED to purge the data dir -- ${DATA} is still present`,
        );
        process.exitCode = 1;
      } else {
        console.log("uninstall: data dir purged");
      }
    } else {
      console.log(
        "uninstall: data kept at",
        DATA,
        "(pass --purge-data to delete)",
      );
    }
  },

  run(args) {
    // EXE_DST is always truthy, so the old `EXE_DST || EXE_SRC` was dead code and there
    // was no existence check: ENOENT yields `status: null`, and `?? 0` reported that as
    // a clean run. The banner printed either way.
    if (!fs.existsSync(EXE_DST)) {
      console.error(
        `summrise run: agent binary not found at ${EXE_DST} -- run 'summrise setup' first`,
      );
      process.exit(1);
    }
    console.log("running summrise-agent (foreground, Ctrl+C to stop)");
    const r = spawnSync(EXE_DST, args.length ? args : [], {
      stdio: "inherit",
    });
    if (r.error || r.status === null) {
      console.error(
        `summrise run: could not start the agent (${r.error ? r.error.message : "no exit status"})`,
      );
      process.exit(1);
    }
    process.exitCode = r.status;
  },

  // C2: cloudflared is a boxed, Summrise-supervised component — operators never
  // touch the binary directly. This CLI is the only handle.
  async tunnel(args) {
    const sub = args[0] || "status";
    const cf = path.join(COMPONENTS_DIR, "cloudflared.exe");
    const cfg = path.join(ETC_DIR, "tunnel.yml");
    const has = fs.existsSync(cf) && fs.existsSync(cfg);
    switch (sub) {
      case "status": {
        if (!has) {
          console.log(
            "tunnel: not installed (cloudflared is OPTIONAL -- local mode needs no tunnel)",
          );
          console.log("  to enable public access: `summrise tunnel install`");
          return;
        }
        // No shell:true — see status(): an unquoted filter through cmd.exe is
        // split at its spaces, so this reported STOPPED while cloudflared ran.
        const cfState = processRunning("cloudflared.exe");
        console.log(
          cfState === "unknown"
            ? "tunnel: state UNKNOWN -- could not list processes (tasklist failed); do not assume it is down"
            : cfState === "yes"
              ? "tunnel: RUNNING"
              : "tunnel: STOPPED",
        );
        console.log("  binary:", cf);
        console.log("  config:", cfg);
        return;
      }
      case "install": {
        // THE tunnel enablement path — shared bootstrap with setup --tunnel.
        // A --reg-key <key> arg enables the automatic token exchange too.
        const ki = args.indexOf("--reg-key");
        const k = ki >= 0 ? args[ki + 1] : "";
        initTunnel(args[1] && !args[1].startsWith("--") ? args[1] : "", k);
        return;
      }
      case "start": {
        if (!has) {
          console.error(
            "tunnel: not installed -- run setup with public-access enabled",
          );
          process.exit(1);
        }
        // npm audit #12: detached/unref are NO-OPS on spawnSync —
        // `summrise tunnel start` blocked the CLI until the tunnel died.
        // intent to background the tunnel); kept for parity.
        // OBSERVE THE SPAWN. stdio was ignored, there was no 'error'/'exit' listener and
        // the CLI exited 0 immediately — so the success line printed for a cloudflared
        // that died on a bad config, a missing credentials file, a gone route, or an
        // already-running instance. Worse, when the SPAWN ITSELF failed there was no
        // 'error' listener either, so node printed the success line and THEN died with an
        // unhandled 'error' stack trace.
        let spawnErr: Error | null = null;
        let exitedEarly: number | null = null;
        const ch = spawn(cf, ["tunnel", "--config", cfg, "run"], {
          stdio: "ignore",
          detached: true,
        });
        ch.on("error", (e: Error) => {
          spawnErr = e;
        });
        ch.on("exit", (code: number | null) => {
          exitedEarly = code === null ? -1 : code;
        });
        ch.unref();
        // Give it long enough to fail visibly, then ASK the same question `tunnel
        // status` asks rather than assuming. `await` here is why this method is async.
        await new Promise((r) => setTimeout(r, 1500));
        if (spawnErr) {
          console.error(
            `tunnel: FAILED to start cloudflared (${(spawnErr as Error).message})`,
          );
          process.exit(1);
        }
        if (exitedEarly !== null) {
          console.error(
            `tunnel: cloudflared exited immediately (code ${exitedEarly}) -- check the config and credentials`,
          );
          process.exit(1);
        }
        const started = processRunning("cloudflared.exe");
        if (started !== "yes") {
          console.error(
            "tunnel: cloudflared did not come up" +
              (started === "unknown"
                ? " (and the process list could not be read, so this is not a verdict)"
                : "") +
              " -- see the tunnel log; the agent still auto-spawns it on boot",
          );
          process.exit(1);
        }
        console.log(
          "tunnel: started in background (agent also auto-spawns it on boot)",
        );
        return;
      }
      case "stop": {
        const r = spawnSync("taskkill", ["/F", "/IM", "cloudflared.exe"], {
          stdio: "inherit",
        });
        if (r.status !== 0) console.log("tunnel: nothing to stop");
        return;
      }
      case "update": {
        console.log(
          "tunnel: version is locked by the Summrise release flow -- update via the installer/npm package.",
        );
        return;
      }
      default:
        console.log(
          "usage: summrise tunnel <status|install|start|stop|update>",
        );
        process.exit(1);
    }
  },
};

// require.main guard: test/cli.test.mjs imports the pure helpers above
// WITHOUT tripping the usage print + process.exit at module load.
if (require.main === module) {
  const [cmd, ...rest] = process.argv.slice(2);
  // `--version` ANSWERS THE CHECK THE INSTALLER'S OWN CHECKLIST MAKES, and it used to fail it. It is not
  // a verb, so it fell through to the usage branch below, printed the verb list and exited 1 — while
  // step 4 of `deploy/README-installer.md` BEGINS with "`summrise --version` / the panel opens". A fresh
  // install that worked therefore reported a failure in the one place the operator is told to look,
  // which is the same defect as a comment promising more than the code does.
  if (cmd === "--version" || cmd === "-v" || cmd === "version") {
    console.log(String(require("../package.json").version || ""));
    process.exit(0);
  }
  if (!cmd || !commands[cmd]) {
    // The list is DERIVED from `commands`, so the help cannot promise a verb that was pruned
    // (`report` and `watch` were still advertised after their round-25 removal — a usage line is a
    // contract, and one that lies is worse than none).
    console.log(
      "summrise <" +
        Object.keys(commands).join("|") +
        "> -- Summrise Agent control",
    );
    Object.keys(commands).forEach((k) => console.log(" ", k));
    process.exit(cmd ? 1 : 0);
  }
  // `rollback` is the only ASYNC command (it awaits the marker read-back), so
  // the dispatcher must catch a rejected promise: an unhandled rejection is a
  // raw stack trace with a non-obvious exit code, and this command runs on a
  // device where the operator sees only the console.
  Promise.resolve(commands[cmd](rest)).catch((e: any) => {
    console.error(`summrise ${cmd}: ${e && e.message ? e.message : e}`);
    process.exit(1);
  });
}
