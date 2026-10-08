//! The CLI. Argument parsing is hand-rolled because the surface is three flags and one positional, and
//! because the exit codes ARE the interface: 0 no findings, 1 findings, 2 usage error.

use std::process::ExitCode;
use summrise_sweep_judge::out::Out;
use summrise_sweep_judge::tools;

const USAGE: &str =
    "usage: summrise-sweep-judge --tool <panel|console|landing> [--expect=<csv>] <report.json>";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut out = Out::new();

    let mut tool: Option<String> = None;
    let mut expect: Option<String> = None;
    let mut file: Option<String> = None;
    let mut i = 0;

    // `--tool panel` and `--tool=panel` are both accepted: the callers are shell scripts, and the two
    // spellings are one keystroke apart in a way nobody should have to discover from a usage error.
    while i < args.len() {
        let arg = args[i].clone();
        if let Some(inline) = arg.strip_prefix("--tool=") {
            tool = Some(inline.to_string());
        } else if arg == "--tool" {
            i += 1;
            match args.get(i) {
                Some(v) => tool = Some(v.clone()),
                None => {
                    out.err(&format!("--tool needs a value\n{USAGE}"));
                    return ExitCode::from(2);
                }
            }
        } else if let Some(inline) = arg.strip_prefix("--expect=") {
            expect = Some(inline.to_string());
        } else if arg == "--expect" {
            i += 1;
            match args.get(i) {
                Some(v) => expect = Some(v.clone()),
                None => {
                    out.err(&format!("--expect needs a value\n{USAGE}"));
                    return ExitCode::from(2);
                }
            }
        } else if arg == "-h" || arg == "--help" {
            out.note(USAGE);
            return ExitCode::from(2);
        } else if arg.starts_with("--") {
            out.err(&format!("unknown option: {arg}\n{USAGE}"));
            return ExitCode::from(2);
        } else if file.is_none() {
            file = Some(arg);
        } else {
            out.err(&format!("unexpected argument: {arg}\n{USAGE}"));
            return ExitCode::from(2);
        }
        i += 1;
    }

    let Some(tool) = tool else {
        out.err(&format!("--tool is required\n{USAGE}"));
        return ExitCode::from(2);
    };
    if !matches!(tool.as_str(), "panel" | "console" | "landing") {
        out.err(&format!(
            "--tool must be panel, console or landing (got \"{tool}\")\n{USAGE}"
        ));
        return ExitCode::from(2);
    }
    let Some(file) = file else {
        out.err(&format!("no report file given\n{USAGE}"));
        return ExitCode::from(2);
    };

    let code = match tool.as_str() {
        "panel" => tools::panel::judge(&file, expect.as_deref().unwrap_or("all"), &mut out),
        "console" => tools::console::judge(&file, &mut out),
        _ => tools::landing::judge(&file, &mut out),
    };
    ExitCode::from(code as u8)
}
