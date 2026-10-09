/**
 * summrise-gate — Cloudflare Worker front door (thin bootstrap).
 *
 * The file used to be a 90KB single dispatcher where every console route
 * lived inline; since round-73 the routes live in DSH-style plugins and this
 * module only wires them up. Since the 2026-08 refactor it owns NOTHING but
 * the front door:
 *
 *   fetch()            host split (console vs API), HTTPS redirect, static
 *                      assets, /v1/* dispatch, public tooling endpoints
 *   ensurePluginCtx()  builds the plugin context once per isolate:
 *                      auth / devices / mcp / translate / admin plugins own
 *                      every /api/* route + /mcp + /v1/* (see src/plugins/)
 *   frontDoor()        /v1/* -> the Rust worker's service binding (the TypeScript impl is deleted)
 *
 * The public CLI-facing tooling surface (GET /api/health, POST
 * /api/summrise-probe + its rate limiter, the /api/summrise-* installer payloads)
 * lives in tooling.ts — index.ts routes to it and re-exports it for the
 * historical import path.
 *
 * Module map:
 *   channels.ts          channel registry (MODELS/ROUTE_INFO/HEALTH + og endpoints)
 *   upstream.ts          pickRoute/passthroughHeaders/stripBracket (route table)
 *   body-scan.ts         10ms-CPU-budget raw-string scans (never parse big bodies)
 *   anthropic-translate.ts Anthropic↔OpenAI SSE translation (pure, zero env)
 *   reliability.ts       fetchWithTimeout/Retry + BreakerDO + timeouts
 *   session.ts           requireSession/sessionSecret (single copy)
 *   http.ts              jsonOk/jsonError/readJson/CORS
 *   store.ts             KV persistence (users/tokens/devices/plugin links)
 *   tooling.ts           public CLI surface (health/probe/installers)
 *   mcp.ts               MCP endpoint handler (Claude Code)
 *   plugins/*            DSH-style route plugins (auth/devices/mcp/translate/admin)
 */

import { seedAdmin } from "./store.ts";
import { jsonOk, jsonError, readJson, CORS_HEADERS, corsHeadersFor, withCors } from "./http.ts";
import {
  buildHealth,
  probeRateLimited,
  summriseProbe,
  encodeBase64Utf8,
  posixInstaller,
  psInstaller,
  serveAssetText,
} from "./tooling.ts";
// Re-export the public tooling surface: tests + external tooling import it
// from the front-door module (historical path).
export {
  buildHealth,
  probeRateLimited,
  summriseProbe,
  encodeBase64Utf8,
  posixInstaller,
  psInstaller,
} from "./tooling.ts";
import { createPluginContext, registerPlugins, dispatch } from "./plugins/registry.ts";
import authPlugin from "./plugins/auth.ts";
import { csrfCookieViolation } from "./auth.ts";
import devicesPlugin from "./plugins/devices.ts";
import mcpPlugin from "./plugins/mcp.ts";
import adminPlugin from "./plugins/admin.ts";

// Re-exported for tooling/tests that target the front door surface.
// BreakerDO/RouteDO must be exported from the entrypoint —
// wrangler binds the Durable Object classes from here.
export { BreakerDO } from "./reliability.ts";
export { RouteDO } from "./route-do.ts";
// **THESE TWO MOVED OUT OF THE DELETED `/v1` HALF.** They were re-exported from `plugins/translate.ts`, which the
// cutover deleted; `plugins/model-route.ts` is where they have lived since the structure refactor, and the console
// reads one of them (`plugins/auth.ts` for `GET /api/me/route`), so the path is direct now.
export { resolveAutoModel, isModelUsable } from "./model-route.ts";

/**
 * Plugin context: built once per isolate with the shared helpers; every
 * /api/* route, /mcp and /v1/* lives in a plugin now. Lazy so a reload never
 * re-registers duplicate routes.
 */
let __pluginCtx: any = null;
function ensurePluginCtx() {
  if (__pluginCtx) return __pluginCtx;
  __pluginCtx = createPluginContext(null, {
    jsonOk,
    jsonError: jsonError as (status: number, message: string, code?: string) => Response,
    readJson,
    CORS_HEADERS,
  });
  registerPlugins(__pluginCtx, [authPlugin, devicesPlugin, mcpPlugin, adminPlugin]);
  return __pluginCtx;
}

/**
 * THE RUST FRONT DOOR'S BINDING, READ ONCE AND TYPED.
 *
 * `WASM_GATE` is the service binding declared in `wrangler.jsonc`; `undefined` when it is absent (a local
 * `wrangler dev` without the sibling worker, a test env, or a deployment whose binding was removed) — and
 * **an absent binding is a 503, not a fallback**: the TypeScript `/v1` implementation was deleted on 2026-10-07,
 * which is exactly what `frontDoor` says below. The rollback lives one level UP, in the DOMAIN TABLE: move the
 * two custom domains back to the worker still running the pre-cutover code (`vale-gate` — see `wrangler.jsonc`,
 * which kept it deployed for that reason and as the Durable Object owner).
 */
