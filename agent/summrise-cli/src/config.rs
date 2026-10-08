//! THE CONFIG IT READS: `etc\config.yaml`, without a YAML dependency.
//!
//! Ported from `parseAgentPort` / `parseDeviceToken` / `agentPort` / `parseTargetArg`. The agent
//! WRITES this file; the CLI only ever reads three things out of it (the bind port, the device
//! token, and — through the tunnel writer — nothing at all). A line scan is what the TypeScript
//! does, and it is enough for all three, so the port keeps it rather than taking a YAML crate whose
//! behaviour on a file the AGENT wrote would be a second opinion.

use crate::host::Host;
use std::path::PathBuf;

/// The canonical port when the config is absent, unreadable or silent.
pub const DEFAULT_AGENT_PORT: u16 = 18080;

/// `server.port` out of `<dir>/config.yaml` — the first `port:` under a TOP-LEVEL `server:`.
///
/// Strict on purpose: `serial:\n  port: 1234` must NOT answer (the oracle pins it), a `0` is
/// ephemeral and refused, and anything outside 1..65535 is not a port.
pub fn parse_agent_port(yaml: &str) -> Option<u16> {
    let mut in_server = false;
    for raw in yaml.split('\n') {
        let line = raw.trim_end_matches(['\r', ' ', '\t']);
        // A line starting at column 0 opens a new top-level key.
        if !line.starts_with(char::is_whitespace) && !line.is_empty() {
            in_server = line.trim_start().starts_with("server") && {
                let rest = &line.trim_start()["server".len()..];
                rest.trim_start().starts_with(':')
            };
        }
        if !in_server {
            continue;
        }
        if let Some(n) = port_line(line) {
            return if n > 0 && n < 65536 {
                Some(n as u16)
            } else {
                None
            };
        }
    }
    None
}

/// `^\s*port\s*:\s*"?(\d{1,5})"?\s*(#.*)?$` — the port line and nothing else.
fn port_line(line: &str) -> Option<u32> {
    let t = line.trim_start();
    if !t.starts_with("port") {
        return None;
    }
    let rest = t["port".len()..].trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    let rest = rest.strip_prefix('"').unwrap_or(rest);
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() || digits.len() > 5 {
        return None;
    }
    let after = &rest[digits.len()..];
    let after = after.strip_prefix('"').unwrap_or(after);
    let after = after.trim();
    if !(after.is_empty() || after.starts_with('#')) {
        return None;
    }
    digits.parse::<u32>().ok()
}

/// The device token, read the same way: a line scan, no YAML dependency. Absent means "cannot talk
/// to the device API", which the callers report as a state rather than as a crash.
pub fn parse_device_token(yaml: &str) -> Option<String> {
    let mut in_server = false;
    for raw in yaml.split('\n') {
        let line = raw.trim_end_matches(['\r', ' ', '\t']);
        if !line.starts_with(char::is_whitespace) && !line.is_empty() {
            in_server = line.trim_start().starts_with("server")
                && line.trim_start()["server".len()..]
                    .trim_start()
                    .starts_with(':');
        }
        if !in_server {
            continue;
        }
        let t = line.trim_start();
        if let Some(rest) = t.strip_prefix("device_token") {
            let rest = rest.trim_start();
            if let Some(rest) = rest.strip_prefix(':') {
                let rest = rest.trim_start();
                let rest = rest.strip_prefix('"').unwrap_or(rest);
                let tok: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
                    .collect();
                if !tok.is_empty() {
                    return Some(tok);
                }
            }
        }
    }
    None
}

/// `agentPort()`: the config's port, or the canonical one. A missing file is the ordinary case —
/// a fresh install has no config yet, because the AGENT writes defaults on first boot.
pub fn agent_port(host: &dyn Host, dir: &str) -> u16 {
    host.read_string(&config_path(dir))
        .ok()
        .and_then(|t| parse_agent_port(&t))
        .unwrap_or(DEFAULT_AGENT_PORT)
}

/// The device token from `<dir>/config.yaml`, or `None`.
pub fn device_token(host: &dyn Host, dir: &str) -> Option<String> {
    host.read_string(&config_path(dir))
        .ok()
        .and_then(|t| parse_device_token(&t))
}

/// A `host:port[/path]` target: the id the DEVICE uses, which is what a probe is filed under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetArg {
    pub host: String,
    pub port: u16,
    pub path: String,
    pub id: String,
}

/// `<dir>\config.yaml` — the ONE place the config's file name is spelled, so the port, the fixtures
/// and a caller cannot disagree about it.
pub fn config_path(dir: &str) -> PathBuf {
    PathBuf::from(dir).join("config.yaml")
}

/// The `etc\tunnel.yml` body this CLI writes when `--tunnel` provisions a tunnel.
///
/// THE INGRESS ADDRESS AND THE LISTEN ADDRESS ARE CHOSEN IN TWO LANGUAGES, SO NOTHING IN EITHER ONE
/// CAN SEE THE OTHER, and they disagreed: this CLI wrote `http://127.0.0.2:<port>` while the
/// agent's own provisioning (`agent/src/tunnel.rs`) writes 127.0.0.1 and calls 127.0.0.2 "a dead
/// address (502)". One file, two writers, two answers — and the LIVE DEVICE settles it: `netstat`
/// shows the listener on 127.0.0.1:18080, and d1's own `tunnel.yml` says
/// `service: http://127.0.0.1:18080`.
///
/// `allow-remote-config: false` IS NOT OPTIONAL EITHER: cloudflared prefers a REMOTE config when one
/// exists, so a stale remote ingress keeps proxying to a dead address "no matter what tunnel.yml
/// says". The agent writes it deliberately; this CLI did not.
pub fn tunnel_yml(tunnel_id: &str, credentials_file: &str, hostname: &str, port: u16) -> String {
    [
        format!("tunnel: {tunnel_id}"),
        format!("credentials-file: {credentials_file}"),
        "allow-remote-config: false".to_string(),
        "ingress:".to_string(),
        format!("  - hostname: {hostname}"),
        format!("    service: http://127.0.0.1:{port}"),
        "  - service: http_status:404".to_string(),
        String::new(),
    ]
    .join("\n")
}

