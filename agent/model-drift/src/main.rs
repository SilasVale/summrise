//! The binary: argv, the four GETs, and the exit codes.
//!
//! The exit codes ARE the interface, and they are the JavaScript's: **0 for a report that was
//! produced** (drift is normal — the report is the product), **1 for `--strict` with a CHECK, and 1
//! for a catalogue that could not be read at all**. An unreadable catalogue is not an empty one, and
//! a report that printed "everything matches" there would be the exact failure this repository keeps
//! finding, so it says `Nothing was compared.` and exits 1 instead of printing a report.
//!
//! # ONE DELIBERATE DIVERGENCE FROM THE `.mjs`
//!
//! The JavaScript read its flags with `argv.includes(...)` and ignored everything it did not
//! recognise, so a typo (`--stict`) silently produced a NON-strict report and exit 0. This binary
//! refuses an argument it does not know, with exit 2 — a typo must not decide whether a CHECK fails
//! the run. `--gateway` without a value is refused the same way, where the JavaScript threw a
//! `TypeError` on `undefined.replace`.

use std::process::ExitCode;
use std::time::Duration;

use summrise_model_drift::{
    advertised_for, cannot_read, diff_channel, exit_code, model_ids, ChannelReport, Report,
    CHANNELS,
};

/// The upstream GET's own identity, kept from the `.mjs` byte for byte: a tool that identified
/// itself differently after the port would be a different client as far as an upstream is concerned.
const USER_AGENT: &str = "summrise-model-drift/1 (+https://agent.saisi.online)";

/// `AbortSignal.timeout(25_000)` — the WHOLE operation, the body read included.
const TIMEOUT: Duration = Duration::from_secs(25);

const DEFAULT_GATEWAY: &str = "https://api.saisi.online";

const USAGE: &str = "usage: summrise-model-drift [--json] [--strict] [--gateway <url>]";

use serde_json::Value;

/// The three flags the `.mjs` had, and nothing else.
struct Args {
    json: bool,
    strict: bool,
    gateway: Option<String>,
}

/// Hand-rolled, because the surface is three flags and the exit codes ARE the interface.
fn parse_args(args: &[String]) -> Result<Args, String> {
    let mut parsed = Args {
        json: false,
        strict: false,
        gateway: None,
    };
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--json" {
            parsed.json = true;
        } else if arg == "--strict" {
            parsed.strict = true;
        } else if let Some(inline) = arg.strip_prefix("--gateway=") {
            parsed.gateway = Some(inline.to_string());
        } else if arg == "--gateway" {
            i += 1;
            match args.get(i) {
                Some(value) => parsed.gateway = Some(value.clone()),
                None => return Err("--gateway needs a value".to_string()),
            }
        } else {
            // NOT the JavaScript's behaviour — see the header. A flag this tool does not know is a
            // typo, and a typo must not decide whether a CHECK fails the run.
            return Err(format!("unrecognised argument: {arg}"));
        }
        i += 1;
    }
    Ok(parsed)
}

/// `--gateway` then `SUMMRISE_GATEWAY` then the public host, with the trailing slashes the
/// `/v1/models` path is appended to removed. An EMPTY environment variable falls through, because
/// `||` is JS truthiness; an empty `--gateway` does not, because the JavaScript used it.
fn resolve_gateway(explicit: Option<&str>, from_env: Option<&str>) -> String {
    let chosen = match explicit {
        Some(gateway) => gateway,
        None => from_env
            .filter(|g| !g.is_empty())
            .unwrap_or(DEFAULT_GATEWAY),
    };
    chosen.trim_end_matches('/').to_string()
}

/// One upstream GET: `HTTP <status>` for a refusal, and the reply's model list otherwise.
async fn fetch_ids(client: &reqwest::Client, url: &str) -> Result<Vec<String>, String> {
    let response = client
        .get(url)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status().as_u16()));
    }
    let body: Value = response.json().await.map_err(|e| e.to_string())?;
    model_ids(&body)
}

