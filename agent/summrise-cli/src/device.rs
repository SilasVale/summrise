//! THE CLI'S DOOR TO THE DEVICE IT MANAGES, AND TO THE GATEWAY IT REGISTERS WITH.
//!
//! Two very different peers, one module, because the DECISION each one makes is the same three-way
//! one and the TypeScript made it twice: *the token is missing*, *the peer did not answer*, *the peer
//! answered something that is not JSON*. Every caller in the CLI has to tell those apart — `monitor`
//! prints the reason on stderr and exits non-zero rather than a JSON body pretending everything is
//! fine, and `tunnel install` falls back to the interactive login when the key exchange fails.
//!
//! **THE ANSWERS ARE A STRUCT AND NOT A `Result`.** `{ok:false, error}` in the TypeScript carries a
//! SENTENCE the caller prints; a Rust `Err(String)` would be the same fact in a shape that makes
//! `?` look natural, and `?` is exactly what a caller must not do — every call site here has a
//! different recovery, and one of them (`monitor add`) continues past a failed probe on purpose.
//!
//! Ported from `deviceApi` and `apiPost` in `agent/summrise-agent-npm/src/summrise.ts`.

use crate::config::{agent_port, device_token};
use crate::endpoints::DEFAULT_API_BASE;
use crate::host::Host;
use crate::monitors::ascii_json;
use serde_json::Value;

/// What a peer answered: the body, or the sentence that explains why there is none.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Answer {
    pub ok: bool,
    pub body: Option<Value>,
    pub error: Option<String>,
}

impl Answer {
    pub fn ok(body: Value) -> Self {
        Answer {
            ok: true,
            body: Some(body),
            error: None,
        }
    }

    pub fn fail(msg: impl Into<String>) -> Self {
        Answer {
            ok: false,
            body: None,
            error: Some(msg.into()),
        }
    }

    /// The sentence a caller prints, whatever the failure was.
    pub fn error(&self) -> &str {
        self.error.as_deref().unwrap_or("the peer did not answer")
    }
}

/// The bearer token out of `etc\config.yaml`, or the reason there is none.
///
/// Split out because the SENTENCE is a contract: it names the file the token is expected in, which
/// is the only actionable thing about it.
pub fn device_api(
    host: &dyn Host,
    etc_dir: &str,
    method: &str,
    path: &str,
    body: Option<&Value>,
) -> Answer {
    let Some(token) = device_token(host, etc_dir) else {
        return Answer::fail(format!(
            "no device token in {}\\config.yaml",
            etc_dir.trim_end_matches('\\')
        ));
    };
    let port = agent_port(host, etc_dir);
    let mut argv: Vec<String> = vec![
        "curl".into(),
        "-sS".into(),
        "-m".into(),
        "15".into(),
        "-X".into(),
        method.into(),
        "-H".into(),
        format!("Authorization: Bearer {token}"),
    ];
    if let Some(b) = body {
        argv.push("-H".into());
        argv.push("content-type: application/json".into());
        argv.push("-d".into());
        argv.push(ascii_json(b, None));
    }
    argv.push(format!("http://127.0.0.1:{port}{path}"));
    let r = host.run(&argv, Some(20_000));
    if r.status != Some(0) {
        // `r.status == None` is a spawn that never happened (no curl on PATH); it is NOT an exit
        // code, and the TypeScript distinguishes it in the parentheses.
        let why = if r.status.is_none() {
            " (curl could not be run)".to_string()
        } else {
            String::new()
        };
        return Answer::fail(format!("device unreachable on 127.0.0.1:{port}{why}"));
    }
    match serde_json::from_str::<Value>(r.stdout.trim()) {
        Ok(v) => Answer::ok(v),
        // A 401 page, an HTML error from a proxy, an empty body: all the same fact to the caller.
        Err(_) => Answer::fail("device sent something that is not JSON"),
    }
}

