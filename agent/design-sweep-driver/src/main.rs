//! THE DRIVER BINARY — emit the program, or read the rows it wrote.
//!
//! **THE EXIT CODE SAYS WHETHER IT COLLECTED, NOT WHETHER THE PAGE PASSED** (`agent/scripts/e2e/e2e.js`'s
//! contract, kept verbatim because the two drivers are read by the same kind of person):
//!
//! ```text
//! 0   the program was assembled and parses, or the rows document reads
//! 1   it could not collect — a probe could not be read, the assembled program does not parse, or the rows
//!     document is not one
//! 2   a usage error
//! ```
//!
//! **`--emit` PERFORMS NO MEASUREMENT AND `--rows` REACHES NO VERDICT.** The verdict is
//! `summrise-sweep-judge`'s, from the same bytes; a process that both drove and judged would be a process
//! whose collector's defect is invisible to its own judge, which is the risk the spec names rather than
//! hides.

use std::process::ExitCode;
use summrise_design_sweep_driver::{assemble, parse, read_probes, steps, PROBES};
use summrise_sweep_judge::plan::{canonical_plan, only};

const USAGE: &str = "\
usage: summrise-design-sweep-driver --emit [--tool <panel|console|landing>] [--only <a,b>] [--surfaces <a,b>]
       summrise-design-sweep-driver --rows <rows.json>

  --emit      write the driver program: the plan, the step list, and the page-side probes read out of
              agent/scripts/lib/design-sweep.mjs and embedded unchanged. The program is PARSED before it is
              printed, so a probe that has stopped parsing fails here rather than on a device as 0 rows.
  --rows      read a rows document {schema, base, plan, observations} and refuse one whose fields are not all
              there — by name, because an absent field is indistinguishable from a run that measured nothing.
  --tool      which sweep the plan is for; the canonical ladder is the panel's (it runs every pass).
  --only      a comma-separated subset of the ladder's rung names.
  --surfaces  a comma-separated surface list; the plan's own surface list is summrise-sweep-plan's business,
              so this is required for --emit rather than guessed.";

fn flag<'a>(argv: &'a [String], name: &str) -> Option<&'a str> {
    let i = argv.iter().position(|a| a == name)?;
    argv.get(i + 1).map(String::as_str)
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.iter().any(|a| a == "--help" || a == "-h") {
        println!("{USAGE}");
        return ExitCode::from(2);
    }
    if let Some(path) = flag(&argv, "--rows") {
        return match std::fs::read_to_string(path) {
            Err(e) => {
                eprintln!("summrise-design-sweep-driver: cannot read {path}: {e}");
                ExitCode::from(1)
            }
            Ok(text) => match parse(&text) {
                Err(e) => {
                    eprintln!("summrise-design-sweep-driver: {path}: {e}");
                    ExitCode::from(1)
                }
                Ok(rows) => {
                    let done = rows.observations.iter().filter(|o| o.collected()).count();
                    let failed = rows.observations.iter().filter(|o| o.failed()).count();
                    println!(
                        "collected {} observation(s) of {} on {} — {} failed (this exit code says COLLECTED, \
                         not passed; run `summrise-sweep-judge` for the verdict)",
                        done,
                        rows.observations.len(),
                        rows.base,
                        failed
                    );
                    ExitCode::SUCCESS
                }
            },
        };
    }
    if !argv.iter().any(|a| a == "--emit") {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }
    match emit(
        flag(&argv, "--tool"),
        flag(&argv, "--only"),
        flag(&argv, "--surfaces"),
    ) {
        Err(e) => {
            eprintln!("summrise-design-sweep-driver: {e}");
            ExitCode::from(1)
        }
        Ok(code) => {
            print!("{code}");
            ExitCode::SUCCESS
        }
    }
}

fn emit(
    tool: Option<&str>,
    only_arg: Option<&str>,
    surfaces_arg: Option<&str>,
) -> Result<String, String> {
    let tool = tool.unwrap_or("panel");
    if tool != "panel" {
        return Err(format!(
            "--tool {tool}: the canonical ladder is the panel's, because it is the only sweep that runs every \
             pass; the other two tools' selections are `summrise-sweep-plan`'s `wants()`. Emitting a ladder \
             for {tool} here would be a second copy of that decision."
        ));
    }
    let plan = match only_arg {
        None => canonical_plan(),
        Some(spec) => {
            let names: Vec<String> = spec
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            only(&names)?
        }
    };
    let surfaces: Vec<String> = surfaces_arg
        .ok_or_else(|| {
            "--emit needs --surfaces: the plan's own surface list is summrise-sweep-plan's, and a \
                         driver that invented one would measure a page nobody asked for"
                .to_string()
        })?
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if surfaces.is_empty() {
        return Err("--surfaces was given with no surface names".to_string());
    }
    let step_list = steps(&plan, &surfaces)?;
    let plan_json =
        serde_json::to_string(&plan).map_err(|e| format!("the plan does not serialize: {e}"))?;
    let steps_json = serde_json::to_string(&step_list)
        .map_err(|e| format!("the step list does not serialize: {e}"))?;
    let probes = read_probes()?;
    let code = assemble(&plan_json, &steps_json, &probes)?;
    eprintln!(
        "collected: {} step(s) over {} surface(s), {} page-side probe(s) embedded unchanged from {}",
        step_list.len(),
        surfaces.len(),
        PROBES.len(),
        summrise_design_sweep_driver::PROBE_MODULE
    );
    Ok(code)
}
