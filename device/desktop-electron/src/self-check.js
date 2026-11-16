"use strict";
// THE INSTALLED HOST'S SELF-CHECK — one read-only document, composed from facts the host owns.
//
// WHY THIS FILE EXISTS. The 2.0.8 device can only be inspected by logging into it and running
// `Get-FileHash`/`summrise-launch`; an operator (or the console) cannot ask the running host what
// bytes it is actually serving. This module answers that on the EXISTING 9444 listener (see the
// branch in `main.ts`): install root, the config's presence and size, `token_present` (a boolean,
// never a value), the agent's base and liveness, the running exe/asar, the installed shell, and
// `current.txt`/`previous.txt`.
//
// WHAT IT DELIBERATELY DOES NOT DO. It starts nothing, stops nothing, opens no browser session and
// touches no network itself (`agentHealth` is injected — `main.ts` passes the tray's own
// `agentResponds`). It never reads the config body: `tokenPresent` arrives as a boolean computed by
// `agentToken() !== null`, so no credential value, credential-derived string or config text can
// exist in this module or in the document it returns. It does not require the policy module: the
// crate that would own a new export is not in this repository (only the vendored glue is tracked),
// so a host-owned read route over existing decisions is the recorded deviation
// (`round12/plan.md` ruling 3), not a new policy decision.
//
// THE ARCHIVE IS READ THROUGH THE RAW FS. Electron patches the normal `fs` with asar support: for
// `<install>\desktop\resources\app.asar`, `statSync` reports a DIRECTORY (so `isFile()` is false and
// `bytes` would be null) and `readFileSync` fails — the self-check would answer null for exactly the
// running bytes it exists to prove. `original-fs` bypasses the patch; under plain Node (the tests)
// that module does not exist and the normal `fs` is already raw. Resolution is lazy, so this file
// still loads without Electron, and every other path (config, exe, shell, version files) stays on
// the normal `fs` — only the archive needs the raw reader.
//
// FAILURES ARE DATA. A missing config, exe, asar, shell or version file yields `exists:false` /
// `bytes:null` / `sha256:null` / `current:null` in the same document — the host must be able to
// prove a broken installation, which is exactly when the operator needs the route. Only an
// unexpected throw reaches the handler's JSON 500.
const fs = require("fs");
const crypto = require("crypto");

/** The one route this document answers on the existing 9444 control listener. GET only: a POST is
 *  the policy router's 404, not a second door. */
const SELF_CHECK_PATH = "/api/shell/self-check";
const SELF_CHECK_METHOD = "GET";
/** Bump when the document's stable contract changes; the console parses on it. */
const SELF_CHECK_SCHEMA = 1;

/** A raw reader for the archive: Electron's `original-fs`, or the normal `fs` under plain Node (and
 *  in any runtime where the built-in is absent). Cached after the first answer; tests inject their
 *  own reader through `deps.archiveFs` instead. */
let _archiveFs = null;
function rawArchiveFs() {
    if (_archiveFs) return _archiveFs;
    let raw = fs;
    try {
        const candidate = require("original-fs");
        if (candidate && typeof candidate.statSync === "function"
            && typeof candidate.readFileSync === "function") {
            raw = candidate;
        }
    } catch { /* plain Node: `fs` is already unpatched */ }
    _archiveFs = raw;
    return raw;
}

/** Existence and size, from `stat` only (never content). A stat that fails is absence, not a throw;
 *  a non-file (a directory at the path) exists but has no byte count. */
function statFile(p, reader = fs) {
    try {
        const st = reader.statSync(p);
        return { exists: true, bytes: st.isFile() ? st.size : null };
    } catch {
        return { exists: false, bytes: null };
    }
}

/** SHA-256 of the file's bytes, or null when it cannot be read. The asar (~2.6 MB) and the shell
 *  (~1.6 MB) are small enough to hash in one read; the route is for an operator, not a hot path. */
function sha256File(p, reader = fs) {
    try {
        return crypto.createHash("sha256").update(reader.readFileSync(p)).digest("hex");
    } catch {
        return null;
    }
}

/** A version file's trimmed text, or null when missing/empty. */
function readVersion(p) {
    try {
        const v = fs.readFileSync(p, "utf8").trim();
        return v === "" ? null : v;
    } catch {
        return null;
    }
}

function text(value) {
    return typeof value === "string" ? value : "";
}

/** The optional string facts keep their key with a null value rather than disappearing. */
function maybeText(value) {
    return typeof value === "string" ? value : null;
}

/**
 * Compose the self-check document.
 *
 * `deps` carries every fact, so this module needs no Electron, no CLI and no policy module:
 *   installRoot, configPath, tokenPresent (a boolean, never a value), agentBase,
 *   agentHealth () => boolean|Promise<boolean>, appPath, exePath, electron, appVersion,
 *   shellPath, currentPath, previousPath, and optionally archiveFs (the raw archive reader; the
 *   default is `original-fs` in Electron and `fs` in Node).
 */
async function collectSelfCheck(deps) {
    const d = deps || {};
    // ONLY THE ARCHIVE USES THE RAW READER. Config, exe, shell and the version files are ordinary
    // files to the patched `fs`; `app.asar` is not — Electron presents it as a directory.
    const archiveFs = d.archiveFs || rawArchiveFs();
    const configPath = text(d.configPath);
    const config = statFile(configPath);
    const exePath = text(d.exePath);
    const exe = statFile(exePath);
    const appPath = text(d.appPath);
    const asar = statFile(appPath, archiveFs);
    const shellPath = text(d.shellPath);
    const shell = statFile(shellPath);

    // The tray's own liveness probe: any HTTP answer is alive; timeout/error is dead. A probe that
    // throws (a caller bug, a socket torn down mid-probe) is dead too — never an exception out of
    // a read route.
    let health = false;
    try {
        health = (await d.agentHealth()) === true;
    } catch {
        health = false;
    }

    return {
        ok: true,
        schema: SELF_CHECK_SCHEMA,
        route: SELF_CHECK_PATH,
        at: new Date().toISOString(),
        install_root: text(d.installRoot),
        // `token_present` is the only fact of that kind: a boolean passed in by the host, computed as
        // `agentToken() !== null`. The config body is never opened here.
        config: { path: configPath, exists: config.exists, bytes: config.bytes,
            token_present: d.tokenPresent === true },
        agent: { base: text(d.agentBase), health },
        host: {
            pid: process.pid,
            exe: exePath,
            exe_bytes: exe.bytes,
            asar: appPath,
            asar_bytes: asar.bytes,
            asar_sha256: sha256File(appPath, archiveFs),
            electron: maybeText(d.electron),
            app_version: maybeText(d.appVersion),
        },
        shell: { path: shellPath, bytes: shell.bytes, sha256: sha256File(shellPath) },
        install: { current: readVersion(d.currentPath), previous: readVersion(d.previousPath) },
    };
}

module.exports = {
    SELF_CHECK_PATH,
    SELF_CHECK_METHOD,
    SELF_CHECK_SCHEMA,
    collectSelfCheck,
};
