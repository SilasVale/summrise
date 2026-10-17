//! `summrise connect <relay-url>` / `summrise disconnect` — THE SWITCH THAT PUTS A DEVICE BEHIND NAT ON THE
//! CONSOLE WITHOUT CLOUDFLARE.
//!
//! **THE RELAY'S OWN HEADER HAS DOCUMENTED THIS COMMAND SINCE THE RELAY WAS WRITTEN** ("The agent side is
//! `summrise connect <relay-url>` — see `agent/src/relay.rs`") and it did not exist: the only way to reach a
//! relay was to hand-edit `config.yaml` and know three key names. A documented command that is not there is
//! worse than an undocumented one, because it is what a reader trusts.
//!
//! **WHAT IT WRITES, AND WHY EACH KEY IS REQUIRED RATHER THAN OPTIONAL:**
//!
//!   * `server.relay_url` — the base the agent polls. Required; this is the whole command.
//!   * `server.relay_token` — the relay's shared secret, and **THE RELAY REFUSES EVERY CALLER WITHOUT ONE**
//!     (`relay.mjs`: "without it every agent is refused, which is the only safe default"), so a connect that
//!     omitted it would produce a device that looks configured and answers nothing. `--token` is therefore
//!     required too, unless the config already carries one (a URL change on a device already connected).
//!   * `server.relay_device` — the name the agent polls with, and **the name the console dials**
//!     (`devices.rs`'s `apply_transport`). Optional: an unnamed device polls the relay's DEFAULT device, which
//!     is a legitimate single-device setup and is exactly what every pre-fleet deployment does.
//!
//! **`disconnect` REMOVES THE THREE KEYS, WHICH IS THE HALF THAT MAKES THIS A SWITCH.** The console's record
//! loses its transport on the next self-registration (the device declares what it is), so removing `relay_url`
//! moves a device back to the tunnel — a one-way door would have been a trap on the day a relay host moved.
//!
//! **IT EDITS ONLY THOSE KEYS.** `config.yaml` belongs to the operator and holds keys this CLI has never heard
//! of; the edit is line-based and preserves every other line, its order and its comments, because a command
//! that rewrote the file would silently drop whatever it did not understand.

use crate::config::config_path;
use crate::dispatch::Outcome;
use crate::host::Host;
use crate::paths::Layout;

/// The three keys, spelled once. `server:` is the section they live in.
const SECTION: &str = "server";
const RELAY_URL: &str = "relay_url";
const RELAY_TOKEN: &str = "relay_token";
const RELAY_DEVICE: &str = "relay_device";

/// **THE URL RULE, AND IT IS A DECISION RATHER THAN A VALIDATION.** A relay URL must be absolute and carry a
/// scheme the agent's HTTP client understands; it must NOT carry a trailing slash (the client appends
/// `/agent/pull`, and a double slash is a different URL to some proxies) and must not carry a query or a
/// fragment (they would be carried into every poll). Letting a plausible-looking URL through is how a device
/// ends up retrying a typo forever, so the refusal names the shape that is wanted.
pub fn normalize_relay_url(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err(
            "a relay URL is required: summrise connect https://relay.example.com".to_string(),
        );
    }
    let lower = trimmed.to_ascii_lowercase();
    if !(lower.starts_with("https://") || lower.starts_with("http://")) {
        return Err(format!(
            "'{trimmed}' has no scheme — use https://relay.example.com (http:// only for a relay on this machine)"
        ));
    }
    if trimmed.contains('?') || trimmed.contains('#') {
        return Err(format!(
            "'{trimmed}' carries a query or fragment — the agent appends its own path to this base"
        ));
    }
    let host = trimmed
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or_default();
    if host.is_empty() || host.starts_with('/') {
        return Err(format!("'{trimmed}' has no host"));
    }
    Ok(trimmed.to_string())
}