#[tokio::main]
async fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse_args(&argv) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("{message}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let gateway = resolve_gateway(
        args.gateway.as_deref(),
        std::env::var("SUMMRISE_GATEWAY").ok().as_deref(),
    );

    let client = match reqwest::Client::builder()
        .timeout(TIMEOUT)
        .user_agent(USER_AGENT)
        .build()
    {
        Ok(client) => client,
        Err(e) => {
            eprintln!("model-drift: cannot build the HTTP client — {e}");
            return ExitCode::from(1);
        }
    };

    let advertised = match fetch_ids(&client, &format!("{gateway}/v1/models")).await {
        Ok(ids) => ids,
        Err(message) => {
            // SAY SO. An unreadable catalogue is not an empty one, and a report that printed
            // "everything matches" here would be the exact failure this repo keeps finding.
            eprintln!("{}", cannot_read(&gateway, &message));
            return ExitCode::from(1);
        }
    };

    let mut channels = Vec::new();
    for channel in CHANNELS {
        let adv = advertised_for(&advertised, channel.prefix);
        // A fetch failure becomes `{ error, advertised }`: the line says this upstream could NOT be
        // checked and NOTHING about what it offers — which is the distinction this tool exists for.
        let report = match fetch_ids(&client, channel.url).await {
            Err(error) => ChannelReport::Unreadable {
                error,
                advertised: adv.len(),
            },
            Ok(offered) => {
                let diff = diff_channel(&adv, &offered);
                ChannelReport::Checked {
                    advertised: adv.len(),
                    offered: offered.len(),
                    diff,
                }
            }
        };
        channels.push((channel.prefix, report));
    }

    let report = Report {
        gateway,
        advertised_total: advertised.len(),
        channels,
    };
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report.to_json())
                .expect("a serde_json::Value serializes")
        );
    } else {
        print!("{}", report.render_text());
    }
    ExitCode::from(exit_code(args.strict, report.any_check()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn the_three_flags_are_read_the_way_the_javascript_read_them() {
        let parsed = parse_args(&args(&["--json", "--strict", "--gateway", "https://x"])).unwrap();
        assert!(parsed.json);
        assert!(parsed.strict);
        assert_eq!(parsed.gateway.as_deref(), Some("https://x"));
        // None of them is required: the bare command is the report.
        let parsed = parse_args(&args(&[])).unwrap();
        assert!(!parsed.json && !parsed.strict);
        assert!(parsed.gateway.is_none());
    }

    #[test]
    fn gateway_also_accepts_the_inline_spelling() {
        // The callers are shell scripts, and the two spellings are one keystroke apart in a way
        // nobody should have to discover from a usage error.
        let parsed = parse_args(&args(&["--gateway=https://x/"])).unwrap();
        assert_eq!(parsed.gateway.as_deref(), Some("https://x/"));
    }

    #[test]
    fn an_argument_the_tool_does_not_know_is_a_usage_error_not_a_silent_report() {
        // THE DELIBERATE DIVERGENCE: `argv.includes` ignored these, so `--stict` produced a
        // non-strict report and exit 0 — the one place a typo changes the verdict.
        assert!(parse_args(&args(&["--stict"])).is_err());
        assert!(parse_args(&args(&["extra"])).is_err());
        assert!(parse_args(&args(&["--gateway"])).is_err());
    }

    #[test]
    fn resolve_gateway_prefers_the_flag_then_the_environment_then_the_public_host() {
        assert_eq!(
            resolve_gateway(Some("https://flag"), Some("https://env")),
            "https://flag"
        );
        assert_eq!(resolve_gateway(None, Some("https://env")), "https://env");
        assert_eq!(resolve_gateway(None, None), DEFAULT_GATEWAY);
        // `process.env.SUMMRISE_GATEWAY || …` is JS truthiness: an EMPTY variable is falsy, so it
        // falls through to the public host rather than producing a gateway of "".
        assert_eq!(resolve_gateway(None, Some("")), DEFAULT_GATEWAY);
    }

    #[test]
    fn resolve_gateway_strips_trailing_slashes_because_the_path_is_appended() {
        assert_eq!(resolve_gateway(Some("https://a///"), None), "https://a");
        assert_eq!(resolve_gateway(Some("https://a"), None), "https://a");
    }
}
