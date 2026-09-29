//! The CDN worker's RELEASE MANIFEST — `GET /api/version`, the route every device's updater calls.
//!
//! ## This is block ④'s first piece, and it is the pure half on purpose
//!
//! `index/src/index.js` is 417 lines across six routes, and this one — `/api/version` — is the only
//! route with NO I/O in its answer: it reads `version.json` (already fetched), validates it, and
//! REBUILDS every URL against **this request's own origin**. Everything else on that route — the
//! ASSETS fetch, the R2 object, the GitHub proxy — is I/O, and I/O cannot be compared byte for byte
//! between two languages without a runtime to run both in.
//!
//! So the manifest derivation is written as a PURE function over (origin, the parsed manifest, a
//! warning sink), with the I/O deliberately outside it. That is what makes the plan's criterion —
//! **"同请求新旧响应字节可比"** — answerable at all: the TypeScript and this crate are handed the same
//! `version.json` and produce the same status, the same headers and the same body BYTES.
//!
//! ## The rules, and each one is a decision somebody had to make
//!
//! * **A manifest that cannot be verified is NOT SERVED.** `agent_update` refuses a bad `sha256`, so
//!   the worker answers 503 rather than shipping one (round 119).
//! * **A FAILURE MUST NOT BE CACHEABLE.** Every error this worker returns goes through one helper
//!   that sets `content-type` and `cache-control: no-store`, because a 503 with no directives may be
//!   stored by a shared cache — and this is the route every device's updater calls (round 130).
//! * **EVERY URL IS REBUILT AGAINST THIS REQUEST'S ORIGIN, never echoed.** A manifest must not be
//!   able to point a device at another host; the flat-name validation below is what makes the
//!   basename safe to splice into a URL.
//! * **A MALFORMED COMPONENT PIN IS DROPPED *AND SAID OUT LOUD*.** Dropping is the safe half;
//!   dropping SILENTLY is the defect round 134 found — the device fetched the component unverified
//!   and `setup` printed "not verified" with nothing on this side to say why.

// `write!` is no longer used here — the body is built with `push_str` so a raw string cannot hide a brace
// escape (see the note on the body below).

/// `SHA256_RE` — the shape every digest in this file must have: 64 hex, either case.
pub const SHA256_RE: &str = "^[0-9a-f]{64}$";

/// A manifest component as `version.json` carries it: a URL to take the basename from, and a pin.
#[derive(Debug, Clone, PartialEq)]
pub struct RawComponent {
    pub url: String,
    pub sha256: String,
}

/// What the worker's answer is, before the runtime turns it into a `Response`.
#[derive(Debug, Clone, PartialEq)]
pub enum ManifestAnswer {
    /// 200 with this JSON body. `content-type: application/json`, `cache-control: no-store`.
    Ok(String),
    /// 503 with this `{"error": …}` body — the one failure this route returns.
    Failure { error: String },
}

impl ManifestAnswer {
    /// The status code, so a test can read it without a runtime.
    pub fn status(&self) -> u16 {
        match self {
            ManifestAnswer::Ok(_) => 200,
            ManifestAnswer::Failure { .. } => 503,
        }
    }

    /// The body bytes, which is what "byte comparable" means here.
    pub fn body(&self) -> &str {
        match self {
            ManifestAnswer::Ok(b) | ManifestAnswer::Failure { error: b } => b,
        }
    }
}

/// The 503 this route answers when the manifest is missing or unverifiable.
///
/// **THROUGH ONE HELPER, AND THE HELPER IS THE POINT**: a bare `new Response("…", {status: 503})`
/// with no `content-type` and no `cache-control` is a failure a shared cache is allowed to keep, and
/// the consequence is an updater that keeps being told the release is unavailable long after it is
/// (round 130, found by the test that was missing exactly this assertion).
pub const MANIFEST_UNAVAILABLE: &str =
    "release manifest unavailable: assets unavailable and no cached manifest";

