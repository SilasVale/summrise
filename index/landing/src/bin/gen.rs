//! gen — render the landing page to disk. Two modes, one renderer.
//!
//!   gen slots  [out-dir]        (default ../src/landing) the two arms, with URL slots
//!   gen render <out.html> <console> <installer> <setup|->
//!                               one document, with the three values interpolated
//!
//! `slots` is what the build writes: the worker picks an arm per request and fills
//! the three URLs, which do not exist at build time — two are built from the
//! request's own origin and the third comes from a per-deployment var. The worker is
//! the ONE place that holds the policy on what may go in them (see `page.rs`).
//!
//! `render` is what `verify.mjs` uses: the same renderer, given the values the
//! worker would compute, so the document can be diffed byte for byte against the
//! page this replaces. A slot that did not survive rendering would ship a page with
//! a literal `{{CONSOLE_URL}}` in an href, so `slots` FAILS rather than writing it.
//!
//! # Why the arms are written as JavaScript
//!
//! The document has TWO consumers and they do not share a module system. The worker
//! imports it as text (wrangler's `**/*.html` rule); plain Node — the index tests,
//! `landing-check.mjs`, the landing design sweep — cannot import a `.html` at all
//! (`ERR_UNKNOWN_FILE_EXTENSION`), and those three are what hold the page's contrast,
//! its one h1, its press states and its reflow. A generated module satisfies both: it
//! is ordinary JavaScript to Node and ordinary JavaScript to the bundler, and the
//! document inside it is still a document a person can read.
//!
//! The escaping is not decoration. A backtick or a `${` anywhere in the CSS, the
//! prose or the two inline scripts would close the literal early — the accident this
//! repository already paid for once (round 93, five red CI jobs) — so both are
//! escaped by construction rather than hoped against.

use summrise_landing::page::render;

fn write_file(path: &std::path::Path, body: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create the output directory");
    }
    std::fs::write(path, body).expect("write the output");
    println!("{:>6} bytes -> {}", body.len(), path.display());
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("slots") => {
            let out_dir = args.get(1).map(String::as_str).unwrap_or("../src/landing");
            for (name, body) in summrise_landing::page::arms() {
                write_file(&std::path::Path::new(out_dir).join(name), &body);
            }
        }
        Some("render") => {
            let [_, out, console, installer, setup] = &args[..] else {
                eprintln!("usage: gen render <out.html> <console> <installer> <setup|->");
                std::process::exit(2);
            };
            let setup = if setup == "-" {
                None
            } else {
                Some(setup.as_str())
            };
            write_file(
                std::path::Path::new(out),
                &render(console, installer, setup),
            );
        }
        _ => {
            eprintln!(
                "usage: gen slots [out-dir] | gen render <out.html> <console> <installer> <setup|->"
            );
            std::process::exit(2);
        }
    }
}
