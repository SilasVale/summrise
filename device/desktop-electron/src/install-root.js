"use strict";
// Where the install root is, from the directory the host runs in.
//
// The installed host runs out of `<install>\desktop\resources\app.asar\src`, so the root is four
// levels up: `<install>`. The old three-level arithmetic landed on `<install>\desktop`, which carries
// no `etc\config.yaml`. `path.win32` keeps the answer the Windows one even when the Node test runs on
// Linux.
const path = require("path");

/** The install root above `...\desktop\resources\app.asar\src`, normalised for Windows. */
function installRootFrom(appDir) {
    return path.win32.join(appDir, "..", "..", "..", "..");
}

module.exports = { installRootFrom };
