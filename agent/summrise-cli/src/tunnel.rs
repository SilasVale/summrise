//! `summrise tunnel ...` AND `initTunnel` — the ONLY handle on the boxed cloudflared.
//!
//! cloudflared is a Summrise-supervised component, not something an operator drives by hand: `setup`
//! stages it, the agent spawns it on boot, and this verb is how a human turns public access on and
//! asks whether it is up. Ported from `initTunnel` and the `tunnel` verb in
//! `agent/summrise-agent-npm/src/summrise.ts`.
//!
//! **THREE THINGS ARE DECIDED HERE RATHER THAN IN THE SCRIPT, AND EVERY ONE OF THEM IS A MEASURED
//! DEFECT.**
//!
//! * `127.0.0.1`, never `127.0.0.2`. Two writers, two answers, one file: the agent's own
//!   provisioning writes `127.0.0.1` and the live device's `netstat` shows the listener there, so
//!   this writer would have repointed a working tunnel at a socket nobody holds. The address is
//!   derived here from [`crate::config::tunnel_yml`], which is the one place it is spelled.
//! * `allow-remote-config: false`, which is not optional: cloudflared prefers a REMOTE config when
//!   one exists, so a stale remote ingress keeps proxying to a dead address "no matter what
//!   tunnel.yml says".
//! * the ingress PORT comes from the live `config.yaml`, so a custom-port install gets a tunnel that
//!   reaches the agent instead of a 502.
//!
//! **AND ONE DEFECT IS CARRIED ACROSS RATHER THAN FIXED, DELIBERATELY.** `init_tunnel`'s default
//! hostname is still the literal developer device — [`DEFAULT_TUNNEL_HOST`], which lives in
//! [`crate::endpoints`] and carries the value and the reasoning — because that is what the
//! TypeScript does and a port that quietly changed it would be a behaviour change wearing a port's
//! commit message. `setup` computes the right host one function away ([`device_host`]) and passes it
//! only when `--tunnel <host>` carries a value; see the port report.

use crate::config::{agent_port, tunnel_yml};
use crate::device::api_post;
// The carried defect, in the ONE file `agent/tests/production_host.rs` declares for this crate's
// hostname defaults — see its doc for why the constant moved and why the value did not change.
use crate::dispatch::Outcome;
use crate::endpoints::DEFAULT_TUNNEL_HOST;
use crate::host::Host;
use crate::host::Tri;
use crate::paths::{win_join, Layout};
use crate::ps::psq;
use serde_json::json;
use std::path::{Path, PathBuf};

fn cloudflared(layout: &Layout) -> PathBuf {
    win_join(&layout.components_dir, "cloudflared.exe")
}

fn tunnel_cfg(layout: &Layout) -> PathBuf {
    win_join(&layout.etc_dir, "tunnel.yml")
}

/// Is the tunnel installed? BOTH halves — the binary and its config — because either one missing
/// means the same thing to every caller.
pub fn tunnel_installed(host: &dyn Host, layout: &Layout) -> bool {
    host.exists(&cloudflared(layout)) && host.exists(&tunnel_cfg(layout))
}

/// The sub-arguments of `tunnel install`: `<hostname>` (when it is not another flag) and
/// `--reg-key <key>`.
///
/// Split out because the "is it a flag or is it the hostname" test is a decision, and it is the one
/// the TypeScript spells inline: `args[1] && !args[1].startsWith("--")`.
pub fn install_args(args: &[String]) -> (String, String) {
    let positional = args
        .get(1)
        .filter(|a| !a.starts_with("--"))
        .cloned()
        .unwrap_or_default();
    let key = args
        .iter()
        .position(|a| a == "--reg-key")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_default();
    (positional, key)
}

