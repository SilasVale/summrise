//! The host guards — **THE RULES THAT DECIDE WHICH HOSTNAMES MAY BE DIALED, AND WITH WHAT.**
//!
//! MOVED HERE FROM `gateway/wasm/src/device.rs` (2026-10-13), WHICH IS WHAT MAKES THEM SHARED RATHER THAN THE
//! WORKER'S. The worker already had them; the AGENT carries its own copies of the same reasoning (`src/web/mod.rs`
//! documents the identical suffix trap in its own words), and a guard rule written twice in two binaries is the one
//! duplication in this repository that is a HOLE rather than a bug — the sentence `device.rs`'s own header carries.
//!
//! **IT IS ALSO THE FIRST TENANT OF `summrise-core`, AND IT SETS THE PATTERN**: the rules are pure (no `Env`, no
//! request, no clock), so the worker calls them directly, the agent will, and the console receives the same rules as
//! EMITTED TYPESCRIPT from the worker's emitter.
//!
//! The worker re-exports every name below from `device.rs`, so its call sites and its own gates (`device-fetch.test.mjs`
//! through the emission, `production_host.rs` through the suffix) read exactly what they read before.

use crate::text::{js_is_integer, js_number_of};

/// The default device-host suffix. **IT IS A PRODUCTION HOSTNAME**, which is why this file is declared in
/// `agent/tests/production_host.rs` beside `device-fetch.ts` — the rule that decides what a device
/// hostname is has to name one.
pub const DEFAULT_DEVICE_HOST_SUFFIX: &str = ".agent.saisi.online";

/// `hostAllowError(hostname, env)`: the allowlist, overridable per deployment.
///
/// Two tests, and the second is the one that is easy to miss: the host must END WITH the suffix **and be
/// strictly longer than it**, so the bare suffix is not a device hostname. The suffix is lowercased and so
/// is the host, because a hostname comparison is case-insensitive; an empty override falls back to the
/// default through `||`, which is a truthiness test rather than a presence test.
pub fn host_allow_error(hostname: &str, device_host_suffix: Option<&str>) -> Option<String> {
    let suffix = match device_host_suffix {
        Some(s) if !s.is_empty() => s.to_lowercase(),
        _ => DEFAULT_DEVICE_HOST_SUFFIX.to_string(),
    };
    let h = hostname.to_lowercase();
    if !h.ends_with(&suffix) || h.len() <= suffix.len() {
        return Some(format!("hostname must be under {suffix}"));
    }
    None
}

/// `deviceHostError(hostname)`: `Some(reason)` when the hostname must NOT be dialled with device
/// credentials.
///
/// THE `172.` ARM IS SECOND-OCTET AND NUMERIC: `172.16/12` is only 16–31, and a blanket
/// `startsWith("172.")` would also refuse the public `172.15.x.x` and `172.32+.x.x`. `Number.isInteger` is
/// what makes `"172.abc.1.1"` not private — `Number("abc")` is NaN.
///
/// THE v6 PREFIXES REQUIRE A COLON, because a hostname STRING never contains one: without that test,
/// `fc.example.com` would false-positive as a unique-local address.
pub fn device_host_error(hostname: &str) -> Option<String> {
    let host = hostname.to_lowercase();
    let is_172_private = if let Some(rest) = host.strip_prefix("172.") {
        let second = rest.split('.').next().unwrap_or("");
        let n = js_number_of(second);
        js_is_integer(n) && (16.0..=31.0).contains(&n)
    } else {
        false
    };
    let is_v6_private = host.contains(':')
        && (host.starts_with("fc") || host.starts_with("fd") || host.starts_with("fe80"));
    let blocked = host == "localhost"
        || host.starts_with("127.")
        || host.starts_with("10.")
        || host.starts_with("192.168.")
        || is_172_private
        || host == "::1"
        || host == "[::1]"
        || is_v6_private
        || host == "0.0.0.0"
        || host == "[::]"
        || host == "169.254.169.254"
        || host.starts_with("::ffff:")
        || host.starts_with("[::ffff:");
    if blocked {
        Some("device hostname resolves to a private/internal address".to_string())
    } else {
        None
    }
}

/// `authorityPrefix`'s scheme test: `^[a-z][a-z0-9+.-]*:`. Moved here with the dial, because both the dial and
/// the device family's own path check read it.
pub fn starts_like_a_scheme(authority: &str) -> bool {
    let Some((head, _)) = authority.split_once(':') else {
        return false;
    };
    let mut chars = head.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'))
}

/// The refusal both authority-shaped bad paths answer with — a `restPath` carrying userinfo (`@`) or a scheme can
/// RE-ROOT the URL the device token travels to (round-120/121).
pub const PROXY_PATH_REFUSAL: &str = "invalid proxy path";

/// `deviceFetch`'s first guard: the AUTHORITY-RELEVANT prefix of `restPath` — up to the first `/`, `?` or `#` — may
/// carry neither userinfo nor a scheme.
///
/// **NARROWED ON PURPOSE** (round-121): an at-sign in a QUERY string is legitimate (`?user=a@b.com`), and the
/// hostname-equality gate below already blocks userinfo smuggling. A leading scheme in the authority is still
/// refused.
pub fn authority_prefix_ok(rest_path: &str) -> bool {
    let authority = rest_path.split(['/', '?', '#']).next().unwrap_or_default();
    !authority.contains('@') && !starts_like_a_scheme(authority)
}

