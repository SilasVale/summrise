//! What the launcher can prove about an install **without starting it or the service**.
//!
//! An operator's first question about a device that misbehaves is "what is actually on disk?" — and
//! until this existed the only answers were a running app (when it ran) or a support session reading
//! three files by hand. The report here is the same facts the supervisor acts on, checked in the same
//! order, printed as lines that can be pasted into a ticket.
//!
//! **Read-only.** Every check opens files and hashes them; nothing is repaired, moved or written. A
//! self-check that changes what it inspects is a diagnostic that can be the incident.
//!
//! **The same deciders, not a second opinion.** The generation question is [`resolve_baseline`], the
//! app-copy question is the supervisor's own hash comparison, and the sidecar question is
//! [`load_sidecar`]. A report that disagreed with the supervisor would be worse than no report.

use std::fs;
use std::path::Path;

use crate::paths::{app_exe, repair_log};
use crate::sha256_bytes;
use crate::{inspect_generation, resolve_baseline, BaselineChoice, Generation};

/// One line of the report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Check {
    /// The fact being checked, stable enough to grep for.
    pub name: &'static str,
    /// Whether the fact is what it should be.
    pub ok: bool,
    /// What was found, in the operator's terms.
    pub detail: String,
}

/// The whole report, in the order the supervisor would care about it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    /// One entry per check, in a stable order.
    pub checks: Vec<Check>,
}

impl Report {
    /// Whether every check passed.
    pub fn ok(&self) -> bool {
        self.checks.iter().all(|check| check.ok)
    }
}

impl std::fmt::Display for Report {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for check in &self.checks {
            let mark = if check.ok { "ok  " } else { "FAIL" };
            writeln!(f, "  {:<18} {mark} {}", check.name, check.detail)?;
        }
        Ok(())
    }
}

/// Inspect an install, and the state document when `data` is given.
pub fn inspect(install: &Path, data: Option<&Path>) -> Report {
    let mut checks = Vec::new();

    // 1. Which generation boots. This is the same call the supervisor makes at start.
    let choice = resolve_baseline(install);
    match &choice {
        BaselineChoice::Current(baseline) => checks.push(Check {
            name: "baseline.current",
            ok: true,
            detail: format!(
                "{} {} (sha256 {})",
                baseline.generation.label(),
                baseline.sidecar.version,
                short(&baseline.sidecar.sha256)
            ),
        }),
        BaselineChoice::Previous(baseline) => checks.push(Check {
            name: "baseline.current",
            ok: false,
            detail: format!(
                "unusable; the previous generation {} would boot {} and repair current",
                baseline.sidecar.version, baseline.sidecar.version
            ),
        }),
        BaselineChoice::NeedsHuman(reason) => checks.push(Check {
            name: "baseline.current",
            ok: false,
            detail: reason.clone(),
        }),
    }

    checks.push(check_generation(install, Generation::Previous));

    // 2. The app copy the supervisor runs: it must hash to the generation's sidecar.
    if let Some(baseline) = choice.baseline() {
        let app = app_exe(install);
        match fs::read(&app) {
            Ok(bytes) => {
                let actual = sha256_bytes(&bytes);
                let ok = actual.eq_ignore_ascii_case(&baseline.sidecar.sha256);
                checks.push(Check {
                    name: "app.copy",
                    ok,
                    detail: if ok {
                        format!("{} matches {}", app.display(), short(&actual))
                    } else {
                        format!(
                            "{} hashes to {} and should be {}",
                            app.display(),
                            short(&actual),
                            short(&baseline.sidecar.sha256)
                        )
                    },
                });
            }
            Err(error) => checks.push(Check {
                name: "app.copy",
                ok: false,
                detail: format!("{} cannot be read ({error})", app.display()),
            }),
        }
    }

    // 3. The repair record: a device that repaired itself says when.
    match fs::metadata(repair_log(install)) {
        Ok(metadata) => {
            let bytes = metadata.len();
            let records = fs::read_to_string(repair_log(install))
                .map(|text| text.lines().count())
                .unwrap_or(0);
            checks.push(Check {
                name: "repair.log",
                ok: true,
                detail: format!("{records} record(s), {bytes} bytes"),
            });
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => checks.push(Check {
            name: "repair.log",
            ok: true,
            detail: "no repair has been recorded".to_owned(),
        }),
        Err(error) => checks.push(Check {
            name: "repair.log",
            ok: false,
            detail: format!("cannot be read ({error})"),
        }),
    }

    // 4. The state document, when the caller can name the data directory.
    match data {
        Some(data) => checks.push(check_state(data)),
        None => checks.push(Check {
            name: "state",
            ok: true,
            detail: "not inspected (no --data)".to_owned(),
        }),
    }

    Report { checks }
}

/// What a generation directory holds, without deciding whether it would boot.
fn check_generation(install: &Path, generation: Generation) -> Check {
    match inspect_generation(install, generation) {
        Ok(baseline) => Check {
            name: match generation {
                Generation::Previous => "baseline.previous",
                // `inspect` only asks for `previous`; the current pair is `resolve_baseline`'s job.
                _ => "baseline.other",
            },
            ok: true,
            detail: format!(
                "{} {}",
                baseline.sidecar.version,
                short(&baseline.sidecar.sha256)
            ),
        },
        Err(_) => Check {
            name: match generation {
                Generation::Previous => "baseline.previous",
                _ => "baseline.other",
            },
            ok: false,
            detail: format!(
                "no usable {} generation (an update could not roll back)",
                generation.label()
            ),
        },
    }
}

/// The state document's version, schema and rollback memory.
fn check_state(data: &Path) -> Check {
    let path = data.join("state.json");
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Check {
                name: "state",
                ok: false,
                detail: format!(
                    "{} does not exist; the device has not started here",
                    path.display()
                ),
            }
        }
        Err(error) => {
            return Check {
                name: "state",
                ok: false,
                detail: format!("{} cannot be read ({error})", path.display()),
            }
        }
    };
    match summrise_core::State::parse(&text) {
        Ok(state) => Check {
            name: "state",
            ok: true,
            detail: format!(
                "{} on {}, schema {}, {} rolled back",
                state.install.version,
                state.install.channel,
                state.schema,
                state.rolled_back.len()
            ),
        },
        Err(error) => Check {
            name: "state",
            ok: false,
            detail: format!(
                "{} is not a document this build reads ({error})",
                path.display()
            ),
        },
    }
}