function wasmGate(env: any): { fetch: (r: Request) => Promise<Response> } | null {
  const binding = env?.WASM_GATE;
  return binding && typeof binding.fetch === "function" ? binding : null;
}

/**
 * THE `/v1` FRONT DOOR — THE BINDING, OR A LOUD 503.
 *
 * **THE TYPESCRIPT IMPLEMENTATION IS DELETED (2026-10-07)**, so there is no fallback to fall back to: a deployment
 * without the `WASM_GATE` binding is a deployment that cannot serve the API, and saying so is better than a 404
 * that reads like a routing mistake. `request` is passed separately from the URL-bearing request because the
 * aliases (`/models`, `/chat/completions`) rewrite the path on the way through.
 */
async function frontDoor(original: Request, env: any, request: Request): Promise<Response> {
  const gate = wasmGate(env);
  if (gate) return await gate.fetch(request);
  return withCors(
    original,
    jsonError(
      503,
      "the Rust front door is not bound to this deployment (WASM_GATE) — the TypeScript implementation was deleted at the cutover",
      "api_error",
    ),
    env,
  );
}

/**
 * THE DEVICE FAMILY'S CUTOVER — WHICH PATHS THE RUST WORKER SERVES, AND WHICH TWO IT DOES NOT.
 *
 * The rule is the same one `/v1` already follows: the TypeScript front door hands a path to the `WASM_GATE`
 * service binding, and the DECISION of what belongs to the family is ONE predicate, mirrored in Rust
 * (`gateway/wasm/src/devices.rs::in_family`) so a path that is handed over is a path that is served. What is
 * being cut over (2026-10-08, landing 5 slice 1): the console's device registry — `POST /api/register`,
 * `POST /api/devices/self-register`, `POST /api/install/tunnel-token`, the register-key routes, `install-cmd`,
 * `GET`/`POST /api/devices`, a device's `mcp`, `delete`, `rename` and `panel-grant`, and the panel-grant redeem —
 * fifteen routes whose decisions are Rust now, proved case for case against this implementation by
 * `gateway/wasm/verify.mjs` (67 cases, including the refusals: no session, a forged cookie, a non-admin, a spent
 * claim, a name that is taken, a device that does not exist).
 *
 * **THE TWO EXCLUSIONS, EACH WITH ITS REASON:**
 *
 *   * `/api/devices/<name>/proxy/...` is `plugins/device-proxy.ts` — a reverse proxy that mints per-device
 *     cookies, rewrites panel HTML and can upgrade a WebSocket. It is not this slice, and it is checked here
 *     FIRST because it shares the prefix.
 *   * `POST|PUT /api/upload` is not under `/api/devices` at all and is not in the predicate: a 100 MiB body
 *     passthrough to the relay, which `workers-rs` 0.8.7 cannot forward as a stream.
 *
 * **AND `devicesRouteNeedsSession` IS A ROUTING TEST, NOT AN AUTHENTICATION ONE.** The Rust worker verifies the
 * cookie itself — this only decides which implementation gets to, and it exists for ONE route family: the eleven
 * routes whose first line is `requireAdmin`. `requireSession` has a second identity arm that is NOT yet ported —
 * the Cloudflare Access JWT (`access.ts`), which authenticates an admin with NO console cookie — so an
 * admin-gated request WITHOUT that cookie is kept on the TypeScript path, where the arm lives. The day the arm
 * is ported this predicate loses the condition and the comment goes with it. Nothing is weakened by it: a caller
 * who SENDS a bogus cookie is routed to the worker, which rejects it. The four routes that never asked for a
 * session (`/api/register`, self-register, tunnel-token, panel-grant redeem) are handed over like the rest.
 */
function isDevicesFamily(method: string, path: string): boolean {
  // The reverse proxy shares the prefix — see the header. Checked first.
  if (/^\/api\/devices\/[^/]+\/proxy/.test(path)) return false;
  if (path.startsWith("/api/devices")) return true;
  return method === "POST" && (path === "/api/register" || path === "/api/install/tunnel-token");
}

/**
 * The eleven routes whose handler starts with `requireAdmin` — the ones the Access arm serves cookie-lessly.
 *
 * **IT MIRRORS THE PLUGIN'S ROUTE TABLE BY METHOD, AND THE FIRST VERSION DID NOT.** It tested
 * `^/api/devices/[^/]+$` for any method, which also matches `POST /api/devices/self-register` — a PUBLIC route —
 * and would have kept it on the TypeScript path forever. `devices-cutover.test.mjs` is what caught it; the
 * generic single-segment shape is the DELETE route and nothing else, because every other single-segment path is
 * a specific route registered before it.
 */
