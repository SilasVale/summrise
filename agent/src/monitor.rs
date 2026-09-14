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
use std::sync::{Arc, Mutex};
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

/// Transitions carried per target. One hour of samples holds at most a handful of real state
/// changes; the cap is there so a target toggling every probe cannot make a response unbounded.
pub const TRANSITIONS_MAX: usize = 20;

/// A note is one line the operator writes about a MOMENT ("I rebooted it"), not a log — bounded so
/// a paste cannot become the watch list's payload.
pub const NOTE_MAX: usize = 200;

/// Targets a device will watch. Small on purpose: this is an operator's instrument, not a
/// network management station, and every target costs a probe per interval.
pub const TARGETS_MAX: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Target {
    pub id: String,
    pub host: String,
    pub port: u16,
    /// WHEN SET, THE PROBE IS AN HTTP GET OF THIS PATH instead of a bare TCP connect — because a
    /// TCP connect cannot tell an operator the difference between a web UI that is DOWN and one
    /// that is ANSWERING 500, and that difference was the whole reason somebody was staring at it.
    /// The path is stored as given (starting with `/`); `None` means the original TCP probe.
    pub path: Option<String>,
    /// WHEN SET, THE RESPONSE BODY MUST CONTAIN THIS TEXT for the probe to count as up.
    ///
    /// WHY IT EXISTS, in the operator's words: a UI answering `200` with a login page, a captive
    /// portal or a "the service is starting" stub is not the thing they asked about. The status
    /// code cannot tell those apart from a working page, and a substring can: it is the cheapest
    /// assertion that fails when the CONTENT changes and passes when the server merely reformats
    /// its headers. Case-sensitive on purpose — the operator writes what they expect to see.
    pub expect: Option<String>,
}

/// One probe: WHEN, whether it answered, and how long it took when it did.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct Probe {
    pub ts_ms: u64,
    pub ok: bool,
    /// Time to connect (TCP) or to the response (HTTP), in milliseconds; `None` for a failed probe
    /// — a latency for a connection that never happened would be a fabricated measurement.
    pub ms: Option<u64>,
    /// THE HTTP STATUS, when this probe was an HTTP one and a response arrived. `None` for a TCP
    /// probe and for a transport failure — and the two are told apart by `path`, not by guessing.
    pub status: Option<u16>,
    /// Whether the EXPECTED TEXT was found, when the target has an expectation. `None` when there
    /// was nothing to match (no expectation, or no response to read) — "no answer" and "the answer
    /// did not say it" are different facts and must not collapse into one `false`.
    pub expect_ok: Option<bool>,
}

