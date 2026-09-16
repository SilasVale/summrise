//! stage-n: device vitals for `/api/status` — CPU busy% and memory used%.
//!
//! Windows-only sources (kernel32 `GetSystemTimes` / `GlobalMemoryStatusEx`
//! via windows-sys); every other platform returns `None` so the endpoint
//! shape degrades gracefully (public call sites stay identical across
//! configs — same convention as the terminal feature gating).
//!
//! CPU is a DELTA metric: `GetSystemTimes` returns boot-relative counters,
//! so utilization is only computable between two samples — which makes the
//! INTERVAL part of the value, and is why this module now owns the clock: a
//! background sampler takes one reading every [`SAMPLE_INTERVAL_SECS`] and keeps
//! the last [`HISTORY_MAX`] of them in a ring. `/api/status` serves the NEWEST
//! sample rather than taking its own, so the number a strip prints and the last
//! point of the chart beside it can never disagree, and two consumers polling at
//! different rates can no longer shorten each other's delta window (which is what
//! happened when both the poller and a sampler called `sample()`).
//!
//! THE HISTORY IS WHAT MAKES A READING USEFUL. "CPU 87%" is a number; "CPU has
//! been above 90% for twelve minutes" is a fact somebody can act on, and it is
//! invisible to every instantaneous instrument this device had.

#[cfg(windows)]
use std::sync::Mutex;

/// ONE STAMPED READING — the unit the history, the route and every chart use.
///
/// `ts_ms` is unix MILLISECONDS, the same axis `crate::operation` orders on and the
/// panel's clocks print, so a vitals sample and an activity record can be placed side
/// by side without a unit conversion nobody would remember to do.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct Sample {
    pub ts_ms: u64,
    pub cpu_pct: Option<f64>,
    pub mem_pct: Option<f64>,
    pub mem_total_mb: Option<u64>,
}

/// THE VITALS SERIES' SHAPE AND ORDER, pinned from this end.
///
/// `agent/tests/fixtures/vitals-series.json` is read here and by the panel's `useVitalsSeries` tests.
/// Two promises travel in it and both are load-bearing on the panel side: the samples go straight into
/// the chart, and `samples[samples.length - 1]` is read as the NEWEST reading's total memory. A reverse
/// here would silently label the oldest reading as current — the same defect the boot-history fixture
/// pins on its own series.
#[cfg(test)]
mod fixture_tests {
    use super::Sample;

    #[test]
    fn a_vitals_sample_carries_the_keys_and_order_the_fixture_promises() {
        let raw = include_str!("../tests/fixtures/vitals-series.json");
        let fixture: serde_json::Value = serde_json::from_str(raw).expect("fixture parses");
        let promised: std::collections::BTreeSet<&str> = fixture["keys"]
            .as_array()
            .expect("keys")
            .iter()
            .filter_map(|k| k.as_str())
            .collect();

        let sample = Sample {
            ts_ms: 1_789_000_000_000,
            cpu_pct: Some(4.5),
            mem_pct: Some(38.2),
            mem_total_mb: Some(16_384),
        };
        let value = serde_json::to_value(sample).expect("serialises");
        let keys: std::collections::BTreeSet<&str> = value
            .as_object()
            .expect("object")
            .keys()
            .map(|k| k.as_str())
            .collect();
        assert_eq!(
            keys, promised,
            "the sample the device serialises and the fixture promises have drifted apart"
        );

        // OLDEST FIRST, and the example must demonstrate it: the panel reads the LAST entry as the
        // newest, so an example in the wrong order would be a fixture that teaches the wrong thing.
        let samples = fixture["example"]["samples"].as_array().expect("samples");
        assert!(
            samples.len() >= 3,
            "an order promise needs more than one point"
        );
        let stamps: Vec<u64> = samples
            .iter()
            .map(|s| s["ts_ms"].as_u64().expect("stamped"))
            .collect();
        let mut sorted = stamps.clone();
        sorted.sort_unstable();
        assert_eq!(
            stamps, sorted,
            "the example must be oldest-first: {stamps:?}"
        );
        assert!(
            stamps[0] > 1_600_000_000_000,
            "unix milliseconds, not seconds: {}",
            stamps[0]
        );
        // And the panel's required key is one this struct actually writes.
        for required in fixture["required_by_panel"].as_array().expect("required") {
            let k = required.as_str().expect("string");
            assert!(
                keys.contains(k),
                "the panel needs `{k}`, which this sample does not carry"
            );
        }
    }
}

