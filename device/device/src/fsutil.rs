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

/// Create `directory` and, on Unix, make it owner-only.
///
/// **THIS IS THE UNIX HALF OF THE PROTECTION WINDOWS GETS FROM THE INSTALLER.** `launch/src/win.rs`
/// calls `winsec::protect_dir` on `<data>` before any credential is written there, and its comment
/// says why: on Windows a file's protection is its parent's inheritable ACEs, so the directory is
/// what protects the files. Unix has the same fact with a different mechanism — a file created in a
/// `0700` directory is still `0644` by umask, so a directory that others may TRAVERSE is not a
/// directory that protects anything: `memory.jsonl`, `events.jsonl` and `state.json` are all
/// world-readable there, and the first of those holds whatever a session asked the device to
/// remember.
///
/// It is idempotent and it does not raise a mode: a directory the operator deliberately opened wider
/// keeps that mode, because narrowing somebody else's decision is not this function's job — a mode of
/// `0755` on a box with one user is a legitimate choice, and the only thing to fix is the state a
/// fresh install starts in. `0700` is applied only when the directory is CREATED here.
pub(crate) fn create_private_dir(directory: &Path) -> io::Result<()> {
    if directory.is_dir() {
        return Ok(());
    }
    fs::create_dir_all(directory)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Read back rather than assume: `create_dir_all` honours the umask, and a mode that was
        // narrowed by it is still owner-only, but a mode the umask left at `0755` is what this call
        // exists to prevent.
        let mut permissions = fs::metadata(directory)?.permissions();
        if permissions.mode() & 0o777 != 0o700 {
            permissions.set_mode(0o700);
            fs::set_permissions(directory, permissions)?;
        }
    }
    Ok(())
}

/// Create a brand-new directory at a random sibling of `path`, returning its path.
///
/// A version directory is built under a temporary name and renamed into place once both of its
/// files are written; the same unpredictable middle and the same `AlreadyExists` retry keep a
/// planted directory from being the one built into.
pub(crate) fn create_new_dir(path: &Path) -> io::Result<PathBuf> {
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
    for _ in 0..16 {
        let nonce = RandomState::new().build_hasher().finish();
        let temporary = path.with_file_name(format!(".{name}.{nonce:016x}.tmp"));
        match fs::create_dir(&temporary) {
            Ok(()) => return Ok(temporary),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!(
            "cannot create a temporary directory beside {} after 16 attempts",
            path.display()
        ),
    ))
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
