//! THE CORS GATE, MOVED FROM `gateway/src/http.ts`.
//!
//! WHY THIS FAMILY. Its own comment says it: "this list is a security surface (it is what a browser is
//! allowed to read answers from)". A drift here does not throw — it WIDENS OR CLOSES A DOOR, and the
//! repository already has the measurement of what that costs: audit P2, where production reflected ANY
//! localhost origin, because the loopback rule did not also ask whether the REQUEST's own host was
//! loopback. That rule is the one subtle thing in this module and the corpus carries its case.
//!
//! `url` IS ALREADY IN THE TREE — `worker` depends on it — so parsing here costs no extra bytes, and
//! `URL`'s own semantics (lowercasing, rejecting a relative string) are the ones being mirrored.

/// The DEFAULT allowed origins, verbatim and in order.
pub const DEFAULT_ALLOWED_ORIGINS: [&str; 2] =
    ["https://ai.saisi.online", "https://api.saisi.online"];

/// `allowedOrigins(env)`: the configured list when it is given AND non-empty, else the default.
///
/// **AN EMPTY OR BLANK CONFIGURATION IS NOT AN EMPTY ALLOWLIST.** `split(",")` on `""` gives `[""]`, the
/// `filter(Boolean)` removes it, and `list.length ? … : ALLOWED_ORIGINS` then answers the DEFAULT — so a
/// misconfigured deployment falls back to the known origins rather than allowing none (or all).
pub fn allowed_origins(configured: Option<&str>) -> Vec<String> {
    let Some(raw) = configured.filter(|c| !c.is_empty()) else {
        return DEFAULT_ALLOWED_ORIGINS
            .iter()
            .map(|s| s.to_string())
            .collect();
    };
    let list: Vec<String> = raw
        .split(',')
        .map(|o| o.trim().to_string())
        .filter(|o| !o.is_empty())
        .collect();
    if list.is_empty() {
        DEFAULT_ALLOWED_ORIGINS
            .iter()
            .map(|s| s.to_string())
            .collect()
    } else {
        list
    }
}

/// `isLoopbackOrigin(origin)`: a parseable http/https URL whose HOSTNAME is localhost or 127.0.0.1.
///
/// The protocol test is why `ftp://localhost` is not a loopback origin, and `URL`'s lowercasing is why
/// `http://LOCALHOST` is one. `http://localhost.evil.com` is not: the hostname is the whole label.
pub fn is_loopback_origin(origin: &str) -> bool {
    let Ok(u) = url::Url::parse(origin) else {
        return false;
    };
    let scheme = u.scheme();
    if scheme != "http" && scheme != "https" {
        return false;
    }
    match u.host_str() {
        Some(h) => h == "localhost" || h == "127.0.0.1",
        None => false,
    }
}

/// `isLoopbackHost(hostname)`: an EXACT, CASE-SENSITIVE match — this is handed a hostname that `URL` has
/// already lowercased, so it is a comparison rather than a normalisation.
pub fn is_loopback_host(hostname: &str) -> bool {
    hostname == "localhost" || hostname == "127.0.0.1"
}

/// `isAllowedOrigin(origin, requestHost, env)` — **AND THE SECOND HALF OF THE LOOPBACK RULE IS THE POINT.**
///
/// A loopback origin is a `wrangler dev` affordance, not a production grant: a request to
/// `http://localhost:<port>` whose Origin is ALSO loopback is local development, while the SAME Origin
/// arriving at the deployed console host is just a foreign local page and gets NO `Access-Control-Allow-
/// Origin`. Pre-fix, production reflected any localhost origin — audit P2.
pub fn is_allowed_origin(
    origin: &str,
    request_host: Option<&str>,
    configured: Option<&str>,
) -> bool {
    if origin.is_empty() {
        return false;
    }
    if allowed_origins(configured).iter().any(|o| o == origin) {
        return true;
    }
    match request_host {
        Some(host) if !host.is_empty() => is_loopback_origin(origin) && is_loopback_host(host),
        _ => false,
    }
}

