"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
// Summrise Desktop host preload — the TWO host-only bridges, exposed to `src/host.html` via
// contextBridge (Electron-native, no HTTP/CORS).
//
// THE HOST PAGE IS THE ONLY FRAME THAT GETS THIS. The main window loads a device page and has NO
// preload at all; the host window is created with `webPreferences.preload = preload.js` and its
// `webContents` is the one sender the shell's `frameOk` admits. The device page therefore cannot
// reach the native views, the tray or any IPC channel.
//
//   window.summriseBrowser  (10) — the embedded browser view: navigate/back/fwd/reload/zoom/place/
//                            state/recover, plus the nav and gone pushes. `place` is the
//                            placeholder-bounds handshake: the host page reports the CSS box the
//                            native `WebContentsView` should cover, and `null` hides it.
//   window.summriseHarness  (8) → the harness view on this machine's loopback: targets/open/place/
//                            go/state/reload/recover, plus the gone push. `targets` is the policy's
//                            door list; `open` lets the MAIN process navigate to its configured base,
//                            so this page never learns the harness port.
//
// Every subscriber returns an unsubscribe function (not a promise): a page that cannot unsubscribe
// leaks a listener per mount and shows duplicated navigation events.
const electron_1 = require("electron");
electron_1.contextBridge.exposeInMainWorld("summriseBrowser", {
    // Every raw URL goes through the main process's ONE load door (sanitizeBrowserUrl); this bridge
    // decides nothing.
    navigate: (url) => electron_1.ipcRenderer.invoke("embedded-browser:navigate", url),
    back: () => electron_1.ipcRenderer.invoke("embedded-browser:back"),
    fwd: () => electron_1.ipcRenderer.invoke("embedded-browser:fwd"),
    reload: () => electron_1.ipcRenderer.invoke("embedded-browser:reload"),
    zoom: (factor) => electron_1.ipcRenderer.invoke("embedded-browser:zoom", factor),
    // The placeholder-bounds handshake: CSS pixels in the window's content coordinate space, or null.
    place: (bounds) => electron_1.ipcRenderer.invoke("embedded-browser:place", bounds),
    state: () => electron_1.ipcRenderer.invoke("embedded-browser:state"),
    // Recovery after a renderer crash — the main process force-recreates the view and navigates to the
    // last URL its door decided.
    recover: () => electron_1.ipcRenderer.invoke("embedded-browser:recover"),
    // Real-navigation pushes (url/title/history after every actual navigation). Returns an unsubscribe fn.
    onNav: (handler) => {
        const listener = (_e, s) => {
            try {
                handler(s);
            }
            catch { /* host-page */ }
        };
        electron_1.ipcRenderer.on("embedded-browser:nav", listener);
        return () => electron_1.ipcRenderer.removeListener("embedded-browser:nav", listener);
    },
    // The view's renderer crashed (reason + exitCode). Returns an unsubscribe fn.
    onGone: (handler) => {
        const listener = (_e, d) => {
            try {
                handler(d);
            }
            catch { /* host-page */ }
        };
        electron_1.ipcRenderer.on("embedded-browser:gone", listener);
        return () => electron_1.ipcRenderer.removeListener("embedded-browser:gone", listener);
    },
});
electron_1.contextBridge.exposeInMainWorld("summriseHarness", {
    // The policy's harness doors (`dshOrigins()`): addressed loopback origins, never a device table this
    // page invents. The answer is `{ ok, targets }`.
    targets: () => electron_1.ipcRenderer.invoke("embedded-dsh:targets"),
    // Navigate the view to the main process's configured harness home. Idempotent.
    open: () => electron_1.ipcRenderer.invoke("embedded-dsh:open"),
    place: (bounds) => electron_1.ipcRenderer.invoke("embedded-dsh:place", bounds),
    // Select one of the doors listed by `targets()`; the main process re-checks it (isDshUrl).
    go: (url) => electron_1.ipcRenderer.invoke("embedded-dsh:go", url),
    state: () => electron_1.ipcRenderer.invoke("embedded-dsh:state"),
    reload: () => electron_1.ipcRenderer.invoke("embedded-dsh:reload"),
    // Recovery after a renderer crash: the main process force-recreates the view.
    recover: () => electron_1.ipcRenderer.invoke("embedded-dsh:recover"),
    // The view's renderer crashed (reason + exitCode). Returns an unsubscribe fn.
    onGone: (handler) => {
        const listener = (_e, d) => {
            try {
                handler(d);
            }
            catch { /* host-page */ }
        };
        electron_1.ipcRenderer.on("embedded-dsh:gone", listener);
        return () => electron_1.ipcRenderer.removeListener("embedded-dsh:gone", listener);
    },
});
