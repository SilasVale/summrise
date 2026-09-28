//! gen — render the landing page to disk, the way the worker would render it.
//!
//! Usage: gen <out.html> [setup|no-setup]
//!
//! The two arms exist because the page has two honest states: a release that
//! publishes an installer, and a tgz-only release that must NOT offer a button
//! pointing at the previous build.

use spike_rust_landing::page::render;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let out = args.get(1).cloned().unwrap_or_else(|| "dist/index.html".into());
    let arm = args.get(2).cloned().unwrap_or_else(|| "setup".into());

    const CONSOLE: &str = "https://agent.saisi.online";
    const INSTALLER: &str = "https://agent.saisi.online/summrise-agent/summrise-agent-latest.tgz";
    const SETUP: &str = "https://agent.saisi.online/summrise-agent/SummriseAgent-Setup.exe";

    let setup = if arm == "no-setup" { None } else { Some(SETUP) };
    let html = render(CONSOLE, INSTALLER, setup);

    if let Some(parent) = std::path::Path::new(&out).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&out, &html).expect("write the page");
    println!("{} bytes -> {}", html.len(), out);
}
