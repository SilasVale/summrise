/* tslint:disable */
/* eslint-disable */

/**
 * The one wrapper with a decision of its own: the JavaScript threw a real `Error`, so a thrown STRING
 * would not be the same refusal (a caller printing `err.message` or matching a regex sees the
 * difference). The message is the TypeScript's, `${port}` included — which is why a non-number is
 * rendered with `String(value)` rather than dropped: `addDshPort("abc")` must say `not a port: abc`.
 */
export function addDshPort(port: number): void;

export function agentBase(): string;

export function certBypassAllowed(url: string): boolean;

export function clearExtraDshPorts(): void;

export function controlOriginOk(origin?: string | null): boolean;

export function dshBase(): string;

export function dshOrigins(): string[];

export function frameUrlOk(url: string): boolean;

export function getAgentPort(): number;

export function getDshPort(): number;

export function isBaseOrigin(url: string): boolean;

export function isDesktopSpaUrl(url: string): boolean;

export function isDshUrl(url: string): boolean;

export function isPrivateHost(hostname: string): boolean;

/**
 * `Option<u16>` rather than a `JsValue` holding `null`: the generated `.d.ts` then says
 * `number | undefined` instead of `any`, and the shell's one call site is `if (port) return port;` — a
 * truthiness test, which `undefined` and `null` answer the same way. The TypeScript answered `null`;
 * this is the one place the JS-visible answer is spelled differently, and nothing can observe it.
 */
export function parseAgentPort(yaml_text: string): number | undefined;

export function sanitizeBrowserUrl(url?: string | null): string;

export function setAgentPort(port: number): void;

export function setDshPort(port: number): void;
