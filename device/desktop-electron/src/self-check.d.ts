/** The document a GET /api/shell/self-check answers. The shape is the stable contract (`schema`); a
 *  new fact is a deliberate change, and `test/self-check.test.mjs` pins the exact key set. */
export const SELF_CHECK_PATH: "/api/shell/self-check";
export const SELF_CHECK_METHOD: "GET";
export const SELF_CHECK_SCHEMA: 1;

/** The slice of a raw file-system reader the archive needs. Electron patches the normal `fs` with
 *  asar support (`statSync(<app.asar>)` is a directory and `readFileSync` fails), so the running
 *  archive must be read through `original-fs`; in Node the normal `fs` is already raw. Exposed so a
 *  test can simulate the patched reader and prove the archive never goes through plain `fs`. */
export interface SelfCheckArchiveFs {
    statSync(path: string): { isFile(): boolean; size: number };
    readFileSync(path: string): Buffer;
}

/** The slice of the file system the config cell needs: metadata plus an open/close readability
 *  probe. It deliberately has no `readFileSync` — the config body is never read, only opened with
 *  mode `"r"` and closed, so this module cannot copy or leak it. Exposed so tests can inject stat,
 *  open and close failures (Windows EACCES/EPERM/ENOENT) without touching a real DACL. */
export interface SelfCheckConfigFs {
    statSync(path: string): { isFile(): boolean; size: number };
    openSync(path: string, flags: "r"): number;
    closeSync(fd: number): void;
}

export interface SelfCheckDeps {
    /** `<install>`, e.g. `C:\Program Files\Summrise`. */
    installRoot: string;
    /** `<install>\etc\config.yaml` — stat'd and opened (`"r"`) only to prove readability; the body
     *  is never read by this module. */
    configPath: string;
    /** `agentToken(true) !== null`: the ONLY token fact, and it is a boolean. */
    tokenPresent: boolean;
    /** Optional stat/open reader for the config path. Defaults to `fs`; a test injects one to
     *  reproduce a denied or missing config without changing a real file's DACL. */
    configFs?: SelfCheckConfigFs;
    /** `agentBase()` — the device port the host resolved, never the config's port. */
    agentBase: string;
    /** The tray's own liveness probe (`agentResponds`): any HTTP answer is alive. */
    agentHealth: () => boolean | Promise<boolean>;
    /** The running asar (`app.getAppPath()`). */
    appPath: string;
    /** The running exe (`process.execPath`). */
    exePath: string;
    /** `process.versions.electron` (optional so a build that lacks it answers null). */
    electron?: string | null;
    /** `app.getVersion()` — the Electron package version, not the release version. */
    appVersion?: string | null;
    /** `<install>\summrise-shell.exe`. */
    shellPath: string;
    /** `<install>\current.txt`. */
    currentPath: string;
    /** `<install>\previous.txt`. */
    previousPath: string;
    /** Optional raw archive reader. Defaults to `original-fs` under Electron, `fs` under Node; a
     *  test injects one to reproduce Electron's patched `fs` without running Electron. */
    archiveFs?: SelfCheckArchiveFs;
}

export interface SelfCheckConfig {
    path: string;
    exists: boolean;
    /** `stat` size, or null when missing/not a file. Kept when the stat succeeded but the open was
     *  denied: the size is proved even though the content is not. */
    bytes: number | null;
    /** True only when `openSync(path, "r")` succeeded. Always present; a denial is not absence. */
    readable: boolean;
    /** True/false from the host's fresh token boolean when the config is readable, `false` when it
     *  is absent, and `null` when it exists but cannot be read (denied stat/open, or not a file). */
    token_present: boolean | null;
}

export interface SelfCheckAgent {
    base: string;
    health: boolean;
}

export interface SelfCheckHost {
    pid: number;
    exe: string;
    exe_bytes: number | null;
    asar: string;
    asar_bytes: number | null;
    asar_sha256: string | null;
    electron: string | null;
    app_version: string | null;
}

export interface SelfCheckShell {
    path: string;
    bytes: number | null;
    sha256: string | null;
}

export interface SelfCheckInstall {
    current: string | null;
    previous: string | null;
}

export interface SelfCheckDocument {
    ok: true;
    schema: number;
    route: string;
    /** ISO-8601 timestamp of the answer. */
    at: string;
    install_root: string;
    config: SelfCheckConfig;
    agent: SelfCheckAgent;
    host: SelfCheckHost;
    shell: SelfCheckShell;
    install: SelfCheckInstall;
}

/** Read-only: starts/stops nothing, opens no session and makes no request of its own. */
export function collectSelfCheck(deps: SelfCheckDeps): Promise<SelfCheckDocument>;
