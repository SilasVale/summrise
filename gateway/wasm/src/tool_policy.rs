//! `gateway/src/tool-policy.ts` — WHICH DOORS MAY REACH A DEVICE TOOL THE CONSOLE DELIBERATELY WITHHOLDS.
//!
//! WHY THIS MODULE EXISTS, in the source's own words: two doors reach device tools and only one of them knew
//! the catalogue. `/mcp` offers a name only if it is registered and not withheld, while the device proxy
//! forwarded ANY `/api/tools/<name>` verbatim — so `terminal_sftp`, kept off the MCP surface because it "takes
//! arbitrary SSH credentials an MCP client should not be offered", was one proxied POST away for anyone holding
//! an admin cookie or a 30-day plugin cookie. A curated catalogue one of two doors has never heard of is not a
//! policy; it is a habit.
//!
//! THE AUDIENCE IS PART OF THE POLICY, not a comment beside it: `panel` is true only where a panel component
//! calls the tool today, which is what makes refusing the rest safe. The source records the same list of direct
//! `callApi` sites next to the table (UpdateCard and ConnModal are the two that matter).
//!
//! KEEP THIS IN STEP WITH THE MCP SURFACE: `gateway/test/mcp-handler.test.mjs` asserts that every name here is
//! in the MCP handler's own NOT_EXPOSED catalogue, so a tool cannot be withheld from one door and unknown to the
//! other. That assertion is the point of the file; without it this is a third list.

/// `WithheldTool` — the reason the MCP surface does not offer the tool, and whether the PROXY may still reach it.
pub struct WithheldTool {
    /// Why the MCP surface does not offer it. Lifted from the catalogue's own words.
    pub reason: &'static str,
    /// May the device PROXY reach it? True only where a panel component calls it today.
    pub panel: bool,
}

/// `WITHHELD_TOOLS` — the table, in the source's order.
///
/// **THE ORDER IS NOT LOAD-BEARING HERE** (the lookup is by name), but the entries are kept in the source's
/// order and grouping so that a reader diffing the two files sees a diff rather than a reshuffle.
pub const WITHHELD_TOOLS: [(&str, WithheldTool); 23] = [
    // ── the OS surface beyond the sanctioned transfer pair: a PTY is strictly more capable ──
    (
        "system_file_list",
        WithheldTool {
            reason: "OS browsing — terminal_* covers it",
            panel: false,
        },
    ),
    (
        "system_file_stat",
        WithheldTool {
            reason: "OS metadata — terminal_* covers it",
            panel: false,
        },
    ),
    (
        "system_file_read",
        WithheldTool {
            reason: "inline ≤1 MiB read; the relay pair is the transfer path",
            panel: false,
        },
    ),
    (
        "system_file_write",
        WithheldTool {
            reason: "inline ≤4 MiB write; the relay pair is the transfer path",
            panel: false,
        },
    ),
    (
        "system_process_list",
        WithheldTool {
            reason: "tasklist is a PTY away",
            panel: false,
        },
    ),
    (
        "system_process_kill",
        WithheldTool {
            reason: "taskkill is a PTY away",
            panel: false,
        },
    ),
    (
        "system_net_test",
        WithheldTool {
            reason: "reachability probe — a PTY away",
            panel: false,
        },
    ),
    // ── the device's own knowledge base: the panel's Memory surface calls these ──
    (
        "memory_save",
        WithheldTool {
            reason: "device KB — panel surface",
            panel: true,
        },
    ),
    (
        "memory_search",
        WithheldTool {
            reason: "device KB — panel surface",
            panel: true,
        },
    ),
    (
        "memory_list",
        WithheldTool {
            reason: "device KB — panel surface",
            panel: true,
        },
    ),
    (
        "memory_update",
        WithheldTool {
            reason: "device KB — panel surface",
            panel: false,
        },
    ),
    (
        "memory_delete",
        WithheldTool {
            reason: "device KB — panel surface",
            panel: true,
        },
    ),
    (
        "memory_export",
        WithheldTool {
            reason: "device KB — panel surface",
            panel: true,
        },
    ),
    // ── the playwright-mcp bridge plumbing: exposing it lets a client route around browser_* ──
    (
        "mcp_client_connect",
        WithheldTool {
            reason: "internal bridge plumbing (mcp-browser.ts calls it)",
            panel: false,
        },
    ),
    (
        "mcp_client_list",
        WithheldTool {
            reason: "session introspection is device-local",
            panel: false,
        },
    ),
    (
        "mcp_client_call",
        WithheldTool {
            reason: "internal bridge plumbing",
            panel: false,
        },
    ),
    (
        "mcp_client_disconnect",
        WithheldTool {
            reason: "teardown is device-local",
            panel: false,
        },
    ),
    // ── swaps the device binary and restarts the agent (drops every session) — the panel's
    //    UpdateCard calls it through this very proxy, which is why panel is true ──
    (
        "agent_update",
        WithheldTool {
            reason: "self-modifying — a CLI action (`summrise update`), not an MCP call",
            panel: true,
        },
    ),
    (
        "page_view",
        WithheldTool {
            reason: "legacy remote-page helper (design plugin)",
            panel: false,
        },
    ),
    // ── terminal_sftp was the pre-relay transfer path; the relay pair is now the ONE method ──
    (
        "terminal_sftp",
        WithheldTool {
            reason: "superseded by the relay pair; takes arbitrary SSH credentials",
            panel: false,
        },
    ),
    (
        "sftp",
        WithheldTool {
            reason: "legacy alias of terminal_sftp",
            panel: false,
        },
    ),
    (
        "terminal_secret_set",
        WithheldTool {
            reason: "alias of secret_set (registered)",
            panel: false,
        },
    ),
    (
        "terminal_secret_get",
        WithheldTool {
            reason: "alias of secret_get (registered)",
            panel: false,
        },
    ),
];