/// How often the sampler reads. 30 s is chosen against the CPU delta it produces: long
/// enough that one busy second does not move the reading, short enough that a load spike
/// and its recovery are both visible, and cheap enough (two kernel32 calls) to run for
/// the life of the process.
pub const SAMPLE_INTERVAL_SECS: u64 = 30;

/// How many readings are kept — two hours at the interval above. Bounded because this is
/// read by every panel poll: an unbounded series would make the endpoint's cost grow with
/// uptime, which is the one thing that must not happen on a device left running for weeks.
pub const HISTORY_MAX: usize = 240;

/// The ring. Poison-recovered rather than unwrapped: a panicking reader must not take the
/// instrument down with it (the same rule the crate's other statics follow).
static HISTORY: std::sync::Mutex<std::collections::VecDeque<Sample>> =
    std::sync::Mutex::new(std::collections::VecDeque::new());

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Record a reading taken NOW. Returns the sample stored, or `None` when the host reported
/// nothing at all — a sample with no reading in it carries no information, and storing it
/// would give a host that cannot report vitals a series of blanks that looks like data.
pub fn record(v: Vitals) -> Option<Sample> {
    if v.cpu_pct.is_none() && v.mem_pct.is_none() {
        return None;
    }
    let s = Sample {
        ts_ms: now_ms(),
        cpu_pct: v.cpu_pct,
        mem_pct: v.mem_pct,
        mem_total_mb: v.mem_total_mb,
    };
    let mut h = HISTORY.lock().unwrap_or_else(|p| p.into_inner());
    h.push_back(s);
    while h.len() > HISTORY_MAX {
        h.pop_front();
    }
    Some(s)
}

/// Record a reading with an explicit stamp. The sampler uses [`record`]; this exists so a
/// test can build a series without sleeping through two hours of wall clock.
pub fn record_at(v: Vitals, ts_ms: u64) {
    if v.cpu_pct.is_none() && v.mem_pct.is_none() {
        return;
    }
    let mut h = HISTORY.lock().unwrap_or_else(|p| p.into_inner());
    h.push_back(Sample {
        ts_ms,
        cpu_pct: v.cpu_pct,
        mem_pct: v.mem_pct,
        mem_total_mb: v.mem_total_mb,
    });
    while h.len() > HISTORY_MAX {
        h.pop_front();
    }
}

/// The whole history, OLDEST FIRST — the order a chart draws in, so no consumer has to
/// remember which way round it is.
pub fn history() -> Vec<Sample> {
    HISTORY
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .iter()
        .copied()
        .collect()
}

/// The newest reading, or `None` before the first sample. `/api/status` serves THIS: one
/// clock, one delta window, and the number beside the chart is the chart's last point.
pub fn latest() -> Option<Sample> {
    HISTORY
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .back()
        .copied()
}

/// Take one reading now and store it. Called by the sampler and, once, at boot.
pub fn sample_now() -> Option<Sample> {
    record(sample())
}

/// Keep taking readings for the life of the process.
///
/// The FIRST reading is taken immediately so a device that just booted has memory to show
/// (CPU needs a second reading — it is a delta — so it appears one interval later, which is
/// the same honest-empty behaviour the endpoint always had).
pub fn spawn_sampler() {
    tokio::spawn(async move {
        let _ = sample_now();
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(SAMPLE_INTERVAL_SECS)).await;
            let _ = sample_now();
        }
    });
}

/// Snapshot of the vitals. `None` = not (yet) computable on this host.
#[derive(Debug, Clone, Copy, Default)]
pub struct Vitals {
    /// CPU busy percentage since the previous `sample()` (0..=100).
    pub cpu_pct: Option<f64>,
    /// Physical memory in use, percentage (0..=100).
    pub mem_pct: Option<f64>,
    /// Total physical memory, MiB.
    pub mem_total_mb: Option<u64>,
}

#[cfg(windows)]
#[derive(Debug, Clone, Copy)]
struct CpuTimes {
    idle: u64,
    kernel: u64, // kernel includes idle (Windows semantics)
    user: u64,
}

