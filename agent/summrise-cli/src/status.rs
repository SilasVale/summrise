//! THE STATUS REPORT'S CONTENT.
//!
//! Ported from `statusReport` in `agent/summrise-agent-npm/src/summrise.ts`, and the content IS the
//! feature: the failure mode is an OMISSION — a `status` that prints three plausible lines while
//! leaving out the release version looks perfectly healthy.
//!
//! INCIDENT this exists for: a device update was issued through the sanctioned flow and the
//! connection dropped, which is the DOCUMENTED behaviour of a successful swap. It was therefore
//! read as "the update started". It had not: no busy marker, no staged `.new.exe`, no
//! `update start` line — the command never reached the device, and the transport failure was
//! indistinguishable from the success signal. `summrise status` reported RUNNING / install dir /
//! panel URL and NOTHING about the release or a pending swap, so it could not answer the only
//! question that mattered.

use crate::update::busy_is_fresh;

/// The facts the report renders. Every `None` in here is a field the CLI could not READ, and each
/// renders as a sentence about the failure rather than as a verdict — the rule this file applies
/// four times.
#[derive(Debug, Clone, Default)]
pub struct StatusFacts {
    /// `None` when the process list could not be READ — which is NOT "stopped".
    pub agent_running: Option<bool>,
    pub install_dir: String,
    pub exe_exists: bool,
    pub port: u16,
    /// Contents of `etc\.summrise-release`, trimmed; `None` when absent/unreadable.
    pub release_version: Option<String>,
    /// mtime of the update-busy marker, or `None` when there is no marker.
    pub update_marker_ms: Option<i64>,
    /// The marker EXISTS but could not be read. `update_marker_ms == None` then means "could not
    /// look", NOT "nothing in flight".
    pub update_marker_unreadable: bool,
    /// This CLI's own version — what `summrise update` would install.
    pub package_version: String,
    /// The newest release the CDN advertises, or `None` when it could not be read. `None` is NOT
    /// "up to date" — see the drift line.
    pub latest_version: Option<String>,
    pub now_ms: i64,
}

