/**
 * RouteDO — Durable Object storing per-user model route selections.
 *
 * Single global instance ("global") holds all users' routes in SQLite.
 * Replaces the KV-based route storage that suffered from cross-isolate
 * eventual consistency (model=auto requests on different isolates read
 * stale values).
 *
 * Storage: `route:{uid}` → model string (e.g. "cm/deepseek/deepseek-v4.1-flash")
 *
 * HTTP API (called via stub.fetch):
 *   GET    /route?uid=xxx       → { model: "..." | null }
 *   PUT    /route               → body { uid, model }   → { ok: true }
 *   DELETE /route?uid=xxx       → { ok: true }
 *
 * **ITS DECISIONS ARE EMITTED FROM RUST (2026-10-13).** The fail-closed `DO_AUTH` gate, the route/method dispatch and
 * the bodies' strings are `gateway/wasm/src/route_do.rs`, reaching this file as `route-do-logic.ts` (GENERATED, and
 * its freshness test is a byte comparison). What is left here is the HOST: the Durable Object classes, `state.storage`,
 * and the `Response` construction. **A host is kept and its decisions move** — the shape the Electron shell has, and
 * the only shape a Durable Object can have, because `state.storage` and `fetch` are the platform's.
 *
 * WHY AN EMISSION RATHER THAN A ROUTE OR A DELETION: a DO's external address is its own (`workers_dev:false` and no
 * routes do not remove it), so this gate is the last line rather than a helper the router could answer — an `async`
 * route cannot serve a predicate `authorized()` calls synchronously — and `npm test` reaches `authorizeDoRequest`
 * directly, which a deleted twin would take with it.
 */

import {
  DO_AUTH_HEADER,
  INTERNAL_ERROR_PREFIX,
  MISSING_UID_BODY,
  NOT_FOUND_BODY,
  UNAUTHORIZED_BODY,
  doAuthorized,
  routeAction,
} from "./route-do-logic.ts";

// The emitted decisions, re-exported so `from "./route-do.ts"` keeps working for callers and tests.
export * from "./route-do-logic.ts";

/**
 * DO external-address defense-in-depth, shared by every DO class (BreakerDO and RouteDO used to each carry a
 * byte-identical copy). **THE DECISION IS THE EMISSION'S; READING THE HEADER IS THIS FILE'S** — a platform call on a
 * `Request`, which is the whole reason the emitted gate takes two strings instead of a request.
 */
export function authorizeDoRequest(request: Request, expectedSecret: string): boolean {
  return doAuthorized(request.headers.get(DO_AUTH_HEADER) || "", expectedSecret || "");
}

/**
 * Shared DO skeleton: state/env plumbing + the authorized() gate. RouteDO
 * and BreakerDO used to each carry byte-identical copies of this head —
 * only their fetch bodies differ.
 */
export class DoAuthBase {
  state: any;
  env: any;
  constructor(state: any, env: any) {
    this.state = state;
    this.env = env;
  }

  // Defense-in-depth: a Durable Object has its own external address even
  // with workers_dev:false + no routes, so the main router's auth is NOT the
  // last line. FAIL CLOSED — when DO_AUTH is unset, authorized() denies
  // every caller (constant-time compare).
  authorized(request: Request): boolean {
    return authorizeDoRequest(request, this.env?.DO_AUTH || "");
  }
}

export class RouteDO extends DoAuthBase {
  async fetch(request: Request): Promise<Response> {
    if (!this.authorized(request)) return new Response(UNAUTHORIZED_BODY, { status: 401 });
    const url = new URL(request.url);
    try {
      // **THE BODY IS READ ONLY FOR `PUT`, AND ONLY THERE**, because the source parses it inside that branch: a body
      // that is not JSON must reach the 500 arm rather than be swallowed into a 400.
      const parsed: any = request.method === "PUT" ? await request.json() : null;
      const uid = request.method === "PUT" ? (parsed?.uid ?? null) : url.searchParams.get("uid");
      const action = routeAction(request.method, url.pathname, uid, parsed?.model);
      switch (action.kind) {
        case "get": {
          const model = (await this.state.storage.get(`route:${action.uid}`)) || null;
          return Response.json({ model });
        }
        case "put": {
          if (action.model != null) {
            await this.state.storage.put(`route:${action.uid}`, action.model);
          } else {
            await this.state.storage.delete(`route:${action.uid}`);
          }
          return Response.json({ ok: true });
        }
        case "delete": {
          await this.state.storage.delete(`route:${action.uid}`);
          return Response.json({ ok: true });
        }
        case "missing-uid":
          return jsonErr(400, MISSING_UID_BODY);
        default:
          return new Response(NOT_FOUND_BODY, { status: 404 });
      }
    } catch (e: any) {
      return jsonErr(500, `${INTERNAL_ERROR_PREFIX}${e.message}`);
    }
  }
}

function jsonErr(status: number, message: string) {
  return Response.json({ error: message }, { status });
}
