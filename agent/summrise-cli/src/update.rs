//! THE VERSION COMPARISON, THE RELEASE CHANNELS, THE UPDATE DECISION, AND THE MARKER THAT MUST BE
//! EARNED.
//!
//! Ported from `busyIsFresh`, `versionTriple`, `newestOf`, `isBehind`, `behindBy`,
//! `updateWouldNotMove`, `latestCdnVersion`, `latestNpmVersion`, `latestReleaseVersion`,
//! `rollbackVersionOk`, `awaitReleaseMarker` and `releaseMarkerVerdict` in
//! `agent/summrise-agent-npm/src/summrise.ts`.

use crate::components::cdn_base;
use crate::host::Host;

/// TEN MINUTES, AND IT MUST EQUAL THE AGENT'S `BUSY_STALE_SECS`.
///
/// It did not: the CLI reclaimed an abandoned marker at ten minutes while
/// `agent/src/plugins/update/tools.rs` refused for an hour, so at eleven minutes this side
/// OVERWROTE a marker the agent still honoured and a CLI update could start alongside a
/// console-launched one — interleaving `Copy-Item` on `*.new`, the half-written-exe hazard this
/// marker exists to prevent. The test below pins the two numbers against each other, because no
/// test inside either language can see the other's.
pub const BUSY_STALE_MINUTES: i64 = 10;

/// The update mutual-exclusion window. A marker older than this is an update that STARTED AND DID
/// NOT FINISH, and is safe to overwrite.
pub fn busy_is_fresh(mtime_ms: i64, now_ms: i64) -> bool {
    now_ms - mtime_ms < BUSY_STALE_MINUTES * 60 * 1000
}

/// `Number(v)` in JavaScript, which is what `versionTriple` maps over the split parts.
///
/// The differences from Rust's `f64::from_str` are real and reachable: `""` is `0`, `" 1"` is `1`,
/// `"0x10"` is `16`, and `"1e3"` is `1000`. A port that used `parse::<i64>()` would call
/// `1..2` (which JavaScript reads as `[1, 0, 2]`) unparseable, and the two sides would then disagree
/// about a version string — the one thing this module exists to compare.
fn js_number(s: &str) -> Option<f64> {
    let t = s.trim();
    if t.is_empty() {
        return Some(0.0);
    }
    if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        return u64::from_str_radix(hex, 16).ok().map(|v| v as f64);
    }
    t.parse::<f64>().ok().filter(|v| v.is_finite())
}

/// ONE PARSE, TWO PRESENTATIONS. The comparison between two `x.y.z` strings was written out once,
/// for the sentence `status` prints; the update guard needed the same fact as a BOOLEAN, and its
/// first version tested the sentence's truthiness — where "0 releases" and "-1 releases" are both
/// TRUTHY, so it would have refused every update, including the correct one.
pub fn version_triple(v: &str) -> Option<[f64; 3]> {
    let parts: Vec<f64> = v.split('.').map(js_number).collect::<Option<Vec<f64>>>()?;
    if parts.len() == 3 {
        Some([parts[0], parts[1], parts[2]])
    } else {
        None
    }
}

/// The newer of two release versions; either may be unknown. Silence is never agreement: with one
/// channel silent the other still answers, and with both silent the answer is `None`.
pub fn newest_of(a: Option<&str>, b: Option<&str>) -> Option<String> {
    // `if (!a) return b; if (!b) return a;` — the TypeScript's FALSY test, which an empty string
    // satisfies. Ported literally, and the parity check is what noticed the difference: a port that
    // "cleaned up" the empty string to `None` returns a different answer for `newestOf(null, "")`.
    //
    // THE BEHAVIOUR IS A DEFECT AND IT IS REPORTED, NOT FIXED: `""` is not `null`, so a caller that
    // renders `latestVersion == null` as "could NOT be checked" would render `""` as a channel that
    // answered. Neither real caller can produce it today (`latestCdnVersion` and `latestNpmVersion`
    // both answer `null`), which is why it has never been seen.
    if a.map(|s| s.is_empty()).unwrap_or(true) {
        return b.map(String::from); // verbatim, INCLUDING an empty string
    }
    if b.map(|s| s.is_empty()).unwrap_or(true) {
        return a.map(String::from);
    }
    let (Some(a), Some(b)) = (a, b) else {
        return None;
    };
    let (ta, tb) = (version_triple(a), version_triple(b));
    match (ta, tb) {
        (None, _) => Some(b.to_string()),
        (_, None) => Some(a.to_string()),
        (Some(ta), Some(tb)) => {
            for i in 0..3 {
                if ta[i] != tb[i] {
                    return Some(if ta[i] > tb[i] { a } else { b }.to_string());
                }
            }
            Some(a.to_string())
        }
    }
}