/// **THE DEVICE-NAME RULE.** The name becomes ONE PERCENT-ENCODED PATH SEGMENT on the console's side
/// (`device.rs`'s `encode_path_segment`) and a query value on the agent's, so anything without a separator is
/// acceptable — but a blank name is not a name, and a `/` would silently address a different device at a
/// different path. The refusal says which character is the problem rather than restating the rule.
pub fn normalize_relay_device(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(
            "a device name cannot be blank — omit --name to use the relay's default device"
                .to_string(),
        );
    }
    if trimmed.contains('/') {
        return Err(format!(
            "'{trimmed}' contains a slash — the name is one path segment on the relay"
        ));
    }
    Ok(trimmed.to_string())
}

/// Set ONE key in a `server:` section, preserving everything else: the other lines, their order and their
/// comments.
///
/// **THE FOUR CASES ARE THE WHOLE COMPLEXITY**: the key exists in the section (replace that line), the section
/// exists without the key (insert after the section header), the section does not exist (append a new one), and
/// the file is empty or absent (write a minimal document). A `serde_yaml` round-trip would be shorter and would
/// DELETE the operator's comments — which is the one thing this file must not lose.
pub fn set_server_key(yaml: &str, key: &str, value: &str) -> String {
    let line = format!("  {key}: {value}");
    let mut out: Vec<String> = Vec::new();
    let mut in_section = false;
    let mut section_at: Option<usize> = None;
    let mut replaced = false;
    for raw in yaml.lines() {
        let is_section =
            raw.trim_end() == format!("{SECTION}:") || raw.starts_with(&format!("{SECTION}: "));
        if is_section {
            in_section = true;
            section_at = Some(out.len());
        } else if !raw.starts_with(' ') && !raw.trim().is_empty() && !raw.starts_with('#') {
            // A NEW TOP-LEVEL KEY ENDS THE SECTION — and a comment does not, because a commented-out key
            // between two real ones is still inside the block a reader sees.
            in_section = false;
        }
        if in_section && !replaced {
            let trimmed = raw.trim_start();
            if trimmed.starts_with(&format!("{key}:")) {
                out.push(line.clone());
                replaced = true;
                continue;
            }
        }
        out.push(raw.to_string());
    }
    if !replaced {
        match section_at {
            Some(at) => {
                // Insert directly after the section header, so the relay settings sit with their siblings.
                out.insert(at + 1, line);
            }
            None => {
                if !out.is_empty() && !out.last().is_some_and(|l| l.trim().is_empty()) {
                    out.push(String::new());
                }
                out.push(format!("{SECTION}:"));
                out.push(line);
            }
        }
    }
    let mut text = out.join("\n");
    text.push('\n');
    text
}

/// Remove ONE key from the `server:` section, leaving every other line as it was.
pub fn remove_server_key(yaml: &str, key: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut in_section = false;
    for raw in yaml.lines() {
        let is_section =
            raw.trim_end() == format!("{SECTION}:") || raw.starts_with(&format!("{SECTION}: "));
        if is_section {
            in_section = true;
        } else if !raw.starts_with(' ') && !raw.trim().is_empty() && !raw.starts_with('#') {
            in_section = false;
        }
        if in_section && raw.trim_start().starts_with(&format!("{key}:")) {
            continue;
        }
        out.push(raw.to_string());
    }
    let mut text = out.join("\n");
    if !text.is_empty() {
        text.push('\n');
    }
    text
}

