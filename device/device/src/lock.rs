//! The serving app's single-instance lock: `<data>/device.lock`.
//!
//! The state document has exactly one writer: the process serving this data directory. The
//! supervisor has a named mutex, but the app did not, so two `run`s on one `<data>` could both
//! read-modify-write `state.json` and the later save silently discarded the other's session row or
//! swap verdict. This lock is advisory (`flock` on Unix, `LockFileEx` on Windows, through
//! [`std::fs::File::try_lock`]), taken before the first read or write of the document and held for
//! the whole process: a second serving app exits non-zero instead of racing.
//!
//! The pid recorded beside the lock is diagnostic, never the lock: the kernel releases the lock
//! when the handle closes, including on a crash, so a stale pid cannot keep the next start out. It
//! lives in `<data>/device.pid` rather than inside the locked file because a Windows `LockFileEx`
//! lock is mandatory — a second process cannot read the bytes of the file the first one locked —
//! and the refused process must be able to name the holder. The hidden `--stage` / `--check-update`
//! modes are short-lived helpers, not the serving app, and deliberately do not take this lock — the
//! canary runbook runs `--check-update` while the service serves.

use std::fmt;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// The lock's file name, relative to the data directory.
pub const LOCK_FILE: &str = "device.lock";

/// The holder's pid, relative to the data directory: readable while the lock is held.
pub const PID_FILE: &str = "device.pid";

/// The lock a serving app holds for its whole lifetime.
///
/// Dropping it closes the file handle, which releases the lock; there is no explicit unlock step
/// that a panic could skip.
#[derive(Debug)]
pub struct DeviceLock {
    /// Never read after `acquire`; holding the handle is what holds the lock.
    _file: File,
}

/// Why a serving app could not take its data directory.
#[derive(Debug)]
pub enum LockError {
    /// Another process holds the lock. `pid` is the holder's recorded pid, when readable.
    Held {
        /// The data directory the process asked for.
        data: PathBuf,
        /// The holder's pid as it recorded it beside the lock.
        pid: Option<u32>,
    },
    /// The lock file could not be created, opened or locked for an I/O reason.
    Io {
        /// The path that could not be opened or locked.
        path: PathBuf,
        /// The filesystem's refusal.
        source: io::Error,
    },
}

impl fmt::Display for LockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LockError::Held { data, pid } => {
                write!(
                    f,
                    "another summrise-device already owns the data directory {}",
                    data.display()
                )?;
                match pid {
                    Some(pid) => write!(f, " (pid {pid})")?,
                    None => write!(f, " (the holder did not record a pid)")?,
                }
                write!(
                    f,
                    "; stop that process before starting another. One serving app per data \
                     directory is allowed — see the canary runbook, \"One serving app per data \
                     directory\". The hidden --check-update and --stage modes are not the serving \
                     app and may run beside it."
                )
            }
            LockError::Io { path, source } => {
                write!(f, "cannot lock {}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for LockError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            LockError::Io { source, .. } => Some(source),
            LockError::Held { .. } => None,
        }
    }
}

impl DeviceLock {
    /// Take `<data>/device.lock`, or report the process that already holds it.
    ///
    /// The data directory is created first (a fresh install has none, and the lock lives in it).
    /// The lock is tried without blocking: waiting would turn a misconfiguration into a hang, and
    /// the operator asking for a second app wants to be told, not queued.
    pub fn acquire(data: &Path) -> Result<DeviceLock, LockError> {
        fs::create_dir_all(data).map_err(|source| LockError::Io {
            path: data.to_path_buf(),
            source,
        })?;
        let path = data.join(LOCK_FILE);
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|source| LockError::Io {
                path: path.clone(),
                source,
            })?;
        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                // The pid record is readable on both platforms; the lock file's own bytes are the
                // Unix-friendly fallback (a Windows `LockFileEx` lock blocks reading them).
                return Err(LockError::Held {
                    data: data.to_path_buf(),
                    pid: read_pid(&data.join(PID_FILE)).or_else(|| read_pid(&path)),
                });
            }
            Err(TryLockError::Error(source)) => {
                return Err(LockError::Io {
                    path: path.clone(),
                    source,
                })
            }
        }
        // The lock is authoritative; the pid is a courtesy for a refused second instance. A write
        // failure does not fail the start, because the next process will still be refused by the
        // kernel — it just cannot name this one.
        let _ = file.set_len(0);
        let _ = file.seek(SeekFrom::Start(0));
        let _ = writeln!(&file, "{}", std::process::id());
        let _ = file.flush();
        let _ = fs::write(data.join(PID_FILE), format!("{}\n", std::process::id()));
        Ok(DeviceLock { _file: file })
    }
}

/// Read the holder's recorded pid. Absent, unreadable or unparsable is `None`.
fn read_pid(path: &Path) -> Option<u32> {
    let mut text = String::new();
    File::open(path)
        .and_then(|mut file| file.read_to_string(&mut text))
        .ok()?;
    text.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    #[test]
    fn the_holder_records_its_pid_for_the_refused_second_instance() {
        let dir = TempDir::new("lock-pid");
        let data = dir.join("data");

        let _lock = DeviceLock::acquire(&data).expect("the first lock is free");

        let recorded = fs::read_to_string(data.join(PID_FILE)).expect("the pid record exists");
        assert_eq!(
            recorded.trim().parse::<u32>().expect("the record is a pid"),
            std::process::id(),
            "the refused second instance names the process that holds the lock"
        );
    }

    #[test]
    fn a_second_lock_names_the_data_directory_and_the_holder() {
        let dir = TempDir::new("lock-held");
        let data = dir.join("data");
        let _first = DeviceLock::acquire(&data).expect("the first lock is free");

        let error = DeviceLock::acquire(&data).expect_err("the second lock is refused");
        let message = error.to_string();

        assert!(
            matches!(error, LockError::Held { .. }),
            "a held lock is the typed refusal, not an I/O error: {error:?}"
        );
        assert!(
            message.contains(&data.display().to_string()),
            "the refusal names the data directory: {message}"
        );
        assert!(
            message.contains(&std::process::id().to_string()),
            "the refusal names the holder's pid: {message}"
        );
    }

    #[test]
    fn dropping_the_holder_releases_the_lock() {
        let dir = TempDir::new("lock-released");
        let data = dir.join("data");
        let first = DeviceLock::acquire(&data).expect("the first lock is free");

        drop(first);

        let _second =
            DeviceLock::acquire(&data).expect("the lock is free once the holder drops it");
    }
}
