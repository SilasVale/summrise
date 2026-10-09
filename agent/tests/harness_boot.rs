//! THE HARNESS BOOTED — the emitter's output, checked on every `cargo test`.
//!
//! ── WHAT THIS REPLACES, AND WHY IT IS A GATE NOW ────────────────────────────────────────────────────
//! `agent/scripts/harness-boot-check.mjs` carried two halves and NOTHING INVOKED IT: a driver
//! (`bootHarness`, which ran the emitter and read what it wrote) and a judgement (`judgeBoot`, a pure
//! function over one probe value). A manual script that judges is LOGIC by the migration's own rule, and
//! the rule's answer for a judgement is Rust — but the stronger reason is the one this repository keeps
//! writing down: **an instrument nobody runs is an instrument nobody has.** The half that CAN be enforced
//! here is enforced here, on every test run, instead of waiting for somebody to remember it.
//!
//! ── THE INVARIANT, AND THE FAILURE IT EXISTS FOR ────────────────────────────────────────────────────
//! `panel-render-audit.mjs` renders the harness the design sweep measures, and it reports itself by EXIT
//! CODE: **2 means it wrote the harness, 1 means the emitted template literal broke** — a stray backtick
//! in the emitter is the usual cause, and it is the same class the pre-commit hook was built for. A
//! harness that was not written, or one that is a fragment, is a sweep that measures nothing and reports
//! whatever it finds as a design defect. So: the emitter must exit 2, and what it wrote must be an app
//! bundle rather than an error page.
//!
//! ── THE HALF THAT IS NOT HERE, NAMED SO IT IS NOT MISTAKEN FOR COVERAGE ─────────────────────────────
//! The DOM side — that the app really mounted, and that the stub really answered — needs a BROWSER, so it
//! cannot run in `cargo test`. Its probe is kept below as `DEVICE_BOOT_PROBE` (a string, which is what a
//! browser evaluates) and its judgement is `judge_boot`, unit-tested here against the three ways it can
//! fail. Running it is a device step, and the device copy of the harness
//! (`C:\ProgramData\Summrise\pwout\panel-harness.html`) is the caller's to check after the download —
//! mixing the two made the old script fail on Linux for a file only a device has.
//!
//! ── THE MUTATION THAT MUST FAIL THIS GATE ───────────────────────────────────────────────────────────
//! MUTATION: `</body></html>\`;` -> `</body>\`;` (dropping the closing tag).
//! RESULT:   **exit 0 — IT DID NOT BITE, and the mutation was wrong rather than the gate.** A harness
//!            without `</html>` is still a valid harness, so the run stayed green: a mutation has to
//!            break the THING the gate checks, not something beside it.
//! MUTATION: a STRAY backtick inside the emitted template literal (`<meta charset="utf-8">\``).
//! RESULT:   exit 101 — "the emitter exited Some(1) — it did not write a harness (a stray backtick in
//!            the emitted template literal is the usual cause of exit 1). stderr: … content="${stamp}"> | ^^^^"
//! MUTATION: raise `MIN_BYTES` past the harness's real size (the truncated-emitter case, made certain).
//! RESULT:   exit 101 — "the emitted harness is only 1031028 bytes — that is not an app bundle"
//! AND THE MUTATION THAT MUST NOT: a clean tree passes and PRINTS the count it measured — 1,031,028 bytes
//! against a 200,000 floor — so the gate is visibly measuring rather than echoing its own constants.

use std::process::Command;

mod common;

/// The emitter whose exit code is the report, and the file it writes.
const EMITTER: &str = "agent/scripts/panel-render-audit.mjs";
const EMITTED: &str = "/tmp/panel-render-audit/panel-harness.html";

/// A floor, not a claim: an app bundle is hundreds of kilobytes, and a fragment is not. The old script
/// used the same number for the same reason.
const MIN_BYTES: usize = 200_000;

/// THE DEVICE'S HALF, KEPT HERE AS THE STRING A BROWSER EVALUATES — so the judgement below has a subject
/// and the flow that needs it can find it. It is not run by this file.
#[allow(dead_code)]
pub const DEVICE_BOOT_PROBE: &str = r#"(() => {
  const root = document.getElementById('root');
  return {
    nodes: root ? root.querySelectorAll('*').length : 0,
    updateLine: (() => {
      const sec = [...document.querySelectorAll('.settings-section')]
        .find((x) => /update/i.test((x.querySelector('h3') || {}).textContent || ''));
      const lat = sec ? sec.querySelector('.update-latest') : null;
      return lat ? lat.textContent.trim() : null;
    })(),
  };
})()"#;

