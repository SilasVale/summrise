//! WHICH HOST A PATH LIVES ON. One store, one rule: the longest matching prefix over normalized absolute
//! paths names the saved connection that serves a path — and nothing else in the system has to know.
//!
//! WHY IT LIVES IN THE AGENT. The alternative (a table in the DSH provider's config) makes adding a workspace a
//! config edit on a machine the operator is not sitting at, and it splits "which host" from "which credentials"
//! across two owners. The agent already owns the credentials (`tools/terminal/secrets.rs`, the keychain) and the
//! saved connections (`tools/terminal/connections.rs`), so it owns this too.
//!
//! NORMALIZATION MIRRORS THE PROVIDER'S, deliberately: `agent/summrise-workspace-dsh/lib/fs-transport.js`
//! `normalizePath` folds the Windows spelling of a POSIX path (measured 2026-09-30: every `resolve()` in a live
//! session arrived as `D:\home\zhengsaisi\summrise\…`). A store that normalized differently would answer
//! "unknown path" for a path the provider had just resolved.

use std::io::Write;
use std::path::PathBuf;

use summrise_agent_core::{recover_guard, DeviceError};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Entry {
    pub path: String,
    pub connection_id: String,
}

fn store_path() -> PathBuf {
    #[cfg(test)]
    if let Some(d) = TEST_DIR.with(|d| d.borrow().clone()) {
        return d.join("summrise-workspace-mappings.json");
    }
    crate::paths::data_dir().join("summrise-workspace-mappings.json")
}

// Test-only store directory. ONE declaration, read by `store_path()` above — a second cell in the test module
// would leave every test writing the real data dir, which is the one thing these tests must not do.
#[cfg(test)]
thread_local! {
    pub(crate) static TEST_DIR: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

/// One spelling per path: POSIX rules, and a Windows-spelled absolute path folded to the POSIX one.
/// `None` when the input cannot name an absolute path — a relative path is refused rather than guessed.
pub fn normalize(path: &str) -> Option<String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return None;
    }
    let folded = match trimmed.as_bytes() {
        [drive, b':', b'\\' | b'/', ..] if drive.is_ascii_alphabetic() => {
            trimmed[2..].replace('\\', "/")
        }
        _ => trimmed.replace('\\', "/"),
    };
    if !folded.starts_with('/') {
        return None;
    }
    let mut out: Vec<&str> = Vec::new();
    for segment in folded.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    Some(format!("/{}", out.join("/")))
}

/// Segment-wise containment: `/home/z` must NOT claim `/home/zeta`.
pub fn contains(parent: &str, child: &str) -> bool {
    if parent == child {
        return true;
    }
    let base = if parent == "/" { "/" } else { parent };
    let base = base.trim_end_matches('/');
    child.len() > base.len() && child.starts_with(base) && child.as_bytes()[base.len()] == b'/'
}

