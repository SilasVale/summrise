//! THE PAGE-SIDE PROBE REGISTRY — which probe measures what, where its text lives, and how it reaches the
//! program a device runs.
//!
//! **THE PROBES ARE JAVASCRIPT AND THEY STAY JAVASCRIPT.** `browser_run_script` takes a JS file, and every
//! one of these is a program a browser evaluates: `surfaceProbe` walks the DOM for headings, landmarks,
//! overflow, clipping and slivers; `namesProbe` reads accessible names; `reflowProbe` measures the layout at
//! a width; `marksProbe` reads the mark families; the three `*_SOURCE` constants are page-side programs too.
//! The manifest reason for them is the one that cannot be argued away, and this module does not argue with
//! it — it NAMES them, and it hands their text to the program unchanged.
//!
//! **WHAT THIS MODULE OWNS IS THE DECISION ABOUT WHICH ONES A SWEEP EMBEDS, AND THE PROOF THAT WHAT IT
//! EMBEDS IS WHAT THE JAVASCRIPT SAYS.** That second half is the part with teeth. A copy of a probe is a
//! program, and a probe that stops parsing returns ZERO ROWS — which reads exactly like a clean sweep, on a
//! device, where no Rust gate can see inside the page. So:
//!
//!   * **THE TEXT IS READ FROM THE JAVASCRIPT AT EMIT TIME, NOT COPIED INTO RUST.** `read_probes` spawns
//!     node, imports `agent/scripts/lib/design-sweep.mjs` and asks it for `fn.toString()` of each registered
//!     name — which is the ONE definition, and it is the same one `piecesModule` would embed. A second
//!     transcription into a Rust `&str` would be a copy that could drift, and a drift here is silent.
//!   * **THE ASSEMBLED PROGRAM IS PARSED BEFORE IT IS RETURNED** (`assemble`), which is the contract
//!     `agent/scripts/lib/sweep-bundle.mjs`'s `assertParses` already states for the four JS emitters. A probe
//!     with a stray brace must fail HERE, loudly, naming the probe — not on a device as "0 rows".
//!
//! **WHY THE PARSE IS NODE'S.** Rust has no JavaScript parser, and writing one to check JavaScript would be
//! a second implementation of the thing the check exists to be able to trust. Node is present wherever a
//! sweep runs — it IS the runtime `browser_run_script` uses — so the check costs nothing a device does not
//! already have.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::process::Command;

/// One page-side probe a sweep can embed: the key the program reads it by, the name it has in
/// `design-sweep.mjs`, and one line saying what it measures — because a registry whose entries are three
/// strings is a registry nobody can check.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Probe {
    pub key: &'static str,
    pub js_name: &'static str,
    pub what: &'static str,
}

impl Probe {
    /// The program a browser evaluates: a FUNCTION for the four probes that take a root selector, a SOURCE
    /// string for the three that are already the text of a program. Both cross as text — `fn.toString()`
    /// is what `piecesModule` embeds for a function, and it is what node is asked for below.
    pub fn is_function(self) -> bool {
        matches!(
            self.js_name,
            "surfaceProbe" | "namesProbe" | "reflowProbe" | "marksProbe"
        )
    }
}

/// **THE REGISTRY, AND EVERY ENTRY NAMES A REAL EXPORT.** `the_registry_names_real_exports` in
/// `tests/passes.rs` asks the JavaScript module for each `js_name`, so a probe that is renamed or deleted
/// fails the gate rather than emitting an `undefined` the page would evaluate as nothing.
pub const PROBES: &[Probe] = &[
    Probe {
        key: "SURFACE",
        js_name: "surfaceProbe",
        what: "headings, landmarks, overflow, clipping and slivers, walked in the page",
    },
    Probe {
        key: "NAMES",
        js_name: "namesProbe",
        what: "accessible names, read the way a screen reader reads them",
    },
    Probe {
        key: "REFLOW",
        js_name: "reflowProbe",
        what: "the layout at a width, for the 1.4.10 reflow criterion",
    },
    Probe {
        key: "MARKS",
        js_name: "marksProbe",
        what: "mark families, per-state silhouettes, collisions, ring+fill",
    },
    Probe {
        key: "UNSTYLED",
        js_name: "UNSTYLED_SOURCE",
        what: "class names the page renders that no rule in the sheet styles",
    },
    Probe {
        key: "THEME",
        js_name: "THEME_SOURCE",
        what: "the theme the page says it is in, read off the body attribute",
    },
    Probe {
        key: "TARGETS",
        js_name: "TARGETS_SOURCE",
        what: "target sizes, WCAG 2.5.8",
    },
];

/// The module the probes live in, relative to the repository root.
pub const PROBE_MODULE: &str = "agent/scripts/lib/design-sweep.mjs";

