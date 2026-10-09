/* tslint:disable */
/* eslint-disable */

export function agentHostLabel(agent_base: string): string;

export function appMenuJson(platform: string, app_name: string): string;

export function aumidReport(platform: string): string;

export function authorization(token?: string | null): string | undefined;

export function autoLaunchPlan(enabled: boolean, exists: boolean): string;

export function browserId(now_ms: number): string;

export function cdpEndpoint(port: number): string;

export function cdpSelfCheckOk(port: number): string;

export function cdpSelfCheckOwnsPort(version_json: string): boolean;

export function cdpUserAgent(version_json: string): string;

export function cdpUserAgentOurs(user_agent?: string | null): boolean;

export function cdpWarningForeign(port: number, user_agent: string): string;

export function cdpWarningNotResponding(port: number): string;

export function cdpWarningUnreachable(port: number): string;

export function controlIsPreflight(method: string): boolean;

export function controlPath(raw_url?: string | null): string | undefined;

export function controlQuery(raw_url: string | null | undefined, key: string): string | undefined;

export function controlRoute(method: string, pathname: string): string;

export function corsAllowOrigin(origin?: string | null): string;

export function deviceToken(config_yaml?: string | null): string | undefined;

export function dshHome(dsh_base: string): string;

/**
 * `admitted` is `isDshUrl(raw)`, evaluated by the host.
 */
export function dshPopupTarget(raw: string, admitted: boolean): string | undefined;

/**
 * `admitted` is `isDshUrl(raw)`, evaluated by the host.
 */
export function dshTarget(raw: string, admitted: boolean): string;

/**
 * The DROP rule for a popup the load door refused. The argument is the DECIDED target.
 */
export function embeddedPopupTarget(target: string): string | undefined;

export function embeddedRecoverUrl(last?: string | null): string;

/**
 * `new Date(at).toTimeString().slice(0, 8)` — with the host's time-zone offset passed in, because wasm
 * has no clock and no zone database.
 */
export function fmtClock(at_ms: number, tz_offset_min: number): string;

export function fmtUptime(secs: number): string;

export function forbiddenFrame(): string;

export function goBackwards(delta: number): boolean;

/**
 * THE ADMISSIBLE HARNESS DOORS, in the order the answer named them.
 *
 * The argument is the ALREADY-PARSED `answer.harnesses` array, because `Array.isArray` and
 * `row?.local_port` are JavaScript operations on a JavaScript value; what is decided here is which
 * numbers pass the range test. The host then calls `addDshPort` — the url-policy crate's — for each,
 * because admitting a door is that crate's decision over its own list and this one must not hold a
 * second copy of it (see `Cargo.toml`). So the shell's harness table is: **the agent names the ports,
 * this crate says which are usable, and the url policy says which doors exist.**
 */
export function harnessDoors(rows: any): Uint16Array;

export function isWaitPage(url: string): boolean;

export function nextRetryMs(current: number): number;

/**
 * `browserOpen`'s plan: `{"reuse": id|null, "evict": id|null}`.
 */
export function planBrowserOpen(existing_json: string, target: string, cap: number): string;

/**
 * ONE POLL'S WHOLE DECISION: the parse, the keep-last rule, the vitals line and the tooltip.
 */
export function refreshTrayHealth(request_json: string): string;

export function resolveAgentPort(env?: string | null, config_port?: number | undefined): number;

export function resolveDshPort(env?: string | null): number;

/**
 * THE ARGUMENT LIST WITH THE DOCUMENTED QUOTING TRAP IN IT — the `/tr` value carries its own inner
 * quotes, because `schtasks` re-parses it as a command line.
 */
export function schtasksCreateArgs(task: string, script: string): string[];

export function schtasksDeleteArgs(task: string): string[];

export function schtasksEndArgs(task: string): string[];

export function schtasksQueryArgs(task: string): string[];

export function schtasksRunArgs(task: string): string[];

export function shellConstants(): string;

export function shouldRetryLoad(is_main_frame: boolean, error_code: number): boolean;

export function shownUrl(live: string | null | undefined, fallback: string): string;

/**
 * `!bounds || bounds.width < 50 || bounds.height < 50`.
 */
export function slotTooSmall(bounds: any): boolean;

export function statusIsAlive(status_code?: number | null): boolean;

export function tokenCacheFresh(cached_at: number, now: number): boolean;

export function trayIconName(platform: string): string;

/**
 * `base_origin` is `isBaseOrigin(mainUrl)`, evaluated by the host — and only when the window is live,
 * which keeps the TypeScript's short-circuit.
 */
export function trayShouldWatch(running: boolean, watch_active: boolean, window_live: boolean, base_origin: boolean): boolean;

/**
 * `desktop_spa` is `isDesktopSpaUrl(url)`, evaluated by the host.
 */
export function tripwireAllows(url: string, desktop_spa: boolean): boolean;

export function tripwireLog(url: string): string;

export function usesAppUserModelId(platform: string): boolean;

export function viewVisible(want_visible: boolean, live_contents: boolean): boolean;

export function watchdogLog(ok: boolean, error?: string | null): string;

export function watchdogShouldStart(misses: number, last_start_at: number, now: number): boolean;

export function windowIconName(): string;

/**
 * `Math.min(3, Math.max(0.5, Number(factor) || 1))` — and the `|| 1` is applied HERE, on the value
 * `Number()` produced, because that is the order the JavaScript evaluated it in.
 */
export function zoomFactor(factor: any): number;
