// Summrise Desktop preload — exposes the native menu command bridge,
// browser-session control, embedded real-render browser, and desktop-app
// settings to the SPA via contextBridge (Electron-native, no HTTP/CORS).
//
// The SPA (agent /desktop/ page) uses:
//   window.summriseBrowser.*   — browser-session windows (driven via CDP :9333)
//   window.summriseEmbedded.*  — embedded REAL browser view (round-246: replaces
//                            the JPEG screencast inside the SPA Browser page)
//   window.summriseDesktop.*   — auto-launch settings + menu command bridge
import { contextBridge, ipcRenderer } from "electron";

contextBridge.exposeInMainWorld("summriseBrowser", {
  open: (url: string) => ipcRenderer.invoke("browser-session:open", url),
  close: (id: string) => ipcRenderer.invoke("browser-session:close", id),
  list: () => ipcRenderer.invoke("browser-session:list"),
});

// round-246: the embedded real-render browser view. The SPA Browser page
// reports its placeholder bounds (via place) and the main process positions
// the WebContentsView over them; navigate() drives the same view the AI sees
// over CDP :9333. Absent from plain-browser (non-Electron) contexts — the
// SPA falls back to the screenshot stream there.
contextBridge.exposeInMainWorld("summriseEmbedded", {
  navigate: (url: string) => ipcRenderer.invoke("embedded-browser:navigate", url),
  back: () => ipcRenderer.invoke("embedded-browser:back"),
  fwd: () => ipcRenderer.invoke("embedded-browser:fwd"),
  reload: () => ipcRenderer.invoke("embedded-browser:reload"),
  zoom: (factor: number) => ipcRenderer.invoke("embedded-browser:zoom", factor),
  place: (bounds: { x: number; y: number; width: number; height: number } | null) =>
    ipcRenderer.invoke("embedded-browser:place", bounds),
  state: () => ipcRenderer.invoke("embedded-browser:state"),
  // round-256: recovery after a renderer crash — the main process force-
  // re-creates the view and navigates to the last URL.
  recover: () => ipcRenderer.invoke("embedded-browser:recover"),
  // round-247: real-navigation pushes from the main process (URL/title/
  // history state after every actual navigation). Returns an unsubscribe fn.
  onNav: (handler: (s: { url: string; canBack: boolean; canFwd: boolean; title: string }) => void) => {
    const listener = (_e: unknown, s: { url: string; canBack: boolean; canFwd: boolean; title: string }) => {
      try { handler(s); } catch { /* SPA-side */ }
    };
    ipcRenderer.on("embedded-browser:nav", listener as never);
    return () => ipcRenderer.removeListener("embedded-browser:nav", listener as never);
  },
  // round-256: the embedded view's renderer crashed (reason + exitCode).
  // Returns an unsubscribe fn.
  onGone: (handler: (d: { reason: string; exitCode: number }) => void) => {
    const listener = (_e: unknown, d: { reason: string; exitCode: number }) => {
      try { handler(d); } catch { /* SPA-side */ }
    };
    ipcRenderer.on("embedded-browser:gone", listener as never);
    return () => ipcRenderer.removeListener("embedded-browser:gone", listener as never);
  },
});

// THE DSH VIEW (the shell's second embedded view). The harness's own UI runs on THIS
// machine's loopback, so the panel never learns its port: `open()` asks the MAIN process to
// navigate the view to its configured base, and the door that decides what that view may load
// (url-policy.isDshUrl) has exactly one origin to check. Same placeholder-bounds handshake as
// the browser view above, because it is the same problem: a native view overlays an empty slot
// the SPA positions.
contextBridge.exposeInMainWorld("summriseDsh", {
  open: () => ipcRenderer.invoke("embedded-dsh:open"),
  place: (bounds: { x: number; y: number; width: number; height: number } | null) =>
    ipcRenderer.invoke("embedded-dsh:place", bounds),
  // SELECTING A HOST: the URL is one of the agent's configured harness doors, and the main process
  // checks it against that list — the pane cannot point the view at an address of its own choosing.
  go: (url: string) => ipcRenderer.invoke("embedded-dsh:go", url),
  state: () => ipcRenderer.invoke("embedded-dsh:state"),
  reload: () => ipcRenderer.invoke("embedded-dsh:reload"),
  // Recovery after a renderer crash: the main process force-re-creates the view.
  recover: () => ipcRenderer.invoke("embedded-dsh:recover"),
  // The view's renderer crashed (reason + exitCode) — without this the pane would show
  // "starting…" forever over a view that will never paint. Returns an unsubscribe fn.
  onGone: (handler: (d: { reason: string; exitCode: number }) => void) => {
    const listener = (_e: unknown, d: { reason: string; exitCode: number }) => {
      try { handler(d); } catch { /* SPA-side */ }
    };
    ipcRenderer.on("embedded-dsh:gone", listener as never);
    return () => ipcRenderer.removeListener("embedded-dsh:gone", listener as never);
  },
});

// Desktop-app settings + menu bridge (Electron-specific — hidden when running
// in a plain browser; the SPA detects the bridge and shows the card only when
// present).
contextBridge.exposeInMainWorld("summriseDesktop", {
  getAutoLaunch: () => ipcRenderer.invoke("desktop:get-auto-launch"),
  setAutoLaunch: (enabled: boolean) => ipcRenderer.invoke("desktop:set-auto-launch", enabled),
  // Native menu → SPA command bridge (stage-l): the main process sends
  // "summrise-menu" with a command id (new-pty, new-ssh, new-serial, new-browser,
  // close-session, next-session, prev-session, export-session,
  // toggle-trajectory, toggle-theme, show-status). Returns an unsubscribe fn.
  onCommand: (handler: (cmd: string) => void) => {
    const listener = (_e: unknown, cmd: string) => { try { handler(cmd); } catch { /* SPA-side */ } };
    ipcRenderer.on("summrise-menu", listener as never);
    return () => ipcRenderer.removeListener("summrise-menu", listener as never);
  },
});
