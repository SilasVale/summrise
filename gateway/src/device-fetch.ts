/**
 * device-fetch — device dialing primitives for summrise-gate (FOUNDATION module).
 *
 * Holds the pieces of the device module that need to be importable without
 * dragging in all of index.js (keeps index.js ↔ mcp.js imports acyclic).
 *
 * Boundary + security review (2026-09-06): correctly layered — every device
 * dial (proxyDevice, MCP terminal tools, browser bridge) flows through
 * deviceFetch with the full SSRF guard stack: authority-prefix sanitization
 * (round-120/121) → parsed-hostname equality → private-IP blocklist
 * (deviceHostError) → the suffix allowlist (DEVICE_HOST_SUFFIX, default
 * .agent.saisi.online) → header hygiene → bounded fetch.
 *
 * THE SUFFIX ALLOWLIST IS APPLIED HERE NOW, and that sentence used to be a lie:
 * `_env` was unused, so the check the paragraph above listed as part of the stack was
 * only ever run at REGISTRATION time — by `/api/register` and self-register. Two admin
 * write paths skipped it entirely (fixed), and any record predating those fixes would
 * have been dialled. A dial-time check is the one that cannot be bypassed by a path
 * someone forgot to guard.
 *
 * REDIRECTS ARE NOT FOLLOWED (round 90). This used to say "known characteristic,
 * accepted: fetch follows redirects, so a device could 302 the gateway toward another
 * PUBLIC host". That is no longer what happens — `redirect: "manual"` — and the old
 * reasoning was wrong anyway: Cloudflare forwards Authorization to a cross-host
 * Location, so the risk was the device's TOKEN, not merely the destination.
 *
 * ── AND THE STACK ITSELF IS RUST'S NOW (2026-10-13) ────────────────────────
 *
 * **EVERY RULE THE FIRST PARAGRAPH NAMES IS `gateway/wasm/src/device.rs`**, reaching this file as
 * `device-dial-logic.ts` (GENERATED, and the freshness test is a byte comparison): the private-IP blocklist, the
 * suffix allowlist, the authority-prefix refusal, the hostname-equality gate, the header hygiene IN ORDER, the
 * timeout bound and the 101 shape. **TWO OF THEM HAD A COPY ON EACH SIDE UNTIL THIS LANDING** — `deviceHostError`,
 * the stage-n SSRF list, and `hostAllowError`, whose Rust twin already existed beside a TypeScript one — and a guard
 * stack duplicated across two languages is the one duplication in this repository that is a HOLE rather than a bug,
 * which is the sentence `device.rs`'s own header carries.
 *
 * **AND THE ORDER LEFT WITH THE RULES.** The stack's sequence is its security argument, so it is `dialPlan` — one
 * function, the four gates, and the URL that survives them — rather than four calls this file makes in a remembered
 * order. What is left here is the HOST: the env reads, the parser, the fetch and the `Response` rebuilds.
 */

import { fetchWithTimeout } from "./reliability.ts";
import {
  dialHeaderOps,
  dialPlan,
  dialTimeoutMs,
  deviceUnreachable,
  hostAllowErrorFor,
  upgradeShape,
} from "./device-dial-logic.ts";

// The emitted rules, re-exported so `from "./device-fetch.ts"` keeps working — `deviceHostError` and
// `hostAllowError` are pinned directly from the test suite, and the registration paths import them here.
export * from "./device-dial-logic.ts";

/**
 * Rebuild a 101 Switching Protocols response carrying the upgraded WebSocket.
 * The old code built `new Response(resp.body, {status:101, headers})` which
 * throws RangeError (101 is only legal with a `webSocket` property), so every
 * WS upgrade through the proxy 500'd. Minimal branch: no header rewriting.
 *
 * **WHICH OF THE THREE SHAPES THIS TAKES IS `upgradeShape`'S** — only a 101 is rebuilt, its socket form when it has
 * a socket, and the body-plus-headers form when it has none. A repack failure is the platform's (#3047), which is
 * why the `catch` is here.
 */
export function build101Response(resp: any) {
  const shape = upgradeShape(resp.status, !!resp.webSocket);
  if (shape === "none") return null;
  if (shape === "socket") {
    try {
      return new Response(null, { status: 101, webSocket: resp.webSocket });
    } catch {
      return resp; // workerd #3047: any repack failure → pass through untouched
    }
  }
  return new Response(resp.body || null, { status: 101, headers: new Headers(resp.headers) });
}

/// The device-hostname allowlist, overridable per-deployment via DEVICE_HOST_SUFFIX.
///
/// IT LIVES HERE, in the leaf module, because BOTH the registration paths (devices.ts) and
/// the DIAL path (deviceFetch below) must apply it, and devices.ts already imports this
/// file. Defining it in devices.ts and importing it back would be a cycle.
// Exported for direct pins (SOLID Round-28; additive — handlers untouched).
//
// **THE RULE IS THE EMISSION'S; READING `env` IS THIS FILE'S** — the emitted twin takes the suffix VALUE, which is
// the shape the Rust has (`host_allow_error(hostname, Option<&str>)`).
export function hostAllowError(hostname: string, env: any): string | null {
  return hostAllowErrorFor(hostname, env?.DEVICE_HOST_SUFFIX);
}

/**
 * Fetch a device panel/MCP path with the Bearer token injected server-side.
 * Shared by proxyDevice (HTTP proxy) and the gateway MCP terminal tools.
 * Behavior is identical to the old inline fetch in proxyDevice (Bearer
 * injection, host/cookie stripped). Returns { status, ok, resp } — resp is
 * undefined when the device is unreachable, with `error` carrying the reason.
 *
 * **THE GUARD STACK IS ONE CALL** (`dialPlan`), and `parse` is the platform's parser rather than an emitted rule: an
 * unparseable authority THROWS out of here, exactly as the source's `new URL` does from outside its `try`.
 */
export async function deviceFetch(env: any, device: any, restPath: string, init: any = {}) {
  const plan = dialPlan(
    String(device.hostname),
    restPath,
    env?.DEVICE_HOST_SUFFIX,
    (url) => new URL(url),
  );
  if ("refusal" in plan) return plan.refusal;
  const headers = new Headers(init.headers || {});
  for (const op of dialHeaderOps(device.token)) {
    if (op.op === "delete") headers.delete(op.name);
    else headers.set(op.name, op.value);
  }
  let resp;
  try {
    // Read methods are bounded (round-55): a blackholed tunnel hung until
    // the platform's subrequest ceiling. round-56: the bound applies to
    // EVERY non-POST method; POST is bounded at 60 s — the shape is `dialTimeoutMs`'s.
    // redirect: "manual" — DO NOT FOLLOW. The guard above ran on the INITIAL url only, so
    // following a redirect would dial an address it never checked (127.0.0.1, a
    // link-local metadata endpoint), AND Cloudflare forwards Authorization/x-summrise-auth to
    // a cross-host redirect target — handing this device's permanent token and proxy
    // secret to a host the operator never registered. A device that redirects is not a
    // device to follow; the 3xx is returned to the caller as-is.
    resp = await fetchWithTimeout(
      plan.url,
      { ...init, headers, redirect: "manual" },
      dialTimeoutMs(init.method === "POST"),
    );
  } catch (e) {
    return { status: 502, ok: false, error: deviceUnreachable((e as Error).message) };
  }
  return { status: resp.status, ok: resp.ok, resp };
}
