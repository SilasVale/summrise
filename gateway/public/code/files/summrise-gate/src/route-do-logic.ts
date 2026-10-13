/**
 * route-do-logic.ts — GENERATED. DO NOT EDIT BY HAND.
 *
 * Emitted by `gateway/wasm/src/route_do.rs` (`render_typescript`), which is now the single source. The freshness test
 * in that module is a byte comparison against this file, so a hand edit fails there with the text and one command to
 * fix it:
 *
 *     SUMMRISE_REFRESH_ROUTE_DO=1 cargo test --manifest-path gateway/wasm/Cargo.toml route_do::emission
 *
 * `gateway/src/route-do.ts` re-exports this module and keeps the HOST: the Durable Object classes, their storage
 * calls, and the `Response` construction. **A host is kept and its decisions move** — the shape the Electron shell
 * has — and the header name below is one name on both sides, because `lib.rs`'s `breaker_call` sends it.
 *
 * The comments below are the source file's own, carried verbatim.
 */

/**
 * DO external-address defense-in-depth, shared by every DO class (BreakerDO
 * and RouteDO used to each carry a byte-identical copy). A Durable Object
 * has its own external address even with workers_dev:false + no routes, so
 * the main router's auth is not the last line. FAIL CLOSED (Auth-core audit
 * MED-2): an unconfigured DO_AUTH must DENY every caller — never wave the
 * gate open. Constant-time compare on the x-do-auth header.
 *
 * Truth table (pinned in route-do.test.mjs):
 *   expected empty → false ALWAYS (even when the header is also absent —
 *     this is what makes it fail-closed rather than a plain equality);
 *   header missing / wrong length / wrong value → false;
 *   header equals expected → true.
 *
 * SOLID Round-8 (DRY/DIP): the comparison itself is auth.ts's `safeEq`
 * primitive — that module's header mandates reaching for its primitives
 * instead of re-implementing them, and this gate previously hand-rolled a
 * second copy of the same loop. The fail-closed empty-secret guard stays
 * HERE (it is gate policy, not comparison semantics — `safeEq("","")`
 * is true, which would wave an unconfigured gate open).
 */
import { safeEq } from "./auth.ts";

export const DO_AUTH_HEADER = "x-do-auth";

export function doAuthorized(got: string, expected: string): boolean {
  if (!expected) return false;
  return safeEq(got, expected);
}

/** What a DO request asks for, decided before any storage is touched. */
export type RouteAction =
  | { kind: "get"; uid: string }
  | { kind: "put"; uid: string; model: string | null }
  | { kind: "delete"; uid: string }
  | { kind: "missing-uid" }
  | { kind: "not-found" };

/** `RouteDO.fetch`'s dispatch, with the request already reduced to its parts. **`model` IS THE PARSED VALUE, not a
 *  string**: the source's `model != null` stores `0` and `false` (`String(model)`) and deletes on `null`, and a port
 *  that sniffed the string first would lose that. An EMPTY `uid` is missing — the source tests truthiness. */
export function routeAction(
  method: string,
  path: string,
  uid: string | null,
  model: unknown,
): RouteAction {
  if (path !== "/route") return { kind: "not-found" };
  if (!uid)
    return method === "GET" || method === "PUT" || method === "DELETE"
      ? { kind: "missing-uid" }
      : { kind: "not-found" };
  if (method === "GET") return { kind: "get", uid };
  if (method === "DELETE") return { kind: "delete", uid };
  if (method === "PUT") return { kind: "put", uid, model: model != null ? String(model) : null };
  return { kind: "not-found" };
}

/** The refusal an unauthorized DO request gets — PLAIN TEXT, not JSON. */
export const UNAUTHORIZED_BODY = "unauthorized";
/** The 404's body — plain text too. */
export const NOT_FOUND_BODY = "not found";
/** The 400's body (`{error: "missing uid"}`). */
export const MISSING_UID_BODY = "missing uid";
/** The prefix on a storage failure's message (`route-do: kv down`). */
export const INTERNAL_ERROR_PREFIX = "route-do: ";