/// `summrise connect <relay-url> [--token <secret>] [--name <device>]`.
pub fn connect_decision(host: &dyn Host, layout: &Layout, args: &[String]) -> Outcome {
    let mut url_arg: Option<String> = None;
    let mut token_arg: Option<String> = None;
    let mut name_arg: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        match arg {
            "--token" | "--name" => {
                let Some(value) = args.get(i + 1) else {
                    return Outcome::fail(2, format!("{arg} needs a value"));
                };
                if arg == "--token" {
                    token_arg = Some(value.clone());
                } else {
                    name_arg = Some(value.clone());
                }
                i += 2;
                continue;
            }
            other if other.starts_with("--") => {
                return Outcome::fail(2, format!("unknown flag {other}"));
            }
            other => {
                if url_arg.is_some() {
                    return Outcome::fail(2, format!("unexpected argument {other}"));
                }
                url_arg = Some(other.to_string());
            }
        }
        i += 1;
    }

    let Some(url_raw) = url_arg else {
        return Outcome::fail(
            2,
            "usage: summrise connect <relay-url> [--token <secret>] [--name <device>]",
        );
    };
    let url = match normalize_relay_url(&url_raw) {
        Ok(url) => url,
        Err(message) => return Outcome::fail(2, message),
    };
    let path = config_path(&layout.dir);
    let existing = host.read_string(&path).unwrap_or_default();

    // **THE TOKEN FALLS BACK TO THE CONFIG'S OWN**, because re-pointing a device at a moved relay should not
    // require re-typing a secret that has not changed — and a secret typed on a command line lands in a shell
    // history, which is a reason to make it optional rather than required.
    let token = match token_arg {
        Some(token) if !token.trim().is_empty() => token.trim().to_string(),
        _ => crate::config::parse_relay_token(&existing).unwrap_or_default(),
    };
    if token.is_empty() {
        return Outcome::fail(
            2,
            "a relay token is required (--token <secret>): a relay without one refuses every agent",
        );
    }
    let name = match name_arg {
        Some(raw) => match normalize_relay_device(&raw) {
            Ok(name) => Some(name),
            Err(message) => return Outcome::fail(2, message),
        },
        None => crate::config::parse_relay_device(&existing),
    };

    let mut yaml = set_server_key(&existing, RELAY_URL, &url);
    yaml = set_server_key(&yaml, RELAY_TOKEN, &token);
    if let Some(name) = &name {
        yaml = set_server_key(&yaml, RELAY_DEVICE, name);
    }
    if let Err(error) = host.write_bytes(&path, yaml.as_bytes()) {
        return Outcome::fail(1, format!("could not write {}: {error}", path.display()));
    }

    let mut out = vec![
        format!("relay: {url}"),
        format!("token: {}", mask_secret(&token)),
    ];
    match &name {
        Some(name) => out.push(format!(
            "device name: {name} — the console will dial this device as /d/{name}/"
        )),
        None => out.push(
            "device name: (none) — this device polls the relay's DEFAULT device, which the console cannot \
             address by name; pass --name to be reachable per device"
                .to_string(),
        ),
    }
    out.push("restart the agent to dial out (summrise restart)".to_string());
    Outcome {
        exit: 0,
        out,
        err: Vec::new(),
    }
}

/// `summrise disconnect` — remove the three keys, so the device goes back to its tunnel.
pub fn disconnect_decision(host: &dyn Host, layout: &Layout) -> Outcome {
    let path = config_path(&layout.dir);
    let existing = match host.read_string(&path) {
        Ok(text) => text,
        Err(error) => {
            return Outcome::fail(1, format!("could not read {}: {error}", path.display()));
        }
    };
    if crate::config::parse_relay_url(&existing).is_none() {
        return Outcome {
            exit: 0,
            out: vec!["this device is not connected to a relay".to_string()],
            err: Vec::new(),
        };
    }
    let mut yaml = remove_server_key(&existing, RELAY_URL);
    yaml = remove_server_key(&yaml, RELAY_TOKEN);
    yaml = remove_server_key(&yaml, RELAY_DEVICE);
    if let Err(error) = host.write_bytes(&path, yaml.as_bytes()) {
        return Outcome::fail(1, format!("could not write {}: {error}", path.display()));
    }
    Outcome {
        exit: 0,
        out: vec![
            "relay settings removed — the device goes back to its tunnel on the next registration"
                .to_string(),
            "restart the agent to stop dialling out (summrise restart)".to_string(),
        ],
        err: Vec::new(),
    }
}