/// Is `latest` ahead of `device` on the SAME release line?
///
/// The update guard needs this as a BOOLEAN: an earlier version asked `behindBy`, whose answer is a
/// PHRASE — truthy for "0 releases" and "-1 releases" alike — so it would have refused every update,
/// including the correct one.
pub fn is_behind(device: &str, latest: &str) -> bool {
    let (Some(a), Some(b)) = (version_triple(device), version_triple(latest)) else {
        return false;
    };
    if a[0] != b[0] || a[1] != b[1] {
        return false;
    }
    b[2] > a[2]
}

/// How far behind, in patch releases, WITHIN THE SAME MINOR — a PHRASE, never a fabricated number.
///
/// The last-5-per-minor CDN prune means a cross-minor jump is a different operation anyway
/// (`summrise rollback` refuses it for the same reason), so it is stated as such.
pub fn behind_by(device: &str, latest: &str) -> String {
    let (Some(a), Some(b)) = (version_triple(device), version_triple(latest)) else {
        return "an unknown number of releases".to_string();
    };
    if a[0] != b[0] || a[1] != b[1] {
        return "a release line, not a patch count".to_string();
    }
    let n = b[2] - a[2];
    if n == 1.0 {
        "1 release".to_string()
    } else {
        format!("{} releases", n as i64)
    }
}

/// WOULD THIS UPDATE MOVE THE DEVICE AT ALL? The parity fact, and it needs NO NETWORK.
///
/// The CLI stamps `<install>\.summrise-release` with ITS OWN version, and the device's `agent_update`
/// reads that file as the local version — so when the device is already on the version this CLI
/// carries, the stamp equals what is there, the swap installs the same build, and nothing moves.
/// That is the measured defect ("update requested 1.2.438 -> 1.2.438", "copy ok=True", release
/// unchanged). THE GUARD THAT ALREADY EXISTS CANNOT SEE IT: it compares the CLI against the RELEASE
/// CHANNEL, and when the CDN is unreadable `latest` is empty and the whole check is skipped, so a
/// network blip re-opens the defect. This one compares two facts already on the machine.
pub fn update_would_not_move(from_version: &str, self_version: &str) -> bool {
    !from_version.is_empty() && from_version == self_version
}

/// `summrise rollback <version>` gate — a plain dotted triple only, because the tgz URL interpolates
/// it and anything else could escape the `/summrise-agent/` prefix.
pub fn rollback_version_ok(v: &str) -> bool {
    let parts: Vec<&str> = v.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

/// The newest version the release CDN advertises, or `None` when it cannot be read.
///
/// Never throws and never guesses — the caller renders `None` as "could not be checked", because a
/// silent failure here is indistinguishable from "current". A body that is not JSON is NOT a
/// version.
pub fn latest_cdn_version(host: &dyn Host) -> Option<String> {
    let base = cdn_base(host);
    let body = host.http_get(&format!("{base}/api/version"), 3)?;
    let j: serde_json::Value = serde_json::from_str(&body).ok()?;
    let v = j
        .get("version")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    if v.is_empty() {
        None
    } else {
        Some(v)
    }
}

/// The registry's own answer to "what is latest", asked in the smallest way npm offers:
/// `/-/package/<name>/dist-tags` is a few bytes, where the packument is a document.
///
/// A dist-tag can point at a prerelease; the device's update path only understands `x.y.z`, and a
/// tag it cannot parse is not a version.
pub fn latest_npm_version(host: &dyn Host) -> Option<String> {
    let body = host.http_get(crate::endpoints::NPM_DIST_TAGS_URL, 3)?;
    let j: serde_json::Value = serde_json::from_str(&body).ok()?;
    let v = j
        .get("latest")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    if rollback_version_ok(&v) {
        Some(v)
    } else {
        None
    }
}

/// What "latest" means now that there are TWO channels: the newer of the CDN's `version.json` and
/// the registry's dist-tag. `None` only when NEITHER answered.
pub fn latest_release_version(host: &dyn Host) -> Option<String> {
    newest_of(
        latest_cdn_version(host).as_deref(),
        latest_npm_version(host).as_deref(),
    )
}

/// The verdict of a release-marker read-back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseMarkerCheck {
    pub ok: bool,
    /// The version actually read, trimmed; `None` when absent or unreadable.
    pub saw: Option<String>,
    /// How long the poll actually waited before giving up. Only meaningful when `ok` is false: a
    /// marker that never arrived is a TIMEOUT, which is weaker evidence than a wrong version.
    pub waited_ms: i64,
}