/// `host:port[/path]` -> the target id the device uses.
///
/// A path is part of the IDENTITY, so `192.168.1.1:80/` and `192.168.1.1:80` are two different
/// checks — which is why this cannot be a `split(':')`.
pub fn parse_target_arg(arg: &str) -> Option<TargetArg> {
    let s = arg.trim();
    let colon = s.find(':')?;
    let (host, rest) = s.split_at(colon);
    let rest = &rest[1..];
    if host.is_empty() || host.contains('/') || host.chars().any(|c| c.is_whitespace()) {
        return None;
    }
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() || digits.len() > 5 {
        return None;
    }
    let port: u32 = digits.parse().ok()?;
    if !(1..=65535).contains(&port) {
        return None;
    }
    let tail = &rest[digits.len()..];
    if !(tail.is_empty() || tail.starts_with('/')) {
        return None;
    }
    Some(TargetArg {
        host: host.to_string(),
        port: port as u16,
        path: tail.to_string(),
        id: format!("{host}:{port}{tail}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::FakeHost;

    /// Oracle: cli.test.mjs:579 — "parseAgentPort: server.port only, strict".
    #[test]
    fn parse_agent_port_reads_server_port_only() {
        assert_eq!(
            parse_agent_port("server:\n  host: \"0.0.0.0\"\n  port: 7740\n"),
            Some(7740)
        );
        assert_eq!(parse_agent_port("server:\n  port: 18080\n"), Some(18080));
        assert_eq!(
            parse_agent_port("server:\n  host: \"127.0.0.1\"\n"),
            None,
            "absent port"
        );
        assert_eq!(
            parse_agent_port("serial:\n  port: 1234\n"),
            None,
            "non-server section ignored"
        );
        assert_eq!(
            parse_agent_port("server:\n  port: 0\n"),
            None,
            "ephemeral rejected"
        );
        assert_eq!(
            parse_agent_port("server:\n  port: 99999\n"),
            None,
            "out of range rejected"
        );
        assert_eq!(parse_agent_port(""), None);
    }

    /// Oracle: cli.test.mjs:609 — "agentPort: reads dir config, defaults 18080".
    #[test]
    fn agent_port_reads_the_dir_config_and_defaults() {
        let missing = FakeHost::new();
        assert_eq!(agent_port(&missing, "/definitely/not/here"), 18080);
        let host =
            FakeHost::new().with_file("/tmp/summrise-port/config.yaml", "server:\n  port: 7740\n");
        assert_eq!(agent_port(&host, "/tmp/summrise-port"), 7740);
    }

    /// The tunnel ingress is the address the agent LISTENS on (see `tunnel_yml`), and the
    /// remote-config escape hatch is off.
    #[test]
    fn tunnel_yml_names_the_listening_address() {
        let host = format!("d1{}", crate::endpoints::DEVICE_HOST_SUFFIX);
        let body = tunnel_yml("tid", "creds.json", &host, 18080);
        assert!(body.contains("tunnel: tid"), "{body}");
        assert!(body.contains("credentials-file: creds.json"), "{body}");
        assert!(body.contains("allow-remote-config: false"), "{body}");
        assert!(body.contains(&format!("  - hostname: {host}")), "{body}");
        assert!(
            body.contains("service: http://127.0.0.1:18080"),
            "the ingress must be the address the agent listens on: {body}"
        );
        assert!(
            !body.contains("127.0.0.2"),
            "127.0.0.2 is a socket nobody holds: {body}"
        );
        assert!(body.contains("  - service: http_status:404"), "{body}");
        assert!(
            body.ends_with('\n'),
            "the file ends with a newline: {body:?}"
        );
    }

    /// The device token — the same line scan, and the same "absent is a state, not a crash" rule.
    #[test]
    fn parse_device_token_reads_the_server_section() {
        assert_eq!(
            parse_device_token("server:\n  device_token: abc.DEF-123_x\n").as_deref(),
            Some("abc.DEF-123_x")
        );
        assert_eq!(parse_device_token("serial:\n  device_token: nope\n"), None);
        assert_eq!(parse_device_token("server:\n  host: x\n"), None);
    }

    /// `host:port[/path]` — a path is part of the identity.
    #[test]
    fn parse_target_arg_keeps_the_path_in_the_id() {
        let t = parse_target_arg("192.168.1.1:80/health").unwrap();
        assert_eq!(t.host, "192.168.1.1");
        assert_eq!(t.port, 80);
        assert_eq!(t.path, "/health");
        assert_eq!(t.id, "192.168.1.1:80/health");
        assert_eq!(
            parse_target_arg("192.168.1.1:80").unwrap().id,
            "192.168.1.1:80"
        );
        assert_eq!(parse_target_arg("h:0"), None);
        assert_eq!(parse_target_arg("h:70000"), None);
        assert_eq!(parse_target_arg("no-port"), None);
    }
}