/// **`WITHHELD_TOOLS[name]` IS AN OWN-PROPERTY LOOKUP IN THE SOURCE, AND THIS IS THE ARM THAT IS NOT.**
///
/// The TypeScript indexes a plain object literal, so a name that is NOT in the table but IS on
/// `Object.prototype` — `toString`, `constructor`, `valueOf`, `hasOwnProperty`, `__proto__`, and the eight
/// others below — finds an INHERITED value. That value is truthy, so the `!w` guard passes, and its `.reason`
/// is `undefined`, so the refusal message ends `…: undefined` instead of the name being forwarded.
///
/// **IT IS A DEFECT AND IT IS REPRODUCED RATHER THAN FIXED, which is this port's rule for the whole surface**:
/// the criterion is the same request producing the same bytes from the shipping implementation and this one, and
/// a "fixed" arm here would be an unmeasured divergence on a route an attacker can reach. A device tool named
/// `toString` is refused rather than forwarded — which is the conservative direction, so nothing is exposed by
/// keeping it; what would be wrong is refusing it with a DIFFERENT sentence than the console's.
///
/// The Rust table is a slice and cannot inherit anything, so the inherited arm has to be written down. The list
/// is exactly `Object.getOwnPropertyNames(Object.prototype)`, and the reason is the JavaScript `undefined`
/// rendered into a template literal.
const INHERITED_PROPERTY_NAMES: [&str; 12] = [
    "constructor",
    "hasOwnProperty",
    "isPrototypeOf",
    "propertyIsEnumerable",
    "toLocaleString",
    "toString",
    "valueOf",
    "__defineGetter__",
    "__defineSetter__",
    "__lookupGetter__",
    "__lookupSetter__",
    "__proto__",
];

/// `proxyMayReachTool(name)` — may a caller arriving through the DEVICE PROXY reach `<name>`?
///
/// A name not in the table is not withheld, so the proxy forwards it: the panel needs the rest of the surface
/// (`terminal_*`, the registered `memory_*` tools, everything the MCP registry offers).
pub fn proxy_may_reach_tool(name: &str) -> Result<(), String> {
    if let Some((_, tool)) = WITHHELD_TOOLS.iter().find(|(key, _)| *key == name) {
        if tool.panel {
            return Ok(());
        }
        return Err(tool.reason.to_string());
    }
    // `WITHHELD_TOOLS["toString"]` — see the const's comment. `w.reason` is `undefined`, and the caller's
    // template literal renders that as the word.
    if INHERITED_PROPERTY_NAMES.contains(&name) {
        return Err("undefined".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_the_source_table() {
        assert_eq!(WITHHELD_TOOLS.len(), 23);
        // The five the panel actually calls through this proxy.
        for name in [
            "memory_save",
            "memory_search",
            "memory_list",
            "memory_delete",
            "memory_export",
            "agent_update",
        ] {
            assert!(proxy_may_reach_tool(name).is_ok(), "{name} is the panel's");
        }
        // A name the table does not know is forwarded, which is what keeps the panel's whole surface working.
        assert!(proxy_may_reach_tool("terminal_execute").is_ok());
        // …and the one the source's own comment names as the reason the file exists.
        assert_eq!(
            proxy_may_reach_tool("terminal_sftp"),
            Err("superseded by the relay pair; takes arbitrary SSH credentials".to_string())
        );
        assert_eq!(
            proxy_may_reach_tool("toString"),
            Err("undefined".to_string()),
            "the inherited-property arm of an object-literal lookup"
        );
    }
}
