"use strict";
// Where the 2.0.0 device says it listens.
//
// The legacy agent answered `/api/status` on its own config.yaml port (18080 by default). The device
// writes the port it actually bound to two files: `%ProgramData%\SummriseHelper\device.port` (the copy
// the logged-in user can read) and `<data>\device.port` (SYSTEM/Administrators). This module is the one
// place the Electron shell resolves that number; the legacy env/config fallback stays in main.js for a
// machine that still runs the old agent.
const fs = require("fs");
const path = require("path");

const DEFAULT_PROGRAM_DATA = "C:\\ProgramData";

/**
 * The device's port, from the user-readable copy first, then the data directory, then undefined.
 *
 * `env` is a parameter so a test can point both roots at a temporary directory without touching the
 * real machine.
 */
function readDevicePort(env = process.env) {
    const programData = env.ProgramData || env.PROGRAMDATA || DEFAULT_PROGRAM_DATA;
    const roots = [
        env.SUMMRISE_HELPER_DATA || path.join(programData, "SummriseHelper"),
        env.SUMMRISE_DATA || path.join(programData, "Summrise"),
    ];
    for (const root of roots) {
        try {
            const text = fs.readFileSync(path.join(root, "device.port"), "utf8").trim();
            const port = Number.parseInt(text, 10);
            if (Number.isInteger(port) && port > 0 && port < 65536) return port;
        } catch {
            // no such file here; the next source
        }
    }
    return undefined;
}

module.exports = { readDevicePort };
