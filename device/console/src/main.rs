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
    // `--setup <data-dir>` is the first run of a deployment: it writes the two secrets the flags
    // name, once, and never replaces one that exists.
    let mut arguments = std::env::args_os().skip(1);
    if let Some(first) = arguments.next() {
        if first == "--setup" {
            let data = arguments.next().ok_or("--setup needs <data-dir>")?;
            let setup = summrise_console::setup(std::path::Path::new(&data))?;
            eprintln!(
                "summrise-console: {} (token {}, seed {})",
                setup.data.display(),
                if setup.token.is_some() {
                    "created"
                } else {
                    "kept"
                },
                if setup.seed_created {
                    "created"
                } else {
                    "kept"
                },
            );
            if let Some(token) = setup.token {
                // Printed once, on purpose: the file is the copy, and this is how the operator takes
                // it somewhere safe. Nothing else in this binary ever prints it.
                println!("{token}");
            }
            return Ok(());
        }
    }

    // `--login-link <public-url> --data <dir>` mints a two-minute code for the page's login box.
    // It is a separate invocation on purpose: the code is a file the running console reads, and the
    // command that prints the link is the operator's, run where they already are.
    let mut probe = std::env::args_os().skip(1);
    if let Some(first) = probe.next() {
        if first == "--login-link" {
            let url = probe.next().ok_or("--login-link needs <public-url>")?;
            let mut data = None;
            while let Some(flag) = probe.next() {
                if flag == "--data" {
                    data = probe.next();
                }
            }
            let data = data.ok_or("--login-link needs --data <dir>")?;
            let data = std::path::PathBuf::from(data);
            let code = summrise_console::new_login_code(120_000);
            let path = summrise_console::write_login_code(&data, &code)?;
            eprintln!(
                "summrise-console: wrote {} (valid two minutes)",
                path.display()
            );
            let separator = if url.to_string_lossy().contains('?') {
                '&'
            } else {
                '?'
            };
            println!("{}{separator}code={}", url.to_string_lossy(), code.code);
            return Ok(());
        }
    }

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
