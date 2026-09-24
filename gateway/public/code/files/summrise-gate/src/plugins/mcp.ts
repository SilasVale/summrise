/**
 * mcp plugin (round-73 extraction) — MCP endpoint + plugin-link status.
 *
 * Routes:
 *   /mcp                      (any method, page host) → handleMcp(request, env)
 *                             GET = SSE stream, POST = JSON-RPC (Claude Code)
 *   GET /api/plugins/status   → agent/tunnel health per device (probe)
 *
 * NOTE: the original index.js guarded /mcp with isPageHost and
 * /api/plugins/status with the admin session (inside handleConsole). Those
 * are fetch/dispatch-layer guards — this plugin registers method+path only;
 * the wiring phase decides where the guards live.
 *
 * Logic copied VERBATIM from index.js (mcp route handler + /api/plugins/status
 * handler + cachedDeviceProbe helper); handleMcp is imported from mcp.ts.
 */

import { handleMcp } from "../mcp.ts";
import { listDevices, touchDeviceSeen, type Device } from "../store.ts";
import { deviceFetch } from "../device-fetch.ts";
import { jsonOk, jsonError, withCors } from "../http.ts";
import { requireSession } from "../session.ts";
import { routeStats, type Plugin, type PluginContext } from "./registry.ts";

const PLUGIN_BASE = "/api/plugins";

/* ---------------- Device module helpers (copied verbatim from index.js) ---------------- */

interface DeviceProbeState {
  tunnel: boolean;
  agent: boolean;
  ts: number;
  /// Agent version from the device's /api/status — the npm RELEASE
  /// (round-304 added it; version is the frozen Cargo protocol anchor
  /// 1.0.145 and useless for update checks). Falls back to version for
  /// pre-1.2.276 agents that lack release.
  version?: string;
  /// THE DEVICE'S OWN UPDATE VERDICT, forwarded instead of re-derived (round 29 of the standing goal). The console
  /// computed `outdated = lastVersion !== install.version` from the KV copy it keeps — which ignores a rollback pin
  /// and can be an hour stale (SEEN_WRITE_INTERVAL_MS) — while the device answers the same question at /api/update
  /// with `newer()` plus the pin. Two computations of one fact, free to disagree; this carries the device's own.
  update?: {
    current?: string;
    latest?: string;
    update_available: boolean;
    pinned_to: string | null;
  };
  /// How the device's PREVIOUS run ended, straight from its /api/status
  /// (round 256). Two fields for one fact, and the split is deliberate: the
  /// fleet card has to DECIDE something (mark the row, or leave it alone), and
  /// a decision made by matching English in `lastBoot` breaks the next time the
  /// device rephrases its sentence. `lastBoot` is that sentence, for the hover.
  ///
  /// ONLY THE FAULT IS CARRIED. "replaced" (the normal consequence of an
  /// update) and "clean-exit" are dropped here on purpose: a fleet view is for
  /// EXCEPTIONS, and forwarding every verdict is how the console grows rows
  /// that all shout and therefore none is read. Absent on agents older than the
  /// field, which reads as "no verdict on record" — never as a crash.
  lastBootKind?: string;
  lastBoot?: string;
  checkedAt: number;
}

// Device /api/status probe with a 30s in-isolate cache — the console polls
// /api/plugins/status every 30s, so a live probe per call would hammer the
// tunnel. The cache bounds it to one tunnel round-trip per 30s per device.
// `fresh` (the console's "check now" button) bypasses the cache read but
// still rewrites it, so a burst of fresh clicks costs at most one probe per
// click and every other poller keeps seeing the refreshed state.
const DEVICE_PROBE_CACHE = new Map<string, any>(); // name -> { at, ok }
const DEVICE_PROBE_TTL_MS = 30000;

// Exported for direct pins (SOLID Round-31; additive — call sites untouched).
/**
 * WHICH VERSION FIELD WINS, and this is a RULE rather than two lines because it has been broken twice.
 *
 * `release` is the npm release (1.2.x, changes every release); `version` is the Cargo protocol version
 * (1.0.x, FROZEN). Round-304: this side read `version`, so the console showed v1.0.145 forever and the
 * outdated badge never cleared.
 *
 * THE SAME RULE AS THE PANEL'S `releaseVersion`
 * (`agent/resources/panel-react/src/lib/agentVersion.ts`), which met it from the other direction: in the
 * desktop shell the status strip read v1.2.354 while Settings reported v1.0.145 for the SAME device. Two
 * copies of a rule is what let them disagree — the panel's own comment — and neither side could see the
 * other until each named it. Change this and change that, or the two answers diverge again.
 *
 * `device-version-rule-check.mjs` holds both to one table.
 */
