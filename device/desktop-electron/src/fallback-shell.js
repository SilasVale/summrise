"use strict";
var __createBinding = (this && this.__createBinding) || (Object.create ? (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    var desc = Object.getOwnPropertyDescriptor(m, k);
    if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
      desc = { enumerable: true, get: function() { return m[k]; } };
    }
    Object.defineProperty(o, k2, desc);
}) : (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    o[k2] = m[k];
}));
var __setModuleDefault = (this && this.__setModuleDefault) || (Object.create ? (function(o, v) {
    Object.defineProperty(o, "default", { enumerable: true, value: v });
}) : function(o, v) {
    o["default"] = v;
});
var __importStar = (this && this.__importStar) || (function () {
    var ownKeys = function(o) {
        ownKeys = Object.getOwnPropertyNames || function (o) {
            var ar = [];
            for (var k in o) if (Object.prototype.hasOwnProperty.call(o, k)) ar[ar.length] = k;
            return ar;
        };
        return ownKeys(o);
    };
    return function (mod) {
        if (mod && mod.__esModule) return mod;
        var result = {};
        if (mod != null) for (var k = ownKeys(mod), i = 0; i < k.length; i++) if (k[i] !== "default") __createBinding(result, mod, k[i]);
        __setModuleDefault(result, mod);
        return result;
    };
})();
Object.defineProperty(exports, "__esModule", { value: true });
exports.fallbackShellPath = fallbackShellPath;
exports.launchFallbackShell = launchFallbackShell;
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
const fs = __importStar(require("fs"));
const path = __importStar(require("path"));
const child_process_1 = require("child_process");
function fsExists(p) {
    try {
        return fs.existsSync(p);
    }
    catch {
        return false;
    }
}
/** The one path rule, also used to NAME the path in an error when the file is missing. */
function fallbackShellCandidate(execPath, env) {
    const override = env && env.SUMMRISE_SHELL_EXE ? env.SUMMRISE_SHELL_EXE : "";
    return override
        ? override
        : path.win32.normalize(path.win32.dirname(execPath) + "\\..\\summrise-shell.exe");
}
function logFallbackError(shellPath, error) {
    console.error(`[summrise] fallback shell failed: ${shellPath}: ${error.message}`);
}
/** The installed fallback shell, or null when it is not there.
 *
 * `SUMMRISE_SHELL_EXE` wins over the sibling derivation; either way the file must exist, because a
 * path the spawn cannot open would turn the tray row into a dead click. `path.win32` is deliberate:
 * the installed product is Windows and the derivation must not depend on the host running the tests.
 */
function fallbackShellPath(execPath, env = process.env, exists = fsExists) {
    const candidate = fallbackShellCandidate(execPath, env);
    return exists(candidate) ? candidate : null;
}
/** Spawn the window-only fallback shell. Detached and stdio-ignored so closing this host, or the
 *  shell's own window, never depends on the other; `windowsHide` keeps a console from flashing.
 *
 *  EVERY CALL IS INDEPENDENT: repeated tray clicks open independent `--panel` windows by design. The
 *  Rust shell owns each window and this host has no handle to de-duplicate them; the user closes the
 *  extra window, exactly as they would a second app window. */
function launchFallbackShell(options) {
    const candidate = fallbackShellCandidate(options.execPath, options.env ?? process.env);
    const shellPath = fallbackShellPath(options.execPath, options.env, options.exists);
    if (!shellPath) {
        const origin = options.env && options.env.SUMMRISE_SHELL_EXE
            ? "from SUMMRISE_SHELL_EXE"
            : "next to the Summrise desktop install";
        return { ok: false, error: `summrise-shell.exe was not found at ${candidate} (${origin})` };
    }
    try {
        const child = (options.spawn ?? child_process_1.spawn)(shellPath, ["--panel"], {
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
    }
    catch (e) {
        return { ok: false, error: `summrise-shell.exe could not be started: ${shellPath}: ${String(e)}` };
    }
}