/// `initTunnel(hostname, regKey)` — login, create, route, and write `tunnel.yml`.
///
/// THE KEY EXCHANGE IS BEST-EFFORT AND SAYS SO: a failed exchange falls back to the interactive
/// browser login, because the alternative is an operator with no way to enable a tunnel at all.
/// Every other step is fatal, and each one names the step that failed rather than "tunnel failed".
pub fn init_tunnel(host: &dyn Host, layout: &Layout, hostname: &str, reg_key: &str) -> Outcome {
    let cf = cloudflared(layout);
    let cfg = tunnel_cfg(layout);
    if !host.exists(&cf) {
        return Outcome::fail(
            1,
            format!(
                "tunnel: cloudflared.exe not staged at {}\n  {}",
                cf.display(),
                crate::components::STAGE_ADVICE
            ),
        );
    }
    let mut out = Outcome::ok();
    let dev_host = if hostname.is_empty() {
        DEFAULT_TUNNEL_HOST.to_string()
    } else {
        hostname.to_string()
    };

    let mut token = host
        .env("CLOUDFLARE_API_TOKEN")
        .filter(|s| !s.is_empty())
        .unwrap_or_default();
    if token.is_empty() && !reg_key.is_empty() {
        out = out.say("tunnel: exchanging registration key for the Cloudflare API token...");
        match api_post(
            host,
            "/api/install/tunnel-token",
            &json!({ "key": reg_key }),
        ) {
            Ok(v) => match v.get("apiToken").and_then(|t| t.as_str()) {
                Some(t) => {
                    token = t.to_string();
                    out = out.say("tunnel: key exchanged (consumed once)");
                }
                None => {
                    out =
                        out.say("tunnel: tunnel-token exchange failed (no token) -- falling back");
                }
            },
            Err(e) => {
                out = out.say(format!(
                    "tunnel: exchange unavailable ({e}) -- falling back"
                ));
            }
        }
    }

    let cfq = cf.to_string_lossy().to_string();
    let login: Vec<String> = if token.is_empty() {
        vec![cfq.clone(), "tunnel".into(), "login".into()]
    } else {
        vec![
            cfq.clone(),
            "tunnel".into(),
            "login".into(),
            "--token".into(),
            token.clone(),
        ]
    };
    if host.run(&login, None).status != Some(0) {
        return out.warn("tunnel: cloudflare login failed").exit(1);
    }

    // THE TUNNEL IS NAMED AFTER THIS DEVICE, one label deep: `summrise-agent-d1` for a device
    // host whose first label is `d1`. That is what makes two devices collide when they claim one
    // name — which is exactly what the wrong default caused.
    let name = format!(
        "summrise-agent-{}",
        dev_host.split('.').next().unwrap_or(&dev_host)
    );
    let _ = host.run(
        &[cfq.clone(), "tunnel".into(), "create".into(), name.clone()],
        None,
    );
    let list = host.run(
        &[
            cfq.clone(),
            "tunnel".into(),
            "list".into(),
            "--name".into(),
            name.clone(),
        ],
        None,
    );
    let Some(tunnel_id) = find_tunnel_id(&list.stdout) else {
        return out.warn("tunnel: could not determine tunnel id").exit(1);
    };
    let route = host.run(
        &[
            cfq.clone(),
            "tunnel".into(),
            "route".into(),
            "dns".into(),
            name.clone(),
            dev_host.clone(),
        ],
        None,
    );
    if route.status != Some(0) {
        return out.warn("tunnel: dns route failed").exit(1);
    }

    let cred = format!(
        "{}\\.cloudflared\\{tunnel_id}.json",
        host.env("USERPROFILE").unwrap_or_default()
    );
    let tun_port = agent_port(host, &layout.etc_dir);
    let body = tunnel_yml(&tunnel_id, &cred, &dev_host, tun_port);
    if host.write_bytes(&cfg, body.as_bytes()).is_err() {
        return out
            .warn(format!("tunnel: could not write {}", cfg.display()))
            .exit(1);
    }
    out = out
        .say("tunnel: installed -- tunnel.yml written, agent spawns it on boot")
        .say(format!("  hostname: {dev_host}"));
    out
}

