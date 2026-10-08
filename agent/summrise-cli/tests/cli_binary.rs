//! THE CASES THAT NEED THE BINARY, OR A FILE OUTSIDE THIS CRATE.
//!
//! Three of the oracle's cases EXECUTE the CLI (with no arguments, which mutates nothing, or with
//! `--version`); two read a file on the other side of a language boundary, because a test on one
//! copy of a fact can only ever compare copies; and one pins the construction that replaced four of
//! the TypeScript's source scans. They live here rather than in a unit test because all six need
//! something a unit cannot reach.

use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap() // agent/
        .parent()
        .unwrap() // repository root
        .to_path_buf()
}

fn read(rel: &str) -> String {
    let p = repo_root().join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{} must be readable: {e}", p.display()))
}

fn cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_summrise-cli"))
        .args(args)
        .output()
        .expect("the ported CLI must run")
}

/// Oracle: cli.test.mjs:2713 — "every CLI verb the root guide promises is one the CLI prints".
///
/// A published tgz lives on devices in the field, so the verb list is a device-facing promise: a verb
/// disappearing needs dual-accept, a rollback point and a device regression, not just a doc edit.
///
/// SAFETY, learned the hard way: this assertion runs the CLI **with NO ARGUMENTS ONLY**. `summrise`
/// with no args prints its verb list and exits 0. Every documented verb MUTATES the machine, so an
/// assertion that "checked" a verb by invoking it would install, swap or delete something. When the
/// direct verification is destructive, find the part of it that is a measurement.
#[test]
fn every_cli_verb_the_root_guide_promises_is_one_the_cli_prints() {
    let guide = read("AGENTS.md");
    let mut promised: Vec<String> = Vec::new();
    for line in guide.lines() {
        if let Some(rest) = line.strip_prefix("summrise ") {
            let verb: String = rest
                .chars()
                .take_while(|c| c.is_ascii_lowercase())
                .collect();
            if !verb.is_empty() && !promised.contains(&verb) {
                promised.push(verb);
            }
        }
    }
    promised.sort();
    assert!(
        promised.len() >= 2,
        "expected the root guide to advertise several CLI verbs, found {} — if the mention syntax \
         changed, fix THIS extractor rather than deleting the test",
        promised.len()
    );

    let out = cli(&[]);
    assert_eq!(out.status.code(), Some(0), "a bare invocation exits 0");
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let missing: Vec<&String> = promised
        .iter()
        .filter(|v| {
            let re =
                regex::Regex::new(&format!(r"(^|[^a-z-]){}([^a-z-]|$)", regex::escape(v))).unwrap();
            !re.is_match(&text)
        })
        .collect();
    assert!(
        missing.is_empty(),
        "these verbs are promised by the root guide but the CLI does not print them: {missing:?}. \
         A published tgz lives on devices in the field, so the verb list is a device-facing promise. \
         CLI printed: {:?}",
        text.lines().next()
    );
}

/// Oracle: cli.test.mjs:3016 — "the help cannot promise a verb that does not exist".
///
/// EXECUTED, NOT READ AS TEXT: what it MEANS is that every verb the help prints is one the
/// dispatcher actually has (the round-25 prune deleted `report` and `watch` while the hand-written
/// usage line kept advertising them).
#[test]
fn the_help_cannot_promise_a_verb_that_does_not_exist() {
    let r = cli(&[]);
    assert_eq!(
        r.status.code(),
        Some(0),
        "a bare invocation prints the help and exits 0"
    );
    let printed: Vec<String> = String::from_utf8_lossy(&r.stdout)
        .lines()
        .skip(1)
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    assert!(
        printed.len() >= 10,
        "expected the verb list, got {} line(s)",
        printed.len()
    );
    for dead in ["report", "watch"] {
        assert!(
            !printed.iter().any(|l| l == dead),
            "the help advertises '{dead}', which the dispatcher does not have"
        );
    }
    // ...and every verb it prints IS one the dispatcher has, or the help promises nothing real.
    for v in &printed {
        assert!(
            summrise_cli::dispatch::VERBS.contains(&v.as_str()),
            "the help prints '{v}', which is not in the verb table"
        );
    }
}

/// Oracle: cli.test.mjs:3066 — "--version answers the installer's acceptance check, and exits 0".
///
/// Step 4 of the installer README BEGINS with "`summrise --version` / the panel opens". It is not a
/// verb, so it fell through to the usage branch, printed the verb list and exited 1 — a fresh install
/// that had worked reported a FAILURE in the one place the operator is told to look. This is also the
/// only documented invocation that mutates nothing, which is why it can be EXECUTED.
#[test]
fn version_answers_the_installers_acceptance_check() {
    let r = cli(&["--version"]);
    assert_eq!(
        r.status.code(),
        Some(0),
        "--version must exit 0, got {:?}: {}{}",
        r.status.code(),
        String::from_utf8_lossy(&r.stdout),
        String::from_utf8_lossy(&r.stderr)
    );
    let out = String::from_utf8_lossy(&r.stdout).trim().to_string();
    assert!(
        regex::Regex::new(r"^\d+\.\d+\.\d+$")
            .unwrap()
            .is_match(&out),
        "and print the version, not the usage list: {out:?}"
    );
}