/// The base headers every CORS answer carries (`CORS_HEADERS` in the source), **AND THE ONE SPELLING THE EMITTED
/// TYPESCRIPT READS.** `render_typescript` writes this table into `gateway/src/cors-logic.ts`, so a header added
/// here reaches the shipping file only through the emitter, and a header added to the emitter alone cannot exist.
pub const CORS_HEADERS: [(&str, &str); 2] = [
    (
        "Access-Control-Allow-Methods",
        "GET,POST,OPTIONS,DELETE,PUT",
    ),
    ("Access-Control-Allow-Headers", "*"),
];

/// `corsHeadersFor(origin, requestHost, env)`: reflect-if-allowlisted plus `Vary`, else the base set with
/// NO `Access-Control-Allow-Origin`.
///
/// **`Vary: Origin` RIDES WITH THE REFLECTION AND ONLY WITH IT** — a cache that stored an answer without
/// it would serve one origin's grant to another.
pub fn cors_headers_for(
    origin: &str,
    request_host: Option<&str>,
    configured: Option<&str>,
) -> Vec<(String, String)> {
    let mut headers: Vec<(String, String)> = CORS_HEADERS
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    if is_allowed_origin(origin, request_host, configured) {
        headers.push((
            "Access-Control-Allow-Origin".to_string(),
            origin.to_string(),
        ));
        headers.push(("Vary".to_string(), "Origin".to_string()));
    }
    headers
}

/// `stampCors(request, headers, env)` — set the CORS pair on an EXISTING header set, or REMOVE the origin.
///
/// **THE `else` BRANCH DELETES, AND THAT IS NOT SYMMETRIC WITH `corsHeadersFor`.** That one starts from a
/// fresh set that never had an ACAO, so "not allowed" means "do not add one". This one is handed the
/// UPSTREAM's headers, which may carry an ACAO of their own — so refusing has to take it away. A port that
/// merely skipped the `if` would forward whatever the upstream said about origins.
///
/// **AND IT TAKES THE ORIGIN ALONE — `Vary` IS NOT ITS TO DELETE.** The source's `else` is one line,
/// `headers.delete("Access-Control-Allow-Origin")` (`gateway/src/http.ts:124`); `Vary` is touched only in the
/// ALLOWED branch, where the source's `headers.set("Vary", "Origin")` REPLACES whatever the upstream sent. This
/// function removed both in every branch, which is a real difference from the console: measured on the file
/// relay's own answer, a `vary: accept-encoding` sent to a refused origin came back DROPPED here and KEPT by the
/// shipping implementation. It has no caller outside its own unit tests today — the two live stamps are
/// `lib::with_cors` and `device_proxy::stamp_cors_like_ts` — which is exactly why it went unnoticed: a helper
/// nothing calls cannot fail a corpus case, and the test beside it asserted the wrong behaviour as if it were
/// the source's. Both are corrected together.
pub fn stamp_cors(
    headers: &mut Vec<(String, String)>,
    origin: &str,
    request_host: Option<&str>,
    configured: Option<&str>,
) {
    headers.retain(|(k, _)| !k.eq_ignore_ascii_case("access-control-allow-origin"));
    if is_allowed_origin(origin, request_host, configured) {
        headers.retain(|(k, _)| !k.eq_ignore_ascii_case("vary"));
        headers.push((
            "Access-Control-Allow-Origin".to_string(),
            origin.to_string(),
        ));
        headers.push(("Vary".to_string(), "Origin".to_string()));
    }
}

/// The SUCCESS path of `relayUpstreamResult`: the upstream's status and body, its headers with the CORS pair
/// stamped, and nothing else touched.
///
/// **`new Response(upstream.body, …)` FORWARDS THE BODY UNTOUCHED** — a passthrough answer is the upstream's
/// bytes, not a reshaping of them — and the only header surgery is `stampCors`. The `x-generation-id`
/// header the source also reads goes to the LOG context rather than to the client, so it is not here.
pub fn forwarded_upstream_response(
    status: u16,
    upstream_headers: &[(String, String)],
    origin: &str,
    request_host: Option<&str>,
    configured: Option<&str>,
    body: &str,
) -> crate::responses::Built {
    let mut headers = upstream_headers.to_vec();
    stamp_cors(&mut headers, origin, request_host, configured);
    crate::responses::Built {
        status,
        headers,
        body: body.to_string(),
    }
}

