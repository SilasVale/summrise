//! The route vocabulary both hosts need: **which prefixes the BUILT-IN router owns.**
//!
//! MOVED HERE FROM `gateway/wasm/src/routing.rs`'s `is_built_in_prefix` AND `gateway/src/channels.ts`'s
//! `RESERVED_PREFIXES` (2026-10-13), which were the same rule written twice — once as a `matches!` list in the
//! worker's routing and once as a derived array in the console — with a test on ONE side asserting the two agree.
//!
//! **WHY THE ORDER OF [`RESERVED_PREFIXES`] IS PART OF THE RULE AND NOT A DETAIL**: the console's 409 refusal
//! PRINTS the list (`prefix "og/" is reserved by a built-in channel (reserved: og, or, …)`), so the order is
//! bytes a client receives. It is the console's own display order — `ROUTE_INFO`'s — with the two card-less
//! channels and the no-prefix sentinel last, exactly as `channels.ts` spelled it.

/// `RESERVED_PREFIXES` — every prefix a CUSTOM provider may not claim, in the order the refusal prints them.
///
/// `"none"` IS IN THE LIST AND IS NOT A PREFIX: it is the no-prefix sentinel of the default channel, reserved
/// because a provider registered under it would be advertised and never serve (see [`is_built_in_prefix`]).
/// `"ds"` and `"amd"` are live ROUTE_TABLE channels with no console card — ds 402s on every request and amd has
/// no V4.1 to advertise — which is why they are not in `ROUTE_INFO` and must be named here.
pub const RESERVED_PREFIXES: [&str; 10] = [
    "og", "or", "nv", "gmi", "qw", "cm", "r4", "none", "ds", "amd",
];

/// `ROUTE_TABLE`'s keys — **"does the BUILT-IN route table own this prefix?"** (`isBuiltInPrefix`).
///
/// It is a LIST rather than a lookup over `pick_route`'s arms because the ORDER is the contract: a built-in
/// prefix is decided by `pick_route` even when a custom provider claims the same prefix, and only a prefix the
/// table does not own may reach `provider_for_prefix`.
///
/// **`"none"` IS NOT BUILT-IN**, and that is the whole difference from [`RESERVED_PREFIXES`]: it is the sentinel
/// for "no prefix at all", so it is reserved (a provider must not claim it) and it is not a route (no built-in
/// channel answers to it).
pub fn is_built_in_prefix(prefix: &str) -> bool {
    RESERVED_PREFIXES.contains(&prefix) && prefix != "none"
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **THE TWO LISTS DIFFER BY EXACTLY ONE ENTRY, AND THE ORDER IS THE WIRE'S.**
    ///
    /// MUTATION: drop `"amd"` from `RESERVED_PREFIXES`.
    /// RESULT:   this test fails on the count AND `is_built_in_prefix("amd")` turns false, which would let a
    ///           provider claim a prefix the built-in table still routes — the shadowed-provider lie the
    ///           reservation exists to prevent.
    #[test]
    fn the_reserved_list_is_the_route_table_plus_the_sentinel() {
        assert_eq!(RESERVED_PREFIXES.len(), 10);
        // The order is the console's 409 message, so it is asserted rather than described.
        assert_eq!(
            RESERVED_PREFIXES.join(", "),
            "og, or, nv, gmi, qw, cm, r4, none, ds, amd"
        );
        // Every entry but the sentinel is a built-in route.
        let built_in: Vec<&str> = RESERVED_PREFIXES
            .iter()
            .copied()
            .filter(|p| is_built_in_prefix(p))
            .collect();
        assert_eq!(
            built_in,
            vec!["og", "or", "nv", "gmi", "qw", "cm", "r4", "ds", "amd"]
        );
        assert!(!is_built_in_prefix("none"), "the sentinel is not a route");
        assert!(!is_built_in_prefix("acme"));
        assert!(
            RESERVED_PREFIXES.contains(&"none"),
            "a provider must not be able to register the no-prefix sentinel"
        );
    }
}