struct Watch {
    target: Target,
    series: VecDeque<Probe>,
    /// THE OPERATOR'S OWN WORDS about the current state — "I rebooted it", "maintenance window".
    ///
    /// WHY IT IS NOT PART OF THE TARGET: a target is a decision that persists (it is written to
    /// `monitors.json`), while a note explains a MOMENT. Keeping it out of the persisted list means
    /// a restart cannot resurrect a note that has stopped being true — the same reasoning that
    /// keeps samples out of the file. It is replaced by the next note and cleared by an empty one.
    note: Option<Note>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Note {
    pub text: String,
    pub at_ms: u64,
}

#[derive(Default)]
struct State {
    watches: Vec<Watch>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

/// WHERE A STATE CHANGE IS ANNOUNCED. Set once at boot (`main.rs`) to the device's event bus,
/// which is the same broadcast the panel's SSE stream and the terminal output share — so a
/// target going down reaches an open panel as `vale-monitor-change` without the monitor
/// knowing anything about SSE, HTTP or the panel.
///
/// NOT SET IN TESTS, and that is the point: the module is silent unless somebody is listening,
/// and the rule that decides WHEN to speak (a flip, not every probe) is testable on its own.
type ChangeSink = Arc<dyn Fn(serde_json::Value) + Send + Sync>;

static SINK: Mutex<Option<ChangeSink>> = Mutex::new(None);

/// Install the sink. A second call replaces it (a restarting supervisor is not an error).
pub fn set_event_sink(sink: ChangeSink) {
    *SINK.lock().unwrap_or_else(|p| p.into_inner()) = Some(sink);
}

fn emit_change(payload: serde_json::Value) {
    let sink = SINK.lock().unwrap_or_else(|p| p.into_inner()).clone();
    if let Some(f) = sink {
        f(payload);
    }
}

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
            // Absent or empty means the TCP probe — the shape every target had before paths
            // existed, so an older persisted list loads unchanged.
            let path = r
                .get("path")
                .and_then(|p| p.as_str())
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .map(str::to_string);
            // The expected text rides with the path: absent on a target that only checks a status.
            let expect = r
                .get("expect")
                .and_then(|p| p.as_str())
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .map(str::to_string);
            Some(Target {
                id,
                host,
                port,
                path,
                expect,
            })
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
fn target_id(host: &str, port: u16, path: Option<&str>) -> String {
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
    match path.map(str::trim).filter(|p| !p.is_empty()) {
        // TWO PATHS ON ONE PORT ARE TWO CHECKS: `/` answering and `/api/health` answering are
        // different facts about one service, and collapsing them would make the monitor unable to
        // say which one broke. The id stays readable because it is what the operator types back.
        Some(p) => format!(
            "{h}:{port}{}",
            if p.starts_with('/') {
                p.to_string()
            } else {
                format!("/{p}")
            }
        ),
        None => format!("{h}:{port}"),
    }
}

/// Validate and normalise a target. `Err` carries the reason a user can act on — this is fed
/// straight from a form in the panel.
/// A path the probe can request, or `None` for the TCP probe. Refused WITH A REASON because this
/// is fed straight from a form: a path with a space in it is a typo, not a URL.
pub fn validate_path(path: &str) -> Result<Option<String>, String> {
    let p = path.trim();
    if p.is_empty() {
        return Ok(None);
    }
    if p.contains(char::is_whitespace) {
        return Err("a path cannot contain spaces".into());
    }
    if p.starts_with("http://") || p.starts_with("https://") {
        return Err(
            "give the path only (\"/status\") — the host and port are their own fields".into(),
        );
    }
    // A missing leading slash is a typo this can fix without guessing: `status` and `/status` are
    // the same intent, and the id must be stable either way.
    Ok(Some(if p.starts_with('/') {
        p.to_string()
    } else {
        format!("/{p}")
    }))
}

/// An expectation the probe can look for, or `None`. Refused WITH A REASON: it is a form field.
pub fn validate_expect(expect: &str) -> Result<Option<String>, String> {
    let e = expect.trim();
    if e.is_empty() {
        return Ok(None);
    }
    if e.len() > 200 {
        return Err("the expected text is too long (200 characters is the limit)".into());
    }
    Ok(Some(e.to_string()))
}

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
    add_target_with_path(data_dir, host, port, "")
}

/// Add a target, optionally with an HTTP path (see [`Target::path`]). The no-path form is the
/// original TCP probe, so every existing caller keeps its meaning.
pub fn add_target_with_path(
    data_dir: &Path,
    host: &str,
    port: u16,
    path: &str,
) -> Result<Target, String> {
    add_target_full(data_dir, host, port, path, "")
}

/// Add a target with a path AND an expected substring (see [`Target::expect`]).
pub fn add_target_full(
    data_dir: &Path,
    host: &str,
    port: u16,
    path: &str,
    expect: &str,
) -> Result<Target, String> {
    let (host, port) = validate_target(host, port)?;
    let path = validate_path(path)?;
    let expect = validate_expect(expect)?;
    // An expectation with no path has nothing to read: the TCP probe has no body. Refused rather
    // than silently ignored — a watch that promises something it does not do is worse than one
    // that says no.
    if expect.is_some() && path.is_none() {
        return Err("an expected text needs an HTTP path to read — add one (e.g. \"/\")".into());
    }
    let id = target_id(&host, port, path.as_deref());
    let target = Target {
        id,
        host,
        port,
        path,
        expect,
    };
    let stored = state_with(|st| {
        if let Some(existing) = st.watches.iter_mut().find(|w| w.target.id == target.id) {
            // THE SAME WATCH, A NEW CRITERION. Adding a URL that is already watched used to return
            // the existing row and ignore the new `expect` — so "I want this URL to require
            // 'login' now" silently did nothing, which is the worst kind of no-op for an
            // instrument. The IDENTITY is the URL; the expectation is an attribute of it, and
            // re-adding updates the attribute while KEEPING the series (same watch, new rule).
            let updated = existing.target.expect != target.expect;
            existing.target = target.clone();
            if updated {
                return target;
            }
            return existing.target.clone();
        }
        if st.watches.len() >= TARGETS_MAX {
            return Target {
                id: String::new(), // sentinel: at capacity
                host: String::new(),
                port: 0,
                path: None,
                expect: None,
            };
        }
        st.watches.push(Watch {
            target: target.clone(),
            series: VecDeque::new(),
            note: None,
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

/// Attach the operator's explanation to a target's CURRENT state, or clear it with an empty
/// string. Returns the note as stored (or `None` after a clear), and `Err` when the target is not
/// being watched — a note about nothing is not a note.
pub fn set_note(id: &str, text: &str) -> Result<Option<Note>, String> {
    let trimmed = text.trim();
    if trimmed.len() > NOTE_MAX {
        return Err(format!("a note is at most {NOTE_MAX} characters"));
    }
    state_with(|st| {
        let Some(w) = st.watches.iter_mut().find(|w| w.target.id == id) else {
            return Err(format!("not watching {id} — nothing to annotate"));
        };
        w.note = if trimmed.is_empty() {
            None
        } else {
            Some(Note {
                text: trimmed.to_string(),
                at_ms: now_ms(),
            })
        };
        Ok(w.note.clone())
    })
}

/// The note currently attached to a target, if any.
pub fn note_of(id: &str) -> Option<Note> {
    state_with(|st| {
        st.watches
            .iter()
            .find(|w| w.target.id == id)
            .and_then(|w| w.note.clone())
    })
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
                    note: None,
                });
            }
        }
    });
}

