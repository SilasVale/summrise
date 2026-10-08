//! The CLI. The exit codes ARE the interface: 0 a plan on stdout, 2 a usage error.
//!
//! It prints the plan as ONE JSON OBJECT on stdout — the emitters embed it verbatim in the emitted
//! bundle, so the plan a payload consumes is the plan this binary printed, not a second computation.

use std::process::ExitCode;
use summrise_sweep_plan::{plan_for, Tool};

const USAGE: &str = "usage: summrise-sweep-plan --tool <panel|console|landing> [--passes=<csv>]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let mut tool: Option<String> = None;
    // `None` means the flag was ABSENT, which is not the same input as `--passes=`: the panel defaults
    // the absent one to `all` and passes an empty one through as "", which wants nothing at all.
    let mut passes: Option<String> = None;
    let mut passes_seen = false;
    let mut i = 0;

    while i < args.len() {
        let arg = args[i].clone();
        if let Some(inline) = arg.strip_prefix("--tool=") {
            tool = Some(inline.to_string());
        } else if arg == "--tool" {
            i += 1;
            match args.get(i) {
                Some(v) => tool = Some(v.clone()),
                None => {
                    eprintln!("--tool needs a value\n{USAGE}");
                    return ExitCode::from(2);
                }
            }
        } else if let Some(inline) = arg.strip_prefix("--passes=") {
            // LAST wins? No: FIRST wins, because the JavaScript reads it with `process.argv.find(...)`
            // — the first match is the one that reaches the payload.
            if !passes_seen {
                passes = Some(inline.to_string());
                passes_seen = true;
            }
        } else if arg == "--passes" {
            i += 1;
            match args.get(i) {
                Some(v) => {
                    if !passes_seen {
                        passes = Some(v.clone());
                        passes_seen = true;
                    }
                }
                None => {
                    eprintln!("--passes needs a value\n{USAGE}");
                    return ExitCode::from(2);
                }
            }
        } else if arg == "-h" || arg == "--help" {
            println!("{USAGE}");
            return ExitCode::from(0);
        } else if arg.starts_with("--") {
            eprintln!("unknown option: {arg}\n{USAGE}");
            return ExitCode::from(2);
        } else {
            eprintln!("unexpected argument: {arg}\n{USAGE}");
            return ExitCode::from(2);
        }
        i += 1;
    }

    let Some(tool_name) = tool else {
        eprintln!("--tool is required\n{USAGE}");
        return ExitCode::from(2);
    };
    let Some(tool) = Tool::parse(&tool_name) else {
        eprintln!("--tool must be panel, console or landing (got \"{tool_name}\")\n{USAGE}");
        return ExitCode::from(2);
    };

    let plan = plan_for(tool, passes.as_deref());
    match serde_json::to_string(&plan) {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("the plan could not be serialized: {e}");
            ExitCode::from(2)
        }
    }
}
