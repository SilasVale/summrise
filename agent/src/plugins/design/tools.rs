//! Tool builders for the design plugin.

use serde_json::{json, Value};

use crate::state::ConfigHandle;
use summrise_agent_core::{DeviceError, ToolDef};

/// Pages viewable via page_view, keyed by a short name. Three sources:
/// `Local(path)`  = this agent's own HTTP surface at host:port (no tunnel).
/// `Remote(url)`  = the deployed worker pages (console / download site) so
///                  the AI sees the REAL production design, not a mirror.
enum PageSource {
    Local(&'static str),
    Remote(&'static str),
}
const PAGES: &[(&str, PageSource)] = &[
    ("status", PageSource::Local("/")),
    ("panel", PageSource::Local("/panel/")),
    ("panel-js", PageSource::Local("/panel/panel.js")),
    (
        "console-js",
        PageSource::Remote("https://api.saisi.online/app.js"),
    ),
    ("panel-css", PageSource::Local("/panel/panel.css")),
    ("panel-html", PageSource::Local("/panel/")),
    ("console", PageSource::Remote("https://api.saisi.online/")),
    (
        "console-css",
        PageSource::Remote("https://api.saisi.online/style.css"),
    ),
    (
        "download",
        PageSource::Remote("https://agent.saisi.online/"),
    ),
    // round-262 (user: extension unused): the Summrise Browser Control extension
    // (popup/options/terminal) was removed — its Embedded page entries went
    // with it. PAGES now covers the agent + deployed console/download only.
];

/// Max bytes returned per page — enough to see tokens/structure without
/// flooding the model with a multi-MB console bundle.
const MAX_PAGE_BYTES: usize = 64 * 1024;

fn parse_target(t: &str, default_port: u16) -> Result<(String, u16), DeviceError> {
    // host:port (default = the CONFIGURED server port) — always the LOCAL agent, never arbitrary.
    // The default used to be a hardcoded 18080 while the agent's port is `config.server.port`;
    // the sibling modules state the rule for exactly this reason (`src/tunnel.rs`: "a hardcoded
    // 18080 502s custom-port installs"). On a custom-port install the old default read a
    // STRANGER'S service on 18080 and presented it as the panel design.
    let (host, port) = match t.rsplit_once(':') {
        Some((h, p)) => (
            h.to_string(),
            p.parse::<u16>().map_err(|_| DeviceError::Internal {
                message: format!("bad port in target: {t}"),
            })?,
        ),
        None => (t.to_string(), default_port),
    };
    // Plugin audit MED: the comment promised "always the LOCAL agent" but
    // the host came straight from the caller — an internal GET-read + port
    // scan primitive from SYSTEM. Enforce it.
    if !matches!(host.as_str(), "127.0.0.1" | "localhost" | "::1") {
        return Err(DeviceError::InvalidParams {
            message: "design target must be the loopback agent (127.0.0.1[:port])".into(),
        });
    }
    Ok((host, port))
}

/// Remove the device token from anything `page_view` returns.
///
/// REDACT BY VALUE. This used to walk the literal `__PANEL_TOKEN__` and replace whatever sat
/// between the next two quotes. That did redact the injected line in the panel HTML — and it
/// ALSO silently rewrote any file that merely MENTIONS the pattern: `page_view(page="panel-js")`
/// returned panel.js with `window.__PANEL_TOKEN__)||""` turned into `…||"<redacted>"`, a
/// semantic edit of the very source the tool exists to show (measured on the shipped bundle:
/// 613,677 bytes returned for a 613,667-byte file). The same walk also MISSED the token in
/// other shapes — `{"__PANEL_TOKEN__":"tok"}` came back with the value intact — because
/// guessing at a shape is not redaction. Knowing the value is.
///
/// The pattern walk survives as a FALLBACK only, for a config with no usable token, where a
/// heuristic that may over-redact beats handing back a live credential.
fn redact_secrets(s: &str, token: Option<&str>) -> (String, usize) {
    if let Some(t) = token.filter(|t| !t.is_empty()) {
        let n = s.matches(t).count();
        return (s.replace(t, "<redacted>"), n);
    }
    let walked = redact_by_pattern(s);
    let n = usize::from(walked != s);
    (walked, n)
}

/// The heuristic fallback: replace a `name = "value"` assignment (e.g. the panel's injected
/// window.__PANEL_TOKEN__) with `<redacted>`. Prefer [`redact_secrets`] with a real token.
fn redact_by_pattern(s: &str) -> String {
    const PATTERN: &str = "__PANEL_TOKEN__";
    if !s.contains(PATTERN) {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(idx) = rest.find(PATTERN) {
        out.push_str(&rest[..idx]);
        rest = &rest[idx..];
        let after = &rest[PATTERN.len()..];
        // Find the opening quote of the value, then the closing quote.
        match after.find('"') {
            Some(q) => match after[q + 1..].find('"') {
                Some(c) => {
                    let end = q + 1 + c;
                    out.push_str(&rest[..PATTERN.len()]);
                    out.push_str(&after[..=q]);
                    out.push_str("<redacted>");
                    out.push('"');
                    rest = &after[end + 1..];
                }
                None => {
                    out.push_str(rest);
                    rest = "";
                    break;
                }
            },
            None => {
                out.push_str(rest);
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

/// `page_view` — fetch a Summrise page's HTML/CSS so the AI can read its design.
///
/// The device has no browser, so "seeing" the design means reading the source.
/// Local pages: / (status), /panel/ (terminal panel HTML), /panel/panel.js,
/// /panel/panel.css. Remote pages: the console (gateway) and download site
/// (index worker) so production design is inspectable.
///
/// ONE argument (`config`) where this used to take four boot clones: the console base, the download site,
/// the token to redact and the local port are read from the LIVE config at the point of use.
pub fn page_view(config: ConfigHandle) -> ToolDef {
    ToolDef::new(
        "page_view",
        "View a Summrise page's design by fetching its HTML/CSS. \
         Pages: status (/), panel (/panel/), panel-js, panel-css, panel-html, \
         console (gateway page), console-css (gateway style.css), console-js, \
         download (download site). \
         Returns up to 64KB of source — read the CSS tokens (--accent, --bg, \
         radii, glass) and HTML structure to evaluate the design. `target` is \
         LOOPBACK ONLY (the agent's own port by default); the remote pages come \
         from the configured console/download URL, and fail explicitly when \
         that is not configured.",
        json!({
            "type": "object",
            "properties": {
                "page": {
                    "type": "string",
                    "enum": ["status", "panel", "panel-js", "panel-css", "panel-html",
                             "console", "console-css", "console-js", "download"],
                    "description": "Which page to fetch."
                },
                "target": {
                    "type": "string",
                    "description": "host:port to fetch from (default 127.0.0.1:18080)."
                }
            },
            "required": ["page"]
        }),
        move |params: Value| {
            // The HANDLE is cloned into the async block (an Arc clone, nothing more): the values it reads
            // are read there, per call, so a settings change or a token rotation between two calls is
            // visible to the second one. Cloning the handle out here — rather than moving it — keeps the
            // outer closure `Fn`, which the tool registry requires.
            let config = config.clone();
            async move {
                // `page` is REQUIRED by the schema. Defaulting a missing or wrong-typed value to
                // "panel" answered a question nobody asked with a page they did not name.
                let page = params.get("page").and_then(|v| v.as_str()).ok_or_else(|| {
                    DeviceError::InvalidParams {
                        message: "page is required — one of the names in the enum".into(),
                    }
                })?;
                let local_port = config.local_port();
                let target = params
                    .get("target")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("127.0.0.1:{local_port}"));
                let (host, port) = parse_target(&target, local_port)?;
                let source = PAGES
                    .iter()
                    .find(|(n, _)| *n == page)
                    .map(|(_, s)| s)
                    .ok_or_else(|| DeviceError::Internal {
                        message: format!("unknown page: {page}"),
                    })?;
                // saisi decouple: remote pages need the configured base; when
                // unset, fail explicitly (never fall back to a hardcoded host).
                // The base is read from the LIVE config HERE, where the URL is built — so a console the
                // operator has just moved away from is not the console this call fetches.
                let remote_url = match page {
                    "console-js" => {
                        let base = config.console_url().ok_or_else(|| DeviceError::Internal {
                        message: "console page requires platform.console_url (not configured — purely local install)".into(),
                    })?;
                        Some(format!("{}/app.js", base.trim_end_matches('/')))
                    }
                    "console" => {
                        let base = config.console_url().ok_or_else(|| DeviceError::Internal {
                        message: "console page requires platform.console_url (not configured — purely local install)".into(),
                    })?;
                        Some(base.trim_end_matches('/').to_string())
                    }
                    "console-css" => {
                        let base = config.console_url().ok_or_else(|| DeviceError::Internal {
                        message: "console page requires platform.console_url (not configured — purely local install)".into(),
                    })?;
                        Some(format!("{}/style.css", base.trim_end_matches('/')))
                    }
                    "download" => {
                        let base = config.download_url().ok_or_else(|| DeviceError::Internal {
                        message: "download page requires platform.download_url (not configured — purely local install)".into(),
                    })?;
                        Some(base.trim_end_matches('/').to_string())
                    }
                    _ => None,
                };

                let url = match source {
                    PageSource::Local(p) => format!("http://{host}:{port}{p}"),
                    PageSource::Remote(u) => remote_url.unwrap_or_else(|| u.to_string()),
                };
                // Local static pages need no auth (the panel HTML is public;
                // the token is injected server-side, the static file itself has
                // no secrets). Remote pages are public console/download sites.
                let mut resp = reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(10))
                    // A redirect is not this tool's business. The loopback gate pins the FIRST
                    // hop; reqwest's default (10 hops) would carry the fetch wherever a 30x
                    // pointed — including off this box — and hand the body back as "the page".
                    // Same rule the gateway's device dialler adopted (`redirect: "manual"`).
                    .redirect(reqwest::redirect::Policy::none())
                    .build()
                    .map_err(|e| DeviceError::Internal {
                        message: format!("client: {e}"),
                    })?
                    .get(&url)
                    .send()
                    .await
                    .map_err(|e| DeviceError::Internal {
                        message: format!("fetch {url}: {e}"),
                    })?;
                // A NON-2xx IS NOT THE PAGE. It used to be returned as `content` with no status
                // field, so a 404/500 — or the 530 page d1's tunnel served for days — was
                // presented to the model as the design of the page it asked for.
                let status = resp.status();
                if !status.is_success() {
                    return Err(DeviceError::Internal {
                        message: format!(
                            "{url} answered HTTP {status} — this is not the page's source"
                        ),
                    });
                }
                // BOUND THE READ, not only the return. `resp.text()` materialises the WHOLE body
                // before the 64KB clip applies, so a chunked response (or a large file) was read
                // into memory on a SYSTEM service box with only the 10s timeout in the way.
                let mut buf: Vec<u8> = Vec::with_capacity(MAX_PAGE_BYTES);
                let mut truncated = false;
                loop {
                    match resp.chunk().await {
                        Ok(Some(chunk)) => {
                            if buf.len() + chunk.len() > MAX_PAGE_BYTES {
                                let room = MAX_PAGE_BYTES - buf.len();
                                buf.extend_from_slice(&chunk[..room]);
                                // PROOF, not a guess: bytes are still arriving.
                                truncated = true;
                                break;
                            }
                            buf.extend_from_slice(&chunk);
                        }
                        Ok(None) => break,
                        Err(e) => {
                            return Err(DeviceError::Internal {
                                message: format!("read {url}: {e}"),
                            })
                        }
                    }
                }
                let body = String::from_utf8_lossy(&buf).into_owned();
                let read = body.len();
                // Redact the device token before returning: the panel HTML embeds it
                // (window.__PANEL_TOKEN__ = "<token>"), and a design review must never see it —
                // a leaked review output is device control. BY VALUE: see `redact_secrets`.
                // THE TOKEN IS READ HERE, after the fetch: what must be scrubbed is the value the device
                // holds NOW. A clone taken at boot holds the PREVIOUS secret, so a rotation leaves it
                // redacting nothing — and `redactions: 0` is a success report, not a failure.
                let (redacted, redactions) =
                    redact_secrets(&body, config.device_token().as_deref());
                // char-boundary-safe truncation: slicing a String at a fixed byte
                // index PANICS when it lands inside a multi-byte UTF-8 char.
                let text = if truncated {
                    crate::text::clip(&redacted, MAX_PAGE_BYTES)
                } else {
                    &redacted[..]
                };

                Ok(json!({
                    "page": page,
                    "url": url,
                    // What was READ and kept — no longer the whole body's length, because the
                    // whole body is no longer read.
                    "bytes": read,
                    "truncated": truncated,
                    "redactions": redactions,
                    "content": text,
                }))
            }
        },
    )
}

#[cfg(test)]
mod design_tests {
    //! round-383: the loopback gate, token redaction, and the page table
    //! had zero tests — including a REAL drift the tests caught on write:
    //! the schema enum still advertised the round-262-deleted extension
    //! pages (every one a guaranteed "unknown page" error).
    use super::*;
    use crate::state::AppState;
    use summrise_agent_core::Config;

    /// A handle over a LIVE config of this test's own: nothing else holds this lock, so the values are
    /// whatever the test sets them to. (`ConfigHandle::new` is crate-private — in production the only way
    /// to obtain a handle is `AppState::config_handle()`, which is the point: a handle always reads a config
    /// that something can still change.)
    fn handle_of(
        console_url: Option<&str>,
        download_url: Option<&str>,
        token: Option<&str>,
        port: u16,
    ) -> ConfigHandle {
        let mut cfg = Config::default();
        cfg.platform.console_url = console_url.map(str::to_string);
        cfg.platform.download_url = download_url.map(str::to_string);
        cfg.server.device_token = token.map(str::to_string);
        cfg.server.port = port;
        ConfigHandle::new(std::sync::Arc::new(std::sync::RwLock::new(cfg)))
    }

    #[test]
    fn parse_target_allows_loopback_only() {
        // The default port is the CALLER's configured port, not a hardcoded 18080: on a
        // custom-port install the old default read a stranger's service on 18080.
        assert_eq!(
            parse_target("127.0.0.1", 18080).unwrap(),
            ("127.0.0.1".into(), 18080)
        );
        assert_eq!(parse_target("127.0.0.1", 7740).unwrap().1, 7740);
        assert_eq!(
            parse_target("127.0.0.1:9999", 7740).unwrap(),
            ("127.0.0.1".into(), 9999)
        );
        assert_eq!(
            parse_target("localhost:1", 18080).unwrap(),
            ("localhost".into(), 1)
        );
        for bad in [
            "example.com",
            "192.168.1.1",
            "10.0.0.1:80",
            "::1",
            "[::1]",
            "",
        ] {
            assert!(
                parse_target(bad, 18080).is_err(),
                "{bad:?} must be rejected"
            );
        }
        assert!(parse_target("127.0.0.1:notaport", 18080).is_err());
    }

    #[test]
    fn redaction_is_by_value_and_leaves_a_mere_mention_alone() {
        // THE CORRUPTION CASE, measured on the shipped bundle before this was fixed: panel.js
        // mentions the pattern with an EMPTY value while the token lives elsewhere in the same
        // document. The old pattern walk rewrote that line into `…||"<redacted>"` — a semantic
        // edit of the very source this tool exists to show. Value redaction must not touch a
        // byte of it.
        let panel_js = "window.__PANEL_TOKEN__)||\"\"; var other = \"TOKENVALUE\";";
        let (out, n) = redact_secrets(panel_js, Some("TOKENVALUE"));
        assert_eq!(n, 1, "{out}");
        assert!(!out.contains("TOKENVALUE"), "the token survived: {out}");
        assert!(
            out.contains("window.__PANEL_TOKEN__)||\"\""),
            "the source was rewritten: {out}"
        );

        // THE LEAK CASE: a shape the pattern walk got wrong (it replaced between the wrong
        // quotes and left the value in place).
        let jsony = "{\"__PANEL_TOKEN__\":\"TOKENVALUE\"}";
        let (out, n) = redact_secrets(jsony, Some("TOKENVALUE"));
        assert_eq!(n, 1);
        assert!(!out.contains("TOKENVALUE"), "the value leaked: {out}");

        // Several occurrences, all scrubbed.
        let twice = format!("{panel_js}{panel_js}");
        assert_eq!(redact_secrets(&twice, Some("TOKENVALUE")).1, 2);

        // No usable token: the pattern walk is the FALLBACK, not the rule.
        let (out, n) = redact_secrets("a __PANEL_TOKEN__=\"x\";", None);
        assert_eq!(n, 1);
        assert!(out.contains("<redacted>"), "{out}");

        // Nothing to do.
        let (out, n) = redact_secrets("<html>hi</html>", Some("TOKENVALUE"));
        assert_eq!((out.as_str(), n), ("<html>hi</html>", 0));
    }

    #[test]
    fn page_table_is_unique_wellformed_and_matches_schema() {
        let mut names: Vec<&str> = PAGES.iter().map(|(n, _)| *n).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), PAGES.len(), "page names must be unique");
        for (name, src) in PAGES {
            match src {
                PageSource::Local(p) => assert!(p.starts_with('/'), "{name}: {p}"),
                PageSource::Remote(u) => assert!(u.starts_with("https://"), "{name}: {u}"),
            }
        }
        // Schema enum parity: every advertised page must resolve (and every
        // resolvable page must be advertised) — the round-262 deletion left
        // 10 dead enum entries that always errored.
        let def = page_view(handle_of(None, None, None, 18080));
        let enums: Vec<String> = def.input_schema["properties"]["page"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect();
        let mut table: Vec<String> = PAGES.iter().map(|(n, _)| n.to_string()).collect();
        let mut enums_sorted = enums.clone();
        enums_sorted.sort();
        table.sort();
        assert_eq!(enums_sorted, table, "schema enum must equal PAGES");
    }

    #[tokio::test]
    async fn page_view_truncates_and_redacts_over_http() {
        // End-to-end through the tool handler against a local stub: a 100 KiB
        // CJK body (multi-byte truncation hazard) carrying an injected token.
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut body = String::from(r#"<script>window.__PANEL_TOKEN__="tok123";</script>"#);
        body.push_str(&"界".repeat(40 * 1024));
        let body_bytes = body.as_bytes().to_vec();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            let mut buf = vec![0u8; 8192];
            let mut req = Vec::new();
            loop {
                let n = sock.read(&mut buf).await.unwrap_or(0);
                if n == 0 {
                    break;
                }
                req.extend_from_slice(&buf[..n]);
                if req.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            let resp = format!(
                "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                body_bytes.len()
            );
            let _ = sock.write_all(resp.as_bytes()).await;
            let _ = sock.write_all(&body_bytes).await;
        });

        let def = page_view(handle_of(None, None, Some("tok123"), 18080));
        let out = def
            .handler
            .call(serde_json::json!({ "page": "panel-js", "target": format!("127.0.0.1:{port}") }))
            .await
            .unwrap();
        assert_eq!(out["truncated"], true);
        // `bytes` is what was READ and kept (the read is bounded now), so it is at most the cap.
        assert!(
            out["bytes"].as_u64().unwrap() <= MAX_PAGE_BYTES as u64,
            "{}",
            out["bytes"]
        );
        assert_eq!(out["redactions"], 1, "the injected token is a redaction");
        assert!(out["url"]
            .as_str()
            .unwrap()
            .contains(&format!("127.0.0.1:{port}")));
        let content = out["content"].as_str().unwrap();
        assert!(content.len() <= MAX_PAGE_BYTES);
        // THE MULTI-BYTE HAZARD, asserted where it can actually fail. The line that used to sit
        // here — `content.is_char_boundary(content.len())` — is true for EVERY &str (the end of
        // a String is always a boundary), so it could never fail and was not the CJK evidence
        // it read as. Strip the ASCII prefix: the remainder must be a whole number of 3-byte
        // chars, which is exactly what a byte-slicing clip would break.
        let cjk = &content[content.find('界').unwrap_or(content.len())..];
        assert_eq!(
            cjk.len() % 3,
            0,
            "a CJK char was cut in half: {} bytes",
            cjk.len()
        );
        assert!(!cjk.is_empty(), "the CJK tail vanished");
        assert!(!content.contains("tok123"), "injected token must not leak");
    }

    #[tokio::test]
    async fn the_redactor_follows_a_token_rotated_after_boot() {
        // The registry used to hand DesignPlugin a CLONE of `device_token` taken once at boot, so rotating
        // the token left the redactor keyed on the OLD value: a payload carrying the NEW one came back with
        // the live secret intact AND `redactions: 0` — a redactor that silently stopped redacting, reporting
        // success. The rotation below is the one `web/mod.rs`'s
        // `token_rotation_takes_effect_on_api_and_mcp_gate` performs (snapshot → update_config), and the tool
        // asked is the one the REGISTRY publishes — so this pins the whole seam (the handle `state.rs` builds
        // for `build_registry`, and the plugin that reads it), not only `page_view`.
        let mut cfg = Config::default();
        cfg.server.device_token = Some("boot-token".into());
        let st = AppState::new(cfg);
        let tool = st
            .plugin_registry
            .find_tool("page_view")
            .expect("the registry must publish page_view");

        let mut cfg = st.config_snapshot();
        cfg.server.device_token = Some("rotated-token".into());
        st.update_config(cfg, false).unwrap();

        // The panel HTML as the agent actually injects it — carrying the ROTATED value.
        let port = stub(
            "HTTP/1.1 200 OK",
            String::new(),
            br#"<script>window.__PANEL_TOKEN__="rotated-token";</script>"#.to_vec(),
        )
        .await;
        let out = tool
            .handler
            .call(serde_json::json!({ "page": "panel-js", "target": format!("127.0.0.1:{port}") }))
            .await
            .unwrap();
        assert_eq!(
            out["redactions"], 1,
            "the NEW token must be redacted: {out}"
        );
        assert!(
            !out["content"].as_str().unwrap().contains("rotated-token"),
            "the live secret leaked: {out}"
        );
    }

    #[tokio::test]
    async fn page_view_follows_a_console_url_changed_through_the_settings_path() {
        // PUT /api/settings merges onto `config_snapshot()` and writes it back through `update_config` —
        // exactly the two steps below — while /api/settings GET reads the new value back from the same
        // snapshot. A plugin holding a boot clone kept fetching the console the operator had moved AWAY
        // from: here that is a dead port, so the call fails outright instead of silently reading the old
        // console. Port 1 is used as the dead one: nothing can listen there.
        let mut cfg = Config::default();
        cfg.platform.console_url = Some("http://127.0.0.1:1".into());
        let st = AppState::new(cfg);
        let tool = st
            .plugin_registry
            .find_tool("page_view")
            .expect("the registry must publish page_view");

        let port = stub(
            "HTTP/1.1 200 OK",
            String::new(),
            b"<html>the new console</html>".to_vec(),
        )
        .await;
        let mut cfg = st.config_snapshot();
        cfg.platform.console_url = Some(format!("http://127.0.0.1:{port}"));
        st.update_config(cfg, false).unwrap();

        let out = tool
            .handler
            .call(serde_json::json!({ "page": "console" }))
            .await
            .unwrap();
        assert_eq!(out["url"], format!("http://127.0.0.1:{port}"));
        assert!(
            out["content"].as_str().unwrap().contains("the new console"),
            "the OLD console was fetched: {out}"
        );
    }

    #[tokio::test]
    async fn page_view_unknown_page_and_remote_without_config_fail_closed() {
        let def = page_view(handle_of(None, None, None, 18080));
        let err = def
            .handler
            .call(serde_json::json!({ "page": "nope" }))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("unknown page"), "{err:?}");
        let err = def
            .handler
            .call(serde_json::json!({ "page": "console" }))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("console_url"), "{err:?}");
    }

    /// A one-shot HTTP stub: answer `status_line` (+ `extra` headers) and `body`, return the port.
    async fn stub(status_line: &str, extra: String, body: Vec<u8>) -> u16 {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let status_line = status_line.to_string();
        tokio::spawn(async move {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            let mut buf = vec![0u8; 8192];
            let mut req = Vec::new();
            loop {
                let n = sock.read(&mut buf).await.unwrap_or(0);
                if n == 0 {
                    break;
                }
                req.extend_from_slice(&buf[..n]);
                if req.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            let head = format!(
                "{status_line}\r\ncontent-length: {}\r\n{extra}connection: close\r\n\r\n",
                body.len()
            );
            let _ = sock.write_all(head.as_bytes()).await;
            let _ = sock.write_all(&body).await;
        });
        port
    }

    #[tokio::test]
    async fn a_non_2xx_is_an_error_not_a_page() {
        // The page a device actually got during the multi-day tunnel outage was a 530 error
        // page. It used to be returned as `content` with no status — the model would have read
        // it as "the design of the download site".
        let port = stub(
            "HTTP/1.1 404 Not Found",
            String::new(),
            b"<html>nope</html>".to_vec(),
        )
        .await;
        let def = page_view(handle_of(None, None, None, 18080));
        let err = def
            .handler
            .call(serde_json::json!({ "page": "panel-js", "target": format!("127.0.0.1:{port}") }))
            .await
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("404"), "{msg}");
        assert!(
            !msg.contains("nope"),
            "the failure must not carry the body: {msg}"
        );
    }

    #[tokio::test]
    async fn a_redirect_is_not_followed() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        // The redirect target: records any contact, so "did we follow it" is a fact, not a guess.
        let hits = Arc::new(AtomicUsize::new(0));
        let inner = hits.clone();
        let target = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let target_port = target.local_addr().unwrap().port();
        tokio::spawn(async move {
            if target.accept().await.is_ok() {
                inner.fetch_add(1, Ordering::SeqCst);
            }
        });
        let port = stub(
            "HTTP/1.1 302 Found",
            format!("location: http://127.0.0.1:{target_port}/\r\n"),
            Vec::new(),
        )
        .await;
        let def = page_view(handle_of(None, None, None, 18080));
        let err = def
            .handler
            .call(serde_json::json!({ "page": "panel-js", "target": format!("127.0.0.1:{port}") }))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("302"), "{err:?}");
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert_eq!(
            hits.load(Ordering::SeqCst),
            0,
            "the redirect target was contacted — the loopback gate pins only the first hop"
        );
    }

    #[tokio::test]
    async fn a_missing_page_is_refused_rather_than_defaulted() {
        let def = page_view(handle_of(None, None, None, 18080));
        let err = def
            .handler
            .call(serde_json::json!({ "page": 3 }))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("page is required"), "{err:?}");
    }
}