/* ─────────────────────────────── THE EMISSION ─────────────────────────────── */

/// The emitted module — **AND THE WHOLE OF IT, BECAUSE EVERY LINE ABOVE IS A DECISION THIS MODULE ALREADY MAKES.**
///
/// WHY AN EMISSION AND NOT A DELETION OR A ROUTE. `gateway/src/http.ts`'s CORS half is reached by the STATUS page
/// and therefore may not ask the worker (`plugins.test.mjs`: *"the worker must never be asked"*), and `npm test`
/// cannot reach `/__decision` at all — so a twin deleted outright would delete the only implementation the tests
/// can reach, and an async route cannot serve a predicate that the response path calls synchronously. An emission
/// can: the Rust is the source, the emitted module is JavaScript, and the freshness test below is a byte comparison.
const TS_HEADER: &str = r#"/**
 * cors-logic.ts — GENERATED. DO NOT EDIT BY HAND.
 *
 * Emitted by `gateway/wasm/src/cors.rs` (`render_typescript`), which is now the single source. The freshness test
 * in that module is a byte comparison against this file, so a hand edit fails there with the text and one command
 * to fix it:
 *
 *     SUMMRISE_REFRESH_CORS=1 cargo test --manifest-path gateway/wasm/Cargo.toml cors::emission
 *
 * `gateway/src/http.ts` re-exports this module and keeps only the Request/Response plumbing — pulling an Origin
 * and a host out of a request, and rebuilding a response — because those are platform calls that decide nothing.
 *
 * The comment below is the file's own, carried verbatim: it is the record of why this list is what it is, and none
 * of it is recoverable from the two rows.
 *
 * CORS allowlist: the console origins used in this repo —
 *   https://ai.saisi.online + https://api.saisi.online (CONSOLE_HOST in wrangler.jsonc),
 * plus http(s) loopback for local `wrangler dev`. Any other Origin gets NO
 *
 * https://dsh.saisi.online WAS HERE UNTIL ROUND 243, and it was here for exactly one reason: the Summrise Code
 * Links browser extension ran a content script on that host and called this API from it. The extension was removed
 * (it shipped nowhere, its feature was off by default, and the half it existed for had already been deleted), so
 * the grant went with it — an allowed origin with no consumer is a permission nobody is using.
 * Access-Control-Allow-Origin header (default-closed). Non-browser clients
 * (Claude Code, curl, gateway server-side) are unaffected by CORS.
 * (Mirrors the satellites' ALLOWED_ORIGINS — proxies/zen-us-proxy/worker/src/lib.rs, which zen-go
 * re-exports. The two JavaScript copies this line used to name were deleted on 2026-10-08.)
 */

/** The DEFAULT allowlist, verbatim and in order (`DEFAULT_ALLOWED_ORIGINS` in `cors.rs`).
 *
 *  **THE LITERAL SPELLING IS LOAD-BEARING, AND THAT IS MEASURED RATHER THAN ASSUMED**: two gates read this file as
 *  TEXT — `gateway/test/cors-allowlist.test.mjs` and `agent/tests/proxy_cors_parity.rs` both anchor on the
 *  DECLARATION (`const ALLOWED_ORIGINS`), then `new Set([`, then `])` — so the list is written as a `new Set([ … ])`
 *  OF QUOTED ORIGINS rather than built from a constant, and `cors.rs` carries a test that pins those anchors.
 *  **AND THAT SENTENCE IS WHY THE ANCHOR IS THE DECLARATION RATHER THAN THE NAME**: the first version of this comment
 *  spelled the bare `ALLOWED_ORIGINS` in prose ABOVE the declaration, and `proxy_cors_parity.rs` — whose reader
 *  anchored on the first MENTION of the name — read the COMMENT: its `new Set([ … ])` slice holds no quoted string,
 *  the list read as EMPTY, and the gate failed this file with *"could not read an allowlist — this proves nothing"*.
 *  The reader anchors on the declaration now, and its own test pins that a mention in prose cannot stand in. */