/// The tunnel id out of `cloudflared tunnel list` — the FIRST UUID in the table.
///
/// The TypeScript's regex is `([0-9a-fA-F]{8}-[0-9a-fA-F-]{27})`, and it is transcribed rather than
/// re-derived: the tail class is deliberately loose (a UUID's remaining groups are not all
/// hex-equal in length) and a "tighter" pattern here would be a second opinion about cloudflared's
/// output format.
pub fn find_tunnel_id(list: &str) -> Option<String> {
    let bytes: Vec<char> = list.chars().collect();
    let is_hex = |c: char| c.is_ascii_hexdigit();
    for start in 0..bytes.len() {
        if start + 35 > bytes.len() {
            break;
        }
        if bytes[start..start + 8].iter().all(|c| is_hex(*c)) && bytes[start + 8] == '-' {
            let tail = &bytes[start + 9..start + 36];
            if tail.iter().all(|c| is_hex(*c) || *c == '-') {
                return Some(bytes[start..start + 36].iter().collect());
            }
        }
    }
    None
}

/// `summrise tunnel [status|install|start|stop|update]`.
pub fn tunnel_decision(host: &dyn Host, layout: &Layout, args: &[String]) -> Outcome {
    let sub = args
        .first()
        .cloned()
        .unwrap_or_else(|| "status".to_string());
    let cf = cloudflared(layout);
    let cfg = tunnel_cfg(layout);
    let has = tunnel_installed(host, layout);
    match sub.as_str() {
        "status" => {
            if !has {
                return Outcome::ok()
                    .say("tunnel: not installed (cloudflared is OPTIONAL -- local mode needs no tunnel)")
                    .say("  to enable public access: `summrise tunnel install`");
            }
            let state = host.process_running("cloudflared.exe");
            let line = match state {
                // A failed READ is not a verdict: do not let an operator conclude it is down.
                Tri::Unknown => "tunnel: state UNKNOWN -- could not list processes (tasklist failed); do not assume it is down",
                Tri::Yes => "tunnel: RUNNING",
                Tri::No => "tunnel: STOPPED",
            };
            Outcome::ok()
                .say(line)
                .say(format!("  binary: {}", cf.display()))
                .say(format!("  config: {}", cfg.display()))
        }
        "install" => {
            let (hostname, key) = install_args(args);
            init_tunnel(host, layout, &hostname, &key)
        }
        "start" => {
            if !has {
                return Outcome::fail(
                    1,
                    "tunnel: not installed -- run setup with public-access enabled",
                );
            }
            // DETACHED, AND THEN ASKED RATHER THAN ASSUMED. The old form exited 0 immediately, so
            // the success line printed for a cloudflared that died on a bad config, a missing
            // credentials file, a gone route, or an already-running instance.
            let spawn = host.spawn_detached(
                &[
                    cf.to_string_lossy().to_string(),
                    "tunnel".into(),
                    "--config".into(),
                    cfg.to_string_lossy().to_string(),
                    "run".into(),
                ],
                1500,
            );
            match spawn {
                Err(e) => Outcome::fail(1, format!("tunnel: FAILED to start cloudflared ({e})")),
                Ok(Some(code)) => Outcome::fail(
                    1,
                    format!(
                        "tunnel: cloudflared exited immediately (code {code}) -- check the config and credentials"
                    ),
                ),
                Ok(None) => match host.process_running("cloudflared.exe") {
                    Tri::Yes => Outcome::ok()
                        .say("tunnel: started in background (agent also auto-spawns it on boot)"),
                    state => Outcome::fail(
                        1,
                        format!(
                            "tunnel: cloudflared did not come up{} -- see the tunnel log; the agent still auto-spawns it on boot",
                            if state == Tri::Unknown {
                                " (and the process list could not be read, so this is not a verdict)"
                            } else {
                                ""
                            }
                        ),
                    ),
                },
            }
        }
        "stop" => {
            let r = host.taskkill_image("cloudflared.exe");
            if r.status != Some(0) {
                // Nothing to stop is not a failure: the operator asked for it to be down.
                Outcome::ok().say("tunnel: nothing to stop")
            } else {
                Outcome::ok()
            }
        }
        "update" => Outcome::ok().say(
            "tunnel: version is locked by the Summrise release flow -- update via the installer/npm package.",
        ),
        _ => Outcome::fail(1, "usage: summrise tunnel <status|install|start|stop|update>"),
    }
}