/// The bound on a release-marker read-back.
#[derive(Debug, Clone)]
pub struct AwaitOpts {
    pub want: String,
    pub timeout_ms: i64,
    pub interval_ms: i64,
}

/// Poll `etc\.summrise-release` until it shows `want`, or the budget expires.
///
/// Bounded on purpose: the swap kills the agent and restarts a scheduled task, so a delay is NORMAL
/// — but a marker that never arrives is a failed swap and must be reported as one. `read`, `sleep`
/// and `now` are INJECTED, which is the TypeScript's seam and is why this is testable without real
/// timers or a device.
pub fn await_release_marker<R, S, N>(
    o: &AwaitOpts,
    mut read: R,
    mut sleep: S,
    mut now: N,
) -> ReleaseMarkerCheck
where
    R: FnMut() -> Option<String>,
    S: FnMut(i64),
    N: FnMut() -> i64,
{
    let start = now();
    let deadline = start + o.timeout_ms;
    let mut saw: Option<String>;
    loop {
        match read() {
            Some(v) => {
                let v = v.trim().to_string();
                saw = if v.is_empty() { None } else { Some(v) };
                if saw.as_deref() == Some(o.want.as_str()) {
                    return ReleaseMarkerCheck {
                        ok: true,
                        saw,
                        waited_ms: now() - start,
                    };
                }
            }
            // absent / unreadable is NOT success and NOT a crash
            None => saw = None,
        }
        if now() >= deadline {
            return ReleaseMarkerCheck {
                ok: false,
                saw,
                waited_ms: now() - start,
            };
        }
        sleep(o.interval_ms);
    }
}

/// What to do about a swap that could not be proven.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    pub write_pin: bool,
    pub exit_code: i32,
    pub message: String,
}

/// Which verb is reporting. The message is printed by TWO callers — `rollback`, which staged a
/// release in order to PIN it, and `update`, which staged one to INSTALL it — and it said
/// "rollback:" with "re-run `summrise rollback <want>`" for both. On the update path that advice is
/// worse than useless: it names the version that just failed to install.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verb {
    Rollback,
    Update,
}

impl Verb {
    fn as_str(self) -> &'static str {
        match self {
            Verb::Rollback => "rollback",
            Verb::Update => "update",
        }
    }
}

