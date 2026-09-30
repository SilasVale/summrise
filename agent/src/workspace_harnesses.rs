//! Where each host's harness IS, and whether it answered — the one thing a two-pane panel needs and
//! nothing else has.
//!
//! WHAT THIS IS FOR. The panel's harness page is two panes: the hosts on the left, and the SELECTED
//! host's own harness on the right. That needs an answer to "which address is host X's harness at", and
//! the agent had none: the workspace tables carry connections and path prefixes, and a saved connection
//! says nothing about whether a harness runs on the far side. So this is that answer, per host.
//!
//! WHY A HARNESS IS ADDRESSED LOCALLY. The desktop shell composes the harness view as a native
//! WebContentsView on THIS machine, so a host's harness is reached through a forward on THIS machine's
//! loopback — the same shape as the portproxy a device already runs to reach a host's SSH. `local_port`
//! is therefore the port the shell can load, and `remote_port` is what it forwards to. Neither is
//! discovered: both are configured, because a forward someone forgot to make is indistinguishable from
//! a harness that is not running, and `record_probe` is what tells them apart.
//!
//! THE HONESTY RULE, which is this repository's own: a surface that reports a state names the thing that
//! was observed and WHEN. So every row carries `checked_ms` and `probe` (what the probe did), and a host
//! that was never probed says `never probed` rather than anything that reads like health. The
//! measurements behind that rule are the two rounds where the panel said `registered · tunnel: ok` while
//! a remote client got 530: the string came from the step that had just run, in the grammar of the
//! outcome the reader wanted, and nothing on that path had asked whether anything could reach the device.

use std::io::Write;
use std::path::PathBuf;

use summrise_agent_core::{recover_guard, DeviceError};

/// What the agent knows about one host's harness. There is no `healthy: bool`: the states are named for
/// what was observed, and `NeverProbed` is a state rather than a flavour of false.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HarnessState {
    /// Configured, and the last probe reached it and got a page.
    Answering,
    /// Configured, and the last probe reached the forward and got no page.
    NotAnswering,
    /// Never probed, so nothing has been observed. NOT the same claim as `not answering`.
    NeverProbed,
}

impl HarnessState {
    pub fn as_str(&self) -> &'static str {
        match self {
            HarnessState::Answering => "answering",
            HarnessState::NotAnswering => "not answering",
            HarnessState::NeverProbed => "never probed",
        }
    }
}

/// One row of the registry. `connection_id` is the id a saved connection already has, so a harness is
/// attached to a host the operator can see rather than to a new notion of "host".
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Harness {
    pub connection_id: String,
    /// The forward on THIS machine, which is what the shell's native view loads.
    pub local_port: u16,
    /// The port the host's own harness serves on.
    pub remote_port: u16,
    /// The last state observed. Only a probe sets it — see `record_probe`.
    pub state: String,
    /// When the probe ran, in milliseconds since the epoch. `None` when it never has.
    #[serde(default)]
    pub checked_ms: Option<u64>,
    /// What the probe did, in words a reader can check: the URL, the status, or the error.
    #[serde(default)]
    pub probe: Option<String>,
}

fn store_path() -> PathBuf {
    #[cfg(test)]
    if let Some(d) = store_dir_override() {
        return d.join("summrise-workspace-harnesses.json");
    }
    crate::paths::data_dir().join("summrise-workspace-harnesses.json")
}

/// Where the store lives, when a test says so. `pub(crate)` because the door tests in `web::mod` are in
/// another module and must be able to point the store somewhere too: writing to the real data dir from a
/// test would overwrite an operator's hosts with fixture rows, and that is the one thing these tests must
/// never do. ONE mechanism, read by `store_path()` above, called by both sets of tests.
#[cfg(test)]
fn store_dir_override() -> Option<PathBuf> {
    DIR_OVERRIDE.with(|d| d.borrow().clone())
}