/// Oracle: cli.test.mjs:2530 — "the tunnel ingress names the address the agent actually listens on".
///
/// THE INGRESS ADDRESS AND THE LISTEN ADDRESS ARE CHOSEN IN TWO LANGUAGES, SO NOTHING IN EITHER ONE
/// CAN SEE THE OTHER. This test is the pin across the language boundary — the same shape as the
/// gateway's code-viewer mirror check, and for the same reason: a test on one copy can only ever
/// compare copies. It reads BOTH the agent's writer and this CLI's builder.
#[test]
fn the_tunnel_ingress_names_the_address_the_agent_actually_listens_on() {
    let agent_writer = read("agent/src/tunnel.rs");
    let good = regex::Regex::new(r"service: http://127\.0\.0\.1:").unwrap();
    let dead = regex::Regex::new(r"service: http://127\.0\.0\.2:").unwrap();
    for (name, text) in [
        ("agent/src/tunnel.rs", agent_writer.as_str()),
        (
            "summrise-cli::config::tunnel_yml",
            &summrise_cli::config::tunnel_yml("tid", "creds", "h", 18080),
        ),
    ] {
        assert!(
            good.is_match(text),
            "{name}: the tunnel ingress must be 127.0.0.1 — that is where the agent listens \
             (netstat: 127.0.0.1:18080) and what the agent's own writer puts in the same file. \
             127.0.0.2 is a socket nobody holds."
        );
        assert!(
            !dead.is_match(text),
            "{name}: 127.0.0.2 is back — the agent calls it a dead address (502)"
        );
        assert!(
            text.contains("allow-remote-config: false"),
            "{name}: the writer must keep allow-remote-config: false. cloudflared prefers a REMOTE \
             config when one exists, so dropping this re-enables a stale remote ingress pointing at \
             a dead address 'no matter what tunnel.yml says'."
        );
    }
}

/// Oracle: cli.test.mjs:2563 — "the agent's own default host agrees with the ingress".
///
/// The third spelling: `ServerConfig::default()` said 127.0.0.2 while the shipped `config.yaml`, the
/// agent's tunnel writer and the live device all say 127.0.0.1 — a default that disagreed with the
/// file it exists to replace.
#[test]
fn the_agents_own_default_host_agrees_with_the_ingress() {
    let core = read("agent/summrise-command-core/src/config.rs");
    assert!(
        regex::Regex::new(r#"host:\s*"127\.0\.0\.1"\.into\(\)"#)
            .unwrap()
            .is_match(&core),
        "ServerConfig::default must bind the same address the tunnel ingress names"
    );
    assert!(
        !regex::Regex::new(r#"host:\s*"127\.0\.0\.2"\.into\(\)"#)
            .unwrap()
            .is_match(&core),
        "the 127.0.0.2 default is back — it disagrees with config.yaml, with tunnel.rs's ingress \
         and with the live device"
    );
}

/// REPLACES SIX ORACLE CASES BY CONSTRUCTION — and this is the test that pins the construction.
///
/// `cli.test.mjs` cases 1758, 2075, 2131, 2167, 2382 and 2449 exist because the TypeScript built cmd.exe command
/// STRINGS and then had to be scanned, by hand-written JavaScript parsers, to prove no interpolated
/// value could escape a quoted region, become an operator, or be expanded as `%NAME%`. The port
/// removes the class: [`summrise_cli::host::Host`] offers only `run(argv)`, plus two NAMED platform
/// operations for the Windows commands that are `cmd.exe` builtins and take no argv (`rmdir`,
/// `taskkill`). There is no command-line method to misuse.
///
/// A construction is not self-proving, so this reads the source and asserts it: `run` spawns a
/// program with argv and never a shell, and the ONLY `cmd` in the crate is the named one.
#[test]
fn no_shell_sits_between_the_cli_and_powershell() {
    let host_rs = read("agent/summrise-cli/src/host.rs");
    let body = host_rs
        .split("fn run(&self, argv: &[String], timeout_ms: Option<u64>) -> RunResult {")
        .nth(1)
        .expect("Host::run must be the one place a program is spawned")
        .split("\n    fn process_running")
        .next()
        .unwrap();
    assert!(
        body.contains("Command::new(prog).args(rest)"),
        "the run body must pass argv straight to the process"
    );
    assert!(
        !body.contains("/c") && !body.contains("sh -c") && !body.contains("shell"),
        "no shell may sit between the CLI and PowerShell:\n{body}"
    );

    // The ONE named platform operation that needs `cmd.exe`, because `rmdir` is a builtin and takes
    // no argv. Anything else reaching for a command line is the defect the four cases pinned.
    let cmd_calls = host_rs.matches("\"cmd\"").count();
    assert_eq!(
        cmd_calls, 1,
        "exactly one named cmd.exe operation is allowed (remove_tree): found {cmd_calls}"
    );
    assert!(
        host_rs.contains("fn remove_tree"),
        "and it must be the named one, so the shell cannot spread"
    );
}