/// A secret is never echoed: the first and last four characters, or a sentence when it is too short to split.
pub fn mask_secret(secret: &str) -> String {
    let chars: Vec<char> = secret.chars().collect();
    if chars.len() <= 8 {
        return format!("({} characters, not shown)", chars.len());
    }
    let head: String = chars[..4].iter().collect();
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("{head}…{tail}")
}

#[cfg(test)]
mod tests {
    //! The decision layer, driven on the in-memory host the rest of this crate's tests use.

    use super::*;
    use crate::testing::FakeHost;

    fn layout() -> Layout {
        // The crate's own convention: a layout is COMPOSED from two roots, never hand-filled.
        Layout::from_roots("D:\\Summrise", "C:\\ProgramData\\Summrise")
    }

    fn seeded(yaml: &str) -> (FakeHost, Layout) {
        let host = FakeHost::new();
        let layout = layout();
        host.write_bytes(&config_path(&layout.dir), yaml.as_bytes())
            .expect("seed");
        (host, layout)
    }

    #[test]
    fn a_url_needs_a_scheme_a_host_and_no_query() {
        assert_eq!(
            normalize_relay_url("https://relay.example.com/"),
            Ok("https://relay.example.com".to_string()),
            "the trailing slash is what the client would double"
        );
        assert_eq!(
            normalize_relay_url("  http://127.0.0.1:18990  "),
            Ok("http://127.0.0.1:18990".to_string()),
            "loopback stays allowed — that is the same-machine relay the README documents"
        );
        assert!(normalize_relay_url("").is_err());
        assert!(normalize_relay_url("relay.example.com").is_err());
        assert!(normalize_relay_url("https://").is_err());
        assert!(normalize_relay_url("https://relay.example.com/?x=1").is_err());
        assert!(normalize_relay_url("https://relay.example.com/#f").is_err());
    }

    #[test]
    fn a_device_name_cannot_be_blank_or_carry_a_slash() {
        assert_eq!(
            normalize_relay_device(" workshop-pc "),
            Ok("workshop-pc".to_string())
        );
        assert!(normalize_relay_device("  ").is_err());
        // A slash would address a different device at a different path.
        assert!(normalize_relay_device("a/b").is_err());
        // Everything else is the relay's business, not this CLI's.
        assert_eq!(
            normalize_relay_device("d1.local"),
            Ok("d1.local".to_string())
        );
    }

    #[test]
    fn setting_a_key_preserves_every_other_line_and_its_comments() {
        let before =
            "# my device\nserver:\n  host: 127.0.0.1\n  port: 18080\nmcp:\n  enabled: true\n";
        let after = set_server_key(before, RELAY_URL, "https://relay.example.com");
        assert!(after.contains("# my device"), "comments survive");
        assert!(after.contains("  port: 18080"), "siblings survive");
        assert!(after.contains("  relay_url: https://relay.example.com"));
        assert!(
            after.contains("mcp:\n  enabled: true"),
            "other sections survive"
        );
        // A SECOND set finds the key it just wrote rather than adding another.
        let again = set_server_key(&after, RELAY_URL, "https://other.example");
        assert_eq!(again.matches("relay_url:").count(), 1);
        assert!(again.contains("relay_url: https://other.example"));
    }