function devicesRouteNeedsSession(method: string, path: string): boolean {
  if (path === "/api/devices") return true; // GET list, POST add
  if (path.startsWith("/api/devices/register-keys")) return true; // GET list, DELETE revoke
  if (path === "/api/devices/register-key" || path === "/api/devices/install-cmd") return true;
  if (path === "/api/devices/panel-grant/redeem" || path === "/api/devices/self-register")
    return false;
  if (/^\/api\/devices\/[^/]+\/(mcp|rename|panel-grant)$/.test(path)) return true;
  // The device delete, and the ONLY route the bare single-segment shape belongs to.
  return method === "DELETE" && /^\/api\/devices\/[^/]+$/.test(path);
}

/** Does this request carry the console session cookie at all? (Its VALIDITY is the Rust worker's decision.) */
function hasSessionCookie(request: Request): boolean {
  return (request.headers.get("cookie") || "").includes("ag_session=");
}

export default {
  async fetch(request: Request, env: any) {
    // Auth-core audit MED-1: global CSRF gate for cookie-authed mutations
    // (device panels are SAME-SITE with the console; SameSite=Lax does not
    // help there). Bearer clients carry no cookie — untouched.
    if (csrfCookieViolation(request)) {
      return withCors(request, jsonError(403, "Cross-site request blocked", "csrf_error"), env);
    }
    const url = new URL(request.url);

    // Force HTTPS: the Secure session cookie is only stored over https; on plain http
    // the browser drops it and login appears to "succeed then bounce back".
    // (Cloudflare normalizes url.protocol to https, so inspect x-forwarded-proto.)
    const proto = String(request.headers.get("x-forwarded-proto") || "")
      .split(",")[0]!
      .trim()
      .toLowerCase();
    if (proto && proto !== "https") {
      return Response.redirect(`https://${url.host}${url.pathname}${url.search}`, 308);
    }

    if (request.method === "OPTIONS") {
      // Global preflight: reflect-if-allowlisted + Vary, NO ACAO otherwise.
      return new Response(null, { headers: corsHeadersFor(request, env) });
    }

    try {
      // Hostname isolation: the console (static page + /api/*) lives only on the
      // CONSOLE_HOST var(s). localhost / 127.0.0.1 are allowed for local `wrangler dev`.
      const consoleHosts = String(env.CONSOLE_HOST || "")
        .split(",")
        .map((s) => s.trim())
        .filter(Boolean);
      const isPageHost =
        url.hostname === "localhost" ||
        url.hostname === "127.0.0.1" ||
        consoleHosts.includes(url.hostname);
      const path = url.pathname;

      // ---- Public tooling endpoints (any host) ----
      if (path === "/api/health") {
        return withCors(request, jsonOk(await buildHealth(env)), env);
      }
      if (request.method === "POST" && path === "/api/summrise-probe") {
        if (await probeRateLimited(env, request)) {
          return withCors(
            request,
            jsonError(429, "probe rate limit exceeded", "rate_limit_error"),
            env,
          );
        }
        const body = await readJson(request);
        return withCors(request, await summriseProbe(env, String(body.model || "")), env);
      }
      if (
        path === "/api/summrise-cli" ||
        path === "/api/summrise-install" ||
        path === "/api/summrise-install.ps1"
      ) {
        const cli = await serveAssetText(env, "/summrise");
        if (cli === null)
          return withCors(
            request,
            jsonError(404, "summrise CLI not found", "not_found_error"),
            env,
          );
        // Genuinely-public installer payloads (curl|sh / irm|iex — CORS-
        // irrelevant non-browser clients): KEEP the ACAO:* wildcard so any
        // browser-hosted install helper keeps working. No session, no secret.
        const publicCors = { "Access-Control-Allow-Origin": "*" };
        if (path === "/api/summrise-cli") {
          return new Response(cli, {
            headers: {
              "Content-Type": "text/plain; charset=utf-8",
              ...CORS_HEADERS,
              ...publicCors,
            },
          });
        }
        const b64 = encodeBase64Utf8(cli);
        const body = path === "/api/summrise-install" ? posixInstaller(b64) : psInstaller(b64);
        return new Response(body, {
          headers: { "Content-Type": "text/plain; charset=utf-8", ...CORS_HEADERS, ...publicCors },
        });
      }

      await seedAdmin(env);

      // ---- Console API + MCP endpoint (page hosts) — all plugin-owned ----
      if (isPageHost && (path.startsWith("/api/") || path === "/mcp")) {
        // **THE DEVICE FAMILY GOES TO THE RUST WORKER WHEN THE BINDING IS THERE.** `seedAdmin` has already run
        // (it is the front door's job and the worker does not do it), the CSRF gate above has already run, and
        // the cookie test applies ONLY to the admin-gated routes — the Access-arm carve-out described on
        // `isDevicesFamily`. Without the binding — a local `wrangler dev`, a test env, a deployment whose binding
        // was removed — this falls through to the TypeScript plugin below, UNCHANGED: that is the rollback, and
        // it is why `plugins/devices.ts` and `store/devices.ts` are still here. Deleting them is the last step of
        // this landing, after the deploy that proves the worker serves this family.
        if (
          isDevicesFamily(request.method, path) &&
          (!devicesRouteNeedsSession(request.method, path) || hasSessionCookie(request)) &&
          wasmGate(env)
        ) {
          return await frontDoor(request, env, request);
        }
        const pctx = ensurePluginCtx();
        const hit = dispatch(
          pctx,
          request.method,
          path,
          request,
          env,
          url,
          url.protocol === "https:",
        );
        if (hit !== null) return withCors(request, await hit, env);
        return withCors(request, jsonError(404, "Not Found", "not_found_error"), env);
      }

      // ---- OpenAI-compatible alias: /models → /v1/models, /chat/completions → /v1/chat/completions ----
      //
      // **THE ALIAS GOES TO THE SAME PLACE `/v1/*` DOES** — leaving it on the TypeScript path while `/v1/*` moved
      // would make one client's two spellings of the same call answer from two implementations, which is the
      // divergence this whole migration exists to remove.
      if ((path === "/models" || path === "/chat/completions") && request.method !== "OPTIONS") {
        const v1Url = new URL(url);
        v1Url.pathname = "/v1" + path;
        return await frontDoor(request, env, new Request(v1Url, request));
      }

      // ---- Static page (Workers Assets): non-/v1/ paths → ai domain only ----
      if (!path.startsWith("/v1/")) {
        if (!isPageHost)
          return withCors(request, jsonError(404, "Not Found", "not_found_error"), env);
        if (env.ASSETS && typeof env.ASSETS.fetch === "function") {
          return withCors(request, await env.ASSETS.fetch(request), env);
        }
        return withCors(request, jsonError(404, "Not Found", "not_found_error"), env);
      }

      // ---- /v1/* gateway (both domains) ----
      //
      // ── THE CUTOVER: `/v1/*` IS SERVED BY THE RUST WORKER ────────────────────────────────────────────────
      //
      // **AND THE ROLLBACK IS THE DOMAIN TABLE, NOT THIS BINDING.** Deleting `WASM_GATE` restores no TypeScript
      // path — that implementation was deleted on 2026-10-07, and `frontDoor` answers 503 without the binding.
      // What restores `/v1` is moving the two console hostnames back to the worker that still runs
      // the pre-cutover code (`vale-gate`, kept deployed by the 2026-10-08 rename as the Durable Object owner —
      // see `wrangler.jsonc`): no DNS change, no Access change, no data migration, because both implementations
      // read the same KV and write the same Durable Object and were measured byte-for-byte equal on 91
      // differential cases plus the four tokenless paths against this live host.
      //
      // WHY A SERVICE BINDING AND NOT A ZONE ROUTE (the plan's first shape, corrected by measurement): these
      // hostnames are Worker CUSTOM DOMAINS, and a route on a custom domain is INERT. Cloudflare's own analytics for
      // the day read `vale-gate 550 requests / vale-gate-wasm 3` while a route for `/v1/models` existed, and
      // the PUBLIC console host answers the console page (200, text/html) — so repointing the custom domain would
      // take the console with it. A binding keeps one hostname and one Access policy. (The hostnames themselves are
      // written in exactly one file, `wrangler.jsonc`, because `agent/tests/production_host.rs` is a ratchet over
      // which files may name them; the live worker's name is there too.)
      //
      // NOTHING IS RE-STAMPED ON THE FORWARDED RESPONSE: the Rust worker applies the per-request CORS itself, and
      // the corpus pins those headers (`front-door-cors-corpus.json`); wrapping it here would add a second opinion.
      //
      // THE ONE KNOWN GAP, MEASURED AND ACCEPTED: `vale-gate-wasm` is missing four channel secrets
      // (`AMD_API_KEY`, `CMD_API_KEY`, `GMI_API_KEY`, `R4_API_KEY`), so those four prefixes answer
      // `502 config_error: <NAME> not configured` to a user who has no key of their own — while `/api/health`
      // reports every channel `ok:true`, because it reports whether the MODELS answer, not whether the KEYS are
      // present. `og/`, the channel in use, has its secret on both workers. `wrangler secret put <NAME> --name
      // vale-gate-wasm` closes the gap.
      return await frontDoor(request, env, request);
    } catch (error) {
      // Never echo raw error internals to clients (logged server-side).
      console.error("[gateway] unhandled:", error);
      return withCors(request, jsonError(500, "Internal error", "api_error"), env);
    }
  },
};
