/**
 * device-dial-logic.ts — GENERATED. DO NOT EDIT BY HAND.
 *
 * Emitted by `gateway/wasm/src/device.rs` (`render_typescript`), which is now the single source. The freshness test
 * in that module is a byte comparison against this file, so a hand edit fails there with the text and one command to
 * fix it:
 *
 *     SUMMRISE_REFRESH_DEVICE_DIAL=1 cargo test --manifest-path gateway/wasm/Cargo.toml device::emission
 *
 * `gateway/src/device-fetch.ts` re-exports this module and keeps the HOST: the env reads, the URL parse, the
 * `fetchWithTimeout` call and the `Response` rebuilds. **The RULES left the file, and the two that were already
 * shared with the Rust worker — the SSRF blocklist and the suffix allowlist — had a copy on each side until now.**
 *
 * The comments below are the source file's own, carried verbatim.
 */

/// The default device-host suffix. **IT IS A PRODUCTION HOSTNAME**, which is why `agent/tests/production_host.rs`
/// declares it beside `device-fetch.ts`.
export const DEFAULT_DEVICE_HOST_SUFFIX = ".agent.saisi.online";

/**
 * `hostAllowError(hostname, env)`'s RULE, with the suffix already resolved — the Rust takes the VALUE, so reading
 * `DEVICE_HOST_SUFFIX` off the environment stays in the caller.
 *
 * Two tests, and the second is the one that is easy to miss: the host must END WITH the suffix **and be strictly
 * longer than it**, so the bare suffix is not a device hostname. The suffix is lowercased and so is the host, because
 * a hostname comparison is case-insensitive; an empty override falls back to the default through `||`, which is a
 * truthiness test rather than a presence test.
 *
 * (The length test counts UTF-16 units here and BYTES in the Rust. Reachable only for a non-ASCII hostname, which the
 * URL parser's own rules punish long before this — recorded rather than quietly aligned, because a port improves
 * nothing.)
 */
export function hostAllowErrorFor(
  hostname: string,
  suffix: string | null | undefined,
): string | null {
  const s = (suffix || DEFAULT_DEVICE_HOST_SUFFIX).toLowerCase();
  const h = String(hostname).toLowerCase();
  if (!h.endsWith(s) || h.length <= s.length) {
    return `hostname must be under ${s}`;
  }
  return null;
}

/**
 * `deviceHostError(hostname)`: an error string when the hostname must NOT be dialled with device credentials.
 *
 * stage-n SSRF audit (HIGH): reject private/link-local/internal IPs — an attacker who registers a device with
 * hostname "169.254.169.254" (cloud metadata) or "127.0.0.1" makes the gateway dial it with the device token.
 *
 * Decimal/hex/octal IPv4 need no extra rules — the WHATWG parser already normalizes them (2130706433/0x7f.0.0.1/
 * 0177.0.0.1 all parse to 127.0.0.1 and hit the 127. block). What it does NOT normalize away: 0.0.0.0, the cloud
 * metadata IP, and IPv4-mapped IPv6 (::ffff:/96 can smuggle 127.0.0.1 past every v4 rule — bracketed or bare).
 * 172.16/12 is ONLY second-octet 16-31 (numeric) — a blanket startsWith("172.") also blocked public 172.15.x.x /
 * 172.32+.x.x. The fc/fd/fe80 unique-local/link-local prefixes must be actual address forms (contain ':': hostnames
 * never do) — otherwise a hostname STRING like 'fc.example.com' false-positives.
 */
export function deviceHostError(hostname: string): string | null {
  const hostLower = String(hostname).toLowerCase();
  let is172Private = false;
  if (hostLower.startsWith("172.")) {
    const second = Number(hostLower.split(".")[1]);
    is172Private = Number.isInteger(second) && second >= 16 && second <= 31;
  }
  const isV6Private =
    hostLower.includes(":") &&
    (hostLower.startsWith("fc") || hostLower.startsWith("fd") || hostLower.startsWith("fe80"));
  if (
    hostLower === "localhost" ||
    hostLower.startsWith("127.") ||
    hostLower.startsWith("10.") ||
    hostLower.startsWith("192.168.") ||
    is172Private ||
    hostLower === "::1" ||
    hostLower === "[::1]" ||
    isV6Private ||
    hostLower === "0.0.0.0" ||
    hostLower === "[::]" ||
    hostLower === "169.254.169.254" ||
    hostLower.startsWith("::ffff:") ||
    hostLower.startsWith("[::ffff:")
  ) {
    return "device hostname resolves to a private/internal address";
  }
  return null;
}

/** The refusal both authority-shaped bad paths answer with (round-120/121). */
export const PROXY_PATH_REFUSAL = "invalid proxy path";

