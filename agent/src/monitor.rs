//! REACHABILITY MONITOR — "is this host:port up, and since when?"
//!
//! WHY IT EXISTS. The operator of this device debugs networks: an ONU that reboots, a router
//! that goes unreachable, a web UI that stops answering. Their tool for that was a hand-rolled
//! loop in a terminal session — `Test-NetConnection` every ten seconds, printing timestamps —
//! because nothing in the product could watch a host. A loop in a session dies with the session,
//! says nothing while it is not being read, and cannot be shown to anyone afterwards.
//!
//! So the DEVICE keeps the watch: a bounded series per target, probed on a timer, readable from
//! the panel and (through it) by an AI. What the panel draws is the same instrument language the
//! vitals series uses — a sparkline with gaps — and the gaps here are the point: **a failed
//! probe is not a slow one**, it is a gap, and the count beside it says how many.
//!
//! WHAT IT DELIBERATELY DOES NOT DO: it does not ping. A TCP connect to a port is what an
//! operator actually cares about ("is SSH answering?"), it needs no raw sockets or privileges,
//! and it tells the truth about a host whose ICMP is filtered but whose service is fine.
//!
//! TARGETS ARE PERSISTED, SAMPLES ARE NOT. A target list is a decision somebody made and must
//! survive an agent restart (the update flow restarts this process routinely); two hours of
//! samples are an instrument reading, and re-deriving them is one probe cycle. The file is
//! written atomically (temp + rename) through the same helper the JSONL family uses.

use serde_json::{json, Value};
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

/// How often every target is probed. 15 s is the interval the operator's own loop used: fast
/// enough that a reboot's down-window is visible (an ONU takes 1-2 minutes to come back), slow
/// enough that eight targets cost eight connects a quarter-minute.
pub const PROBE_INTERVAL_SECS: u64 = 15;

/// Per-probe connect budget. Longer than a LAN round trip by two orders of magnitude, shorter
/// than an operator's patience: a target that cannot answer in 3 s is DOWN, not "slow", and the
/// series must say so rather than hanging the prober on one dead host.
const PROBE_TIMEOUT_SECS: u64 = 3;

/// Samples kept per target: 240 × 15 s = one hour. Bounded because every panel poll reads them.
pub const SERIES_MAX: usize = 240;

/// Targets a device will watch. Small on purpose: this is an operator's instrument, not a
/// network management station, and every target costs a probe per interval.
pub const TARGETS_MAX: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Target {
    pub id: String,
    pub host: String,
    pub port: u16,
}

/// One probe: WHEN, whether it answered, and how long it took when it did.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct Probe {
    pub ts_ms: u64,
    pub ok: bool,
    /// Connect time in milliseconds; `None` for a failed probe — a latency for a connection
    /// that never happened would be a fabricated measurement.
    pub ms: Option<u64>,
}

struct Watch {
    target: Target,
    series: VecDeque<Probe>,
}

#[derive(Default)]
struct State {
    watches: Vec<Watch>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Where the target list lives. Under DataDir beside the other runtime state — NOT under `etc/`,
/// which holds what an administrator writes, and not in the log dir, which rotates.
pub fn targets_path(data_dir: &Path) -> PathBuf {
    data_dir.join("monitors.json")
}

/// Parse the persisted target list. Tolerant by design: an unreadable or malformed file is an
/// EMPTY list, never an error and never a panic — a monitor list must not stop the agent from
/// starting (the same rule the run journal follows).
pub fn parse_targets(text: &str) -> Vec<Target> {
    let v: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let rows = v
        .get("targets")
        .and_then(|t| t.as_array())
        .cloned()
        .unwrap_or_default();
    rows.iter()
        .filter_map(|r| {
            let host = r.get("host")?.as_str()?.trim().to_string();
            let port = r.get("port")?.as_u64()? as u16;
            let id = r.get("id")?.as_str()?.to_string();
            if host.is_empty() || port == 0 || id.is_empty() {
                return None;
            }
            Some(Target { id, host, port })
        })
        .take(TARGETS_MAX)
        .collect()
}

fn render_targets(targets: &[Target]) -> String {
    json!({ "targets": targets }).to_string()
}

/// A stable-enough id for a target: host:port, lowercased, with anything that would make a
/// path or a selector ambiguous removed. Two adds of the same host:port are the SAME target —
/// the alternative is two probers hammering one host and two identical rows in the panel.
fn target_id(host: &str, port: u16) -> String {
    let mut h: String = host
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == ':' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if h.is_empty() {
        h = "target".to_string();
    }
    format!("{h}:{port}")
}

/// Validate and normalise a target. `Err` carries the reason a user can act on — this is fed
/// straight from a form in the panel.
pub fn validate_target(host: &str, port: u16) -> Result<(String, u16), String> {
    let host = host.trim().to_string();
    if host.is_empty() {
        return Err("a host is required (an IP address or a name)".into());
    }
    if host.len() > 253 {
        return Err("that host name is longer than DNS allows".into());
    }
    if host
        .chars()
        .any(|c| c.is_whitespace() || c == '/' || c == '\\')
    {
        return Err("a host cannot contain spaces or slashes".into());
    }
    if port == 0 {
        return Err("a port is required (22 for SSH, 80 for a web UI, …)".into());
    }
    Ok((host, port))
}

fn state_with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    let st = guard.get_or_insert_with(State::default);
    f(st)
}