/// The path half of the dial URL: the source prepends `/` unless it is already there.
pub fn dial_path(rest_path: &str) -> String {
    if rest_path.starts_with('/') {
        rest_path.to_string()
    } else {
        format!("/{rest_path}")
    }
}

/// **BELT-AND-SUSPENDERS**: whatever the parser produced, the host MUST be the device's own. Case-insensitive,
/// because WHATWG lowercases special-scheme hosts while registration stores hostnames verbatim.
pub fn hostname_mismatch(parsed_host: &str, hostname: &str) -> bool {
    parsed_host.to_lowercase() != hostname.to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **THE SSRF LIST, PINNED WHERE IT NOW LIVES.** These rows are the stage-n audit's own examples; the worker's
    /// frozen corpus (`gateway/wasm/fixtures/translate-corpus.json`) covers the same rule over 30+ cases and still
    /// replays in `device.rs`, which is the differential that proves the MOVE changed nothing.
    ///
    /// MUTATION: drop the `172.` second-octet range test and refuse every `172.` host.
    /// RESULT:   `172.15.0.1` and `172.32.0.1` become blocked — public addresses a blanket prefix would have
    ///           refused, which is the reason the arm is numeric rather than textual.
    #[test]
    fn the_private_addresses_are_refused_and_the_public_ones_are_not() {
        for blocked in [
            "localhost",
            "127.0.0.1",
            "10.1.2.3",
            "192.168.1.1",
            "172.16.0.1",
            "172.31.255.255",
            "0.0.0.0",
            "169.254.169.254",
            "::1",
            "[::1]",
            "::ffff:127.0.0.1",
            "[::ffff:7f00:1]",
            "fd00::1",
            "fe80::1",
        ] {
            assert!(
                device_host_error(blocked).is_some(),
                "{blocked} must be blocked"
            );
        }
        for allowed in [
            "dev.agent.saisi.online",
            "172.15.0.1",
            "172.32.0.1",
            "fc.example.com",
            "8.8.8.8",
            "172.abc.1.1",
        ] {
            assert_eq!(
                device_host_error(allowed),
                None,
                "{allowed} must be allowed"
            );
        }
        assert_eq!(
            device_host_error("169.254.169.254").unwrap(),
            "device hostname resolves to a private/internal address",
            "the message is the wire's, and the emission carries this exact string"
        );
    }

    /// **THE ALLOWLIST, AND THE ROW THAT IS EASY TO MISS**: the host must end with the suffix AND be STRICTLY
    /// longer than it, so the bare suffix is not a device hostname. The override falls back through a truthiness
    /// test (`""` is not an override), which is the source's `||`.
    #[test]
    fn the_suffix_allowlist() {
        assert_eq!(host_allow_error("d1.agent.saisi.online", None), None);
        assert_eq!(
            host_allow_error("D1.Agent.Saisi.Online", None),
            None,
            "case-insensitive"
        );
        assert!(
            host_allow_error("agent.saisi.online", None).is_some(),
            "the bare suffix is not a host"
        );
        assert!(host_allow_error("evil.example", None).is_some());
        assert_eq!(
            host_allow_error("d1.lab.internal", Some(".lab.internal")),
            None
        );
        assert_eq!(
            host_allow_error("d1.agent.saisi.online", Some("")),
            None,
            "an EMPTY override is not an override"
        );
        assert!(host_allow_error("d1.agent.saisi.online", Some(".lab.internal")).is_some());
        assert_eq!(
            host_allow_error("evil.example", Some(".lab.internal")).unwrap(),
            "hostname must be under .lab.internal"
        );
    }

    /// **THE AUTHORITY GUARDS.** The scheme test is CASE-INSENSITIVE, and the at-sign test is narrowed to the
    /// authority-relevant prefix so `?user=a@b.com` stays legitimate.
    #[test]
    fn the_authority_guards() {
        assert!(authority_prefix_ok("/api/x"));
        assert!(authority_prefix_ok("/x?user=a@b.com"));
        assert!(authority_prefix_ok("x"));
        assert!(!authority_prefix_ok("@evil.example/x"));
        assert!(!authority_prefix_ok("https://evil.example/x"));
        assert!(!authority_prefix_ok("HTTPS://evil.example"));
        assert!(starts_like_a_scheme("https:"));
        assert!(!starts_like_a_scheme("https"));
        assert_eq!(dial_path("/x"), "/x");
        assert_eq!(dial_path("x"), "/x");
        assert!(!hostname_mismatch("Host.Example", "host.example"));
        assert!(hostname_mismatch("evil.example", "host.example"));
        assert_eq!(PROXY_PATH_REFUSAL, "invalid proxy path");
        assert_eq!(DEFAULT_DEVICE_HOST_SUFFIX, ".agent.saisi.online");
    }
}