fn record(id: &str, probe: Probe) {
    // The flip is decided INSIDE the same lock that appends the probe, so the announcement can
    // never describe a series that has since moved on. The payload is emitted after the lock is
    // released: a subscriber that calls back into this module (the panel asking for a snapshot)
    // must not deadlock against the writer.
    let announced = state_with(|st| {
        let w = st.watches.iter_mut().find(|w| w.target.id == id)?;
        // The state the series was in before this probe, and when that run began.
        let previous = w.series.back().map(|p| p.ok);
        let mut run_start = probe.ts_ms;
        if previous.is_some() {
            let current_state = w.series.back().map(|p| p.ok).unwrap_or(probe.ok);
            run_start = w
                .series
                .iter()
                .rev()
                .take_while(|p| p.ok == current_state)
                .last()
                .map(|p| p.ts_ms)
                .unwrap_or(probe.ts_ms);
        }
        w.series.push_back(probe);
        while w.series.len() > SERIES_MAX {
            w.series.pop_front();
        }
        match previous {
            // A FIRST probe is not a change: nothing was known before it. Announcing "up" for a
            // target that was simply never probed would be the device inventing an event.
            None => None,
            Some(prev) if prev == probe.ok => None,
            Some(_) => Some((w.target.clone(), run_start)),
        }
    });

    if let Some((target, run_start)) = announced {
        emit_change(json!({
            "ev": "monitor-change",
            "id": target.id,
            "host": target.host,
            "port": target.port,
            "up": probe.ok,
            "at_ms": probe.ts_ms,
            // How long the state that just ENDED had lasted — for a recovery, the outage.
            "lasted_ms": probe.ts_ms.saturating_sub(run_start),
            "ms": probe.ms,
            // The HTTP status behind the verdict, when there was one: "is DOWN" and "is DOWN,
            // answering 500" are different sentences and the second is the one that helps.
            "status": probe.status,
            "path": target.path,
            "expect_ok": probe.expect_ok,
        }));
    }
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
    let probe = match target.path.as_deref() {
        Some(path) => probe_http(&target, path).await,
        None => probe_tcp(&target).await,
    };
    record(id, probe);
    Some(probe)
}

/// The original probe: can a TCP connection be made to this port?
///
/// A refused connection is a REACHABLE host with nothing on that port — which for this instrument
/// is DOWN (the service the operator cares about is not there), and the distinction is drawn in the
/// panel's wording rather than here: this probe answers one question.
async fn probe_tcp(target: &Target) -> Probe {
    let started = now_ms();
    let addr = format!("{}:{}", target.host, target.port);
    let ok = match tokio::time::timeout(
        Duration::from_secs(PROBE_TIMEOUT_SECS),
        tokio::net::TcpStream::connect(addr.as_str()),
    )
    .await
    {
        Ok(Ok(_stream)) => true,
        Ok(Err(_)) | Err(_) => false,
    };
    Probe {
        ts_ms: now_ms(),
        ok,
        ms: if ok {
            Some(now_ms().saturating_sub(started))
        } else {
            None
        },
        status: None,
        expect_ok: None,
    }
}

