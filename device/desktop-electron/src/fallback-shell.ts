// The installed Rust shell, opened as the window-only fallback.
//
// THE THIN HOST IS STILL THE DEFAULT. This module is the ONE trigger for the fallback: an explicit
// tray row or `--fallback`, never a boot path. It spawns the installed `summrise-shell.exe --panel`
// DETACHED, so the shell outlives this Electron process and owns its own window. The normal tray
// mode has no `--panel`: that argv is the Rust shell's window-only mode (no tray, helper, first-run,
// geometry or reconnect), which is the only fallback this host supports.
//
// THE PATH IS DERIVED FROM THIS EXECUTABLE, NOT FROM A CONFIG FILE: the installed host is
// `<install>\desktop\SummriseDesktop.exe`, so its fallback sibling is `<install>\summrise-shell.exe`.
// `SUMMRISE_SHELL_EXE` is the explicit override (dev machines).
//
// A missing file, a spawn that throws, or a spawn that returns no pid, is a RESULT, not an exception:
// the caller shows it. A LATE `error` event is logged by this module, because Node reports ENOENT/
// EACCES asynchronously and an unlistened `error` would be an uncaught exception in Electron's main
// process.
import * as fs from "fs";
import * as path from "path";
import { spawn } from "child_process";

/** The existence test, injectable so the path rules are testable without a Windows install. */
export type FallbackExists = (p: string) => boolean;
/** The spawn seam, narrowed to what this module uses: the caller does not hand us a ChildProcess.
 *  `on` is optional so a test fake can omit it, but the real `ChildProcess` always has it. */
export type FallbackSpawn = (
  command: string,
  args: string[],
  options: { detached: boolean; stdio: "ignore"; windowsHide: boolean },
) => {
  pid?: number;
  unref(): void;
  on?: (event: "error", listener: (error: Error) => void) => unknown;
};

function fsExists(p: string): boolean {
  try {
    return fs.existsSync(p);
  } catch {
    return false;
  }
}

/** The one path rule, also used to NAME the path in an error when the file is missing. */
function fallbackShellCandidate(execPath: string, env: NodeJS.ProcessEnv): string {
  const override = env && env.SUMMRISE_SHELL_EXE ? env.SUMMRISE_SHELL_EXE : "";
  return override
    ? override
    : path.win32.normalize(path.win32.dirname(execPath) + "\\..\\summrise-shell.exe");
}

function logFallbackError(shellPath: string, error: Error): void {
  console.error(`[summrise] fallback shell failed: ${shellPath}: ${error.message}`);
}

/** The installed fallback shell, or null when it is not there.
 *
 * `SUMMRISE_SHELL_EXE` wins over the sibling derivation; either way the file must exist, because a
 * path the spawn cannot open would turn the tray row into a dead click. `path.win32` is deliberate:
 * the installed product is Windows and the derivation must not depend on the host running the tests.
 */
export function fallbackShellPath(
  execPath: string,
  env: NodeJS.ProcessEnv = process.env,
  exists: FallbackExists = fsExists,
): string | null {
  const candidate = fallbackShellCandidate(execPath, env);
  return exists(candidate) ? candidate : null;
}

/** Spawn the window-only fallback shell. Detached and stdio-ignored so closing this host, or the
 *  shell's own window, never depends on the other; `windowsHide` keeps a console from flashing.
 *
 *  EVERY CALL IS INDEPENDENT: repeated tray clicks open independent `--panel` windows by design. The
 *  Rust shell owns each window and this host has no handle to de-duplicate them; the user closes the
 *  extra window, exactly as they would a second app window. */
export function launchFallbackShell(options: {
  execPath: string;
  env?: NodeJS.ProcessEnv;
  exists?: FallbackExists;
  spawn?: FallbackSpawn;
}): { ok: true; path: string; pid?: number } | { ok: false; error: string } {
  const candidate = fallbackShellCandidate(options.execPath, options.env ?? process.env);
  const shellPath = fallbackShellPath(options.execPath, options.env, options.exists);
  if (!shellPath) {
    const origin = options.env && options.env.SUMMRISE_SHELL_EXE
      ? "from SUMMRISE_SHELL_EXE"
      : "next to the Summrise desktop install";
    return { ok: false, error: `summrise-shell.exe was not found at ${candidate} (${origin})` };
  }
  try {
    const child = (options.spawn ?? spawn)(shellPath, ["--panel"], {
      detached: true,
      stdio: "ignore",
      windowsHide: true,
    });
    // THE LISTENER COMES BEFORE THE PID TEST. Node does not throw for ENOENT/EACCES; it returns a
    // child and emits `error` asynchronously. An unlistened `error` is an uncaught exception in the
    // Electron main process, so attach it even when the pid below fails the launch.
    child.on?.("error", (error) => { logFallbackError(shellPath, error); });
    // NO PID MEANS THE PROCESS NEVER STARTED: reporting {ok:true} here is the dead click this module
    // exists to prevent. The dialog gets the path now; the listener above records the cause.
    if (child.pid === undefined) {
      return { ok: false, error: `summrise-shell.exe did not start: ${shellPath}` };
    }
    child.unref();
    return { ok: true, path: shellPath, pid: child.pid };
  } catch (e) {
    return { ok: false, error: `summrise-shell.exe could not be started: ${shellPath}: ${String(e)}` };
  }
}