/// `POST <gateway>/<path>` with a JSON body — the tunnel-token exchange.
///
/// `Result` here and not [`Answer`], because the TypeScript THROWS: its one caller wraps the call in
/// a try/catch whose whole purpose is to fall back to the interactive login, and a shape that let
/// the caller ignore the failure silently would lose that.
pub fn api_post(host: &dyn Host, path: &str, body: &Value) -> Result<Value, String> {
    let base = host
        .env("SUMMRISE_API_BASE")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_API_BASE.to_string());
    let argv: Vec<String> = vec![
        "curl".into(),
        "-sS".into(),
        "-m".into(),
        "30".into(),
        "-X".into(),
        "POST".into(),
        "-H".into(),
        "content-type: application/json".into(),
        "-d".into(),
        serde_json::to_string(body).unwrap_or_default(),
        format!("{base}{path}"),
    ];
    let r = host.run(&argv, Some(32_000));
    if r.status != Some(0) {
        return Err(format!("gateway unreachable: {}", r.stderr.trim()));
    }
    let out = r.stdout.trim();
    serde_json::from_str::<Value>(out).map_err(|_| {
        // The 120-character slice is the TypeScript's, and it is deliberate: an HTML error page is
        // kilobytes, and the useful part is always at the front.
        let head: String = out.chars().take(120).collect();
        format!("gateway bad response: {head}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::config_path;
    use crate::testing::FakeHost;
    use serde_json::json;

    fn configured() -> FakeHost {
        FakeHost::new().with_file(
            &config_path("D:\\Summrise\\etc").to_string_lossy(),
            "server:\n  port: 18080\n  device_token: tok.ABC-1\n",
        )
    }

    /// THE TOKEN IS THE FIRST QUESTION, and its absence is a sentence naming the file — not a
    /// generic "unreachable", which sends an operator to look at the network for a config problem.
    #[test]
    fn a_missing_token_names_the_file_it_is_expected_in() {
        let host = FakeHost::new();
        let a = device_api(&host, "D:\\Summrise\\etc", "GET", "/api/monitors", None);
        assert!(!a.ok);
        assert!(
            a.error()
                .contains("no device token in D:\\Summrise\\etc\\config.yaml"),
            "{}",
            a.error()
        );
        assert!(host.runs().is_empty(), "and it must not run curl at all");
    }

    /// The request is curl ARGV — no shell, and the body goes through the ASCII escaper, which is
    /// the encoding boundary a console code page would otherwise mangle.
    #[test]
    fn a_body_is_carried_as_the_escaped_json_argument() {
        let host = configured().script_runs(vec![crate::host::RunResult {
            status: Some(0),
            stdout: "{\"ok\":true}".into(),
            ..Default::default()
        }]);
        let a = device_api(
            &host,
            "D:\\Summrise\\etc",
            "POST",
            "/api/monitors/add",
            Some(&json!({"host": "d1", "port": 22})),
        );
        assert!(a.ok);
        let argv = &host.runs()[0];
        assert_eq!(argv[0], "curl");
        assert!(argv.contains(&"Authorization: Bearer tok.ABC-1".to_string()));
        assert!(argv
            .iter()
            .any(|x| x.starts_with("http://127.0.0.1:18080/")));
        let body = argv
            .iter()
            .position(|x| x == "-d")
            .map(|i| argv[i + 1].clone());
        assert_eq!(body.as_deref(), Some("{\"host\":\"d1\",\"port\":22}"));
    }

    /// A NON-ZERO EXIT IS "UNREACHABLE", AND A BODY THAT IS NOT JSON IS A DIFFERENT SENTENCE. The
    /// second one is the case the TypeScript added after a proxy answered a login page.
    #[test]
    fn unreachable_and_not_json_are_two_different_answers() {
        let host = configured().script_runs(vec![crate::host::RunResult {
            status: Some(7),
            ..Default::default()
        }]);
        let a = device_api(&host, "D:\\Summrise\\etc", "GET", "/api/monitors", None);
        assert!(a.error().contains("device unreachable on 127.0.0.1:18080"));

        let host = configured().script_runs(vec![crate::host::RunResult {
            status: Some(0),
            stdout: "<html>login</html>".into(),
            ..Default::default()
        }]);
        let a = device_api(&host, "D:\\Summrise\\etc", "GET", "/api/monitors", None);
        assert_eq!(a.error(), "device sent something that is not JSON");
    }

    /// The gateway POST is a `Result`, and its two messages are the TypeScript's own: a transport
    /// failure and a bad body are not the same, and `tunnel install` falls back on the first.
    #[test]
    fn the_gateway_post_reports_transport_and_body_failures_differently() {
        let host = FakeHost::new().script_runs(vec![crate::host::RunResult {
            status: Some(6),
            stderr: "Could not resolve host".into(),
            ..Default::default()
        }]);
        let e = api_post(&host, "/api/install/tunnel-token", &json!({"key": "k"})).unwrap_err();
        assert_eq!(e, "gateway unreachable: Could not resolve host");

        let host = FakeHost::new().script_runs(vec![crate::host::RunResult {
            status: Some(0),
            stdout: "not json".into(),
            ..Default::default()
        }]);
        let e = api_post(&host, "/api/install/tunnel-token", &json!({"key": "k"})).unwrap_err();
        assert_eq!(e, "gateway bad response: not json");
    }
}
