//! `summrise-console` — the operator-side service.
//!
//! It reads its operator token, grant key, data directory and listener from the command line, then
//! serves until stopped. The listener is the only piece of this binary the library does not own.

use std::net::SocketAddr;
use std::process::ExitCode;

use summrise_console::{Config, Console};

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("summrise-console: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_args()?;
    let console = Console::load(&config)?;

    // Loopback by default: exposing the console is an explicit `--bind 0.0.0.0`, and that
    // deployment sits behind the operator's TLS terminator. Registration needs an enrollment code
    // and a signature either way; plain HTTP on the listener is the terminator's job, as the design
    // says.
    let address = SocketAddr::new(config.bind, config.port);
    let listener = tokio::net::TcpListener::bind(address).await?;
    eprintln!(
        "summrise-console: listening on http://{}",
        listener.local_addr()?
    );

    axum::serve(listener, console.router()).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;
    use summrise_console::DEFAULT_BIND;

    #[test]
    fn the_default_bind_is_loopback_and_the_network_is_an_explicit_choice() {
        assert_eq!(DEFAULT_BIND, std::net::IpAddr::V4(Ipv4Addr::LOCALHOST));
        assert!(SocketAddr::new(DEFAULT_BIND, 18_082).ip().is_loopback());

        let terminator = std::net::IpAddr::V4(Ipv4Addr::UNSPECIFIED);
        assert!(
            SocketAddr::new(terminator, 18_082).ip().is_unspecified(),
            "the TLS-terminator deployment passes --bind 0.0.0.0 explicitly"
        );
    }
}