export const ALLOWED_ORIGINS: ReadonlySet<string> = new Set([
  "https://ai.saisi.online",
  "https://api.saisi.online",
]);

/** THE ORIGINS THIS DEPLOYMENT ALLOWS, from configuration when it is given (round 88). The set above is the DEFAULT
 *  and is unchanged; `CONSOLE_ORIGINS` (comma-separated) overrides it. The parameter is optional on purpose, and
 *  that is the staging: every existing caller keeps the exact behaviour it had, and the callers that HAVE an `env`
 *  adopt it as they are touched — because this list is a security surface (it is what a browser is allowed to read
 *  answers from), and threading it through every response-stamping path in one commit is how a CORS grant gets
 *  widened by accident.
 *
 *  WHY IT EXISTS AT ALL: the gateway's tests must be able to run against a test domain instead of a production one,
 *  and today they cannot — 76 of 916 fail the moment the fixtures stop spelling the real host (round 48's
 *  measurement). */
export function allowedOrigins(configured?: string | null): ReadonlySet<string> {
  if (!configured) return ALLOWED_ORIGINS;
  const list = configured
    .split(",")
    .map((o) => o.trim())
    .filter(Boolean);
  return list.length ? new Set(list) : ALLOWED_ORIGINS;
}

export function isLoopbackOrigin(origin: string): boolean {
  try {
    const u = new URL(origin);
    return (
      (u.protocol === "http:" || u.protocol === "https:") &&
      (u.hostname === "localhost" || u.hostname === "127.0.0.1")
    );
  } catch {
    return false;
  }
}

export function isLoopbackHost(hostname: string): boolean {
  return hostname === "localhost" || hostname === "127.0.0.1";
}

export function isAllowedOrigin(
  origin: string,
  requestHost?: string | null,
  configured?: string | null,
): boolean {
  if (!origin) return false;
  if (allowedOrigins(configured).has(origin)) return true;
  // Loopback origins are a `wrangler dev` affordance, not a production grant:
  // a request to http://localhost:<port> whose Origin is also loopback is
  // local dev; the SAME Origin arriving at the deployed console host is just
  // a foreign local page and gets NO ACAO. (Pre-fix, production reflected
  // any localhost origin — audit P2.)
  return isLoopbackOrigin(origin) && !!requestHost && isLoopbackHost(requestHost);
}

// Base CORS headers shared by every response. Deliberately NO
// Access-Control-Allow-Origin here — the origin is reflected per request
// (corsHeadersFor / stampCors / withCors), otherwise a static `*`
// would ride along on every merge of this constant.
export const CORS_HEADERS: Record<string, string> = {
  "Access-Control-Allow-Methods": "GET,POST,OPTIONS,DELETE,PUT",
  "Access-Control-Allow-Headers": "*",
};

/** Per-request CORS headers: reflect-if-allowlisted + Vary, else no ACAO.
 *
 *  **IT TAKES THE ORIGIN AND THE HOST, NOT A `Request`** — this is the Rust signature. Extracting them from a
 *  request is a platform call and stays in `http.ts`; deciding with them is this module's. */
export function corsHeadersFor(
  origin: string,
  requestHost?: string | null,
  configured?: string | null,
): Record<string, string> {
  const headers: Record<string, string> = { ...CORS_HEADERS };
  if (isAllowedOrigin(origin, requestHost, configured)) {
    headers["Access-Control-Allow-Origin"] = origin;
    headers["Vary"] = "Origin";
  }
  return headers;
}