/// The credentials path `cloudflared` writes the login to — one spelling, used by the caller that
/// has to quote it into `tunnel.yml`.
pub fn credentials_dir(host: &dyn Host) -> String {
    format!(
        "{}\\.cloudflared",
        host.env("USERPROFILE").unwrap_or_default()
    )
}

/// A quoting helper kept next to its one use, so the tunnel config's path is not the one place in
/// the crate that rolls its own escaping.
pub fn quoted_path(p: &Path) -> String {
    psq(&p.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::FakeHost;

    fn layout() -> Layout {
        Layout::from_roots("D:\\Summrise", "C:\\ProgramData\\Summrise")
    }

    /// A device host for the tests that need one, COMPOSED FROM THE PRODUCT'S OWN SUFFIX rather than
    /// written out: the literal it replaces was a production hostname sitting in a file that has no
    /// business naming one, and a host built from [`crate::endpoints::DEVICE_HOST_SUFFIX`] is the
    /// same string while being unable to drift from the suffix the real `device_host()` uses.
    fn dev_host() -> String {
        format!("d9{}", crate::endpoints::DEVICE_HOST_SUFFIX)
    }

    fn installed() -> FakeHost {
        FakeHost::new()
            .with_file("D:\\Summrise\\components\\cloudflared.exe", "MZ")
            .with_file("D:\\Summrise\\etc\\tunnel.yml", "tunnel: x\n")
    }

    /// THE THREE-STATE RULE, on the verb whose whole job is to answer "is it up". A failed
    /// `tasklist` must not read as STOPPED.
    #[test]
    fn status_does_not_call_an_unreadable_process_list_stopped() {
        let host = installed().with_process("cloudflared.exe", Tri::Unknown);
        let r = tunnel_decision(&host, &layout(), &["status".into()]);
        assert_eq!(r.exit, 0);
        assert!(r.out[0].contains("state UNKNOWN"), "{:?}", r.out);
        assert!(!r.out[0].contains("STOPPED"), "{:?}", r.out);
    }

    /// Not installed is not an error — cloudflared is OPTIONAL and local mode needs no tunnel — and
    /// the sentence names the command that fixes it.
    #[test]
    fn an_absent_tunnel_is_reported_with_the_way_out() {
        let host = FakeHost::new();
        let r = tunnel_decision(&host, &layout(), &[]);
        assert_eq!(r.exit, 0, "{r:?}");
        assert!(r.out[0].contains("not installed"), "{:?}", r.out);
        assert!(r.out[1].contains("summrise tunnel install"), "{:?}", r.out);

        // ...but STARTING an absent tunnel IS a failure: the operator asked for something that
        // cannot happen.
        let r = tunnel_decision(&host, &layout(), &["start".into()]);
        assert_eq!(r.exit, 1);
    }

    /// `--reg-key` in the ARGUMENT position of `<hostname>` is the trap the positional test closes.
    #[test]
    fn a_flag_is_never_read_as_the_hostname() {
        assert_eq!(
            install_args(&["install".into(), "--reg-key".into(), "ABC".into()]),
            (String::new(), "ABC".to_string())
        );
        assert_eq!(
            install_args(&["install".into(), dev_host()]),
            (dev_host(), String::new())
        );
    }

    /// The id is the FIRST UUID in the listing, and a listing without one is `None` — not an empty
    /// string, which would be written into `tunnel.yml` as `tunnel: `.
    #[test]
    fn the_tunnel_id_is_the_first_uuid_or_nothing() {
        let listing = "ID                                   NAME\n\
                       9c2b1a44-1f0e-4a5b-9d3c-7e6f5a4b3c2d  summrise-agent-d9\n";
        assert_eq!(
            find_tunnel_id(listing).as_deref(),
            Some("9c2b1a44-1f0e-4a5b-9d3c-7e6f5a4b3c2d")
        );
        assert_eq!(find_tunnel_id("no tunnels here"), None);
        assert_eq!(find_tunnel_id(""), None);
    }

    /// The ingress names the address the agent LISTENS on, and the port comes from the live config.
    #[test]
    fn the_ingress_is_loopback_on_the_configured_port() {
        let host = installed()
            .with_file("D:\\Summrise\\etc\\config.yaml", "server:\n  port: 19090\n")
            .with_env("CLOUDFLARE_API_TOKEN", "tok")
            .script_runs(vec![
                crate::host::RunResult {
                    status: Some(0),
                    ..Default::default()
                },
                crate::host::RunResult {
                    status: Some(0),
                    ..Default::default()
                },
                crate::host::RunResult {
                    status: Some(0),
                    stdout: "9c2b1a44-1f0e-4a5b-9d3c-7e6f5a4b3c2d  x\n".into(),
                    ..Default::default()
                },
                crate::host::RunResult {
                    status: Some(0),
                    ..Default::default()
                },
            ]);
        let r = init_tunnel(&host, &layout(), &dev_host(), "");
        assert_eq!(r.exit, 0, "{r:?}");
        let body = String::from_utf8(host.file("D:\\Summrise\\etc\\tunnel.yml").unwrap()).unwrap();
        assert!(body.contains("service: http://127.0.0.1:19090"), "{body}");
        assert!(body.contains("allow-remote-config: false"), "{body}");
        assert!(!body.contains("127.0.0.2"), "{body}");
    }

    /// A missing cloudflared names `setup`, never "reinstall the package": the package carries no
    /// components by design, so a reinstall cannot stage this.
    #[test]
    fn a_missing_cloudflared_does_not_advise_a_reinstall() {
        let host = FakeHost::new();
        let r = init_tunnel(&host, &layout(), "", "");
        assert_eq!(r.exit, 1);
        let text = r.err.join("\n");
        assert!(text.contains("not staged at"), "{text}");
        assert!(text.contains("run 'summrise setup' to stage it"), "{text}");
        assert!(!text.contains("reinstall the package"), "{text}");
    }

    /// A failed key exchange FALLS BACK to the interactive login rather than abandoning the
    /// operator — and it says which of the two happened.
    #[test]
    fn a_failed_exchange_falls_back_to_the_interactive_login() {
        let host = installed().script_runs(vec![
            crate::host::RunResult {
                status: Some(6),
                stderr: "no route".into(),
                ..Default::default()
            }, // the exchange
            crate::host::RunResult {
                status: Some(0),
                ..Default::default()
            }, // login
            crate::host::RunResult {
                status: Some(0),
                ..Default::default()
            }, // create
            crate::host::RunResult {
                status: Some(0),
                stdout: "9c2b1a44-1f0e-4a5b-9d3c-7e6f5a4b3c2d x".into(),
                ..Default::default()
            },
            crate::host::RunResult {
                status: Some(0),
                ..Default::default()
            }, // route
        ]);
        let r = init_tunnel(&host, &layout(), &dev_host(), "KEY-1");
        assert_eq!(r.exit, 0, "{r:?}");
        assert!(
            r.out.iter().any(|l| l.contains("exchange unavailable")),
            "{:?}",
            r.out
        );
        // The login was invoked WITHOUT --token, which is what "falling back" means.
        let login = &host.runs()[1];
        assert!(!login.contains(&"--token".to_string()), "{login:?}");
    }
}