/// The pin is the device's protection against being auto-upgraded back, and the marker is what every
/// UI believes — writing either on an unproven swap is how a device ends up misreporting its own
/// version and refusing the update that would fix it.
pub fn release_marker_verdict(
    c: &ReleaseMarkerCheck,
    want: &str,
    verb: Verb,
    from: Option<&str>,
) -> Verdict {
    if c.ok {
        return Verdict {
            write_pin: true,
            exit_code: 0,
            message: format!(
                "{v}: pinned to {want} -- auto-upgrade refused until 'summrise rollback --clear' or a forced agent_update",
                v = verb.as_str()
            ),
        };
    }
    // WHAT WAS OBSERVED, NOT WHAT IT MEANS. `ok: false` is a TIMEOUT: either the swap failed, or it
    // is still running (it kills and restarts the agent), or the marker could not be read at all —
    // and `saw == None` is the weakest of the three. The old wording asserted "the swap did NOT
    // take" for all of them.
    let window = format!("{}s", (c.waited_ms as f64 / 1000.0).round() as i64);
    let last = c
        .saw
        .clone()
        .unwrap_or_else(|| "empty or unreadable".to_string());
    let tail = match verb {
        Verb::Update => format!(
            "Check the update log and `summrise status`, then re-run `summrise update`. If it fails \
             the same way, `summrise rollback {}` pins the release this device is ACTUALLY running \
             — which is not the one that failed to install.",
            from.unwrap_or("<the version it was on>")
        ),
        Verb::Rollback => format!(
            "Check the update log and `summrise status`, then re-run 'summrise rollback {want}'."
        ),
    };
    Verdict {
        write_pin: false,
        exit_code: 1,
        message: format!(
            "{v}: no release marker showing {want} within {window} (last read: {last}). \
             NOT pinned (a pin would claim a version this device may not be running) and no release \
             marker written. {tail}",
            v = verb.as_str()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::FakeHost;

    /// Oracle: cli.test.mjs:367 — "newestOf: the newer of two channels — and silence is never
    /// agreement".
    #[test]
    fn newest_of_takes_the_newer_and_never_invents_one() {
        assert_eq!(
            newest_of(Some("1.2.452"), Some("1.2.453")).as_deref(),
            Some("1.2.453")
        );
        assert_eq!(
            newest_of(Some("1.2.453"), Some("1.2.452")).as_deref(),
            Some("1.2.453")
        );
        // Across a release line the triple still decides.
        assert_eq!(
            newest_of(Some("1.2.9"), Some("1.3.0")).as_deref(),
            Some("1.3.0")
        );
        // ONE channel silent: the other still answers.
        assert_eq!(newest_of(None, Some("1.2.452")).as_deref(), Some("1.2.452"));
        assert_eq!(newest_of(Some("1.2.452"), None).as_deref(), Some("1.2.452"));
        // BOTH silent: unknown. A status that cannot check must never render as "current".
        assert_eq!(newest_of(None, None), None);
        // An unparseable version is not a version.
        assert_eq!(
            newest_of(Some("garbage"), Some("1.2.452")).as_deref(),
            Some("1.2.452")
        );
        assert_eq!(
            newest_of(Some("1.2.452"), Some("not-a-version")).as_deref(),
            Some("1.2.452")
        );
    }

    /// Oracle: cli.test.mjs:385 — "busyIsFresh: the 10-minute update-exclusion window".
    #[test]
    fn busy_is_fresh_is_the_ten_minute_window() {
        let now = 1_700_000_000_000i64;
        const MIN: i64 = 60_000;
        assert!(
            busy_is_fresh(now - 9 * MIN, now),
            "9 min old = in-progress, refuse"
        );
        assert!(
            !busy_is_fresh(now - 11 * MIN, now),
            "11 min old = stale marker, proceed"
        );
        assert!(busy_is_fresh(now, now), "brand-new = fresh");
        assert!(
            !busy_is_fresh(now - 10 * MIN - 1, now),
            "just past the window"
        );
    }

    /// Oracle: cli.test.mjs:2666 — "delivery drift: counts patches within a minor, refuses a count
    /// across one".
    #[test]
    fn behind_by_counts_patches_within_a_minor_only() {
        assert_eq!(behind_by("1.2.9", "1.2.12"), "3 releases");
        assert_eq!(behind_by("1.2.9", "1.2.10"), "1 release");
        // AND THE SAME FACT AS A BOOLEAN. The update guard's first version asked `behindBy`, whose
        // answer is a PHRASE — truthy for "0 releases" and "-1 releases" alike — so it would have
        // refused every update including the correct one. These cases are the ones that caught it.
        assert!(is_behind("1.2.439", "1.2.440"));
        assert!(!is_behind("1.2.440", "1.2.440"));
        assert!(!is_behind("1.2.440", "1.2.439"));
        assert!(!is_behind("1.2.440", "1.3.0"));
        assert!(!is_behind("", "1.2.440"));
        assert!(!is_behind("garbage", "1.2.440"));
        // A cross-minor jump is a different operation, so it is not "N releases".
        assert_eq!(
            behind_by("1.1.9", "1.2.0"),
            "a release line, not a patch count"
        );
        assert_eq!(
            behind_by("garbage", "1.2.0"),
            "an unknown number of releases"
        );
    }

    /// Oracle: cli.test.mjs:1114 — "rollbackVersionOk: plain dotted triples only (URL interpolation
    /// gate)".
    #[test]
    fn rollback_version_ok_is_a_plain_triple() {
        assert!(rollback_version_ok("1.2.307"));
        assert!(rollback_version_ok("0.0.1"));
        assert!(!rollback_version_ok("1.2"), "two parts");
        assert!(!rollback_version_ok("1.2.3.4"), "four parts");
        assert!(!rollback_version_ok("1.2.307/../../evil"), "path escape");
        assert!(!rollback_version_ok("--clear"), "flag is not a version");
        assert!(!rollback_version_ok(""));
    }

    /// Oracle: cli.test.mjs:3088 — "updateWouldNotMove: the parity fact, and it needs no network".
    #[test]
    fn update_would_not_move_is_the_no_op_guard() {
        assert!(
            update_would_not_move("1.2.438", "1.2.438"),
            "same version is the no-op"
        );
        assert!(
            !update_would_not_move("1.2.437", "1.2.438"),
            "a real step forward must proceed"
        );
        assert!(
            !update_would_not_move("", "1.2.438"),
            "no marker is not a no-op"
        );
        assert!(
            !update_would_not_move("1.2.438", ""),
            "nor is an unknown CLI version"
        );
    }

    /// Oracle: cli.test.mjs:2594 — "the update staleness window is the same on both sides of the
    /// lock".
    ///
    /// The marker PATH agreed between the two sides; the WINDOW did not. Neither language can see
    /// the other's number, which is why this reads both files.
    #[test]
    fn the_update_staleness_window_matches_the_agents() {
        let rust = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../src/plugins/update/tools.rs"
        ))
        .expect("the agent's update plugin must be readable from this crate");
        let re = regex::Regex::new(r"const BUSY_STALE_SECS: u64 = (\d+);").unwrap();
        let secs: u64 = re
            .captures(&rust)
            .expect("BUSY_STALE_SECS must stay a literal this pin can read")[1]
            .parse()
            .unwrap();
        assert_eq!(
            BUSY_STALE_MINUTES as u64 * 60,
            secs,
            "the two sides disagree about when an abandoned update marker may be reclaimed: the \
             CLI says {BUSY_STALE_MINUTES} minutes, the agent says {secs} seconds. One lock with two \
             rules means whichever is shorter steals the marker from the longer one."
        );
    }

    /// The release channel: a silent CDN is `None`, and an unparseable body is not a version.
    #[test]
    fn the_release_channel_never_guesses() {
        let silent = FakeHost::new();
        assert_eq!(latest_cdn_version(&silent), None);
        let not_json = FakeHost::new().with_http(
            &format!("{}/api/version", crate::endpoints::DEFAULT_CDN_BASE),
            "<html>",
        );
        assert_eq!(latest_cdn_version(&not_json), None);
        let ok = FakeHost::new().with_http(
            &format!("{}/api/version", crate::endpoints::DEFAULT_CDN_BASE),
            r#"{"version":" 1.2.500 "}"#,
        );
        assert_eq!(latest_cdn_version(&ok).as_deref(), Some("1.2.500"));
        // A dist-tag pointing at a prerelease is not a version the device can install.
        let pre = FakeHost::new().with_http(
            crate::endpoints::NPM_DIST_TAGS_URL,
            r#"{"latest":"1.2.501-alpha.1"}"#,
        );
        assert_eq!(latest_npm_version(&pre), None);
        // One silent channel is not agreement, and both silent is unknown.
        let one = FakeHost::new().with_http(
            &format!("{}/api/version", crate::endpoints::DEFAULT_CDN_BASE),
            r#"{"version":"1.2.500"}"#,
        );
        assert_eq!(latest_release_version(&one).as_deref(), Some("1.2.500"));
        assert_eq!(latest_release_version(&FakeHost::new()), None);
    }

    /// Oracle: cli.test.mjs:1488 — "awaitReleaseMarker: only a MARKER THAT SHOWS THE TARGET counts
    /// as success".
    #[test]
    fn await_release_marker_needs_the_target() {
        let opts = |timeout_ms: i64| AwaitOpts {
            want: "1.2.322".to_string(),
            timeout_ms,
            interval_ms: 10,
        };

        // The swap wrote the target version: success, immediately.
        let r = await_release_marker(&opts(5000), || Some("1.2.322".to_string()), |_| {}, || 0);
        assert!(r.ok, "marker already at target => success");
        assert_eq!(r.saw.as_deref(), Some("1.2.322"));

        // The marker never moves off the OLD version: the swap failed. This is the incident shape —
        // and the caller must NOT write the pin or claim the version.
        let mut t = 0i64;
        let r = await_release_marker(
            &opts(100),
            || Some("1.2.321".to_string()),
            |_| {},
            || {
                t += 50;
                t
            },
        );
        assert!(
            !r.ok,
            "a marker that never reaches the target is NOT success"
        );
        assert_eq!(
            r.saw.as_deref(),
            Some("1.2.321"),
            "reports what it actually saw"
        );

        // The marker arrives late but within the budget: still success. The swap kills the agent
        // and restarts the task, so a delay is normal, not a failure.
        let mut n = 0;
        let mut t = 0i64;
        let r = await_release_marker(
            &opts(5000),
            || {
                n += 1;
                Some(if n < 3 { "1.2.321" } else { "1.2.322" }.to_string())
            },
            |_| {},
            || {
                t += 50;
                t
            },
        );
        assert!(r.ok, "a marker that arrives within the budget is success");

        // Unreadable marker (absent, or a torn write) is NOT success and NOT a crash.
        let mut t = 0i64;
        let r = await_release_marker(
            &opts(100),
            || None,
            |_| {},
            || {
                t += 50;
                t
            },
        );
        assert!(!r.ok, "a missing marker is a failure, never a silent pass");
        assert_eq!(r.saw, None, "absence is reported as null");

        // Whitespace/newline around the marker is not a mismatch.
        let r = await_release_marker(&opts(100), || Some("1.2.322\r\n".to_string()), |_| {}, || 0);
        assert!(r.ok, "the marker is compared trimmed");
    }

    /// Oracle: cli.test.mjs:1580 — "releaseMarkerVerdict: the pin is written ONLY on a proven
    /// swap".
    #[test]
    fn release_marker_verdict_gates_the_pin() {
        let proven = ReleaseMarkerCheck {
            ok: true,
            saw: Some("1.2.322".to_string()),
            waited_ms: 0,
        };
        let v = release_marker_verdict(&proven, "1.2.322", Verb::Rollback, None);
        assert!(v.write_pin);
        assert_eq!(v.exit_code, 0);

        let unproven = ReleaseMarkerCheck {
            ok: false,
            saw: Some("1.2.321".to_string()),
            waited_ms: 90_000,
        };
        let v = release_marker_verdict(&unproven, "1.2.322", Verb::Rollback, None);
        assert!(
            !v.write_pin,
            "an unproven swap must not pin the device to a version it is not running"
        );
        assert_eq!(v.exit_code, 1, "the caller must see a non-zero exit");
        assert!(
            v.message.contains("1.2.321"),
            "names the version the device IS on: {}",
            v.message
        );
        assert!(
            v.message.to_lowercase().contains("not pinned"),
            "states plainly that the pin was not written: {}",
            v.message
        );
        // THE BOUND, NOT A CONCLUSION. `ok: false` is a TIMEOUT: a slow-but-successful swap looks the
        // same as a failed one, so the message must say what was observed and over how long.
        assert!(v.message.contains("within 90s"), "{}", v.message);
        assert!(
            !v.message.to_lowercase().contains("did not take"),
            "must not assert a conclusion a timeout cannot support: {}",
            v.message
        );

        // `saw == None` is the weakest evidence of the three and must not read as a verdict.
        let blind = release_marker_verdict(
            &ReleaseMarkerCheck {
                ok: false,
                saw: None,
                waited_ms: 90_000,
            },
            "1.2.322",
            Verb::Rollback,
            None,
        );
        assert!(
            blind.message.contains("empty or unreadable"),
            "{}",
            blind.message
        );
        assert!(
            !blind.message.contains("device is on"),
            "and must not claim a version was seen: {}",
            blind.message
        );
    }

    /// The update path's own sentence. Ported from the `verb` branch of `releaseMarkerVerdict`,
    /// which the oracle's message cases cover through the default verb; this is the other arm, and
    /// it exists because naming the FAILED version to roll back to is worse than useless.
    #[test]
    fn the_update_verb_names_a_different_remedy_than_rollback() {
        let v = release_marker_verdict(
            &ReleaseMarkerCheck {
                ok: false,
                saw: Some("1.2.321".to_string()),
                waited_ms: 30_000,
            },
            "1.2.322",
            Verb::Update,
            Some("1.2.321"),
        );
        assert!(v.message.starts_with("update:"));
        assert!(
            v.message.contains("`summrise rollback 1.2.321`"),
            "{}",
            v.message
        );
        assert!(
            !v.message.contains("re-run 'summrise rollback 1.2.322'"),
            "the update path must not point at the version that failed to install: {}",
            v.message
        );
    }

    /// The two guards `summrise update` refuses on, as one decision — see
    /// [`crate::dispatch::update_decision`] for the wiring.
    #[test]
    fn an_update_refuses_on_a_fresh_marker_and_on_a_no_op() {
        // A fresh marker means another update is running: refuse.
        assert!(busy_is_fresh(1_000, 1_100));
        // The no-op: the device is already on the version this CLI carries.
        assert!(update_would_not_move("1.2.438", "1.2.438"));
        // And a target the URL gate refuses never reaches the network.
        assert!(!rollback_version_ok("1.2.438/../../evil"));
    }
}
