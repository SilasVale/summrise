//! A temporary-file discipline that a pre-planted name cannot redirect (NEW-8).
//!
//! Every atomic write in this crate lands in a sibling first and is renamed into place. The sibling
//! used to be `<name>.tmp`: a principal with write access to the directory could plant a symlink at
//! that predictable name, and `File::create` would follow it, writing the verified release bytes (or
//! the state document) through the link to another file. The temporary now has a random suffix and
//! is opened with `create_new`, so a planted file can never be the one written through; the worst
//! case is one retry under a different name.

use std::collections::hash_map::RandomState;
use std::fs::{self, File};
use std::hash::{BuildHasher, Hasher};
use std::io;
use std::path::{Path, PathBuf};

/// Create a brand-new temporary file beside `path`, returning its path and open handle.
///
/// The name keeps the destination's name so a leftover in a directory is attributable, but its
/// unpredictable middle makes guessing it impossible. `create_new` is the enforcement: even an exact
/// match fails with `AlreadyExists` and the loop retries, so the write can never land in a file this
/// process did not create.
pub(crate) fn create_new_sibling(path: &Path) -> io::Result<(PathBuf, File)> {
    create_new_sibling_with(path, false)
}

/// Create a brand-new temporary file beside `path`, owner-only from the instant it exists.
///
/// A token or a key is a credential even while it still carries its temporary name: the mode is set
/// at `open`, not after, so there is no window in which another user can read the bytes. The umask
/// may narrow 0600 and never widen it.
pub(crate) fn create_new_sibling_private(path: &Path) -> io::Result<(PathBuf, File)> {
    create_new_sibling_with(path, true)
}

fn create_new_sibling_with(path: &Path, private: bool) -> io::Result<(PathBuf, File)> {
    let name = path
        .file_name()
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{} has no file name", path.display()),
            )
        })?
        .to_string_lossy()
        .into_owned();
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    if private {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    #[cfg(not(unix))]
    let _ = private;
    for _ in 0..16 {
        // `RandomState` is seeded from OS entropy per hasher, so the nonce is not derivable from the
        // clock or the pid; the retry loop covers the (astronomically unlikely) collision.
        let nonce = RandomState::new().build_hasher().finish();
        let temporary = path.with_file_name(format!(".{name}.{nonce:016x}.tmp"));
        match options.open(&temporary) {
            Ok(file) => return Ok((temporary, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!(
            "cannot create a temporary file beside {} after 16 attempts",
            path.display()
        ),
    ))
}
