//! `summrise-broker` — the phase-2 rendezvous, as a process.
//!
//! It serves the local rendezvous on plain HTTP: `POST /register`,
//! `GET /lookup/:peer_id`, `POST /request/:target` and `GET /requests/:target`.
//! The request routes carry a signed note naming a requester, a target and a
//! relay session, and no payload. It prints the address it bound on **stdout,
//! as one line, and nothing else**, so a script can start it on port 0 and
//! learn the port without guessing.
//!
//! Flags:
//!
//! - `--bind <address>` — the listener. The default is `127.0.0.1:0`: loopback
//!   and an ephemeral port.
//! - `--help` — the usage line.
//!
//! There is no TLS here. The VPS relay/broker and its certificate are phase 2b.

#![forbid(unsafe_code)]

use std::process::ExitCode;

use summrise_mesh::broker::Broker;

const USAGE: &str = "usage: summrise-broker [--bind <address>]";
const DEFAULT_BIND: &str = "127.0.0.1:0";

/// What the command line asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Arguments {
    /// Serve on this address.
    Serve {
        /// The listener address.
        bind: String,
    },
    /// Print the usage and exit successfully.
    Help,
}

fn parse_arguments(mut args: impl Iterator<Item = String>) -> Result<Arguments, String> {
    let mut bind = DEFAULT_BIND.to_owned();
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--bind" => {
                bind = args
                    .next()
                    .ok_or_else(|| format!("--bind needs an address; {USAGE}"))?;
            }
            "--help" | "-h" => return Ok(Arguments::Help),
            other => return Err(format!("unknown argument {other:?}; {USAGE}")),
        }
    }
    Ok(Arguments::Serve { bind })
}

#[tokio::main]
async fn main() -> ExitCode {
    let arguments = match parse_arguments(std::env::args().skip(1)) {
        Ok(arguments) => arguments,
        Err(message) => {
            eprintln!("summrise-broker: {message}");
            return ExitCode::from(2);
        }
    };
    let bind = match arguments {
        Arguments::Help => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Arguments::Serve { bind } => bind,
    };

    let listener = match tokio::net::TcpListener::bind(&bind).await {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("summrise-broker: cannot bind {bind}: {error}");
            return ExitCode::FAILURE;
        }
    };
    let address = match listener.local_addr() {
        Ok(address) => address,
        Err(error) => {
            eprintln!("summrise-broker: cannot read the bound address: {error}");
            return ExitCode::FAILURE;
        }
    };
    // THE CONTRACT: one line, the bound address, before the first request is
    // served. A caller that reads this line has no need to guess a port.
    println!("{address}");

    if let Err(error) = Broker::new().serve(listener).await {
        eprintln!("summrise-broker: the listener failed: {error}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Arguments, String> {
        parse_arguments(args.iter().map(|argument| (*argument).to_owned()))
    }

    #[test]
    fn the_default_bind_is_loopback_and_ephemeral() {
        assert_eq!(
            parse(&[]),
            Ok(Arguments::Serve {
                bind: DEFAULT_BIND.to_owned()
            })
        );
    }

    #[test]
    fn bind_comes_from_the_flag() {
        assert_eq!(
            parse(&["--bind", "0.0.0.0:9090"]),
            Ok(Arguments::Serve {
                bind: "0.0.0.0:9090".to_owned()
            })
        );
    }

    #[test]
    fn a_bind_without_a_value_is_refused() {
        assert!(parse(&["--bind"]).is_err());
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
