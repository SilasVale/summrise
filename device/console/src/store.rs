//! One atomic file replacement, shared by the two files the console persists.
//!
//! Both `devices.json` and `codes.json` are written whole through a sibling temporary file, synced,
//! and renamed into place. A reader therefore sees either the previous file or the complete new one,
//! never a half-written list. The rename replaces the destination on Unix and on Windows (Rust's
//! `fs::rename` uses `MoveFileExW` with `MOVEFILE_REPLACE_EXISTING` there).
//!
//! Both files are **0600 on Unix**: the registry holds token hashes and the codes file holds the
//! hashes a registration spends, and neither is for another local account's eyes. Windows has no
//! POSIX mode; there the ACL of the data directory is what keeps them private, and this module
//! deliberately does not pretend a mode it cannot set.

use std::fs;
use std::io::Write;
use std::path::Path;

use serde::Serialize;

/// Serialize `value` and atomically replace `path` with it.
pub(crate) fn write_json_atomic<T>(path: &Path, value: &T) -> std::io::Result<()>
where
    T: Serialize + ?Sized,
{
    let bytes = serde_json::to_vec(value).map_err(std::io::Error::other)?;
    write_atomic(path, &bytes)
}

/// Create the data directory if it is absent.
///
/// On Unix it is created 0700: it holds the registry, the enrollment codes and the two indexes.
/// An existing directory keeps the mode its operator gave it; the files inside it are still 0600.
pub(crate) fn create_data_dir(data: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;

        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(data)
    }
    #[cfg(not(unix))]
    {
        // Windows relies on the ACLs of the install tree.
        fs::create_dir_all(data)
    }
}

/// Write `bytes` through `<path>.json.tmp`, sync it, and rename it over `path`.
fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let temporary = path.with_extension("json.tmp");

    {
        let mut options = fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            // `mode` only applies when the temporary file is created; a leftover from an interrupted
            // write could have a looser mode, so the final file is pinned explicitly before the
            // rename.
            file.set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        file.write_all(bytes)?;
        file.sync_all()?;
    }

    fs::rename(&temporary, path)
}
