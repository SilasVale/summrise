//! THE DEVICE-HOST RULES, MOVED FROM `gateway/src/device-fetch.ts`.
//!
//! WHY THESE TWO. One decides WHAT A DEVICE HOSTNAME IS, and the other decides which hostnames must never
//! be dialled with a device credential — the stage-n SSRF audit, whose comment names the attack: a device
//! registered with hostname `169.254.169.254` (cloud metadata) or `127.0.0.1` makes the gateway dial it
//! with the device token. Both are pure, and a drift in either is a hole rather than a bug.
//!
//! **`device_host_error` DOES NOT REPEAT THE WHATWG PARSER'S WORK**, and the source says why: decimal, hex
//! and octal IPv4 (`2130706433`, `0x7f.0.0.1`, `0177.0.0.1`) are normalized to `127.0.0.1` by the URL
//! parser before this sees them. What it does NOT normalize away is the list below — including
//! IPv4-mapped IPv6, which can smuggle `127.0.0.1` past every v4 rule.

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
        match js_number_of(second) {
            Some(n) => n.fract() == 0.0 && (16.0..=31.0).contains(&n),
            None => false,
        }
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

/// `Number(x)` for the one place this module needs it: `Number.isInteger(Number(second))`.
///
/// `None` is `NaN`, which is what makes a non-numeric second octet not private. An EMPTY string is `0` and
/// therefore an integer — `"172..1.1"` takes the private branch, exactly as the JavaScript does.
fn js_number_of(s: &str) -> Option<f64> {
    let t = s.trim();
    if t.is_empty() {
        return Some(0.0);
    }
    t.parse::<f64>().ok()
}

#[cfg(test)]
mod oracle_corpus {
    //! `fixtures/translate-corpus.json`'s `device` cases were produced by the SHIPPING TypeScript
    //! (`gateway/wasm/oracle-translate.mjs`), and this replays them.
    use super::*;

    fn corpus() -> serde_json::Value {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/translate-corpus.json"
        );
        let text = std::fs::read_to_string(path).expect("the corpus is committed");
        serde_json::from_str(&text).expect("the corpus parses")
    }

    #[test]
    fn every_device_host_rule_matches_the_shipping_typescript() {
        let doc = corpus();
        let mut checked = 0;
        for case in doc["cases"].as_array().expect("cases") {
            let func = case["fn"].as_str().unwrap_or("?");
            let name = case["name"].as_str().unwrap_or("?");
            let input = &case["input"];
            let want = &case["expected"]["value"];
            let got: Option<String> = match func {
                "host_allow_error" => host_allow_error(
                    input["hostname"].as_str().unwrap_or(""),
                    input["suffix"].as_str(),
                ),
                "device_host_error" => device_host_error(input["hostname"].as_str().unwrap_or("")),
                _ => continue,
            };
            let as_json = match got {
                Some(s) => serde_json::Value::String(s),
                None => serde_json::Value::Null,
            };
            assert_eq!(&as_json, want, "{func} / {name}");
            checked += 1;
        }
        assert!(checked >= 30, "the device corpus shrank to {checked} cases");
    }
}