/// `manifest(origin, version_json_text) -> ManifestAnswer` — THE WHOLE ROUTE.
///
/// `origin` is THIS request's origin, and every URL in the answer is built from it. `warn` receives
/// the lines the TypeScript's `console.warn` would print, so the "dropped and said out loud" half is
/// part of the compared surface and not a log line nobody can assert on.
pub fn manifest(origin: &str, version_json: &str, warn: &mut dyn FnMut(String)) -> ManifestAnswer {
    let parsed: serde_json::Value = match serde_json::from_str(version_json) {
        Ok(v) => v,
        // An unparseable manifest is the SAME answer as a missing one, and the TypeScript's is a
        // `try { … } catch { fall through }` into the shared 503 — not a second, friendlier error.
        Err(_) => return failure(MANIFEST_UNAVAILABLE),
    };

    let ver = parsed
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let sha = parsed
        .get("sha256")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    if ver.is_empty() || !is_sha256(sha) {
        return failure(MANIFEST_UNAVAILABLE);
    }

    // The tarball name is DATA, not decoration: `version.json` names the exact file, and the URL is
    // REBUILT from a flat basename — a hostile manifest must not escape `/summrise-agent/`.
    // An absent or invalid one falls back to the derived versioned name so older manifests keep
    // working; the smoke pins the consistent case.
    let tb_raw = parsed
        .get("tarball")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let tb = if is_flat_tarball(tb_raw) {
        tb_raw.to_string()
    } else {
        format!("summrise-agent-{ver}.tgz")
    };

    // THE BODY IS BUILT WITH push_str, NOT ONE BIG write! WITH {{ }}, and the reason is a bug this
    // crate had: a RAW string (`r#"…"#`) does not process `{{` as an escape, so the first version
    // emitted `{{"version":…}` — a body that is not JSON, caught by the test below on the second run.
    // Escaped braces inside a raw string are a silent way to ship malformed JSON to every device's
    // updater, so the literals here are plain text with no brace escapes to get wrong.
    let mut body = String::new();
    body.push_str("{\"version\":");
    body.push_str(&json_string(ver));
    body.push_str(",\"download\":");
    body.push_str(&json_string(&format!("{origin}/summrise-agent/{tb}")));
    body.push_str(",\"sha256\":");
    body.push_str(&json_string(sha));

    // ── THE PINNED COMPONENTS ────────────────────────────────────────────────────────────────
    // The key ORDER is the wire format: `Object.entries` walks insertion order, and a body compared
    // byte for byte is sensitive to it, so the same order is reproduced here.
    if let Some(entries) = parsed.get("components").and_then(|c| c.as_object()) {
        let mut comps = String::from("{");
        let mut any = false;
        for (key, val) in entries {
            // `if (!val || typeof val !== "object") continue`
            if !val.is_object() {
                continue;
            }
            let url = val.get("url");
            // `val.url ? String(val.url).split("/").pop() : ""` — TRUTHINESS on the url, and the
            // LAST path segment. A url that is absent, empty or non-string yields "" and is dropped
            // by the flat-name test below.
            let full = match url {
                // `val.url ? … : ""` — TRUTHINESS, so an absent, null, empty or `false` url is "".
                Some(serde_json::Value::String(s)) if !s.is_empty() => s.clone(),
                Some(u) if !u.is_null() && !u.is_boolean() => u.to_string(),
                _ => String::new(),
            };
            // `String(val.url).split("/").pop()` — THE LAST SEGMENT, and it is the whole reason a
            // full URL is safe to accept here: the basename is what gets spliced onto THIS origin, so
            // `https://evil.example/electron.zip` contributes `electron.zip` and nothing else.
            // `"".split("/")` is `[""]` and its `pop()` is `""`, which the flat-name test then drops.
            let name = full.rsplit('/').next().unwrap_or("").to_string();
            if !is_flat_name(&name) {
                continue;
            }
            let pin = val.get("sha256");
            let pin = match pin {
                Some(serde_json::Value::String(s)) => s.clone(),
                _ => String::new(),
            };
            if !is_sha256(&pin) {
                // DROPPED *AND SAID*. The parenthetical is
                // `typeof val.sha256 === "string" ? val.sha256.slice(0, 12) : typeof val.sha256` —
                // the first TWELVE CHARACTERS of the digest it rejected, so a reader can see the
                // typo, and `typeof` when the field is not a string at all. The first version had the
                // two arms the wrong way round and printed `string` for a three-character digest,
                // which is the half of the message that was supposed to name what was wrong.
                let shown = match val.get("sha256") {
                    Some(serde_json::Value::String(s)) => s.chars().take(12).collect::<String>(),
                    Some(other) => json_type_of(other),
                    None => "undefined".to_string(),
                };
                warn(format!(
                    "[version] component \"{key}\" dropped: sha256 is not a 64-hex digest ({shown}) \
                     — the manifest will not pin it, so setup stages it unverified"
                ));
                continue;
            }
            if any {
                comps.push(',');
            }
            any = true;
            // THE KEY IS ESCAPED LIKE ANY OTHER STRING. `Object.entries` gives the key as a JS
            // string and `comps[key] = …` writes it back through the same escaping `JSON.stringify`
            // applies — so a key carrying a quote is legal input, and emitting it raw produced a
            // body that is not JSON at all (measured: `{"a"b":…}`).
            comps.push_str(&json_string(key));
            comps.push_str(":{\"url\":");
            comps.push_str(&json_string(&format!("{origin}/summrise-agent/{name}")));
            comps.push_str(",\"sha256\":");
            comps.push_str(&json_string(&pin));
            comps.push('}');
        }
        if any {
            comps.push('}');
        }
        if any {
            // The components object is assembled with its own brace discipline for the same reason.
            body.push_str(",\"components\":");
            body.push_str(&comps);
        }
    }

    // ── THE INSTALLER, AND ONLY WHEN *THIS* RELEASE PUBLISHED ONE ────────────────────────────
    // A tgz-only publish (the documented emergency path) leaves the versionless alias serving the
    // PREVIOUS release while /api/version advertises the new one, so linking it unconditionally
    // hands a fresh install the old build and says nothing. The manifest is the only thing that
    // knows: it carries `installer` + `installer_sha256` exactly when one was produced.
    let inst = parsed
        .get("installer")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let inst_sha = parsed
        .get("installer_sha256")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    if is_flat_installer(inst) && !inst_sha.is_empty() && is_sha256(inst_sha) {
        body.push_str(",\"installer\":");
        body.push_str(&json_string(&format!("{origin}/summrise-agent/{inst}")));
        body.push_str(",\"installer_sha256\":");
        body.push_str(&json_string(inst_sha));
    }

    body.push('}');
    ManifestAnswer::Ok(body)
}