    #[test]
    fn a_key_is_added_to_the_section_it_belongs_to_even_when_that_section_is_absent() {
        // No `server:` block at all: one is created rather than the key landing at the top level.
        let after = set_server_key("mcp:\n  enabled: true\n", RELAY_TOKEN, "sekret");
        let lines: Vec<&str> = after.lines().collect();
        let section = lines
            .iter()
            .position(|l| *l == "server:")
            .expect("a server section");
        assert_eq!(lines[section + 1], "  relay_token: sekret");
        assert!(after.contains("mcp:"));
        // A commented-out key does not end a section.
        let before = "server:\n  port: 1\n#  relay_url: old\nmcp:\n  a: 1\n";
        let after = set_server_key(before, RELAY_URL, "https://r.example");
        assert!(
            after.contains("#  relay_url: old"),
            "the comment is kept as a comment"
        );
        assert!(after.contains("  relay_url: https://r.example"));
        assert_eq!(after.matches("relay_url").count(), 2);
    }

    #[test]
    fn removing_a_key_leaves_no_hole_and_no_other_change() {
        let before = "server:\n  port: 18080\n  relay_url: https://r.example\n  relay_token: t\n  relay_device: d1\nmcp:\n  a: 1\n";
        let mut after = remove_server_key(before, RELAY_URL);
        after = remove_server_key(&after, RELAY_TOKEN);
        after = remove_server_key(&after, RELAY_DEVICE);
        assert_eq!(after, "server:\n  port: 18080\nmcp:\n  a: 1\n");
    }

    #[test]
    fn connect_writes_all_three_keys_and_never_echoes_the_token() {
        let (host, layout) = seeded("server:\n  port: 18080\n");
        let outcome = connect_decision(
            &host,
            &layout,
            &[
                "https://relay.example.com/".to_string(),
                "--token".to_string(),
                "a-very-long-relay-secret".to_string(),
                "--name".to_string(),
                "workshop-pc".to_string(),
            ],
        );
        assert_eq!(outcome.exit, 0, "{:?}", outcome.err);
        let written = host
            .read_string(&config_path(&layout.dir))
            .expect("the config is written");
        assert!(written.contains("relay_url: https://relay.example.com"));
        assert!(written.contains("relay_token: a-very-long-relay-secret"));
        assert!(written.contains("relay_device: workshop-pc"));
        assert!(written.contains("  port: 18080"), "nothing else moves");
        // The output masks the secret: a command's output is as public as its history.
        let printed = outcome.out.join("\n");
        assert!(!printed.contains("a-very-long-relay-secret"), "{printed}");
        assert!(printed.contains("a-ve…cret"), "{printed}");
    }

    #[test]
    fn connect_refuses_without_a_url_or_a_token_and_keeps_a_stored_token() {
        let (host, layout) = seeded("server:\n  relay_token: stored-secret\n");
        // No URL at all: usage.
        assert_eq!(connect_decision(&host, &layout, &[]).exit, 2);
        // A URL with no token anywhere: refused, because a relay without a token refuses every agent.
        let (bare, bare_layout) = seeded("server:\n  port: 1\n");
        let refused = connect_decision(&bare, &bare_layout, &["https://r.example".to_string()]);
        assert_eq!(refused.exit, 2);
        assert!(refused.err.join(" ").contains("token"));
        // A URL change on a device that already has a token: the stored one is reused.
        let ok = connect_decision(&host, &layout, &["https://r.example".to_string()]);
        assert_eq!(ok.exit, 0, "{:?}", ok.err);
        let written = host.read_string(&config_path(&layout.dir)).expect("config");
        assert!(written.contains("relay_token: stored-secret"));
    }

    #[test]
    fn disconnect_removes_the_three_keys_and_is_idempotent() {
        let (host, layout) = seeded(
            "server:\n  port: 18080\n  relay_url: https://r.example\n  relay_token: t\n  relay_device: d1\n",
        );
        let outcome = disconnect_decision(&host, &layout);
        assert_eq!(outcome.exit, 0);
        let written = host.read_string(&config_path(&layout.dir)).expect("config");
        assert_eq!(written, "server:\n  port: 18080\n");
        // A SECOND disconnect says so rather than failing.
        let again = disconnect_decision(&host, &layout);
        assert_eq!(again.exit, 0);
        assert!(again.out.join(" ").contains("not connected"));
    }
}
