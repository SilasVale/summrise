/**
 * tool-policy — WHICH DOORS MAY REACH A DEVICE TOOL THE CONSOLE DELIBERATELY WITHHOLDS.
 *
 * WHY THIS FILE EXISTS. Two doors reach device tools, and only one of them knew the catalogue:
 * `/mcp` offers a name only if it is registered and not withheld, while the device proxy
 * (`plugins/device-proxy.ts`) forwarded ANY `/api/tools/<name>` verbatim. So `terminal_sftp` — kept
 * off the MCP surface because it "takes arbitrary SSH credentials an MCP client should not be
 * offered" — was one proxied POST away for anyone holding an admin cookie or a 30-day plugin cookie.
 * A curated catalogue that one of two doors has never heard of is not a policy; it is a habit.
 *
 * THE AUDIENCE IS THE MISSING FACT, so it is recorded here beside the reason. The proxy exists to
 * serve the PANEL, and the panel's whole tool surface is callTool() with literal names —
 * terminal_read/select/list/open/close/write/resize, memory_list/search/save/update/delete/export,
 * agent_update and terminal_saved_connections (direct callApi sites in UpdateCard and ConnModal).
 * Everything else in this table is reachable by no UI at all, which is what makes refusing it safe.
 *
 * KEEP THIS IN STEP WITH THE MCP SURFACE: `gateway/test/mcp-handler.test.mjs` asserts that every name
 * here is in its NOT_EXPOSED catalogue, so a tool cannot be withheld from one door and unknown to
 * the other. That assertion is the point of the file; without it this is a third list.
 */
export interface WithheldTool {
  /** Why the MCP surface does not offer it. Lifted from the catalogue's own words. */
  reason: string;
  /** May the device PROXY reach it? True only where a panel component calls it today. */
  panel: boolean;
}

export const WITHHELD_TOOLS: Record<string, WithheldTool> = {
  // ── the OS surface beyond the sanctioned transfer pair: a PTY is strictly more capable ──
  system_file_list: { reason: "OS browsing — terminal_* covers it", panel: false },
  system_file_stat: { reason: "OS metadata — terminal_* covers it", panel: false },
  system_file_read: {
    reason: "inline ≤1 MiB read; the relay pair is the transfer path",
    panel: false,
  },
  system_file_write: {
    reason: "inline ≤4 MiB write; the relay pair is the transfer path",
    panel: false,
  },
  system_process_list: { reason: "tasklist is a PTY away", panel: false },
  system_process_kill: { reason: "taskkill is a PTY away", panel: false },
  system_net_test: { reason: "reachability probe — a PTY away", panel: false },

  // ── the device's own knowledge base: the panel's Memory surface calls these ──
  memory_save: { reason: "device KB — panel surface", panel: true },
  memory_search: { reason: "device KB — panel surface", panel: true },
  memory_list: { reason: "device KB — panel surface", panel: true },
  memory_update: { reason: "device KB — panel surface", panel: false },
  memory_delete: { reason: "device KB — panel surface", panel: true },
  memory_export: { reason: "device KB — panel surface", panel: true },

  // ── the playwright-mcp bridge plumbing: exposing it lets a client route around browser_* ──
  mcp_client_connect: {
    reason: "internal bridge plumbing (mcp-browser.ts calls it)",
    panel: false,
  },
  mcp_client_list: { reason: "session introspection is device-local", panel: false },
  mcp_client_call: { reason: "internal bridge plumbing", panel: false },
  mcp_client_disconnect: { reason: "teardown is device-local", panel: false },

  // ── swaps the device binary and restarts the agent (drops every session) — the panel's
  //    UpdateCard calls it through this very proxy, which is why panel is true ──
  agent_update: {
    reason: "self-modifying — a CLI action (`summrise update`), not an MCP call",
    panel: true,
  },

  page_view: { reason: "legacy remote-page helper (design plugin)", panel: false },

  // ── terminal_sftp was the pre-relay transfer path; the relay pair is now the ONE method ──
  terminal_sftp: {
    reason: "superseded by the relay pair; takes arbitrary SSH credentials",
    panel: false,
  },
  sftp: { reason: "legacy alias of terminal_sftp", panel: false },

  terminal_secret_set: { reason: "alias of secret_set (registered)", panel: false },
  terminal_secret_get: { reason: "alias of secret_get (registered)", panel: false },
};

/**
 * May a caller arriving through the DEVICE PROXY reach <name>?
 *
 * A name not in the table is not withheld, so the proxy forwards it — the panel needs the rest of
 * the surface (terminal_*, the registered memory_* tools, everything the MCP registry offers).
 */
export function proxyMayReachTool(name: string): { ok: true } | { ok: false; reason: string } {
  const w = WITHHELD_TOOLS[name];
  if (!w || w.panel) return { ok: true };
  return { ok: false, reason: w.reason };
}
