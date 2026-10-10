//! THE VERDICT — THE CHECK LIST, THE THREE NUMBERS, AND THE EXIT CODE.
//!
//! **THREE STATES, AND THE THIRD IS NOT A POLITE FOURTH.** A check whose arm does not apply on this device is
//! SKIPPED: not passed (which would be a lie) and not failed (which would report a defect that is not one).
//! That distinction cost eleven rounds on the `mcp` thread, so it is a first-class outcome here too, and the
//! summary counts it separately from BOTH numbers.
//!
//! **ZERO CHECKS IS NOT A PASS.** `exit(failed.length ? 1 : 0)` once read an empty result list as success, so
//! a run that executed NOTHING printed `== 0/0 passed ==` and exited 0. The exit codes are 0 ran and all
//! passed, 1 ran and something failed, 2 DID NOT RUN — a bad `--only` (refused in `plan.rs`), a rows document
//! that cannot be read, or no checks executed at all.

use crate::rows::Got;
use crate::text::ascii;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    Pass,
    Fail,
    Skip,
}

#[derive(Clone, Debug)]
pub struct Check {
    pub name: String,
    pub outcome: Outcome,
    pub detail: String,
}

impl Check {
    pub fn pass(name: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            outcome: Outcome::Pass,
            detail: detail.into(),
        }
    }

    pub fn fail(name: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            outcome: Outcome::Fail,
            detail: detail.into(),
        }
    }

    pub fn skip(name: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            outcome: Outcome::Skip,
            detail: reason.into(),
        }
    }

    /// A condition already computed, with its detail already composed — the shape every ported check ends in.
    pub fn pass_or_fail(name: impl Into<String>, ok: bool, detail: impl Into<String>) -> Self {
        if ok {
            Self::pass(name, detail)
        } else {
            Self::fail(name, detail)
        }
    }

    /// A check over one observation, from the three states that observation can be in. Returns `None` when the
    /// driver never got here, which is how a section stops: the suite's early returns used to express exactly
    /// this by never registering the check.
    ///
    /// A RECORDED TRANSPORT FAILURE IS NAMED, NOT SWALLOWED. In the JS the call threw and the suite aborted
    /// with one anonymous `suite` failure; here the section stops at the same place and the failure says which
    /// call it was.
    pub fn on(
        name: impl Into<String>,
        got: Got<'_>,
        judge: impl FnOnce(&serde_json::Value) -> (bool, String),
    ) -> Option<Self> {
        let name = name.into();
        match got {
            Got::Missing => None,
            Got::Failed(e) => Some(Self::fail(name, format!("the call failed: {e}"))),
            Got::Ok(v) => {
                let (ok, detail) = judge(v);
                Some(if ok {
                    Self::pass(name, detail)
                } else {
                    Self::fail(name, detail)
                })
            }
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Verdict {
    pub checks: Vec<Check>,
    /// Lines that are not checks: the click triage, and anything else a reader needs to interpret a failure.
    /// Each carries the number of checks that preceded it, so the triage prints where the JS printed it —
    /// immediately after the check that missed — rather than being swept to the end of the report.
    pub notes: Vec<(usize, String)>,
}

impl Verdict {
    pub fn push(&mut self, c: Check) {
        self.checks.push(c);
    }

    /// Push only when a check was produced. `Check::on` answers `None` when the driver never got there.
    pub fn push_some(&mut self, c: Option<Check>) -> bool {
        match c {
            Some(c) => {
                self.checks.push(c);
                true
            }
            None => false,
        }
    }

    /// The one-line form every section uses: judge one observation. Answers `false` when the driver never got
    /// here, which is the caller's signal to stop the section.
    pub fn on(
        &mut self,
        name: impl Into<String>,
        got: Got<'_>,
        judge: impl FnOnce(&serde_json::Value) -> (bool, String),
    ) -> bool {
        self.push_some(Check::on(name, got, judge))
    }

    pub fn note(&mut self, line: impl Into<String>) {
        self.notes.push((self.checks.len(), line.into()));
    }

    pub fn ran(&self) -> usize {
        self.checks
            .iter()
            .filter(|c| c.outcome != Outcome::Skip)
            .count()
    }

    pub fn passed(&self) -> usize {
        self.checks
            .iter()
            .filter(|c| c.outcome == Outcome::Pass)
            .count()
    }

    pub fn failed(&self) -> usize {
        self.checks
            .iter()
            .filter(|c| c.outcome == Outcome::Fail)
            .count()
    }

    pub fn skipped(&self) -> usize {
        self.checks
            .iter()
            .filter(|c| c.outcome == Outcome::Skip)
            .count()
    }

    /// 0 = ran and all passed, 1 = ran and something failed, 2 = DID NOT RUN.
    pub fn exit_code(&self) -> i32 {
        if self.checks.is_empty() {
            return 2;
        }
        if self.failed() > 0 {
            1
        } else {
            0
        }
    }

    /// The lines the suite printed, in the order it printed them, plus the summary. A `note` goes where the
    /// JS put its triage line: among the checks that produced it.
    pub fn lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut notes = self.notes.iter().peekable();
        for (i, c) in self.checks.iter().enumerate() {
            while let Some((at, line)) = notes.peek() {
                if *at > i {
                    break;
                }
                out.push(format!("  [triage] {}", ascii(line)));
                notes.next();
            }
            let tag = match c.outcome {
                Outcome::Pass => "PASS",
                Outcome::Fail => "FAIL",
                Outcome::Skip => "SKIP",
            };
            let detail = ascii(&c.detail);
            let line = if c.detail.is_empty() {
                format!("{tag} {}", c.name)
            } else {
                format!("{tag} {}  -- {}", c.name, detail)
            };
            out.push(line);
        }
        for (_, line) in notes {
            out.push(format!("  [triage] {}", ascii(line)));
        }
        let skipped = self.skipped();
        out.push(format!(
            "\n== {}/{} passed =={}",
            self.passed(),
            self.ran(),
            if skipped > 0 {
                format!(" ({skipped} skipped)")
            } else {
                String::new()
            }
        ));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rows::Got;
    use serde_json::json;

    #[test]
    fn a_skip_is_in_neither_number() {
        let mut v = Verdict::default();
        v.push(Check::pass("a", ""));
        v.push(Check::skip("b", "not this arm"));
        assert_eq!((v.ran(), v.passed(), v.failed(), v.skipped()), (1, 1, 0, 1));
        assert_eq!(v.exit_code(), 0);
        assert!(v.lines()[1].starts_with("SKIP b  -- not this arm"));
        assert!(v.lines()[2].contains("== 1/1 passed == (1 skipped)"));
    }

    #[test]
    fn zero_checks_is_exit_2_and_not_a_pass() {
        let v = Verdict::default();
        assert_eq!(v.exit_code(), 2);
        assert!(v.lines()[0].contains("== 0/0 passed =="));
    }

    #[test]
    fn a_failure_is_exit_1_even_alongside_passes_and_skips() {
        let mut v = Verdict::default();
        v.push(Check::pass("a", ""));
        v.push(Check::skip("b", ""));
        v.push(Check::fail("c", "no"));
        assert_eq!(v.exit_code(), 1);
    }

    #[test]
    fn a_missing_observation_produces_no_check_and_a_failure_names_the_call() {
        assert!(Check::on("x", Got::Missing, |_| (true, String::new())).is_none());
        let c = Check::on("x", Got::Failed("tool terminal_open HTTP 500"), |_| {
            (true, String::new())
        })
        .unwrap();
        assert_eq!(c.outcome, Outcome::Fail);
        assert!(c.detail.contains("terminal_open HTTP 500"), "{}", c.detail);
    }

    #[test]
    fn the_detail_is_ascii_sanitized_on_the_way_out() {
        let c = Check::fail("x", json!({"error": "系统找不到指定的文件"}).to_string());
        let mut v = Verdict::default();
        v.push(c);
        assert!(!v.lines()[0].contains('系'), "{}", v.lines()[0]);
        assert!(v.lines()[0].contains("???"), "{}", v.lines()[0]);
    }
}