/// The watched targets, in the order they were added (the panel lists them as the operator
/// built the list).
pub fn targets() -> Vec<Target> {
    state_with(|st| st.watches.iter().map(|w| w.target.clone()).collect())
}

/// Add a target. Idempotent by id: adding an existing host:port returns the existing target and
/// changes nothing.
pub fn add_target(data_dir: &Path, host: &str, port: u16) -> Result<Target, String> {
    let (host, port) = validate_target(host, port)?;
    let id = target_id(&host, port);
    let target = Target { id, host, port };
    let stored = state_with(|st| {
        if let Some(existing) = st.watches.iter().find(|w| w.target.id == target.id) {
            return existing.target.clone();
        }
        if st.watches.len() >= TARGETS_MAX {
            return Target {
                id: String::new(), // sentinel: at capacity
                host: String::new(),
                port: 0,
            };
        }
        st.watches.push(Watch {
            target: target.clone(),
            series: VecDeque::new(),
        });
        target.clone()
    });
    if stored.port == 0 {
        return Err(format!("this device watches at most {TARGETS_MAX} targets"));
    }
    // Persist the list (best-effort — see the module header).
    let _ = crate::jsonl::rewrite_atomically(&targets_path(data_dir), &render_targets(&targets()));
    Ok(stored)
}

/// Remove a target. `false` when it was not being watched — the caller reports that honestly
/// rather than pretending a removal happened.
pub fn remove_target(data_dir: &Path, id: &str) -> bool {
    let removed = state_with(|st| {
        let before = st.watches.len();
        st.watches.retain(|w| w.target.id != id);
        st.watches.len() != before
    });
    if removed {
        let _ =
            crate::jsonl::rewrite_atomically(&targets_path(data_dir), &render_targets(&targets()));
    }
    removed
}

/// Load persisted targets at boot. Called once; a second call is a no-op so a test (or a
/// supervisor) cannot double-load.
pub fn load_targets(data_dir: &Path) {
    let Ok(text) = std::fs::read_to_string(targets_path(data_dir)) else {
        return;
    };
    let parsed = parse_targets(&text);
    state_with(|st| {
        if st.watches.is_empty() {
            for t in parsed {
                st.watches.push(Watch {
                    target: t,
                    series: VecDeque::new(),
                });
            }
        }
    });
}

fn record(id: &str, probe: Probe) {
    state_with(|st| {
        if let Some(w) = st.watches.iter_mut().find(|w| w.target.id == id) {
            w.series.push_back(probe);
            while w.series.len() > SERIES_MAX {
                w.series.pop_front();
            }
        }
    });
}

/// One target's series, OLDEST FIRST (the order a chart draws in).
pub fn series(id: &str, limit: usize) -> Vec<Probe> {
    state_with(|st| {
        st.watches
            .iter()
            .find(|w| w.target.id == id)
            .map(|w| {
                let skip = w.series.len().saturating_sub(limit);
                w.series.iter().skip(skip).copied().collect()
            })
            .unwrap_or_default()
    })
}