/// WHAT A WEB UI IS ACTUALLY DOING, which a TCP connect cannot say.
///
/// The rule for `ok`, written down because it is a judgement and not a measurement:
/// **a response arrived AND its status is below 500**. A UI that answers `401` is WORKING (it wants
/// credentials); one that answers `500` is not serving anybody, and calling that "up" because the
/// socket opened is exactly the reading that sends an operator looking in the wrong place. The
/// status is recorded either way, so the CARD can show the number and the operator can disagree with
/// the verdict.
///
/// No redirects are followed: the question is "what does THIS url answer", and a 302 to a login page
/// is a true and useful answer. HTTPS is not supported here on purpose — a TLS check needs a
/// certificate story (is a self-signed cert "up"?), and an instrument that guesses would be worse
/// than one that says what it does.
async fn probe_http(target: &Target, path: &str) -> Probe {
    let started = now_ms();
    let url = format!("http://{}:{}{}", target.host, target.port, path);
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(PROBE_TIMEOUT_SECS))
        .redirect(reqwest::redirect::Policy::none())
        .build()
    {
        Ok(c) => c,
        Err(_) => {
            return Probe {
                ts_ms: now_ms(),
                ok: false,
                ms: None,
                status: None,
                expect_ok: None,
            }
        }
    };
    match client.get(&url).send().await {
        Ok(resp) => {
            let status = resp.status().as_u16();
            // THE EXPECTATION IS CHECKED ON A BOUNDED PREFIX of the body: enough for any real
            // page's marker, and bounded so a target streaming gigabytes cannot make the prober
            // the problem. `None` when there is nothing to match.
            let expect_ok = match target.expect.as_deref() {
                None => None,
                Some(needle) => {
                    const BODY_CAP: usize = 256 * 1024;
                    let bytes = resp.bytes().await.unwrap_or_default();
                    let head = &bytes[..bytes.len().min(BODY_CAP)];
                    Some(String::from_utf8_lossy(head).contains(needle))
                }
            };
            Probe {
                ts_ms: now_ms(),
                // BOTH must hold: a 5xx is not serving anybody, and a 200 whose body does not
                // carry the expected text is not the page the operator asked about.
                ok: status < 500 && expect_ok.unwrap_or(true),
                ms: Some(now_ms().saturating_sub(started)),
                status: Some(status),
                expect_ok,
            }
        }
        // No response at all: the same answer a TCP probe would give, and the one case where a
        // path-bearing target reports no status.
        Err(_) => Probe {
            ts_ms: now_ms(),
            ok: false,
            ms: None,
            status: None,
            expect_ok: None,
        },
    }
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

/// ONE STATE CHANGE, with the number an operator writes down: how long the state it ENDED had
/// lasted. For a `down` entry that is the uptime that just ended; for an `up` entry it is the
/// OUTAGE — the duration somebody pastes into a bug report.
///
/// Derived from the series rather than stored beside it: one source of truth (the probes), so a
/// transition cannot disagree with the chart drawn from the same bytes.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct Transition {
    /// When the new state was first observed.
    pub at_ms: u64,
    /// The state now in effect, as of `at_ms`.
    pub up: bool,
    /// How long the PREVIOUS state lasted, ending at `at_ms`.
    pub lasted_ms: u64,
}

/// The state changes in a series, OLDEST FIRST, at most `limit` of the newest.
///
/// The first probe is not a transition — nothing was observed before it — so a series that
/// begins down reports no `down` entry, which is correct: the device did not see it fall.
pub fn transitions(probes: &[Probe], limit: usize) -> Vec<Transition> {
    let mut out: Vec<Transition> = Vec::new();
    let mut run_start = match probes.first() {
        Some(p) => p.ts_ms,
        None => return out,
    };
    let mut current = probes[0].ok;
    for p in probes.iter().skip(1) {
        if p.ok != current {
            out.push(Transition {
                at_ms: p.ts_ms,
                up: p.ok,
                lasted_ms: p.ts_ms.saturating_sub(run_start),
            });
            current = p.ok;
            run_start = p.ts_ms;
        }
    }
    if out.len() > limit {
        out.drain(..out.len() - limit);
    }
    out
}