/// The fixture's own version, which the stub is built to answer. The card reading anything else means the
/// stub did not answer — not that the panel is wrong.
const STUB_ANSWER: &str = "1.2.433 available";

/// A probe result, as the browser reports it.
pub struct Probe {
    pub nodes: usize,
    pub update_line: Option<String>,
}

/// `judgeBoot(probe, pageErrors)` — every way the boot can be a lie, in the order the old script checked
/// them, returning the problems rather than a bool so a failure says WHICH one.
pub fn judge_boot(probe: Option<&Probe>, page_errors: &[String]) -> Vec<String> {
    let mut problems = Vec::new();
    if !page_errors.is_empty() {
        problems.push(format!("page threw: {}", page_errors.join("; ")));
    }
    let nodes = probe.map(|p| p.nodes).unwrap_or(0);
    if nodes < 40 {
        problems.push(format!("the app did not mount (#root has {nodes} nodes)"));
    }
    let line = probe.and_then(|p| p.update_line.clone());
    if line.as_deref() != Some(STUB_ANSWER) {
        problems.push(format!(
            "the stub did not answer: the update card reads {} instead of {:?}",
            match line {
                Some(l) => format!("{l:?}"),
                None => "null".to_string(),
            },
            STUB_ANSWER
        ));
    }
    problems
}

#[test]
fn the_emitter_writes_a_harness_and_it_is_an_app_bundle() {
    let root = common::repo();
    let emitter = root.join(EMITTER);
    assert!(
        emitter.is_file(),
        "the emitter is missing ({}), so this gate cannot see what the sweep measures",
        emitter.display()
    );

    // The emitter reports by EXIT CODE: 2 wrote the harness, 1 is a broken template literal, and anything
    // else is a third thing this gate should not guess about.
    let out = Command::new("node")
        .arg(&emitter)
        .current_dir(&root)
        .output()
        .expect("node must be runnable — this gate shells out to the emitter, which is Node");
    let code = out.status.code();
    assert_eq!(
        code,
        Some(2),
        "the emitter exited {code:?} — it did not write a harness (a stray backtick in the emitted \
         template literal is the usual cause of exit 1). stderr: {}",
        String::from_utf8_lossy(&out.stderr).lines().take(3).collect::<Vec<_>>().join(" | ")
    );

    let html = std::fs::read_to_string(EMITTED)
        .unwrap_or_else(|e| panic!("the emitter exited 2 but {EMITTED} could not be read: {e}"));
    assert!(
        html.len() >= MIN_BYTES,
        "the emitted harness is only {} bytes — that is not an app bundle",
        html.len()
    );
    println!(
        "harness-boot: the emitter exited 2 and wrote {} bytes (floor {MIN_BYTES})",
        html.len()
    );
}

#[test]
fn the_boot_judgement_names_each_way_it_can_fail() {
    // The mutation that must NOT fail: a probe that mounted and a stub that answered.
    let good = Probe {
        nodes: 412,
        update_line: Some(STUB_ANSWER.to_string()),
    };
    assert!(
        judge_boot(Some(&good), &[]).is_empty(),
        "a clean probe must produce no problems"
    );

    // Every branch, so a clause that silently stops working is caught here rather than on a device.
    let threw = judge_boot(Some(&good), &["boom".to_string()]);
    assert!(
        threw.iter().any(|p| p.contains("page threw: boom")),
        "{threw:?}"
    );

    let unmounted = judge_boot(
        Some(&Probe {
            nodes: 12,
            update_line: good.update_line.clone(),
        }),
        &[],
    );
    assert!(
        unmounted.iter().any(|p| p.contains("did not mount")),
        "{unmounted:?}"
    );

    let no_answer = judge_boot(
        Some(&Probe {
            nodes: 412,
            update_line: Some("1.0.0 available".to_string()),
        }),
        &[],
    );
    assert!(
        no_answer.iter().any(|p| p.contains("did not answer")),
        "{no_answer:?}"
    );

    // A missing probe is the first problem, not a panic.
    let nothing = judge_boot(None, &[]);
    assert_eq!(
        nothing.len(),
        2,
        "a missing probe is two findings: no mount, no answer: {nothing:?}"
    );
}