/// Probe one target once, record the result, and return it. Separated from the timer so a test
/// (and the panel's "check now") can drive exactly one probe.
pub async fn probe_once(id: &str) -> Option<Probe> {
    let target = targets().into_iter().find(|t| t.id == id)?;
    let started = now_ms();
    let addr = format!("{}:{}", target.host, target.port);
    let ok = match tokio::time::timeout(
        Duration::from_secs(PROBE_TIMEOUT_SECS),
        tokio::net::TcpStream::connect(addr.as_str()),
    )
    .await
    {
        Ok(Ok(_stream)) => true,
        // A refused connection is a REACHABLE host with nothing on that port — which for this
        // instrument is DOWN (the service the operator cares about is not there), and the
        // distinction is drawn in the panel's wording rather than here: the probe answers one
        // question, "did a connection to this port succeed".
        Ok(Err(_)) | Err(_) => false,
    };
    let probe = Probe {
        ts_ms: now_ms(),
        ok,
        ms: if ok {
            Some(now_ms().saturating_sub(started))
        } else {
            None
        },
    };
    record(id, probe);
    Some(probe)
}

/// Keep probing every target for the life of the process.
pub fn spawn_prober() {
    tokio::spawn(async move {
        loop {
            for t in targets() {
                let _ = probe_once(&t.id).await;
            }
            tokio::time::sleep(Duration::from_secs(PROBE_INTERVAL_SECS)).await;
        }
    });
}

/// How many DROPS in one window make a link worth calling unstable rather than merely down.
///
/// TWO, not one: a single drop is often the operator's own doing (a reboot they issued, an
/// update that restarted the agent), while a link that drops, recovers and drops again inside
/// one window is a PATTERN — the same distinction the load chip draws between a spike and a
/// sustained load.
///
/// The count is DROPS, not "flaps that recovered", and the name says so: a target that is down
/// NOW contributes the drop that started its outage, and the operator reading a count of one
/// beside `up_now: false` is reading the truth ("it dropped once and has not come back"),
/// whereas a count filtered to recovered outages would silently say zero about a link that is
/// down. Whether the state is current is `up_now`'s job, not this number's.
pub const UNSTABLE_DROPS: u64 = 2;

/// Count the up→down transitions in a series, OLDEST FIRST.
///
/// A transition is a probe that failed where the previous one answered. The FIRST probe cannot
/// be one (there is nothing before it to fall from), so a target watched since it was already
/// down reports zero drops — correctly: nothing was seen to fall.
pub fn count_drops(probes: &[Probe]) -> u64 {
    let mut drops = 0u64;
    for w in probes.windows(2) {
        if w[0].ok && !w[1].ok {
            drops += 1;
        }
    }
    drops
}

/// One target's summary over the samples it has: how many probes, how many answered, the share
/// that did, the latency range, and WHEN the state last changed (the number an operator reads
/// first — "down since 18:41" is the whole story).
pub fn summary(id: &str) -> Value {
    let probes = series(id, SERIES_MAX);
    if probes.is_empty() {
        return json!({
            "probes": 0,
            "up": 0,
            "down": 0,
            "up_pct": Value::Null,
            "up_now": Value::Null,
            "since_ms": Value::Null,
            "latency": Value::Null,
            // No probes, no transitions to count — and NOT a fabricated zero that would read
            // as "measured, stable".
            "drops": Value::Null,
        });
    }
    let up = probes.iter().filter(|p| p.ok).count();
    let down = probes.len() - up;
    let latencies: Vec<u64> = probes.iter().filter_map(|p| p.ms).collect();
    // `since_ms` = the stamp of the OLDEST probe in the current run of identical states. Walk
    // back from the newest until the state differs; that boundary is when the state began.
    let current = probes[probes.len() - 1].ok;
    let mut since = probes[probes.len() - 1].ts_ms;
    for p in probes.iter().rev() {
        if p.ok != current {
            break;
        }
        since = p.ts_ms;
    }
    let latency = if latencies.is_empty() {
        Value::Null
    } else {
        json!({
            "min": latencies.iter().min().copied().unwrap_or(0),
            "avg": latencies.iter().sum::<u64>() / latencies.len() as u64,
            "max": latencies.iter().max().copied().unwrap_or(0),
        })
    };
    json!({
        "probes": probes.len(),
        "up": up,
        "down": down,
        // A share of the probes TAKEN, computed from them rather than from a clock: with an
        // empty series there is no share, and `null` says so.
        "up_pct": (up as f64 / probes.len() as f64 * 100.0).round(),
        "up_now": current,
        "since_ms": since,
        "latency": latency,
        "drops": count_drops(&probes),
    })
}