/// The one line of node that turns the module into text. Kept as a single program so the whole registry
/// costs ONE process, and so the thing being asked is readable here rather than assembled from pieces.
fn node_program() -> String {
    let names: Vec<&str> = PROBES.iter().map(|p| p.js_name).collect();
    format!(
        "const m = await import(process.argv[1]);\n\
         const out = {{}};\n\
         for (const n of {names}) out[n] = typeof m[n] === 'function' ? m[n].toString() : m[n];\n\
         if (Object.values(out).some((v) => typeof v !== 'string' || v.length === 0)) {{\n\
         \x20 console.error('one of these is not a non-empty string: ' + JSON.stringify(Object.entries(out).map(([k, v]) => [k, typeof v, String(v).length])));\n\
         \x20 process.exit(3);\n\
         }}\n\
         process.stdout.write(JSON.stringify(out));\n",
        names = serde_json::to_string(&names).expect("a list of strings serializes")
    )
}

/// The probe sources, keyed by `Probe::key`, read from the JavaScript module by node.
///
/// **ONE PROCESS FOR THE WHOLE REGISTRY**, and a failure here is an `Err` rather than a default: a probe
/// that could not be read must not become an empty string the program embeds and the page evaluates as
/// nothing.
pub fn read_probes() -> Result<BTreeMap<String, String>, String> {
    let root = summrise_sweep_judge::paths::repo_root().ok_or_else(|| {
        format!(
            "could not find the repository root (the walk looks for {PROBE_MODULE}), so the probes cannot be \
             read — an emitted program with no probes would measure nothing and read as a clean sweep"
        )
    })?;
    let module = root.join(PROBE_MODULE);
    if !module.is_file() {
        return Err(format!(
            "{} is not there, so the page-side probes this driver embeds do not exist",
            module.display()
        ));
    }
    let url = format!("file://{}", module.display());
    let out = Command::new("node")
        .arg("--input-type=module")
        .arg("-e")
        .arg(node_program())
        .arg(&url)
        .output()
        .map_err(|e| {
            format!(
                "could not run node to read the probes out of {}: {e} — the probes are JavaScript and node is \
                 what reads them; the same node is what `browser_run_script` runs on a device",
                module.display()
            )
        })?;
    if !out.status.success() {
        return Err(format!(
            "node exited {} reading the probes out of {}:\n{}",
            out.status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "on a signal".into()),
            module.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let raw: BTreeMap<String, String> = serde_json::from_slice(&out.stdout)
        .map_err(|e| format!("node did not print the probe sources as JSON: {e}"))?;
    for p in PROBES {
        match raw.get(p.js_name) {
            Some(s) if !s.is_empty() => {}
            _ => {
                return Err(format!(
                    "`{}` is registered as `{}` and {} does not export a non-empty string for it",
                    p.key,
                    p.js_name,
                    module.display()
                ))
            }
        }
    }
    Ok(PROBES
        .iter()
        .map(|p| (p.key.to_string(), raw[p.js_name].clone()))
        .collect())
}

/// **THE ASSEMBLED PROGRAM: the probes as declarations, then the plan and the step list as data.**
///
/// The shape is `piecesModule`'s — `const <name> = <source>;` at the top, referenced by name — because the
/// payloads already read their probes that way and because a function expression inlined into an object
/// would carry no name for the passes that call each other.
///
/// **AND IT IS PARSED BEFORE IT IS RETURNED.** `node --check` on the assembled text, so a stray brace in a
/// probe fails HERE with the probe's name in the message. That is Review Focus 1 of the plan: *"a page-side
/// payload is JS inside a Rust template; a stray brace must fail loudly at emit, not return zero rows that
/// read as a clean sweep."*
pub fn assemble(
    plan_json: &str,
    steps_json: &str,
    probes: &BTreeMap<String, String>,
) -> Result<String, String> {
    let mut code = String::new();
    code.push_str("// generated by summrise-design-sweep-driver — do not edit.\n");
    code.push_str("// The probes are `agent/scripts/lib/design-sweep.mjs`'s own text, read at emit time and\n");
    code.push_str("// embedded unchanged; the plan and the step list are Rust data.\n");
    code.push_str("\"use strict\";\n");
    let mut spans: Vec<(usize, usize)> = Vec::new();
    for p in PROBES {
        let src = probes.get(p.key).ok_or_else(|| {
            format!(
                "the program is missing the `{}` probe ({}), which the ladder names",
                p.key, p.js_name
            )
        })?;
        let start = code.lines().count() + 1;
        code.push_str(&format!("const {} = {};\n", p.key, src));
        spans.push((start, code.lines().count()));
    }
    code.push_str(&format!("const PLAN = {plan_json};\n"));
    code.push_str(&format!("const STEPS = {steps_json};\n"));
    code.push_str("module.exports = { PLAN, STEPS");
    for p in PROBES {
        code.push_str(&format!(", {}", p.key));
    }
    code.push_str(" };\n");
    check_parses(&code, probes, &spans)?;
    Ok(code)
}

/// `node --check` over the assembled program, and — when it fails — over each probe ON ITS OWN, so the
/// refusal NAMES the probe a reader has to fix.
///
/// **THE LINE NUMBER IS NOT ENOUGH, AND THAT WAS MEASURED RATHER THAN GUESSED.** The first version blamed
/// the probe whose declaration contained the reported line. Handed a truncated `REFLOW` — a template literal
/// that lost its tail — node reported `Unexpected end of input` at the LAST LINE OF THE FILE, which falls
/// inside no probe at all, and the refusal came back as a path and a line number. So the fallback is one
/// small parse per probe, which costs seven node runs on a path that is already a failure and turns "the
/// program does not parse" into "the `REFLOW` probe does not parse".
fn check_parses(
    code: &str,
    probes: &BTreeMap<String, String>,
    spans: &[(usize, usize)],
) -> Result<(), String> {
    let err = match parse_alone(code) {
        Ok(()) => return Ok(()),
        Err(e) => e,
    };
    let blamed = blame_by_line(&err, spans)
        .or_else(|| blame_by_isolation(probes))
        .unwrap_or_else(|| {
            "the defect is outside every probe, so it is in the plan or the step list".to_string()
        });
    Err(format!(
        "the assembled program does not parse — {blamed}:\n{err}"
    ))
}

/// `node --check` on a scratch file. A file rather than `new Function`, because `new Function` wraps the
/// source in a FUNCTION BODY, where `const` and `return` behave differently, and `node --check` parses it as
/// the module it will be.
fn parse_alone(code: &str) -> Result<(), String> {
    // **A UNIQUE PATH PER CALL, AND THAT IS NOT DEFENSIVE PROGRAMMING — IT WAS A MEASURED FAILURE.** The
    // first version named the file after the process id alone, and `cargo test` runs a file's tests in
    // PARALLEL: two of them wrote the same path, one removed it while the other's `node --check` was
    // starting, and the refusal came back as `Error: Cannot find module '/tmp/…-515474.js'` — a test failure
    // that named neither the probe nor the defect. A counter and a timestamp make the name unique per call.
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let path = std::env::temp_dir().join(format!(
        "summrise-sweep-driver-assemble-{}-{seq}-{nanos}.js",
        std::process::id()
    ));
    std::fs::write(&path, code)
        .map_err(|e| format!("could not stage the program at {}: {e}", path.display()))?;
    let out = Command::new("node").arg("--check").arg(&path).output();
    let _ = std::fs::remove_file(&path);
    let out = out.map_err(|e| format!("could not run node to parse the assembled program: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
}

/// `<path>:<line>` from node's own report, mapped to the probe whose declaration contains it.
fn blame_by_line(err: &str, spans: &[(usize, usize)]) -> Option<String> {
    let line = err
        .lines()
        .next()?
        .rsplit(':')
        .next()?
        .trim()
        .parse::<usize>()
        .ok()?;
    spans
        .iter()
        .position(|(lo, hi)| line >= *lo && line <= *hi)
        .and_then(|i| PROBES.get(i))
        .map(|p| {
            format!(
                "the `{}` probe ({}) is the one that does not parse",
                p.key, p.js_name
            )
        })
}

/// Parse each probe on its own and name the first that fails. The honest answer when the reported line falls
/// at EOF, which is exactly what an unterminated template literal reports.
fn blame_by_isolation(probes: &BTreeMap<String, String>) -> Option<String> {
    for p in PROBES {
        let src = probes.get(p.key)?;
        if parse_alone(&format!("\"use strict\";\nconst {} = {};\n", p.key, src)).is_err() {
            return Some(format!(
                "the `{}` probe ({}) is the one that does not parse, and the other {} do",
                p.key,
                p.js_name,
                PROBES.len() - 1
            ));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The registry is a list a reader can check, so it is asserted rather than trusted: seven probes, keys
    /// unique, every one naming what it measures.
    #[test]
    fn the_registry_is_seven_named_probes() {
        assert_eq!(PROBES.len(), 7);
        let mut keys: Vec<&str> = PROBES.iter().map(|p| p.key).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), PROBES.len(), "two probes share a key");
        for p in PROBES {
            assert!(
                !p.what.is_empty(),
                "{} says nothing about what it measures",
                p.key
            );
            assert!(!p.js_name.is_empty(), "{} names no export", p.key);
        }
    }
}