/// The first twelve hex characters: enough to identify a digest in a sentence.
fn short(sha256: &str) -> String {
    let text: String = sha256.chars().take(12).collect();
    if sha256.chars().count() > 12 {
        format!("{text}…")
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::generation_exe;
    use std::path::PathBuf;

    /// A generation pair: bytes on disk and a sidecar naming their digest.
    fn write_generation(
        install: &Path,
        generation: Generation,
        version: &str,
        bytes: &[u8],
    ) -> PathBuf {
        let exe = generation_exe(install, generation.dir_name());
        fs::create_dir_all(exe.parent().expect("a generation has a directory")).expect("mkdir");
        fs::write(&exe, bytes).expect("write the generation");
        let sidecar = serde_json::json!({
            "version": version,
            "channel": "stable",
            "sha256": sha256_bytes(bytes),
            "signature": "00".repeat(64),
        });
        fs::write(
            crate::paths::generation_sidecar(install, generation.dir_name()),
            sidecar.to_string(),
        )
        .expect("write the sidecar");
        exe
    }

    fn tempdir(tag: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("summrise-self-{}-{}", std::process::id(), tag));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("temp dir");
        path
    }

    #[test]
    fn a_healthy_install_reports_every_check_ok() {
        let root = tempdir("healthy");
        let install = root.join("install");
        let current = write_generation(&install, Generation::Current, "2.0.3", b"current bytes");
        write_generation(&install, Generation::Previous, "2.0.2", b"previous bytes");
        // The app copy the supervisor would run.
        fs::copy(&current, app_exe(&install)).expect("copy the app");

        let report = inspect(&install, None);
        assert!(report.ok(), "a healthy install passes:\n{report}");
        let names: Vec<&str> = report.checks.iter().map(|check| check.name).collect();
        assert_eq!(
            names,
            [
                "baseline.current",
                "baseline.previous",
                "app.copy",
                "repair.log",
                "state"
            ]
        );
    }

    #[test]
    fn a_missing_app_copy_is_named_not_guessed() {
        let root = tempdir("noapp");
        let install = root.join("install");
        write_generation(&install, Generation::Current, "2.0.3", b"current bytes");

        let report = inspect(&install, None);
        let app = report
            .checks
            .iter()
            .find(|check| check.name == "app.copy")
            .expect("the app copy is checked");
        assert!(!app.ok);
        assert!(app.detail.contains("cannot be read"), "{}", app.detail);
    }

    #[test]
    fn a_corrupt_current_generation_names_the_previous_one() {
        let root = tempdir("corrupt");
        let install = root.join("install");
        write_generation(&install, Generation::Current, "2.0.3", b"current bytes");
        write_generation(&install, Generation::Previous, "2.0.2", b"previous bytes");
        fs::write(generation_exe(&install, "current"), b"corrupted").expect("corrupt it");

        let report = inspect(&install, None);
        let current = report
            .checks
            .iter()
            .find(|check| check.name == "baseline.current")
            .expect("the current generation is checked");
        assert!(!current.ok);
        assert!(
            current.detail.contains("2.0.2"),
            "the report names what would boot instead: {}",
            current.detail
        );
    }

    #[test]
    fn the_state_line_reports_the_version_and_the_rollback_memory() {
        let root = tempdir("state");
        let install = root.join("install");
        let data = root.join("data");
        let current = write_generation(&install, Generation::Current, "2.0.3", b"current bytes");
        fs::copy(&current, app_exe(&install)).expect("copy the app");
        fs::create_dir_all(&data).expect("data dir");
        let document = serde_json::json!({
            "schema": 1,
            "install": {"dir": install.to_string_lossy(), "version": "2.0.3", "channel": "stable"},
            "baseline": {
                "version": "2.0.3",
                "sha256": "00".repeat(32),
                "exe": "/install/baseline/current/summrise-device.exe",
            },
            "service": {"state": "running", "pid": 1, "startedAt": "2026-01-01T00:00:00Z"},
            "lastVerdict": {"ts": "2026-01-01T00:00:00Z", "outcome": "ok", "detail": "started 2.0.3"},
            "sessions": [],
            "update": {"phase": "idle", "to": null, "outcome": null, "lastResultAtMs": null},
            "rolledBack": [],
            "repairOffset": 0
        });
        fs::write(data.join("state.json"), document.to_string()).expect("write the state");

        let report = inspect(&install, Some(&data));
        let line = report
            .checks
            .iter()
            .find(|check| check.name == "state")
            .expect("the state is checked");
        assert!(line.ok, "{}", line.detail);
        assert!(line.detail.contains("2.0.3"), "{}", line.detail);
    }
}
