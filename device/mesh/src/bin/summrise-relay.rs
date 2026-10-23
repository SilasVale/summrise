//! `summrise-relay` — the mesh's relay floor, as a process.
//!
//! It listens on TCP, pairs two clients by session id, and forwards
//! length-prefixed frame bodies between them without reading them. It prints
//! the address it bound on **stdout, as one line, and nothing else**, so a
//! script can start it on port 0 and learn the port without guessing.
//!
//! Flags:
//!
//! - `--bind <address>` — the listener. The default is `127.0.0.1:0`:
//!   loopback and an ephemeral port.
//! - `--pair-timeout-ms <milliseconds>` — how long the first client of a
//!   session waits for the second. The default is 30 seconds; it is a flag
//!   because the fallback budget belongs to the deployment, not to this
//!   binary.
//! - `--help` — the usage line.
//!
//! There is no TLS here. The relay forwards opaque frame bodies and knows
//! nothing about their contents; a future layer can run its own pinned
//! connection end to end through the splice without teaching this process
//! about it.

#![forbid(unsafe_code)]

use std::process::ExitCode;
use std::time::Duration;

use summrise_mesh::relay::RelayServer;

const USAGE: &str = "usage: summrise-relay [--bind <address>] [--pair-timeout-ms <milliseconds>]";
const DEFAULT_BIND: &str = "127.0.0.1:0";
const DEFAULT_PAIR_TIMEOUT_MS: u64 = 30_000;

/// What the command line asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Arguments {
    /// Serve on this address with this pair timeout.
    Serve {
        /// The listener address.
        bind: String,
        /// The pair timeout in milliseconds.
        pair_timeout_ms: u64,
    },
    /// Print the usage and exit successfully.
    Help,
}

fn parse_arguments(mut args: impl Iterator<Item = String>) -> Result<Arguments, String> {
    let mut bind = DEFAULT_BIND.to_owned();
    let mut pair_timeout_ms = DEFAULT_PAIR_TIMEOUT_MS;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--bind" => {
                bind = args
                    .next()
                    .ok_or_else(|| format!("--bind needs an address; {USAGE}"))?;
            }
            "--pair-timeout-ms" => {
                let value = args
                    .next()
                    .ok_or_else(|| format!("--pair-timeout-ms needs a number; {USAGE}"))?;
                pair_timeout_ms = value.parse::<u64>().map_err(|error| {
                    format!("--pair-timeout-ms is milliseconds, not {value:?}: {error}; {USAGE}")
                })?;
            }
            "--help" | "-h" => return Ok(Arguments::Help),
            other => return Err(format!("unknown argument {other:?}; {USAGE}")),
        }
    }
    Ok(Arguments::Serve {
        bind,
        pair_timeout_ms,
    })
}

#[tokio::main]
async fn main() -> ExitCode {
    let arguments = match parse_arguments(std::env::args().skip(1)) {
        Ok(arguments) => arguments,
        Err(message) => {
            eprintln!("summrise-relay: {message}");
            return ExitCode::from(2);
        }
    };
    let (bind, pair_timeout_ms) = match arguments {
        Arguments::Help => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Arguments::Serve {
            bind,
            pair_timeout_ms,
        } => (bind, pair_timeout_ms),
    };

    let server = match RelayServer::bind(&bind, Duration::from_millis(pair_timeout_ms)).await {
        Ok(server) => server,
        Err(error) => {
            eprintln!("summrise-relay: cannot bind {bind}: {error}");
            return ExitCode::FAILURE;
        }
    };
    // THE CONTRACT: one line, the bound address, before the first pairing is
    // served. A caller that reads this line has no need to guess a port.
    println!("{}", server.local_addr());
    server.run().await;
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Arguments, String> {
        parse_arguments(args.iter().map(|argument| (*argument).to_owned()))
    }

    #[test]
    fn the_defaults_are_loopback_ephemeral_and_patient() {
        assert_eq!(
            parse(&[]),
            Ok(Arguments::Serve {
                bind: DEFAULT_BIND.to_owned(),
                pair_timeout_ms: DEFAULT_PAIR_TIMEOUT_MS
            })
        );
    }

    #[test]
    fn bind_comes_from_the_flag() {
        assert_eq!(
            parse(&["--bind", "0.0.0.0:9090"]),
            Ok(Arguments::Serve {
                bind: "0.0.0.0:9090".to_owned(),
                pair_timeout_ms: DEFAULT_PAIR_TIMEOUT_MS
            })
        );
    }

    #[test]
    fn the_pair_timeout_comes_from_the_flag() {
        assert_eq!(
            parse(&["--pair-timeout-ms", "250"]),
            Ok(Arguments::Serve {
                bind: DEFAULT_BIND.to_owned(),
                pair_timeout_ms: 250
            })
        );
    }

    #[test]
    fn flags_can_be_given_in_either_order() {
        assert_eq!(
            parse(&["--pair-timeout-ms", "250", "--bind", "127.0.0.1:7"]),
            Ok(Arguments::Serve {
                bind: "127.0.0.1:7".to_owned(),
                pair_timeout_ms: 250
            })
        );
    }

    #[test]
    fn a_flag_without_a_value_is_refused() {
        assert!(parse(&["--bind"]).is_err());
        assert!(parse(&["--pair-timeout-ms"]).is_err());
    }

    #[test]
    fn a_bad_timeout_is_refused_with_the_value_in_the_message() {
        let error = parse(&["--pair-timeout-ms", "soon"]).expect_err("not a number");
        assert!(error.contains("soon"), "the error names the value: {error}");
    }

    #[test]
    fn an_unknown_argument_is_refused() {
        let error = parse(&["--port", "1"]).expect_err("an unknown flag is not a bind");
        assert!(
            error.contains("--port"),
            "the error names the argument: {error}"
        );
    }

    #[test]
    fn help_prints_usage_and_stops() {
        assert_eq!(parse(&["--help"]), Ok(Arguments::Help));
        assert_eq!(parse(&["-h"]), Ok(Arguments::Help));
    }
}