/** The AUTHORITY-RELEVANT prefix of `restPath` — up to the first `/`, `?` or `#` — may carry neither userinfo nor a
 *  scheme. **NARROWED ON PURPOSE** (round-121): an at-sign in a QUERY string is legitimate (`?user=a@b.com`), and the
 *  hostname-equality gate already blocks userinfo smuggling. */
export function authorityPrefixOk(restPath: string): boolean {
  const authority = restPath.split(/[/?#]/, 1)[0]!;
  return !authority.includes("@") && !/^[a-z][a-z0-9+.-]*:/i.test(authority);
}

/** The path half of the dial URL: `/` is prepended unless it is already there. */
export function dialPath(restPath: string): string {
  return restPath.startsWith("/") ? restPath : `/${restPath}`;
}

/** **BELT-AND-SUSPENDERS**: whatever the parser produced, the host MUST be the device's own — case-insensitively,
 *  because WHATWG lowercases special-scheme hosts while registration stores hostnames verbatim. */
export function hostnameMismatch(parsedHost: string, hostname: string): boolean {
  return parsedHost.toLowerCase() !== String(hostname).toLowerCase();
}

/** A guard refusal: the shape every path in the stack answers with. */
export type DialRefusal = { status: 400; ok: false; resp: undefined; error: string };

/** What the guards decided: either a refusal to return as-is, or the URL to dial. */
export type DialPlan = { refusal: DialRefusal } | { url: string };

/** THE GUARD STACK, IN ORDER — and the order IS the security argument, which is why it is here rather than in the
 *  caller: the authority-prefix sanitization (round-120/121) comes BEFORE the URL is built, then the parsed-hostname
 *  equality, then the private-IP blocklist, then the suffix allowlist.
 *
 *  `parse` is the platform's parser and **MAY THROW — the source's `new URL` sits outside its `try`, so an
 *  unparseable authority escapes `deviceFetch` instead of becoming a refusal**, and this function does not catch
 *  it. */
export function dialPlan(
  hostname: string,
  restPath: string,
  suffix: string | null | undefined,
  parse: (url: string) => URL,
): DialPlan {
  if (!authorityPrefixOk(restPath)) {
    return { refusal: { status: 400, ok: false, resp: undefined, error: PROXY_PATH_REFUSAL } };
  }
  const parsed = parse(`https://${hostname}${dialPath(restPath)}`);
  if (hostnameMismatch(parsed.hostname, hostname)) {
    return { refusal: { status: 400, ok: false, resp: undefined, error: PROXY_PATH_REFUSAL } };
  }
  const hostErr = deviceHostError(parsed.hostname);
  if (hostErr) return { refusal: { status: 400, ok: false, resp: undefined, error: hostErr } };
  const suffixErr = hostAllowErrorFor(hostname, suffix);
  if (suffixErr) return { refusal: { status: 400, ok: false, resp: undefined, error: suffixErr } };
  // **THE DIAL GOES TO THE PARSED URL, NOT THE STRING THAT WAS PARSED** — the WHATWG parser lowercases the host, and
  // `device-fetch.test.mjs` pins exactly that (`dFresh` arrives as `dfresh`). The Rust dials `Url::to_string()` for
  // the same reason.
  return { url: parsed.toString() };
}

/** One header operation the dial applies, IN ORDER, after copying the caller's own headers. */
export type DialHeaderOp =
  { op: "delete"; name: string } | { op: "set"; name: string; value: string };

/** The header hygiene: `host` and `cookie` are DELETED and `Authorization` is SET — in that order, so a caller
 *  sending any of the three has it removed or replaced. */
export function dialHeaderOps(token: string): DialHeaderOp[] {
  return [
    { op: "delete", name: "host" },
    { op: "delete", name: "cookie" },
    { op: "set", name: "Authorization", value: `Bearer ${token}` },
  ];
}

/** The bound, by method (round-55/56): reads are bounded, and POST — a slow-loris vector on a blackholed tunnel — is
 *  bounded at 60 s where everything else gets 15 s. */
export function dialTimeoutMs(isPost: boolean): number {
  return isPost ? 60000 : 15000;
}

/** The transport refusal's message. */
export function deviceUnreachable(message: string): string {
  return `Device unreachable: ${message}`;
}

/** Which of the three forms a proxied response is rebuilt into. */
export type UpgradeShape = "none" | "socket" | "body";

/** `build101Response`'s shape: only a 101 is rebuilt at all, with its socket when it has one and as the
 *  body-plus-headers form when it does not. */
export function upgradeShape(status: number, hasSocket: boolean): UpgradeShape {
  if (status !== 101) return "none";
  return hasSocket ? "socket" : "body";
}