#[cfg(windows)]
static PREV_CPU: Mutex<Option<CpuTimes>> = Mutex::new(None);

/// Take one vitals sample. Cheap; safe to call per status request.
pub fn sample() -> Vitals {
    #[cfg(windows)]
    {
        windows_vitals()
    }
    #[cfg(not(windows))]
    {
        Vitals::default()
    }
}

#[cfg(windows)]
fn windows_vitals() -> Vitals {
    use std::mem::zeroed;
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    use windows_sys::Win32::System::Threading::GetSystemTimes;

    let mut out = Vitals::default();

    // ── CPU: busy% between this call and the previous one ──────────────
    unsafe {
        let (mut idle, mut kernel, mut user): (
            windows_sys::Win32::Foundation::FILETIME,
            windows_sys::Win32::Foundation::FILETIME,
            windows_sys::Win32::Foundation::FILETIME,
        ) = (zeroed(), zeroed(), zeroed());
        if GetSystemTimes(&mut idle, &mut kernel, &mut user) != 0 {
            let to_u64 = |ft: &windows_sys::Win32::Foundation::FILETIME| {
                ((ft.dwHighDateTime as u64) << 32) | ft.dwLowDateTime as u64
            };
            let cur = CpuTimes {
                idle: to_u64(&idle),
                kernel: to_u64(&kernel),
                user: to_u64(&user),
            };
            let prev = PREV_CPU
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .replace(cur);
            if let Some(prev) = prev {
                out.cpu_pct = cpu_busy_pct(
                    prev.idle,
                    prev.kernel.saturating_add(prev.user),
                    cur.idle,
                    cur.kernel.saturating_add(cur.user),
                );
            }
        }
    }

    // ── Memory: used% + total MiB ───────────────────────────────────────
    unsafe {
        let mut stat: MEMORYSTATUSEX = zeroed();
        stat.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        if GlobalMemoryStatusEx(&mut stat) != 0 {
            if stat.ullTotalPhys > 0 {
                let used = stat.ullTotalPhys - stat.ullAvailPhys;
                out.mem_pct = Some(round1(used as f64 / stat.ullTotalPhys as f64 * 100.0));
                out.mem_total_mb = Some(stat.ullTotalPhys / (1024 * 1024));
            }
        }
    }

    out
}

#[allow(dead_code)] // used by the Windows path; unit-tested on all hosts
fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

