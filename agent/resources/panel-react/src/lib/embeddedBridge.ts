// THE ELECTRON BRIDGE, as the panel sees it — typed, and pinned to the preload.
//
// WHY THIS FILE EXISTS. `vale-desktop-electron/src/preload.ts` exposes three objects through
// contextBridge and the panel calls them. Each side is typed independently, so each compiles
// happily while a renamed member breaks the DESKTOP APP silently — and only there: these surfaces
// exist behind `window.valeEmbedded`, which a plain-browser harness does not have, so no rendered
// sweep has ever seen them (rounds 45 and 50 both had to name them as the project's blind spot).
//
// The interface used to live inside EmbeddedBrowserPane, which is why nothing could check it: a
// fixture cannot compare against a type. Now the shape has a RUNTIME MANIFEST beside it, and the
// type system keeps the two honest in both directions:
//
//   * every entry must be a real member (`satisfies readonly (keyof …)[]`);
//   * every member must be listed (`Missing extends never`).
//
// …and `src/lib/__tests__/embeddedBridge.test.ts` compares the manifest with
// `agent/tests/fixtures/embedded-bridge.json`, which the shell's own suite checks against the
// preload. Renaming a member on either side now fails a test on one of them.

/** One navigation push from the main process.
 *
 *  All four are REQUIRED, as the component's original local interface had them: the main process
 *  sends the whole state after every real navigation, and the panel's onNav handler reads them
 *  without guarding. Writing them optional here (my first attempt) silently weakened the type and
 *  broke the handler's call sites — the compiler caught it, which is the whole point of moving the
 *  shape out of the component and into a file something can check. */
export interface EmbeddedNavState {
  url: string;
  canBack: boolean;
  canFwd: boolean;
  title: string;
}

/** The embedded REAL-browser view (absent in a plain browser). */
export interface EmbeddedBridge {
  navigate: (url: string) => Promise<unknown>;
  back: () => Promise<unknown>;
  fwd: () => Promise<unknown>;
  reload: () => Promise<unknown>;
  zoom: (factor: number) => Promise<unknown>;
  place: (bounds: { x: number; y: number; width: number; height: number } | null) => Promise<unknown>;
  state: () => Promise<{ ok: boolean; url?: string; canBack?: boolean; canFwd?: boolean; visible?: boolean }>;
  recover: () => Promise<unknown>;
  onNav: (handler: (s: EmbeddedNavState) => void) => () => void;
  onGone: (handler: (d: { reason: string; exitCode: number }) => void) => () => void;
}

/** Browser-session windows, driven over CDP. */
export interface ValeBrowserBridge {
  open: (url: string) => Promise<unknown>;
  close: (id: string) => Promise<unknown>;
  list: () => Promise<unknown>;
}

/** Auto-launch settings and the native menu command bridge. */
export interface ValeDesktopBridge {
  getAutoLaunch: () => Promise<unknown>;
  setAutoLaunch: (enabled: boolean) => Promise<unknown>;
  onCommand: (handler: (cmd: string) => void) => () => void;
}

/** THE RUNTIME MANIFEST of the embedded bridge — the list of members above, checkable at run time
 *  and type-checked against the interface in both directions. */
export const EMBEDDED_MEMBERS = [
  "navigate",
  "back",
  "fwd",
  "reload",
  "zoom",
  "place",
  "state",
  "recover",
  "onNav",
  "onGone",
] as const satisfies readonly (keyof EmbeddedBridge)[];

/** Compile-time completeness: a member added to the interface but not to the manifest is an error
 *  here, so the pair cannot drift. (The type is never constructed — that is the point.) */
type MissingFromManifest = Exclude<keyof EmbeddedBridge, (typeof EMBEDDED_MEMBERS)[number]>;
const _complete: MissingFromManifest extends never ? true : never = true;
void _complete;

/** RUNTIME MANIFESTS for the other two bridges, with the same two-way type check as above. */
export const BROWSER_MEMBERS = ["open", "close", "list"] as const satisfies readonly (keyof ValeBrowserBridge)[];
type MissingBrowser = Exclude<keyof ValeBrowserBridge, (typeof BROWSER_MEMBERS)[number]>;
const _browserComplete: MissingBrowser extends never ? true : never = true;
void _browserComplete;

export const DESKTOP_MEMBERS = ["getAutoLaunch", "setAutoLaunch", "onCommand"] as const satisfies readonly (keyof ValeDesktopBridge)[];
type MissingDesktop = Exclude<keyof ValeDesktopBridge, (typeof DESKTOP_MEMBERS)[number]>;
const _desktopComplete: MissingDesktop extends never ? true : never = true;
void _desktopComplete;

/** The bridge, or null in a plain browser. */
export function embeddedBridge(): EmbeddedBridge | null {
  return ((window as unknown as { valeEmbedded?: EmbeddedBridge }).valeEmbedded ?? null) as EmbeddedBridge | null;
}

/** Browser-session windows, or null in a plain browser.
 *
 *  THIS ACCESSOR EXISTS BECAUSE App.tsx REACHED THROUGH `(window as any)`. It called
 *  `bridge.open("about:blank")` with no type at all, so a renamed member in the preload would not
 *  have failed anything — it would have fallen through to the local-control fallback, or produced
 *  "Browser sessions need the Vale desktop app" on a machine that HAS the desktop app. The shape was
 *  already written here (`ValeBrowserBridge`); nothing used it. */
export function browserBridge(): ValeBrowserBridge | null {
  return ((window as unknown as { valeBrowser?: ValeBrowserBridge }).valeBrowser ?? null) as ValeBrowserBridge | null;
}

/** The desktop bridge (auto-launch + menu commands), or null in a plain browser. */
export function desktopBridge(): ValeDesktopBridge | null {
  return ((window as unknown as { valeDesktop?: ValeDesktopBridge }).valeDesktop ?? null) as ValeDesktopBridge | null;
}
