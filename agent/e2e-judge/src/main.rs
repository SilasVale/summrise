//! THE CLI. Three verbs, and each one says what it is for rather than being inferred from a flag order.
//!
//! ```text
//! summrise-e2e-judge --plan  [--only a,b] [--no-browser]           what will run, as data
//! summrise-e2e-judge --bundle [--only a,b] [--no-browser] [-o f]   the driver a device runs
//! summrise-e2e-judge --rows <rows.json> [--expect a,b]             the verdict, as an exit code
//! ```
//!
//! The three share one plan, and that is the point: the thing a device RUNS and the thing that JUDGES what it
//! recorded cannot disagree about which sections were asked for.
//!
//! **`--expect` IS THE SWEEPS' LESSON, APPLIED.** A partial run that found nothing reads exactly like a clean
//! full one unless the caller declares what it asked for, so a recording made with `--only terminal` cannot be
//! handed to the judge as though the whole suite had passed: the caller says what it wanted and a mismatch is
//! exit 2 — DID NOT RUN — rather than a green line.

use std::path::PathBuf;
use std::process::ExitCode;

use summrise_e2e_judge::bundle;
use summrise_e2e_judge::plan::{self, Plan};
use summrise_e2e_judge::rows::Rows;

const USAGE: &str = "\
summrise-e2e-judge — the device e2e suite's plan and judge

  --plan                     print the plan as JSON (what the driver will run)
  --bundle                   print the driver a device runs, with the plan embedded
  --rows <path>              judge a rows document (also accepted as a bare path)
  --only a,b                 restrict the plan to these sections
  --no-browser               drop the four sections that need CDP 9333
  --expect a,b               the sections the caller ASKED FOR; a recording that ran
                             anything else is refused (exit 2) rather than reported
  --driver <path>            the driver source --bundle reads
                             (default: agent/scripts/e2e/e2e.js, or $SUMMRISE_E2E_DRIVER)
  -o, --out <path>           write --bundle's output here instead of stdout

exit: 0 ran and all passed · 1 ran and something failed · 2 DID NOT RUN
      (a bad --only, an unreadable rows document, or --expect not matching the run)
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(2)
        }
    }
}

fn value_of(args: &[String], flag: &str) -> Option<String> {
    let i = args.iter().position(|a| a == flag)?;
    args.get(i + 1).cloned()
}

fn run(args: &[String]) -> Result<ExitCode, String> {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print!("{USAGE}");
        return Ok(ExitCode::SUCCESS);
    }

    let only = value_of(args, "--only");
    let no_browser = args.iter().any(|a| a == "--no-browser");
    let plan = match plan::plan_from_cli(only.as_deref(), no_browser) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            eprintln!("EXIT 2: THE SUITE DID NOT RUN — a name that selects nothing is not a pass.");
            return Ok(ExitCode::from(2));
        }
    };

    if args.iter().any(|a| a == "--plan") {
        println!(
            "{}",
            serde_json::to_string_pretty(&plan).map_err(|e| e.to_string())?
        );
        return Ok(ExitCode::SUCCESS);
    }

    if args.iter().any(|a| a == "--bundle") {
        let text = bundle_driver(&plan, args)?;
        match value_of(args, "-o").or_else(|| value_of(args, "--out")) {
            Some(path) => {
                std::fs::write(&path, &text).map_err(|e| format!("cannot write {path}: {e}"))?;
                eprintln!("wrote {} bytes to {path}", text.len());
            }
            None => print!("{text}"),
        }
        return Ok(ExitCode::SUCCESS);
    }

    let path = value_of(args, "--rows").or_else(|| {
        args.iter()
            .find(|a| !a.starts_with('-') && Some(a.as_str()) != only.as_deref())
            .cloned()
    });
    let Some(path) = path else {
        return Err(format!("no rows document given\n\n{USAGE}"));
    };
    let text = std::fs::read_to_string(&path).map_err(|e| format!("cannot read {path}: {e}"))?;
    let doc = Rows::parse(&text).map_err(|e| format!("{path}: {e}"))?;

    // `--expect` is a declaration, so a recording that ran something else is refused rather than reported.
    if let Some(expect) = value_of(args, "--expect") {
        let mut asked: Vec<String> = expect
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        let mut ran: Vec<String> = doc
            .plan
            .sections
            .iter()
            .map(|s| s.name().to_string())
            .collect();
        asked.sort();
        ran.sort();
        if asked != ran {
            return Err(format!(
                "the recording ran [{}] and --expect declares [{}] — a partial run that found nothing reads \
                 exactly like a clean full one unless the caller says what it asked for.\n\
                 FIX: pass the sections this recording actually ran, or re-run the driver with the plan you \
                 meant (summrise-e2e-judge --plan --only {}).",
                ran.join(","),
                asked.join(","),
                asked.join(",")
            ));
        }
    }

    let verdict = summrise_e2e_judge::judge(&doc);
    for line in verdict.lines() {
        println!("{line}");
    }
    if verdict.checks.is_empty() {
        eprintln!("EXIT 2: THE SUITE DID NOT RUN — zero checks executed. A skip is not a pass.");
    }
    Ok(ExitCode::from(verdict.exit_code() as u8))
}

/// The driver source, from `--driver`, `$SUMMRISE_E2E_DRIVER`, or the checkout this binary was built in.
fn bundle_driver(plan: &Plan, args: &[String]) -> Result<String, String> {
    let path = value_of(args, "--driver")
        .or_else(|| std::env::var("SUMMRISE_E2E_DRIVER").ok())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .expect("the crate lives in agent/")
                .join("scripts/e2e/e2e.js")
        });
    let source = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read the driver source {}: {e}", path.display()))?;
    bundle::bundle(&source, plan)
}