/// `JSON.stringify({error})` for the one failure body.
fn failure(error: &str) -> ManifestAnswer {
    ManifestAnswer::Failure {
        error: format!("{{\"error\":{}}}", json_string(error)),
    }
}

/// `/^[0-9a-f]{64}$/i` — 64 hex digits, either case. `chars().all` over a fixed length, and the
/// length is checked first so a 63- or 65-hex string cannot pass on `all`.
pub fn is_sha256(s: &str) -> bool {
    s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// `/^[A-Za-z0-9][A-Za-z0-9._-]*$/` — a flat name: no slashes, so splicing it into a URL cannot
/// escape the prefix.
fn is_flat_name(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphanumeric() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
}

/// `/^summrise-agent-[A-Za-z0-9][A-Za-z0-9._-]*\.tgz$/`
fn is_flat_tarball(s: &str) -> bool {
    let Some(rest) = s.strip_prefix("summrise-agent-") else {
        return false;
    };
    let Some(stem) = rest.strip_suffix(".tgz") else {
        return false;
    };
    is_flat_name(stem)
}

/// `/^SummriseAgent-Setup-[0-9]+\.[0-9]+\.[0-9]+\.exe$/` — the versioned shape only, never a path.
fn is_flat_installer(s: &str) -> bool {
    let Some(rest) = s.strip_prefix("SummriseAgent-Setup-") else {
        return false;
    };
    let Some(vers) = rest.strip_suffix(".exe") else {
        return false;
    };
    let parts: Vec<&str> = vers.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

/// A JSON string literal, quotes and all. `serde_json` is the authority for escaping; this is its
/// own `to_string` on a `Value::String`, which is exactly what `JSON.stringify` produces for a
/// string.
fn json_string(s: &str) -> String {
    serde_json::Value::String(s.to_string()).to_string()
}

/// The word `typeof` would print, for the warning's parenthetical. Only the three that the
/// manifest can actually carry are named; anything else is `"object"`, which is what it is.
fn json_type_of(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Null => "object".to_string(),
        serde_json::Value::Bool(_) => "boolean".to_string(),
        serde_json::Value::Number(_) => "number".to_string(),
        serde_json::Value::String(_) => "string".to_string(),
        serde_json::Value::Array(_) => "object".to_string(),
        serde_json::Value::Object(_) => "object".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORIGIN: &str = "https://dl.local";
    const SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn run(json: &str) -> (ManifestAnswer, Vec<String>) {
        let mut warnings = Vec::new();
        let mut sink = |m: String| warnings.push(m);
        (manifest(ORIGIN, json, &mut sink), warnings)
    }

    fn good() -> String {
        format!(r#"{{"version":"1.2.297","sha256":"{SHA}","tarball":"summrise-agent-latest.tgz"}}"#)
    }

    #[test]
    fn the_consistent_case_answers_the_three_fields() {
        let (a, w) = run(&good());
        assert_eq!(a.status(), 200);
        assert_eq!(w, Vec::<String>::new());
        assert_eq!(
            a.body(),
            format!(
                r#"{{"version":"1.2.297","download":"https://dl.local/summrise-agent/summrise-agent-latest.tgz","sha256":"{SHA}"}}"#
            )
        );
    }

    #[test]
    fn an_unverifiable_manifest_is_503_and_names_itself() {
        // THE THREE REASONS, and all three are the same answer: absent, unparseable, and a digest
        // `agent_update` would refuse. Shipping one is the bug round 119 fixed.
        for json in [
            r#"{}"#,
            r#"{"version":"1.2.297"}"#,
            r#"{"version":"1.2.297","sha256":"abc"}"#,
            r#"{"version":"1.2.297","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#,
            "not json at all",
        ] {
            let (a, _) = run(json);
            assert_eq!(a.status(), 503, "{json}");
            assert_eq!(
                a.body(),
                r#"{"error":"release manifest unavailable: assets unavailable and no cached manifest"}"#
            );
        }
    }

    #[test]
    fn a_hostile_tarball_name_falls_back_to_the_derived_one() {
        // A name with a slash must not escape `/summrise-agent/` — the flat-name test is the whole
        // reason the URL can be REBUILT rather than echoed.
        let (a, _) = run(&format!(
            r#"{{"version":"1.2.297","sha256":"{SHA}","tarball":"../../evil.tgz"}}"#
        ));
        assert!(
            a.body()
                .contains("summrise-agent/summrise-agent-1.2.297.tgz"),
            "{}",
            a.body()
        );
    }

    #[test]
    fn a_malformed_component_pin_is_dropped_and_said() {
        let (a, w) = run(&format!(
            r#"{{"version":"1.2.297","sha256":"{SHA}","components":{{"cloudflared":{{"url":"https://x/y/cloudflared.exe","sha256":"abc"}},"playwright":{{"url":"https://x/y/summrise-playwright.zip","sha256":"{SHA}"}}}}}}"#
        ));
        assert_eq!(a.status(), 200);
        let body = a.body();
        assert!(
            body.contains("playwright"),
            "the well-formed sibling is still pinned: {body}"
        );
        assert!(
            !body.contains("\"cloudflared\""),
            "the bad pin is not shipped: {body}"
        );
        // DROPPED *AND SAID*, and the message names what was wrong — the half round 134 found missing.
        assert_eq!(w.len(), 1, "{}", w.join(" | "));
        assert!(w[0].contains("cloudflared"), "{}", w[0]);
        assert!(w[0].contains("not a 64-hex digest"), "{}", w[0]);
    }

    #[test]
    fn the_installer_is_advertised_only_with_both_fields() {
        let with_both = format!(
            r#"{{"version":"1.2.297","sha256":"{SHA}","installer":"SummriseAgent-Setup-1.2.297.exe","installer_sha256":"{SHA}"}}"#
        );
        assert!(run(&with_both).0.body().contains("installer_sha256"));
        // One field alone is no promise — the round-125 rule.
        for json in [
            format!(
                r#"{{"version":"1.2.297","sha256":"{SHA}","installer":"SummriseAgent-Setup-1.2.297.exe"}}"#
            ),
            format!(r#"{{"version":"1.2.297","sha256":"{SHA}","installer_sha256":"{SHA}"}}"#),
            format!(
                r#"{{"version":"1.2.297","sha256":"{SHA}","installer":"../../evil.exe","installer_sha256":"{SHA}"}}"#
            ),
        ] {
            assert!(!run(&json).0.body().contains("\"installer\""), "{json}");
        }
    }

    #[test]
    fn the_component_url_is_rebuilt_against_this_request() {
        // A manifest that names another host must not be able to send a device there: the basename
        // is taken and the origin is this request's.
        let (a, _) = run(&format!(
            r#"{{"version":"1.2.297","sha256":"{SHA}","components":{{"electron":{{"url":"https://evil.example/electron.zip","sha256":"{SHA}"}}}}}}"#
        ));
        assert!(
            a.body()
                .contains("https://dl.local/summrise-agent/electron.zip"),
            "{}",
            a.body()
        );
        assert!(!a.body().contains("evil.example"), "{}", a.body());
    }
}
