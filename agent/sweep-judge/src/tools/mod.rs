//! The three adapters' `judge(file)` wrappers: each reads the JSON, calls the shared judge with ITS OWN
//! options, adds its own rules, prints, and returns an exit code.

pub mod console;
pub mod landing;
pub mod panel;

use crate::out::Out;
use serde_json::Value;

/// `JSON.parse(readFileSync(file, "utf8"))`.
///
/// The JavaScript lets a read or parse failure escape as a stack trace and node exits 1; this prints the
/// same kind of sentence and returns `None`, which the caller turns into the same exit code.
pub fn read_report(file: &str, out: &mut Out) -> Option<Value> {
    let text = match std::fs::read_to_string(file) {
        Ok(t) => t,
        Err(e) => {
            out.err(&format!("could not read {file}: {e}"));
            return None;
        }
    };
    match serde_json::from_str::<Value>(&text) {
        Ok(v) => Some(v),
        Err(e) => {
            out.err(&format!("could not parse {file}: {e}"));
            None
        }
    }
}

/// The trailing block every adapter ends with: `\n{n} finding(s):\n  ` + the findings, to STDERR, and the
/// exit code that goes with it.
pub fn verdict(findings: &[String], ok_line: &str, out: &mut Out) -> i32 {
    if findings.is_empty() {
        out.note(ok_line);
        return 0;
    }
    out.err(&format!(
        "\n{} finding(s):\n  {}",
        findings.len(),
        findings.join("\n  ")
    ));
    1
}