/// The most recent transitions for one target — what `/api/monitors` and `monitor_list` carry,
/// and what the card's outage list draws.
pub fn recent_transitions(id: &str, limit: usize) -> Vec<Transition> {
    transitions(&series(id, SERIES_MAX), limit)
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
            "last_status": Value::Null,
            "last_expect_ok": Value::Null,
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
        // THE NUMBER AN OPERATOR ASKS FOR BY NAME when the target is a web UI: what did it answer?
        // `null` for a TCP target and for a probe that got no response at all.
        "last_status": probes[probes.len() - 1].status,
        // …and whether the body said what the operator expected. `null` when there was nothing to
        // match, `false` when the page answered and did NOT contain it — which is the case a
        // status code cannot express.
        "last_expect_ok": probes[probes.len() - 1].expect_ok,
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
                // The PATH is part of what makes two targets different, so a row that omitted it
                // would show three identical names for three different checks (the panel caught
                // exactly that on d1: `127.0.0.1:18080` three times over).
                "path": t.path,
                // The operator's explanation, when one is attached to the current state.
                "note": note_of(&t.id),
                "summary": summary(&t.id),
                // The story, not just the shape: each entry is a state change with how long
                // the state it ended had lasted (an outage, for an "up" entry).
                "transitions": recent_transitions(&t.id, TRANSITIONS_MAX),
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

    /// SERIALISES THE TESTS THAT TOUCH THE TARGET LIST. It is a process-global by design (the prober
    /// and every route read it), and cargo runs one binary's tests IN PARALLEL — which CI caught:
    /// two tests added the SAME id (`127.0.0.1:1`, "reserved, refused") and one of them removed it at
    /// the end, so the other's `probe_once` found nothing and panicked at `.expect("probed")`.
    /// The same family as round 259's data-dir lock and round 261's metrics ring: the shared thing
    /// gets one lock, and the ids stay readable instead of being made unique by hand.
    ///
    /// A TOKIO mutex, because half these tests are async: a std guard across an `.await` is refused
    /// by clippy (round 259 learned that in CI too). The sync tests use `blocking_lock()`.
    static TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    async fn serial() -> tokio::sync::MutexGuard<'static, ()> {
        TEST_LOCK.lock().await
    }

    fn serial_sync() -> tokio::sync::MutexGuard<'static, ()> {
        TEST_LOCK.blocking_lock()
    }

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
        assert_eq!(
            target_id("192.168.1.1", 22, None),
            target_id("192.168.1.1", 22, None)
        );
        assert_eq!(target_id("Host.Local", 80, None), "host.local:80");
        // A name that would be ambiguous as a selector is normalised, not rejected: the
        // operator's intent is the host, and the id is ours to choose.
        assert_eq!(target_id("fe80::1", 22, None), "fe80::1:22");
        // A PATH IS PART OF THE IDENTITY: two paths on one port are two different checks, and the
        // id is what the operator types back to remove one of them.
        assert_eq!(target_id("h", 80, Some("/")), "h:80/");
        assert_eq!(target_id("h", 80, Some("status")), "h:80/status");
        assert_ne!(
            target_id("h", 80, Some("/a")),
            target_id("h", 80, Some("/b"))
        );
        assert_ne!(target_id("h", 80, None), target_id("h", 80, Some("/")));
    }

    /// A PATH IS VALIDATED WITH A REASON, and normalised rather than guessed at.
    #[test]
    fn a_path_is_normalised_and_refused_with_a_reason() {
        assert_eq!(validate_path(""), Ok(None));
        assert_eq!(validate_path("   "), Ok(None));
        assert_eq!(validate_path("status"), Ok(Some("/status".into())));
        assert_eq!(validate_path("/status"), Ok(Some("/status".into())));
        assert_eq!(validate_path(" /a/b?c=1 "), Ok(Some("/a/b?c=1".into())));
        assert!(validate_path("/a b").is_err());
        let err = validate_path("http://192.168.1.1/status").unwrap_err();
        assert!(err.contains("path only"), "{err}");
    }

    /// A PATH SURVIVES THE PERSISTED LIST, and a list written before paths existed loads as TCP
    /// targets — the migration is "absent means the old shape", not a version number.
    #[test]
    fn paths_survive_the_round_trip_and_old_lists_still_load() {
        let parsed = parse_targets(
            r#"{"targets":[
                {"id":"192.168.1.1:22","host":"192.168.1.1","port":22},
                {"id":"192.168.1.1:80/","host":"192.168.1.1","port":80,"path":"/"},
                {"id":"x:9","host":"x","port":9,"path":"  "}
            ]}"#,
        );
        assert_eq!(parsed.len(), 3, "{parsed:?}");
        assert_eq!(parsed[0].path, None, "an old row is a TCP target");
        assert_eq!(parsed[1].path.as_deref(), Some("/"));
        assert_eq!(parsed[2].path, None, "a blank path is no path");
        // …and rendering what we parsed gives the same list back.
        let again = parse_targets(&render_targets(&parsed));
        assert_eq!(again, parsed);
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

    /// THE HTTP PROBE, against a REAL server that answers 200 and one that answers 500 — which is
    /// the whole reason paths exist: a TCP connect calls both of them "up".
    ///
    /// The judgement is pinned in both directions too: `200` is up, `404` is UP (the service
    /// answers; the path is what is missing, and the operator can see the number), `500` is DOWN
    /// (nobody is being served), and no-response carries NO status rather than a fabricated one.
    #[tokio::test]
    async fn an_http_probe_tells_a_working_ui_from_one_that_answers_500() {
        let _serial = serial().await;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        // One listener, three paths: /ok answers 200, /bad answers 500, /missing answers 404.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let port = listener.local_addr().expect("addr").port();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                tokio::spawn(async move {
                    let mut buf = [0u8; 512];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]).to_string();
                    let code = if req.starts_with("GET /bad ") {
                        "500 Internal Server Error"
                    } else if req.starts_with("GET /missing ") {
                        "404 Not Found"
                    } else {
                        "200 OK"
                    };
                    let body = "x";
                    let resp = format!(
                        "HTTP/1.1 {code}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = sock.write_all(resp.as_bytes()).await;
                    let _ = sock.flush().await;
                });
            }
        });

        let d = dir("http");
        let ok = add_target_with_path(&d, "127.0.0.1", port, "/ok").expect("added");
        assert_eq!(ok.path.as_deref(), Some("/ok"));
        assert_eq!(ok.id, format!("127.0.0.1:{port}/ok"));
        let bad = add_target_with_path(&d, "127.0.0.1", port, "bad").expect("added");
        assert_eq!(
            bad.id,
            format!("127.0.0.1:{port}/bad"),
            "a bare path is normalised"
        );
        let missing = add_target_with_path(&d, "127.0.0.1", port, "/missing").expect("added");
        // A TCP target on the SAME port, for contrast: it cannot see the difference.
        let tcp = add_target(&d, "127.0.0.1", port).expect("added");

        let p = probe_once(&ok.id).await.expect("probed");
        assert!(p.ok, "{p:?}");
        assert_eq!(p.status, Some(200), "{p:?}");
        assert!(p.ms.is_some(), "{p:?}");

        let p = probe_once(&missing.id).await.expect("probed");
        assert!(p.ok, "a 404 means the service ANSWERS: {p:?}");
        assert_eq!(p.status, Some(404));

        let p = probe_once(&bad.id).await.expect("probed");
        assert!(!p.ok, "a 500 is not serving anybody: {p:?}");
        assert_eq!(
            p.status,
            Some(500),
            "…and it says WHY, which the TCP probe cannot"
        );

        let p = probe_once(&tcp.id).await.expect("probed");
        assert!(p.ok, "the TCP probe still calls that port up: {p:?}");
        assert_eq!(p.status, None, "and carries no status to confuse anyone");

        for id in [ok.id, bad.id, missing.id, tcp.id] {
            remove_target(&d, &id);
        }
    }

    /// CONTENT, NOT JUST A STATUS CODE — the case a 200 cannot express.
    ///
    /// A server that answers `200 OK` with a page that does NOT contain the expected text is DOWN
    /// for this instrument: a login page, a captive portal or a "starting up" stub all answer 200,
    /// and an operator who asked "is my UI serving?" is not answered by the number alone. Pinned in
    /// both directions against a REAL server, including the honest three-way record: status 200,
    /// the expectation FALSE, and `ok: false` saying so.
    #[tokio::test]
    async fn an_expectation_makes_a_200_that_lacks_the_text_count_as_down() {
        let _serial = serial().await;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let port = listener.local_addr().expect("addr").port();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                tokio::spawn(async move {
                    let mut buf = [0u8; 512];
                    let _ = sock.read(&mut buf).await;
                    let body = "<html><title>OpenWrt</title>ready</html>";
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = sock.write_all(resp.as_bytes()).await;
                    let _ = sock.flush().await;
                });
            }
        });

        let d = dir("expect");
        let hit = add_target_full(&d, "127.0.0.1", port, "/", "OpenWrt").expect("added");
        assert_eq!(hit.expect.as_deref(), Some("OpenWrt"));

        // 1. The body carries the text: up, with the status AND the match recorded.
        let p = probe_once(&hit.id).await.expect("probed");
        assert!(p.ok, "{p:?}");
        assert_eq!(p.status, Some(200));
        assert_eq!(p.expect_ok, Some(true), "the body carries the text: {p:?}");

        // 2. The SAME URL, a different criterion — re-adding updates the attribute and keeps the
        //    watch (and its series). This is what an operator means by "require 'login' now".
        let replaced = add_target_full(&d, "127.0.0.1", port, "/", "login").expect("added");
        assert_eq!(
            replaced.id, hit.id,
            "the expectation is an attribute, not part of the identity"
        );
        assert_eq!(replaced.expect.as_deref(), Some("login"));
        let p = probe_once(&replaced.id).await.expect("probed");
        assert!(!p.ok, "200 with the WRONG body is down: {p:?}");
        assert_eq!(
            p.status,
            Some(200),
            "…and the status is still recorded, so the card can show it"
        );
        assert_eq!(p.expect_ok, Some(false), "{p:?}");
        assert!(p.ms.is_some(), "the request did complete: {p:?}");

        // 3. Dropping the expectation restores the old behaviour: the status decides, and
        //    "nothing to match" is NOT a failure.
        let plain = add_target_full(&d, "127.0.0.1", port, "/", "").expect("added");
        assert_eq!(plain.expect, None);
        let p = probe_once(&plain.id).await.expect("probed");
        assert!(p.ok, "{p:?}");
        assert_eq!(
            p.expect_ok, None,
            "nothing to match is not a failure: {p:?}"
        );

        // 4. An expectation without a path is REFUSED, with a reason a form can show.
        let err = add_target_full(&d, "127.0.0.1", port, "", "OpenWrt").unwrap_err();
        assert!(err.contains("path"), "{err}");
        assert!(validate_expect(&"x".repeat(201)).is_err());
        assert_eq!(validate_expect("  "), Ok(None));

        remove_target(&d, &hit.id);
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
            status: None,
            expect_ok: None,
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

    /// THE TRANSITION RULE: a change is a change, the duration is how long the state it ENDED
    /// lasted, and a series that begins down reports no fall (nothing was seen to fall).
    #[test]
    fn transitions_carry_the_duration_of_the_state_they_end() {
        let p = |ok: bool, secs: u64| Probe {
            ts_ms: 1_700_000_000_000 + secs * 1_000,
            ok,
            ms: if ok { Some(1) } else { None },
            status: None,
            expect_ok: None,
        };
        // 60 s up, then down for 30 s, then up: two transitions with the durations an operator
        // writes down — the uptime that ended, then the OUTAGE.
        let series = [
            p(true, 0),
            p(true, 30),
            p(true, 60),
            p(false, 90),
            p(false, 120),
            p(true, 150),
        ];
        let t = transitions(&series, 10);
        assert_eq!(t.len(), 2, "{t:?}");
        assert_eq!(
            t[0],
            Transition {
                at_ms: p(false, 90).ts_ms,
                up: false,
                lasted_ms: 90_000
            }
        );
        assert_eq!(
            t[1],
            Transition {
                at_ms: p(true, 150).ts_ms,
                up: true,
                lasted_ms: 60_000
            }
        );

        // A series that BEGINS down: no fall was observed, so no entry — the first observed
        // state is a starting point, not an event.
        assert!(transitions(&[p(false, 0), p(false, 15)], 10).is_empty());
        // …but the recovery IS an event, and it names the outage as measured from the first
        // observation (which is the honest bound: the device does not know what came before).
        let t = transitions(&[p(false, 0), p(false, 15), p(true, 45)], 10);
        assert_eq!(
            t,
            vec![Transition {
                at_ms: p(true, 45).ts_ms,
                up: true,
                lasted_ms: 45_000
            }]
        );
        // Steady and degenerate inputs.
        assert!(transitions(&[p(true, 0), p(true, 15)], 10).is_empty());
        assert!(transitions(&[], 10).is_empty());
        assert!(transitions(&[p(true, 0)], 10).is_empty());
        // The cap keeps the NEWEST entries, still oldest-first.
        let toggling: Vec<Probe> = (0..10).map(|i| p(i % 2 == 0, i * 15)).collect();
        let capped = transitions(&toggling, 3);
        assert_eq!(capped.len(), 3);
        assert!(
            capped.windows(2).all(|w| w[0].at_ms < w[1].at_ms),
            "{capped:?}"
        );
        assert_eq!(capped[2].at_ms, toggling[toggling.len() - 1].ts_ms);
    }

    #[test]
    fn the_series_is_bounded_and_the_summary_counts_what_it_has() {
        let _serial = serial_sync();
        let id = format!("bounded-{}:22", std::process::id());
        state_with(|st| {
            st.watches.retain(|w| w.target.id != id);
            st.watches.push(Watch {
                target: Target {
                    id: id.clone(),
                    host: "bounded".into(),
                    port: 22,
                    path: None,
                    expect: None,
                },
                series: VecDeque::new(),
                note: None,
            });
        });
        for i in 0..(SERIES_MAX + 5) as u64 {
            record(
                &id,
                Probe {
                    ts_ms: 1_700_000_000_000 + i * 15_000,
                    ok: i % 5 != 0,
                    ms: if i % 5 != 0 { Some(3) } else { None },
                    status: None,
                    expect_ok: None,
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
        let _serial = serial_sync();
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
                    path: None,
                    expect: None,
                },
                series: VecDeque::new(),
                note: None,
            });
        });
        let sum = summary(&id);
        assert_eq!(sum["probes"], 0, "{sum}");
        assert!(sum["up_pct"].is_null(), "{sum}");
        assert!(sum["up_now"].is_null(), "{sum}");
        assert!(sum["latency"].is_null(), "{sum}");
        state_with(|st| st.watches.retain(|w| w.target.id != id));
    }

    /// THE ANNOUNCEMENT RULE, tested on its own: exactly one event per FLIP, never for a first
    /// probe (the device did not see it fall) and never for a probe that changed nothing. The
    /// payload carries the outage/uptime that just ended, which is the number a reader wants.
    ///
    /// The sink is process-global, so this test restores it — the same discipline the other
    /// globals in this file get.
    #[tokio::test]
    async fn a_state_flip_is_announced_once_and_a_steady_probe_is_not() {
        let _serial = serial().await;
        use std::sync::atomic::{AtomicUsize, Ordering};
        let d = dir("announce");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let down = add_target(&d, "127.0.0.1", 1).expect("added"); // port 1: refused

        let seen: Arc<Mutex<Vec<serde_json::Value>>> = Arc::new(Mutex::new(Vec::new()));
        let count = Arc::new(AtomicUsize::new(0));
        {
            let seen = seen.clone();
            let count = count.clone();
            set_event_sink(Arc::new(move |v| {
                count.fetch_add(1, Ordering::SeqCst);
                seen.lock().unwrap_or_else(|p| p.into_inner()).push(v);
            }));
        }

        // FIRST probe: no announcement — the device did not observe a change, it observed a
        // state for the first time.
        probe_once(&down.id).await.expect("probed");
        assert_eq!(
            count.load(Ordering::SeqCst),
            0,
            "a first probe is not a change"
        );
        // The SAME state again: still nothing.
        probe_once(&down.id).await.expect("probed");
        assert_eq!(count.load(Ordering::SeqCst), 0, "no change, no event");

        // A target that CAN answer, probed twice: up, up — no event (it was never down).
        let up = add_target(&d, "127.0.0.1", port).expect("added");
        probe_once(&up.id).await.expect("probed");
        probe_once(&up.id).await.expect("probed");
        assert_eq!(count.load(Ordering::SeqCst), 0, "steady is silent");

        // Now make it FLAP: the listener is dropped, so the next probe is a real down.
        drop(listener);
        probe_once(&up.id).await.expect("probed");
        let events = seen.lock().unwrap_or_else(|p| p.into_inner()).clone();
        assert_eq!(events.len(), 1, "{events:?}");
        assert_eq!(events[0]["ev"], "monitor-change");
        assert_eq!(events[0]["id"], up.id);
        assert_eq!(events[0]["up"], false);
        assert!(events[0]["lasted_ms"].as_u64().is_some(), "{:?}", events[0]);

        // And one more down: the state did not change, so nothing more is announced.
        probe_once(&up.id).await.expect("probed");
        assert_eq!(
            count.load(Ordering::SeqCst),
            1,
            "no second event for the same state"
        );

        *SINK.lock().unwrap_or_else(|p| p.into_inner()) = None;
        remove_target(&d, &up.id);
        remove_target(&d, &down.id);
    }

    #[tokio::test]
    async fn adding_and_removing_persists_the_list_beside_the_other_runtime_state() {
        let _serial = serial().await;
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
        let _serial = serial().await;
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