/// The report, line by line.
pub fn status_report(f: &StatusFacts) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    out.push(
        match f.agent_running {
            None => "status: UNKNOWN -- the process list could not be read (tasklist failed); this is not a verdict",
            Some(true) => "status: RUNNING",
            Some(false) => "status: STOPPED",
        }
        .to_string(),
    );
    out.push(format!("install dir: {}", f.install_dir));
    out.push(format!(
        "panel: {}",
        if f.exe_exists {
            format!("http://127.0.0.1:{}/panel/", f.port)
        } else {
            "(not installed)".to_string()
        }
    ));
    // A device without a release marker is not "on some version" — it is a device whose version is
    // UNKNOWN (a fresh box, or an install predating the marker). Printing this CLI's version here
    // would be a fabricated fact.
    out.push(format!(
        "release: {}",
        f.release_version
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "unknown (no release marker)".to_string())
    ));
    out.push(format!("this CLI: {}", f.package_version));

    match f.update_marker_ms {
        None => out.push(
            if f.update_marker_unreadable {
                "update: state UNKNOWN -- the busy marker exists but could not be read (permissions or a lock); do not assume no update is running"
            } else {
                "update: none in flight"
            }
            .to_string(),
        ),
        Some(ms) if busy_is_fresh(ms, f.now_ms) => {
            let secs = ((f.now_ms - ms) as f64 / 1000.0).round().max(0.0) as i64;
            out.push(format!(
                "update: IN FLIGHT (marker {secs}s old -- a swap is running now; the connection drops for ~10s)"
            ));
        }
        Some(ms) => {
            let mins = ((f.now_ms - ms) as f64 / 60_000.0).round() as i64;
            out.push(format!(
                "update: a previous update STARTED AND DID NOT FINISH (marker {mins} min old). \
                 Check the log tail, then re-run 'summrise update' -- a stale marker is safe to overwrite."
            ));
        }
    }

    // The drift line: what a human actually wants from `status` after an update. Only claimed when
    // the running version is KNOWN — otherwise the comparison would be against a guess.
    let release = f.release_version.clone().filter(|s| !s.is_empty());
    if let Some(rel) = &release {
        if *rel != f.package_version {
            out.push(format!(
                "update: device runs {rel}, this CLI is {} -- run 'summrise update' to swap, then 'summrise status' again to confirm.",
                f.package_version
            ));
        }
    }

    // THE DELIVERY GAP, WHICH NOTHING ELSE IN THIS REPO CHECKS. `release` answers "what is this
    // device running"; it did NOT answer "is that current", and the two questions are answered by
    // different machines. `None` IS NOT "UP TO DATE": if the CDN could not be read the line says so
    // instead of staying silent, because silence here reads exactly like agreement.
    match &f.latest_version {
        None => out.push(
            "latest: could NOT be checked (the release CDN did not answer) -- this says nothing about whether the device is current"
                .to_string(),
        ),
        Some(latest) if release.is_some() && release.as_deref() != Some(latest.as_str()) => {
            let rel = release.clone().unwrap_or_default();
            out.push(format!(
                "latest: {latest} is on the CDN -- THIS DEVICE IS BEHIND by {}; run 'summrise update'",
                crate::update::behind_by(&rel, latest)
            ));
        }
        Some(latest) => {
            if release.is_some() {
                out.push(format!("latest: {latest} (this device is current)"));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> StatusFacts {
        StatusFacts {
            agent_running: Some(true),
            install_dir: "D:\\Summrise".to_string(),
            exe_exists: true,
            port: 18080,
            release_version: Some("1.2.359".to_string()),
            update_marker_ms: None,
            update_marker_unreadable: false,
            package_version: "1.2.359".to_string(),
            latest_version: Some("1.2.359".to_string()),
            now_ms: 1_700_000_000_000,
        }
    }

    /// Oracle: cli.test.mjs:1259 — "statusReport: an UNREADABLE process list is not 'STOPPED'".
    ///
    /// `agentRunning: null` means the process list could not be READ. Reporting STOPPED there is a
    /// claim of ABSENCE from a failed read — and for the agent itself that is the worst possible
    /// answer ("your device is down" when nobody looked).
    #[test]
    fn an_unreadable_process_list_is_not_stopped() {
        let unknown = status_report(&StatusFacts {
            agent_running: None,
            ..base()
        })
        .join("\n");
        assert!(unknown.contains("UNKNOWN"), "{unknown}");
        assert!(unknown.contains("not a verdict"), "{unknown}");
        assert!(
            !unknown.contains("status: STOPPED"),
            "must not claim the agent is stopped when the probe never answered"
        );
        // The two real answers must still be exact.
        assert!(status_report(&StatusFacts {
            agent_running: Some(true),
            ..base()
        })
        .join("\n")
        .contains("status: RUNNING"));
        assert!(status_report(&StatusFacts {
            agent_running: Some(false),
            ..base()
        })
        .join("\n")
        .contains("status: STOPPED"));
    }

    /// Oracle: cli.test.mjs:1315 — "statusReport: reports the running release and a FAILED update,
    /// not just RUNNING".
    #[test]
    fn status_reports_the_release_and_a_failed_update() {
        let now = 1_700_000_000_000i64;

        // (a) An update was attempted and never finished: the marker survives with its original
        //     mtime. This is THE diagnostic the incident lacked.
        let stalled = status_report(&StatusFacts {
            agent_running: Some(true),
            install_dir: "D:\\Summrise".to_string(),
            exe_exists: true,
            port: 18080,
            release_version: Some("1.2.321".to_string()),
            update_marker_ms: Some(now - 27 * 60_000),
            package_version: "1.2.322".to_string(),
            now_ms: now,
            ..Default::default()
        })
        .join("\n");
        assert!(
            stalled.contains("1.2.321"),
            "must name the version the device runs"
        );
        assert!(
            stalled.to_lowercase().contains("did not finish"),
            "a marker past the freshness window means an update STARTED AND DID NOT FINISH"
        );
        // The DRIFT line specifically — the one that answers "am I up to date?".
        let drift_line = stalled
            .lines()
            .find(|l| l.contains("device runs 1.2.321"))
            .unwrap_or_else(|| {
                panic!("must state the DRIFT, not merely print both numbers: {stalled}")
            });
        assert!(drift_line.contains("1.2.322") || stalled.contains("this CLI: 1.2.322"));

        // (b) Nothing in flight and the device matches this CLI: say so plainly.
        let current = status_report(&StatusFacts {
            release_version: Some("1.2.322".to_string()),
            package_version: "1.2.322".to_string(),
            update_marker_ms: None,
            now_ms: now,
            ..base()
        })
        .join("\n");
        assert!(current.contains("1.2.322"));
        assert!(
            !current.to_lowercase().contains("did not finish"),
            "an absent marker must NOT be reported as a failed update"
        );

        // (c) An update IS in flight (fresh marker): distinct from both above.
        let in_flight = status_report(&StatusFacts {
            release_version: Some("1.2.321".to_string()),
            update_marker_ms: Some(now - 30_000),
            package_version: "1.2.322".to_string(),
            now_ms: now,
            ..base()
        })
        .join("\n");
        assert!(
            in_flight.to_lowercase().contains("in flight"),
            "{in_flight}"
        );

        // (d) An install with no release marker at all says "unknown" rather than inventing one.
        let unknown = status_report(&StatusFacts {
            agent_running: Some(false),
            release_version: None,
            package_version: "1.2.322".to_string(),
            now_ms: now,
            ..base()
        })
        .join("\n");
        assert!(unknown.contains("STOPPED"));
        assert!(unknown.to_lowercase().contains("unknown"));
    }

    /// Oracle: cli.test.mjs:2640 — "delivery drift: NAMES the gap when the device is behind the
    /// CDN".
    #[test]
    fn delivery_drift_names_the_gap() {
        let out = status_report(&StatusFacts {
            release_version: Some("1.2.340".to_string()),
            package_version: "1.2.340".to_string(),
            latest_version: Some("1.2.345".to_string()),
            ..base()
        })
        .join("\n");
        assert!(out.contains("THIS DEVICE IS BEHIND by 5 releases"), "{out}");
        assert!(out.contains("1.2.345"), "{out}");
    }

    /// Oracle: cli.test.mjs:2648 — "delivery drift: says CURRENT only when it actually compared".
    #[test]
    fn delivery_drift_says_current_only_when_compared() {
        let out = status_report(&StatusFacts {
            release_version: Some("1.2.340".to_string()),
            package_version: "1.2.340".to_string(),
            latest_version: Some("1.2.340".to_string()),
            ..base()
        })
        .join("\n");
        assert!(out.contains("this device is current"), "{out}");
        assert!(!out.contains("BEHIND"), "{out}");
    }

    /// Oracle: cli.test.mjs:2656 — "delivery drift: an unreadable CDN is NOT agreement".
    #[test]
    fn an_unreadable_cdn_is_not_agreement() {
        let out = status_report(&StatusFacts {
            latest_version: None,
            ..base()
        })
        .join("\n");
        assert!(out.contains("could NOT be checked"), "{out}");
        assert!(
            out.contains("says nothing about whether the device is current"),
            "{out}"
        );
        assert!(!out.contains("is current)"), "{out}");
    }

    /// The marker's THIRD state: it exists but could not be read. `update_marker_ms == None` then
    /// means "could not look", NOT "nothing in flight" — the same rule the fields around it follow,
    /// and the reason `rollback --clear` had to be fixed too.
    #[test]
    fn an_unreadable_busy_marker_is_not_nothing_in_flight() {
        let out = status_report(&StatusFacts {
            update_marker_ms: None,
            update_marker_unreadable: true,
            ..base()
        })
        .join("\n");
        assert!(out.contains("state UNKNOWN"), "{out}");
        assert!(out.contains("do not assume no update is running"), "{out}");
    }
}