export function wireVersion(j: unknown): string | undefined {
  const o = (j ?? {}) as { release?: unknown; version?: unknown };
  if (typeof o.release === "string" && o.release) return o.release;
  if (typeof o.version === "string" && o.version) return o.version;
  // ABSENT, NOT FABRICATED — and this is where the two halves DELIBERATELY differ. The panel returns "" here
  // and renders it as "v?" through `releaseVersionLabel`; this side leaves the field UNSET, because a state
  // object that says `version: ""` claims an answer it does not have. `device-probe.test.mjs` pins it:
  // "no version fields → absent, not fabricated". Extracting this function nearly changed that silently.
  return undefined;
}
export async function cachedDeviceProbe(
  env: any,
  device: Device,
  fresh = false,
): Promise<DeviceProbeState> {
  const hit = DEVICE_PROBE_CACHE.get(device.name);
  // round-98: the TTL check read hit.at but the cache stores ts — the 30s
  // cache NEVER hit, so every /api/plugins/status poll live-probed every
  // device through the tunnel (the hammering the cache exists to prevent).
  if (!fresh && hit && Date.now() - hit.ts < DEVICE_PROBE_TTL_MS) return hit;
  // Probe through the tunnel; classify the failure: a tunnel-level error
  // (1033/530 — origin unreachable) vs an agent-level error (HTTP response).
  // round-101: deviceFetch NEVER throws — it returns {status:502,
  // ok:false, error:'Device unreachable: …'} on any network failure, so the
  // old try/catch left tunnel=true always (the catch was unreachable) and a
  // down device showed tunnel_up:true. Classify from the returned shape:
  // the 502-with-unreachable error string is the tunnel-level case.
  const state: DeviceProbeState = {
    tunnel: false,
    agent: false,
    ts: Date.now(),
    checkedAt: Date.now(),
  };
  const res = await deviceFetch(env, device, "/api/status");
  if (res && res.status !== undefined) {
    const tunnelLevel =
      !res.ok && typeof res.error === "string" && res.error.includes("Device unreachable");
    state.tunnel = !tunnelLevel;
    state.agent = res.ok;
    if (res.ok && res.resp) {
      const j: any = await res.resp.json().catch(() => null);
      // Prefer the npm release over the frozen Cargo version (round-304:
      // version never changes, so the console showed v1.0.145 forever and
      // the outdated badge never cleared). Pre-release agents fall back.
      state.version = wireVersion(j);
      // THE ONE VERDICT THE FLEET SHOWS (round 256): a device whose last run
      // crashed is a device somebody should look at. The device sends both
      // halves; only the fault and its sentence are forwarded — see the field
      // comments on DeviceProbeState for why "replaced" is dropped here.
      if (j && j.last_boot_kind === "crashed" && typeof j.last_boot === "string" && j.last_boot) {
        state.lastBootKind = "crashed";
        state.lastBoot = j.last_boot;
      }
    }
  }
  // A SECOND CALL, AND ONLY WHEN THE FIRST ANSWERED: a device that is down, or older than /api/update, costs one
  // request rather than two. A device that answers but has no verdict (an agent before this route existed) leaves
  // `update` absent, and the console falls back to its own comparison — degraded, not wrong.
  if (res && res.ok) {
    const up = await deviceFetch(env, device, "/api/update");
    if (up && up.ok && up.resp) {
      const u: any = await up.resp.json().catch(() => null);
      if (
        u &&
        typeof u === "object" &&
        (typeof u.current === "string" || typeof u.latest === "string")
      ) {
        state.update = {
          ...(typeof u.current === "string" ? { current: u.current } : {}),
          ...(typeof u.latest === "string" ? { latest: u.latest } : {}),
          update_available: u.update_available === true,
          pinned_to: typeof u.pinned_to === "string" ? u.pinned_to : null,
        };
      }
    }
  }
  if (DEVICE_PROBE_CACHE.size >= 64) DEVICE_PROBE_CACHE.clear();
  DEVICE_PROBE_CACHE.set(device.name, state);
  return state;
}

/* ---------------- Routes ---------------- */

// GET /api/plugins/status — agent/tunnel health per device (probe, 30s cache)
// (handler body copied verbatim from index.js handleConsole). round-159:
// adds the agent version + probe timestamp from the /api/status probe and a
// `?fresh=1` cache bypass for the console's "check now" button; successful
// probes feed touchDeviceSeen (write-bounded lastSeen/lastVersion refresh).
/** `ctx` is OPTIONAL (round-182): it exists only to add the route statistics, so a
 *  caller without one gets the SAME response it always got — no field, no behaviour
 *  change. That is what keeps this an additive change instead of a signature break. */