/** Stamp (or strip) ACAO on a mutable Headers object, per request origin.
 *
 *  **THE `else` DELETES, AND THAT IS NOT SYMMETRIC WITH `corsHeadersFor`.** That one starts from a fresh set which
 *  never had an ACAO, so "not allowed" means "do not add one"; this one is handed headers that may already carry
 *  the upstream's, so refusing has to take it away. `Vary` is touched only in the ALLOWED branch, where the
 *  source's `set` REPLACES whatever the upstream sent. */
export function stampCors(
  headers: Headers,
  origin: string,
  requestHost?: string | null,
  configured?: string | null,
): void {
  if (isAllowedOrigin(origin, requestHost, configured)) {
    headers.set("Access-Control-Allow-Origin", origin);
    headers.set("Vary", "Origin");
  } else {
    headers.delete("Access-Control-Allow-Origin");
  }
}
"#;

/// **THE PRODUCER.** The emitted module is exactly this string.
pub fn render_typescript() -> String {
    TS_HEADER.to_string()
}

/// The committed emission, and the path the refresh command writes.
pub fn typescript_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/cors-logic.ts")
}

#[cfg(test)]
mod emission {
    use super::*;

    /// **THE FRESHNESS CHECK — AND IT IS THE REASON THE EMISSION CANNOT DRIFT.** The same shape `mcp_tools.rs`,
    /// `settings_logic.rs` and `byok.rs` use.
    #[test]
    fn the_typescript_is_what_this_module_emits() {
        let path = typescript_path();
        let rendered = render_typescript();
        if std::env::var("SUMMRISE_REFRESH_CORS").is_ok_and(|v| !v.is_empty()) {
            std::fs::write(&path, &rendered).expect("write cors-logic.ts");
            return;
        }
        let committed = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "{} missing ({e}) — run SUMMRISE_REFRESH_CORS=1 cargo test cors::emission",
                path.display()
            )
        });
        assert_eq!(
            committed, rendered,
            "gateway/src/cors-logic.ts is not what cors.rs emits — \
             run SUMMRISE_REFRESH_CORS=1 cargo test cors::emission"
        );
    }

    /// **AND THE SPELLING TWO OTHER GATES PARSE IS PART OF THE CONTRACT, NOT AN ACCIDENT OF STYLE.** The freshness
    /// test alone would happily pin a list edited on BOTH sides; this one pins the SUBJECT (the two origins, in the
    /// source's order) and the ANCHORS `cors-allowlist.test.mjs` and `proxy_cors_parity.rs` slice between.
    ///
    /// MUTATION: drop an origin from `DEFAULT_ALLOWED_ORIGINS` and re-emit.
    /// RESULT:   this test fails on the origin; the two external gates then fail on the drift it caused.
    #[test]
    fn the_emitted_module_carries_the_origins_and_the_anchors_two_gates_read() {
        assert_eq!(
            DEFAULT_ALLOWED_ORIGINS,
            ["https://ai.saisi.online", "https://api.saisi.online"]
        );
        let ts = render_typescript();
        let at = ts
            .find("export const ALLOWED_ORIGINS")
            .expect("cors-allowlist.test.mjs anchors on this name");
        let open = ts[at..].find("new Set([").expect("...then on `new Set([`") + at;
        let close = ts[open..].find("])").expect("...and slices to `])`") + open;
        let slice = &ts[open..close];
        assert!(
            slice.contains("\"https://ai.saisi.online\"")
                && slice.contains("\"https://api.saisi.online\""),
            "the allowlist must be a `new Set([ … ])` OF QUOTED ORIGINS, or both gates read zero: {slice}"
        );
    }

    /// **AND THE BASE HEADERS ARE THE EMITTED TABLE, NOT A SECOND COPY BESIDE IT.**
    ///
    /// MUTATION: change `CORS_HEADERS`' methods string here.
    /// RESULT:   the emitted bytes change, the freshness test fails, and `http.test.mjs`/the corpus cases that
    ///           carry the preflight answer fail with them.
    #[test]
    fn the_base_headers_are_one_table() {
        let ts = render_typescript();
        for (name, value) in CORS_HEADERS {
            assert!(
                ts.contains(&format!("\"{name}\": \"{value}\"")),
                "the emitter lost {name}"
            );
        }
        // ...and `cors_headers_for` builds from the same table rather than repeating it.
        let pairs = cors_headers_for("https://nothing.example", None, None);
        assert_eq!(
            pairs,
            CORS_HEADERS
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect::<Vec<_>>()
        );
    }
}