#[cfg(test)]
thread_local! {
    static DIR_OVERRIDE: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

/// Point the store at `dir` for the calling test. `pub(crate)` so a test in another module can call it.
#[cfg(test)]
pub(crate) fn use_store_dir(dir: PathBuf) {
    DIR_OVERRIDE.with(|d| *d.borrow_mut() = Some(dir));
}

/// The repo's idiom for surviving a poisoned lock instead of panicking on it, and the same one
/// `workspace_mappings` takes: every read and every write is inside it, so a read-modify-write in
/// `configure` cannot interleave with another.
static STORE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn read_all() -> Vec<Harness> {
    let Ok(text) = std::fs::read_to_string(store_path()) else {
        return Vec::new();
    };
    // A store that will not parse answers empty rather than taking the door down: the panel can still
    // list its hosts, and the failure belongs on the write path where it can be fixed.
    serde_json::from_str::<Vec<Harness>>(&text).unwrap_or_default()
}

fn write_all(rows: &[Harness]) -> Result<(), DeviceError> {
    let path = store_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let encoded = serde_json::to_string_pretty(rows).map_err(|e| DeviceError::Internal {
        message: format!("encode harnesses: {e}"),
    })?;
    crate::atomic::replace(&path, crate::atomic::Hardening::None, |out| {
        out.write_all(encoded.as_bytes())
    })
    .map_err(|e| DeviceError::Internal {
        message: format!("write {}: {e}", path.display()),
    })
}

/// Milliseconds since the epoch, the stamp `checked_ms` carries. It is written by the CALLER's clock at
/// the moment the probe finished, so the number is a time and not a duration.
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Every configured harness, in configuration order.
pub fn list() -> Vec<Harness> {
    let _guard = recover_guard(&STORE_LOCK);
    read_all()
}

/// The row for one host, if it has one.
pub fn get(connection_id: &str) -> Option<Harness> {
    let _guard = recover_guard(&STORE_LOCK);
    read_all()
        .into_iter()
        .find(|h| h.connection_id == connection_id)
}

/// Point a host at a harness. Configuring is IDEMPOTENT by connection id, and a second call with
/// different ports REPLACES the row: the operator is correcting the forward, not adding a second one,
/// and two forwards for one host would leave the panel choosing between them.
///
/// A re-point does NOT carry the last probe's state across: the ports changed, so the old answer was
/// about a different address, and reporting it would be exactly the stale claim this store exists to
/// stop. The row comes back `never probed` and the next probe fills it in.
pub fn configure(
    connection_id: &str,
    local_port: u16,
    remote_port: u16,
) -> Result<(), DeviceError> {
    let id = connection_id.trim();
    if id.is_empty() {
        return Err(DeviceError::InvalidParams {
            message: "connection_id is required".into(),
        });
    }
    if local_port == 0 || remote_port == 0 {
        return Err(DeviceError::InvalidParams {
            message: format!(
                "both ports are required, and 0 is not a port: {local_port}/{remote_port}"
            ),
        });
    }
    let _guard = recover_guard(&STORE_LOCK);
    let mut rows = read_all();
    rows.retain(|r| r.connection_id != id);
    rows.push(Harness {
        connection_id: id.to_string(),
        local_port,
        remote_port,
        state: HarnessState::NeverProbed.as_str().to_string(),
        checked_ms: None,
        probe: None,
    });
    write_all(&rows)
}

/// Forget a host's harness, by name. `false` when there was nothing to forget, which is a fact the caller
/// can print rather than a failure to swallow.
pub fn forget(connection_id: &str) -> Result<bool, DeviceError> {
    let _guard = recover_guard(&STORE_LOCK);
    let mut rows = read_all();
    let before = rows.len();
    rows.retain(|r| r.connection_id != connection_id);
    if rows.len() == before {
        return Ok(false);
    }
    write_all(&rows)?;
    Ok(true)
}

/// Record what a probe found. This is the ONLY thing that sets `state`, which is why no other code
/// infers it. A probe for a host with no row is ignored rather than creating one: "we could not reach
/// this" is not a configuration.
pub fn record_probe(connection_id: &str, state: HarnessState, probe: &str) {
    let _guard = recover_guard(&STORE_LOCK);
    let mut rows = read_all();
    let Some(row) = rows.iter_mut().find(|r| r.connection_id == connection_id) else {
        return;
    };
    row.state = state.as_str().to_string();
    row.probe = Some(probe.to_string());
    row.checked_ms = Some(now_ms());
    let _ = write_all(&rows);
}

/// The address the shell loads for this host. Always this machine's loopback: the harness view is a
/// native view here, and a URL pointing anywhere else would be a different product.
pub fn url_for(h: &Harness) -> String {
    format!("http://127.0.0.1:{}", h.local_port)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each test gets its OWN store file — a unique path per test, not merely a per-thread reference to
    /// one shared path, because `list()` counts what is in the file and two tests sharing it see each
    /// other's rows. This is the same fixture shape `workspace_mappings` uses.
    fn isolated(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("summrise-harness-{}", std::process::id()))
            .join(name);
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        use_store_dir(dir.clone());
        dir
    }

    /// MUTATION: delete the `retain` in `configure` and `list()` answers with two rows for one host —
    /// the panel then has two addresses to choose between for a single host, which is the defect
    /// configuring twice is supposed to prevent.
    #[test]
    fn configuring_the_same_host_twice_leaves_one_row() {
        isolated("twice");
        configure("ssh:zhengsaisi@10.10.61.83:22122", 7738, 7738).unwrap();
        configure("ssh:zhengsaisi@10.10.61.83:22122", 7738, 7738).unwrap();
        assert_eq!(list().len(), 1, "one host, one forward");
    }

    /// MUTATION: carry `checked_ms`/`probe` across a re-point (the shape an earlier version had) and
    /// this test sees `answering` on a port nobody probed.
    #[test]
    fn re_pointing_a_host_forgets_what_the_last_probe_said() {
        let _ = isolated("repoint");
        let id = "ssh:a@b:22";
        configure(id, 7738, 7738).unwrap();
        record_probe(id, HarnessState::Answering, "GET / -> 200");
        assert_eq!(get(id).unwrap().state, "answering");

        configure(id, 7801, 7738).unwrap();
        let after = get(id).unwrap();
        assert_eq!(after.local_port, 7801);
        assert_eq!(
            after.state, "never probed",
            "the ports changed, so the old answer was about a different address"
        );
        assert!(after.checked_ms.is_none());
    }

    #[test]
    fn a_probe_only_fills_a_row_that_exists() {
        isolated("probe-only");
        record_probe(
            "ssh:nobody@nowhere:22",
            HarnessState::Answering,
            "GET / -> 200",
        );
        assert!(
            list().is_empty(),
            "'we could not reach this' is not a configuration"
        );
    }

    #[test]
    fn a_probe_stamps_the_time_and_the_sentence_it_ran() {
        isolated("stamp");
        let id = "ssh:a@b:22";
        configure(id, 7738, 7738).unwrap();
        record_probe(
            id,
            HarnessState::NotAnswering,
            "GET http://127.0.0.1:7738/ -> connection refused",
        );
        let row = get(id).unwrap();
        assert_eq!(row.state, "not answering");
        assert!(
            row.checked_ms.is_some_and(|ms| ms > 1_600_000_000_000),
            "a wall-clock stamp"
        );
        assert!(row.probe.as_deref().unwrap().contains("connection refused"));
    }

    #[test]
    fn a_missing_store_lists_empty_rather_than_failing() {
        isolated("absent");
        let _ = std::fs::remove_file(store_path());
        assert!(list().is_empty(), "no store, no hosts, and no failure");
    }

    #[test]
    fn a_store_that_will_not_parse_lists_empty_rather_than_failing() {
        let dir = isolated("corrupt");
        std::fs::write(dir.join("summrise-workspace-harnesses.json"), "{ not json").unwrap();
        assert!(
            list().is_empty(),
            "the panel can still list hosts; the write path is where it is fixed"
        );
    }

    #[test]
    fn zero_is_refused_because_it_is_not_a_port() {
        isolated("zero");
        let e = configure("ssh:a@b:22", 0, 7738).unwrap_err();
        assert!(matches!(e, DeviceError::InvalidParams { .. }), "got {e:?}");
    }

    #[test]
    fn forgetting_a_host_says_whether_there_was_one() {
        isolated("forget");
        configure("ssh:a@b:22", 7738, 7738).unwrap();
        assert!(forget("ssh:a@b:22").unwrap());
        assert!(
            !forget("ssh:a@b:22").unwrap(),
            "nothing left to forget is a fact, not a failure"
        );
    }

    #[test]
    fn the_url_is_this_machines_loopback() {
        let h = Harness {
            connection_id: "ssh:a@b:22".into(),
            local_port: 7801,
            remote_port: 7738,
            state: "never probed".into(),
            checked_ms: None,
            probe: None,
        };
        assert_eq!(url_for(&h), "http://127.0.0.1:7801");
    }
}