/// The whole picture: every target with its series and summary — what the panel's card renders
/// and what an AI can read in one call.
pub fn snapshot() -> Value {
    let rows: Vec<Value> = targets()
        .into_iter()
        .map(|t| {
            json!({
                "id": t.id,
                "host": t.host,
                "port": t.port,
                "summary": summary(&t.id),
                "series": series(&t.id, SERIES_MAX),
            })
        })
        .collect();
    json!({
        "ok": true,
        "interval_secs": PROBE_INTERVAL_SECS,
        "series_max": SERIES_MAX,
        "targets": rows,
    })
}

/// Resolve a target's socket address for diagnostics — the same shape the prober dials.
pub fn dial_addr(target: &Target) -> Option<SocketAddr> {
    format!("{}:{}", target.host, target.port).parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("vale-monitor-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("temp dir");
        d
    }

    #[test]
    fn a_target_is_validated_with_a_reason_a_form_can_show() {
        assert!(validate_target("", 22).is_err());
        assert!(validate_target("   ", 22).is_err());
        assert!(validate_target("192.168.1.1", 0).is_err());
        assert!(validate_target("host with space", 22).is_err());
        assert!(validate_target("a/b", 22).is_err());
        assert_eq!(
            validate_target(" 192.168.1.1 ", 22).expect("valid"),
            ("192.168.1.1".to_string(), 22)
        );
    }

    #[test]
    fn the_same_host_and_port_is_one_target_not_two() {
        // Two rows for one host would be two probers and two identical charts.
        assert_eq!(target_id("192.168.1.1", 22), target_id("192.168.1.1", 22));
        assert_eq!(target_id("Host.Local", 80), "host.local:80");
        // A name that would be ambiguous as a selector is normalised, not rejected: the
        // operator's intent is the host, and the id is ours to choose.
        assert_eq!(target_id("fe80::1", 22), "fe80::1:22");
    }

    #[test]
    fn a_persisted_list_survives_a_corrupt_file_and_skips_junk_rows() {
        let text = r#"{"targets":[
            {"id":"192.168.1.1:22","host":"192.168.1.1","port":22},
            {"host":"no-id","port":22},
            {"id":"x:0","host":"x","port":0},
            {"id":"192.168.1.1:80","host":"192.168.1.1","port":80}
        ]}"#;
        let parsed = parse_targets(text);
        assert_eq!(parsed.len(), 2, "{parsed:?}");
        assert_eq!(parsed[0].id, "192.168.1.1:22");
        assert_eq!(parsed[1].port, 80);
        // Garbage is an EMPTY list, never an error: a monitor list must not stop the agent.
        assert!(parse_targets("not json").is_empty());
        assert!(parse_targets("{}").is_empty());
    }

    /// THE DROP RULE. It counts FALLS (up → down), not outages: a target that is down now still
    /// contributes the drop that started its outage — `up_now` is what says whether it is
    /// current — and a series that begins down has nothing to fall from.
    #[test]
    fn drops_count_falls_not_outages() {
        let p = |ok: bool, i: u64| Probe {
            ts_ms: 1_700_000_000_000 + i * 15_000,
            ok,
            ms: if ok { Some(1) } else { None },
        };
        // Steady: nothing fell.
        assert_eq!(count_drops(&[p(true, 0), p(true, 1), p(true, 2)]), 0);
        // ONE ongoing outage counts as ONE drop: it fell, and `up_now` says it has not come
        // back. Reporting zero here would be the silent kind of wrong.
        assert_eq!(count_drops(&[p(true, 0), p(false, 1), p(false, 2)]), 1);
        // Up → down → up → down → up: TWO drops — the pattern worth naming unstable.
        assert_eq!(
            count_drops(&[p(true, 0), p(false, 1), p(true, 2), p(false, 3), p(true, 4)]),
            2
        );
        // A series that BEGINS down has nothing to fall from; the next fall counts.
        assert_eq!(count_drops(&[p(false, 0), p(true, 1), p(false, 2)]), 1);
        // Degenerate inputs are zero, never a panic.
        assert_eq!(count_drops(&[]), 0);
        assert_eq!(count_drops(&[p(true, 0)]), 0);
    }

    #[test]
    fn the_series_is_bounded_and_the_summary_counts_what_it_has() {
        let id = format!("bounded-{}:22", std::process::id());
        state_with(|st| {
            st.watches.retain(|w| w.target.id != id);
            st.watches.push(Watch {
                target: Target {
                    id: id.clone(),
                    host: "bounded".into(),
                    port: 22,
                },
                series: VecDeque::new(),
            });
        });
        for i in 0..(SERIES_MAX + 5) as u64 {
            record(
                &id,
                Probe {
                    ts_ms: 1_700_000_000_000 + i * 15_000,
                    ok: i % 5 != 0,
                    ms: if i % 5 != 0 { Some(3) } else { None },
                },
            );
        }
        let s = series(&id, SERIES_MAX * 2);
        assert_eq!(s.len(), SERIES_MAX, "the ring keeps the newest");
        // Oldest first, and the tail is the LAST recorded probe.
        assert!(s.windows(2).all(|w| w[0].ts_ms <= w[1].ts_ms));
        assert_eq!(
            s[s.len() - 1].ts_ms,
            1_700_000_000_000 + (SERIES_MAX as u64 + 4) * 15_000
        );

        let sum = summary(&id);
        assert_eq!(sum["probes"], SERIES_MAX, "{sum}");
        assert!(sum["up_pct"].as_f64().is_some(), "{sum}");
        assert_eq!(sum["latency"]["avg"], 3, "{sum}");
        // The state's start is the oldest probe of the CURRENT run, not the newest probe.
        assert!(
            sum["since_ms"].as_u64().unwrap()
                < 1_700_000_000_000 + (SERIES_MAX as u64 + 4) * 15_000
        );
        state_with(|st| st.watches.retain(|w| w.target.id != id));
    }

    #[test]
    fn an_empty_series_summarises_as_unknown_not_as_zero_percent() {
        // "No probes yet" and "everything is down" are different facts; a share of nothing is
        // not 0%.
        let id = format!("empty-{}:22", std::process::id());
        state_with(|st| {
            st.watches.retain(|w| w.target.id != id);
            st.watches.push(Watch {
                target: Target {
                    id: id.clone(),
                    host: "empty".into(),
                    port: 22,
                },
                series: VecDeque::new(),
            });
        });
        let sum = summary(&id);
        assert_eq!(sum["probes"], 0, "{sum}");
        assert!(sum["up_pct"].is_null(), "{sum}");
        assert!(sum["up_now"].is_null(), "{sum}");
        assert!(sum["latency"].is_null(), "{sum}");
        state_with(|st| st.watches.retain(|w| w.target.id != id));
    }

    #[tokio::test]
    async fn adding_and_removing_persists_the_list_beside_the_other_runtime_state() {
        let d = dir("persist");
        let t = add_target(&d, "192.0.2.10", 22).expect("added");
        assert_eq!(t.host, "192.0.2.10");
        // Idempotent: the same host:port is the same target.
        let again = add_target(&d, "192.0.2.10", 22).expect("idempotent");
        assert_eq!(again.id, t.id);
        assert_eq!(targets().iter().filter(|x| x.id == t.id).count(), 1);
        // Written where the next boot will look for it.
        let text = std::fs::read_to_string(targets_path(&d)).expect("persisted");
        assert!(parse_targets(&text).iter().any(|x| x.id == t.id), "{text}");
        // And removal is honest about what it did.
        assert!(remove_target(&d, &t.id));
        assert!(
            !remove_target(&d, &t.id),
            "removing twice reports the second as a no-op"
        );
        let text = std::fs::read_to_string(targets_path(&d)).expect("rewritten");
        assert!(!parse_targets(&text).iter().any(|x| x.id == t.id), "{text}");
    }

    /// A REAL probe against a port that cannot answer, and one that must: the loopback port the
    /// test itself listens on. This is the only assertion that proves the prober measures what
    /// the panel draws.
    #[tokio::test]
    async fn a_probe_reports_up_for_a_listening_port_and_down_for_a_closed_one() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let d = dir("probe");
        let up = add_target(&d, "127.0.0.1", port).expect("added");
        let down = add_target(&d, "127.0.0.1", 1).expect("added"); // port 1: reserved, refused

        let p = probe_once(&up.id).await.expect("probed");
        assert!(p.ok, "a listening port must probe UP: {p:?}");
        assert!(p.ms.is_some(), "an UP probe carries a latency");
        let p = probe_once(&down.id).await.expect("probed");
        assert!(!p.ok, "a refused port must probe DOWN: {p:?}");
        assert!(
            p.ms.is_none(),
            "a DOWN probe must NOT carry a fabricated latency"
        );
        // Both are in the series the panel reads.
        assert_eq!(series(&up.id, 10).len(), 1);
        assert_eq!(summary(&up.id)["up_now"], true);
        assert_eq!(summary(&down.id)["up_now"], false);
        remove_target(&d, &up.id);
        remove_target(&d, &down.id);
    }
}