#[cfg(test)]
mod oracle_corpus {
    //! `fixtures/translate-corpus.json`'s `cors` cases were produced by the SHIPPING TypeScript
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

    /// **A PLAIN OBJECT KEEPS ITS SPELLING; A `Headers` OBJECT DOES NOT.** `corsHeadersFor` returns a
    /// plain `Record<string, string>`, so the oracle recorded `Access-Control-Allow-Methods` as the source
    /// spells it — the opposite of `passthroughHeaders`, whose `Headers` object folds every name. Both
    /// facts are the runtime's, and a test that folded here would be comparing against a shape the
    /// JavaScript never produces.
    fn as_object(pairs: Vec<(String, String)>) -> serde_json::Map<String, serde_json::Value> {
        pairs
            .into_iter()
            .map(|(k, v)| (k, serde_json::Value::String(v)))
            .collect()
    }

    #[test]
    fn stamping_removes_an_origin_it_does_not_allow() {
        // **THE ASYMMETRY WITH `corsHeadersFor`, WHICH IS THE WHOLE REASON THIS FUNCTION EXISTS.** That one
        // builds a fresh set, so "not allowed" means "add nothing". This one is handed the UPSTREAM's
        // headers, which may carry an ACAO of their own — so refusing has to take it away, and a port that
        // merely skipped the `if` would forward whatever the upstream claimed about origins.
        let upstream = vec![
            ("content-type".to_string(), "application/json".to_string()),
            (
                "access-control-allow-origin".to_string(),
                "https://evil.example".to_string(),
            ),
            ("vary".to_string(), "Accept-Encoding".to_string()),
        ];
        let mut headers = upstream.clone();
        stamp_cors(
            &mut headers,
            "https://evil.example",
            Some("api.saisi.online"),
            None,
        );
        assert!(
            !headers
                .iter()
                .any(|(k, _)| k.eq_ignore_ascii_case("access-control-allow-origin")),
            "the upstream's origin must be REMOVED: {headers:?}"
        );
        // **AND ITS `Vary` STAYS — THIS ASSERTION USED TO SAY THE OPPOSITE, AND THAT IS WHY THE LIVE CODE READ
        // AS CORRECT.** The source's `else` is ONE line, `headers.delete("Access-Control-Allow-Origin")`
        // (`gateway/src/http.ts:124`); `Vary` is replaced only where the source replaces it, in the ALLOWED
        // branch below (`headers.set("Vary", "Origin")`). A test asserting that refusing takes `Vary` with it
        // made this function the authority for an asymmetry it does not have — and the function it "justified"
        // is the one the door actually uses. Measured on the file relay's own answer: a device or a relay that
        // sent `vary: accept-encoding` to a refused origin came back with it DROPPED here and KEPT by the
        // shipping console. `verify.mjs` carries the pairing now; this pins it one level down.
        assert_eq!(
            headers
                .iter()
                .filter(|(k, _)| k.eq_ignore_ascii_case("vary"))
                .map(|(_, v)| v.as_str())
                .collect::<Vec<_>>(),
            vec!["Accept-Encoding"],
            "an upstream Vary is not ours to delete when we refuse the origin: {headers:?}"
        );
        assert!(
            headers.iter().any(|(k, _)| k == "content-type"),
            "the rest rides"
        );

        // An ALLOWED origin is stamped, and the upstream's value is replaced rather than duplicated.
        let mut headers = upstream.clone();
        stamp_cors(
            &mut headers,
            "https://ai.saisi.online",
            Some("api.saisi.online"),
            None,
        );
        let acao: Vec<&String> = headers
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case("access-control-allow-origin"))
            .map(|(_, v)| v)
            .collect();
        assert_eq!(acao, vec!["https://ai.saisi.online"], "{headers:?}");
        // **AND HERE `Vary` IS REPLACED, WHICH IS THE OTHER HALF OF THE ASYMMETRY.** `Vary: Origin` is what the
        // ALLOWED branch writes, so the upstream's `Accept-Encoding` does not survive it — the two branches
        // differ in exactly this, and pinning only one of them is how the pair came to be misread.
        let vary: Vec<&String> = headers
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case("vary"))
            .map(|(_, v)| v)
            .collect();
        assert_eq!(
            vary,
            vec!["Origin"],
            "the allowed branch REPLACES the upstream's Vary: {headers:?}"
        );
    }

    #[test]
    fn the_forwarded_response_is_the_upstreams_bytes_plus_the_cors_pair() {
        let upstream = vec![
            ("content-type".to_string(), "application/json".to_string()),
            ("x-generation-id".to_string(), "gen-1".to_string()),
        ];
        let got = forwarded_upstream_response(
            200,
            &upstream,
            "https://ai.saisi.online",
            Some("api.saisi.online"),
            None,
            "{\"ok\":true}",
        );
        assert_eq!(got.status, 200);
        assert_eq!(got.body, "{\"ok\":true}", "the body is forwarded untouched");
        // **THE KEY IS COMPARED CASE-INSENSITIVELY, AND THE VALUE KEEPS THE SOURCE'S SPELLING.** `stampCors`
        // writes `Access-Control-Allow-Origin` on a `Headers` object, which folds it; this port carries a
        // `Vec` of pairs, so the spelling is the source's — and a test that compared the folded form would be
        // asserting something the runtime does rather than something the function does.
        assert!(
            got.headers.iter().any(
                |(k, v)| k.eq_ignore_ascii_case("access-control-allow-origin")
                    && v == "https://ai.saisi.online"
            ),
            "headers were {:?}",
            got.headers
        );
        // The generation id is the LOG context's, not the client's — the source reads it into `ctx`.
        assert!(got.headers.iter().any(|(k, _)| k == "x-generation-id"));
    }

    #[test]
    fn every_cors_decision_matches_the_shipping_typescript() {
        let doc = corpus();
        let mut checked = 0;
        for case in doc["cases"].as_array().expect("cases") {
            let func = case["fn"].as_str().unwrap_or("?");
            let name = case["name"].as_str().unwrap_or("?");
            let input = &case["input"];
            let want = &case["expected"]["value"];
            let configured = input["configured"].as_str();
            let host = input["requestHost"].as_str();
            match func {
                "allowed_origins" => {
                    let got: Vec<serde_json::Value> = allowed_origins(configured)
                        .into_iter()
                        .map(serde_json::Value::String)
                        .collect();
                    assert_eq!(&serde_json::Value::Array(got), want, "{func} / {name}");
                }
                "is_loopback_origin" => {
                    let got = is_loopback_origin(input["origin"].as_str().unwrap_or(""));
                    assert_eq!(&serde_json::Value::Bool(got), want, "{func} / {name}");
                }
                "is_loopback_host" => {
                    let got = is_loopback_host(input["host"].as_str().unwrap_or(""));
                    assert_eq!(&serde_json::Value::Bool(got), want, "{func} / {name}");
                }
                "is_allowed_origin" => {
                    let got =
                        is_allowed_origin(input["origin"].as_str().unwrap_or(""), host, configured);
                    assert_eq!(&serde_json::Value::Bool(got), want, "{func} / {name}");
                }
                "cors_headers_for" => {
                    let got =
                        cors_headers_for(input["origin"].as_str().unwrap_or(""), host, configured);
                    assert_eq!(
                        &serde_json::Value::Object(as_object(got)),
                        want,
                        "{func} / {name}"
                    );
                }
                _ => continue,
            }
            checked += 1;
        }
        assert!(checked >= 25, "the cors corpus shrank to {checked} cases");
    }
}