async function pluginStatus(request: Request, env: any, ctx?: PluginContext): Promise<Response> {
  const fresh = new URL(request.url).searchParams.get("fresh") === "1";
  const devices = await listDevices(env);
  const out: Record<
    string,
    {
      agent_up: boolean;
      tunnel_up: boolean;
      version?: string;
      last_boot_kind?: string;
      last_boot?: string;
      update?: {
        current?: string;
        latest?: string;
        update_available: boolean;
        pinned_to: string | null;
      };
      checked_at: number;
    }
  > = {};
  for (const d of devices) {
    // Agent + tunnel health: probe the device's own /api/status through its
    // tunnel (cached 30s — the console polls every 60s, `CONSOLE_POLL_MS` in
    // gateway/ui/src/lib/deviceState.ts, so a request the console makes is NEVER
    // served by this cache). THAT margin, not the console's interval, is the whole
    // justification for the number: the 30s window exists for a second view and for
    // a manual refresh, and any value comfortably under the poll would do.
    // It said "the console polls every 30s already" until round 156 — a false
    // reason, which is worse than none, because it is what a reader would trust
    // when deciding whether this cache can grow.
    const probe = await cachedDeviceProbe(env, d, fresh);
    if (probe.agent) {
      // Bounded write: only touches KV when the version changed or the last
      // write is >1h old — see touchDeviceSeen.
      await touchDeviceSeen(env, d.name, probe.version).catch(() => {});
    }
    out[d.name] = {
      agent_up: probe.agent,
      tunnel_up: probe.tunnel,
      ...(probe.version ? { version: probe.version } : {}),
      // Both halves or neither: the console shows a mark keyed on the KIND and
      // the sentence as its hover, and a kind with no sentence would be a mark
      // an operator cannot interrogate. `cachedDeviceProbe` only ever sets them
      // together, so this guard is the assertion, not the rule.
      ...(probe.lastBootKind && probe.lastBoot
        ? { last_boot_kind: probe.lastBootKind, last_boot: probe.lastBoot }
        : {}),
      // The device's own answer to "am I behind?", when it gave one. The console prefers it and only re-derives
      // when this is absent.
      ...(probe.update ? { update: probe.update } : {}),
      checked_at: probe.checkedAt,
    };
  }
  // The dead-route report (round 179's instrument, wired here in round 182). ADDITIVE:
  // `routes` appears only when a ctx was passed, so this response keeps its old shape for
  // any caller that has none.
  return jsonOk({ devices: out, ...(ctx ? { routes: routeStats(ctx) } : {}) });
}

export default {
  name: "mcp",
  deps: [],
  setup(ctx: PluginContext) {
    // ---- MCP endpoint (Claude Code) — admin token, page host only ----
    // index.js had no method filter here (GET = SSE stream, POST = JSON-RPC).
    ctx.routes.push({
      match: (_m, p) => p === "/mcp",
      // withCors at the plugin exit: handleMcp's own 401/405/parse-error
      // responses are built bare (no CORS stamp); the front-door dispatch
      // re-stamps idempotently, but direct plugin consumers get CORS here.
      handler: async (request: Request, env: any) =>
        withCors(request, await handleMcp(request, env), env),
    });
    // ---- GET /api/plugins/status (was inside handleConsole, admin-gated) ----
    // round-83: the migration dropped the admin gate — the plugin route runs
    // BEFORE the legacy session checks, so it returned the device inventory
    // to unauthenticated callers (verified live). Restore the guard through
    // the shared session module (round-88's full contract: sess-revoked
    // blacklist + enabled check — the hand-rolled copy had drifted).
    ctx.routes.push({
      match: (m, p) => m === "GET" && p === `${PLUGIN_BASE}/status`,
      handler: async (request: Request, env: any) => {
        const user = await requireSession(request, env);
        // TWO CASES, TWO STATUSES — they were collapsed into one 401, and that
        // logged people OUT. The console's client treats ANY 401 as a dead session
        // (`api/client.ts` -> notifyUnauthorized -> setUser(null) -> the login page),
        // so a logged-in NON-ADMIN who merely opened the Overview was ejected: the
        // landing page probes this endpoint for every role, measured live as
        // `/api/me -> 200` followed by `/api/plugins/status -> 401` and the auth card
        // rendering. The session cookie was valid the whole time.
        //
        // 401 means "you are not authenticated"; this user IS. `auth.ts` already
        // returns 403 "Admin only" for the same situation; this is that.
        if (!user) return jsonError(401, "Not logged in", "authentication_error");
        if (user.role !== "admin") return jsonError(403, "Admin only", "forbidden");
        return pluginStatus(request, env, ctx);
      },
    });
  },
} satisfies Plugin;