fn read_all() -> Vec<Entry> {
    match std::fs::read_to_string(store_path()) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

/// Serialize writers: two registrations that read the same file would otherwise drop one (the same round-109
/// defect `tools/terminal/connections.rs` records for its own store). Taken through `recover_guard`, which is the
/// repo's idiom for surviving a poisoned lock instead of panicking on it.
static STORE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn write_all(entries: &[Entry]) -> Result<(), DeviceError> {
    let path = store_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let encoded = serde_json::to_string(entries).map_err(|e| DeviceError::Internal {
        message: format!("encode mappings: {e}"),
    })?;
    // THE CRATE'S ONE ATOMIC REPLACE, not another hand-rolled temp+rename: `atomic.rs` exists because that dance
    // had SEVEN spellings and three of them never called `sync_all`, so a power cut after the rename could leave
    // the replaced file empty. It also removes the temp on ANY failure, which a hand-rolled version forgets.
    crate::atomic::replace(&path, crate::atomic::Hardening::None, |out| {
        out.write_all(encoded.as_bytes())
    })
    .map_err(|e| DeviceError::Internal {
        message: format!("write {}: {e}", path.display()),
    })
}

pub fn list() -> Vec<Entry> {
    let _guard = recover_guard(&STORE_LOCK);
    read_all()
}

pub fn register(path: &str, connection_id: &str) -> Result<(), DeviceError> {
    let key = normalize(path).ok_or_else(|| DeviceError::InvalidParams {
        message: format!("not an absolute path: {path:?}"),
    })?;
    let _guard = recover_guard(&STORE_LOCK);
    let mut entries = read_all();
    entries.retain(|e| e.path != key);
    entries.push(Entry {
        path: key,
        connection_id: connection_id.to_string(),
    });
    write_all(&entries)
}

pub fn unregister(path: &str) -> Result<bool, DeviceError> {
    let Some(key) = normalize(path) else {
        return Ok(false);
    };
    let _guard = recover_guard(&STORE_LOCK);
    let mut entries = read_all();
    let before = entries.len();
    entries.retain(|e| e.path != key);
    if entries.len() == before {
        return Ok(false);
    }
    write_all(&entries)?;
    Ok(true)
}

/// The longest matching prefix wins, so `/home/zhengsaisi` and `/home/zhengsaisi/summrise` can be two workspaces
/// on two hosts. `None` means "no mapping" — which the door turns into the inline fallback, not into an error.
pub fn resolve(path: &str) -> Option<Entry> {
    let key = normalize(path)?;
    let _guard = recover_guard(&STORE_LOCK);
    read_all()
        .into_iter()
        .filter(|e| contains(&e.path, &key))
        .max_by_key(|e| e.path.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each test gets its OWN store file — a unique path per test, not merely a per-thread reference to one
    /// shared path: `list()` counts what is in the file, so two tests sharing it see each other's entries.
    fn with_dir<T>(f: impl FnOnce() -> T) -> T {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NTH: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "sm-mappings-{}-{}",
            std::process::id(),
            NTH.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        TEST_DIR.with(|d| *d.borrow_mut() = Some(dir.clone()));
        let out = f();
        TEST_DIR.with(|d| *d.borrow_mut() = None);
        out
    }

    #[test]
    fn a_path_has_one_spelling_and_windows_spelling_is_the_same_path() {
        assert_eq!(
            normalize("/home/zhengsaisi/summrise/").as_deref(),
            Some("/home/zhengsaisi/summrise")
        );
        assert_eq!(
            normalize("/home//zhengsaisi/./summrise").as_deref(),
            Some("/home/zhengsaisi/summrise")
        );
        assert_eq!(
            normalize("/home/zhengsaisi/x/../summrise").as_deref(),
            Some("/home/zhengsaisi/summrise")
        );
        // The deployment really sends both spellings (commit b3eda13d): the drive letter carries no
        // information on the far side of the seam, so it must fold to the same key.
        assert_eq!(
            normalize("D:\\home\\zhengsaisi\\summrise").as_deref(),
            Some("/home/zhengsaisi/summrise")
        );
        assert_eq!(
            normalize("d:/home/zhengsaisi\\summrise").as_deref(),
            Some("/home/zhengsaisi/summrise")
        );
    }

    #[test]
    fn containment_is_segment_wise_so_z_never_claims_zeta() {
        assert!(contains("/home/z", "/home/z"));
        assert!(contains("/home/z", "/home/z/a"));
        assert!(!contains("/home/z", "/home/zeta"));
        assert!(!contains("/home/z", "/home/zeta/a"));
        assert!(contains("/", "/anything"));
    }

    #[test]
    fn the_longest_prefix_wins() {
        with_dir(|| {
            register("/home/zhengsaisi", "ssh:a:22").unwrap();
            register("/home/zhengsaisi/summrise", "ssh:b:22").unwrap();
            assert_eq!(
                resolve("/home/zhengsaisi/other").unwrap().connection_id,
                "ssh:a:22"
            );
            assert_eq!(
                resolve("/home/zhengsaisi/summrise/agent")
                    .unwrap()
                    .connection_id,
                "ssh:b:22"
            );
            assert!(resolve("/tmp").is_none());
        });
    }

    #[test]
    fn register_is_idempotent_and_unregister_says_whether_it_removed_anything() {
        with_dir(|| {
            register("/srv/x", "ssh:a:22").unwrap();
            register("/srv/x", "ssh:a:22").unwrap();
            assert_eq!(list().len(), 1);
            register("/srv/x", "ssh:c:22").unwrap();
            assert_eq!(
                resolve("/srv/x").unwrap().connection_id,
                "ssh:c:22",
                "a re-register replaces"
            );
            assert!(unregister("/srv/x").unwrap());
            assert!(
                !unregister("/srv/x").unwrap(),
                "removing nothing is a fact, not an error"
            );
        });
    }

    #[test]
    fn a_windows_spelled_request_finds_a_posix_mapping() {
        with_dir(|| {
            register("/home/zhengsaisi/summrise", "ssh:a:22").unwrap();
            assert_eq!(
                resolve("D:\\home\\zhengsaisi\\summrise\\agent")
                    .unwrap()
                    .connection_id,
                "ssh:a:22"
            );
        });
    }

    #[test]
    fn a_relative_or_empty_path_is_refused_by_name() {
        with_dir(|| {
            assert!(register("relative/path", "ssh:a:22").is_err());
            assert!(register("", "ssh:a:22").is_err());
            assert!(resolve("relative/path").is_none());
        });
    }
}