/// Pure CPU-delta math, extracted for host-independent tests (round-378):
/// busy% = (total_delta − idle_delta) / total_delta. Counter regress (VM
/// migrate, wrap) saturates to zero delta → None, never a negative or NaN.
#[allow(dead_code)] // used by the Windows path; unit-tested on all hosts
fn cpu_busy_pct(prev_idle: u64, prev_total: u64, cur_idle: u64, cur_total: u64) -> Option<f64> {
    let d_idle = cur_idle.saturating_sub(prev_idle);
    let d_total = cur_total.saturating_sub(prev_total);
    if d_total == 0 {
        return None;
    }
    Some(round1(
        d_total.saturating_sub(d_idle) as f64 / d_total as f64 * 100.0,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SERIALISES THE TESTS THAT TOUCH THE RING. It is a process-global by design (the sampler
    /// owns it) and cargo runs one binary's tests IN PARALLEL, so two tests recording at once
    /// interleave: **CI caught `latest()` disagreeing with a series snapshot taken a moment
    /// earlier, and a second run caught the count in `a_reading_the_host_cannot_take_is_not_a_sample`
    /// moving under it** (round 261). The assertions were the racy part, not the code — but the
    /// fix belongs here, where the shared thing is.
    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn guard() -> std::sync::MutexGuard<'static, ()> {
        TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner())
    }

    #[test]
    fn sample_never_panics_and_round1_is_exact() {
        let v = sample();
        // Non-Windows: all None. Windows: values in range when present.
        if let Some(c) = v.cpu_pct {
            assert!((0.0..=100.0).contains(&c));
        }
        if let Some(m) = v.mem_pct {
            assert!((0.0..=100.0).contains(&m));
        }
        assert_eq!(round1(12.34), 12.3);
        assert_eq!(round1(12.36), 12.4);
        assert_eq!(round1(100.0), 100.0);
    }

    fn vitals(cpu: Option<f64>, mem: Option<f64>) -> Vitals {
        Vitals {
            cpu_pct: cpu,
            mem_pct: mem,
            mem_total_mb: Some(16384),
        }
    }

    #[test]
    fn the_history_keeps_the_newest_and_reads_oldest_first() {
        let _serial = guard();
        // The ring is GLOBAL, so this test owns its own slice of the stamp space and
        // asserts about ITS samples rather than about the length of the whole ring —
        // cargo runs tests in parallel and a second test may be recording at the same
        // time. (The property under test — order and trimming — does not need isolation;
        // the counts do.)
        let base = 1_700_000_000_000u64;
        for i in 0..5u64 {
            record_at(vitals(Some(i as f64), Some(50.0)), base + i * 1_000);
        }
        let h = history();
        let mine: Vec<&Sample> = h.iter().filter(|s| s.ts_ms >= base).collect();
        assert_eq!(mine.len(), 5);
        // OLDEST FIRST: the chart draws in this order.
        assert_eq!(mine[0].cpu_pct, Some(0.0));
        assert_eq!(mine[4].cpu_pct, Some(4.0));
        assert!(mine.windows(2).all(|w| w[0].ts_ms <= w[1].ts_ms));
        // `latest()` IS the tail of `history()` — the same value `/api/status` serves, and
        // the last point of the chart. Asserted as that RELATION rather than against this
        // test's own stamp: the ring is global, so a parallel test may legitimately have
        // pushed after these.
        let tail = history().last().copied();
        assert_eq!(
            latest(),
            tail,
            "latest() must be the tail history() ends on"
        );
    }

    #[test]
    fn a_reading_the_host_cannot_take_is_not_a_sample() {
        let _serial = guard();
        // A host with no vitals (every non-Windows build) must produce an EMPTY history,
        // not a series of blanks that a chart would draw as data.
        assert!(record(vitals(None, None)).is_none());
        let before = history().len();
        record_at(vitals(None, None), 1);
        assert_eq!(history().len(), before, "an empty reading stores nothing");
    }

    #[test]
    fn the_ring_is_bounded() {
        let _serial = guard();
        // Bounded for the same reason every list route here is: the cost of reading it
        // must not grow with uptime. HISTORY_MAX + 1 pushes must leave exactly MAX.
        let base = 1_600_000_000_000u64;
        for i in 0..(HISTORY_MAX + 1) as u64 {
            record_at(vitals(Some(1.0), Some(2.0)), base + i);
        }
        let h = history();
        let mine: Vec<&Sample> = h.iter().filter(|s| s.ts_ms >= base).collect();
        assert_eq!(
            mine.len(),
            HISTORY_MAX,
            "the front is dropped, never the tail"
        );
        assert_eq!(mine[mine.len() - 1].ts_ms, base + HISTORY_MAX as u64);
    }

    #[cfg(not(windows))]
    #[test]
    fn non_windows_sample_is_all_none() {
        // The graceful-degradation contract: every other platform reports
        // no vitals (endpoint shape carries nulls, never fabricates).
        let v = sample();
        assert_eq!(v.cpu_pct, None);
        assert_eq!(v.mem_pct, None);
        assert_eq!(v.mem_total_mb, None);
    }

    #[test]
    fn cpu_delta_math() {
        // Half the delta busy → 50%.
        assert_eq!(cpu_busy_pct(100, 1000, 200, 1200), Some(50.0));
        // Fully idle / fully busy bounds.
        assert_eq!(cpu_busy_pct(0, 0, 100, 100), Some(0.0));
        assert_eq!(cpu_busy_pct(0, 0, 0, 100), Some(100.0));
        // No tick between samples → None, not 0/0 NaN.
        assert_eq!(cpu_busy_pct(50, 500, 50, 500), None);
        // Counter regress saturates → None, never negative.
        assert_eq!(cpu_busy_pct(900, 1000, 100, 200), None);
        // Rounding to one decimal through the shared helper.
        assert_eq!(cpu_busy_pct(0, 0, 0, 3), Some(100.0));
        assert_eq!(cpu_busy_pct(1, 0, 2, 3), Some(66.7));
    }
}
