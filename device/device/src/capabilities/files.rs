//! A sandboxed filesystem capability (design §4.5).
//!
//! Every path is relative to one root — `--files-root`, or `<data>/workspace` by default, which the
//! device creates at startup. The refusals are the sandbox: an absolute path, any `..` component, a
//! non-UTF-8 name, a symlink that resolves outside the root, a write whose parent is absent or
//! escapes, and a read larger than the caller's byte cap. A path is checked after canonicalization,
//! so the question is not what the string looks like but where the filesystem says it is.
//!
//! **A write does not create missing parents.** `files.mkdir` creates one directory under an
//! existing parent, so the client chooses the tree instead of the capability guessing it; a write's
//! `MissingParent` refusal names `files.mkdir` as the fix. The root itself needs no mkdir: startup
//! creates it.
//!
//! `list` returns at most [`MAX_LIST_ENTRIES`] names. A larger directory is a typed refusal rather
//! than a prefix that looks like the whole directory.
//!
//! On Windows a component must be spelled the way the filesystem stores it. An 8.3 short name
//! (`PROGRA~1`) or a name with a trailing dot is an alternate spelling of a name `list` would report
//! differently, so it is refused rather than resolved. The check compares the requested component
//! with the parent directory's entries **before the component is resolved** (the parent is
//! canonicalized only to prove it is inside the root), so the answer is the same on a volume that
//! generates 8.3 names and one that does not. A *new* name that merely
//! looks like an alias (`backup~1.txt`) is refused for the same reason: the sandbox will not guess
//! whether the OS would map it to another file.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

/// The most entries [`list`] returns; a larger directory is [`FilesError::TooManyEntries`].
pub const MAX_LIST_ENTRIES: usize = 1000;

/// What a directory entry is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A regular file.
    File,
    /// A directory.
    Directory,
    /// Anything else — a symlink (which is listed by its own name, never by its target), a socket,
    /// a device.
    Other,
}

/// One entry [`list`] found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The entry's name, one path component and valid UTF-8.
    pub name: String,
    /// What it is.
    pub kind: Kind,
    /// Its length in bytes; `0` for anything but a file.
    pub len: u64,
}

/// Why a filesystem call was refused or failed.
///
/// The refusals are typed rather than stringly: a caller (MCP, the panel) can render them, and a
/// test names the rule it is proving.
#[derive(Debug)]
pub enum FilesError {
    /// The path is absolute (or, on Windows, drive-relative or a UNC prefix).
    Absolute,
    /// The path contains a `..` component, even one that would stay inside the root.
    Parent,
    /// A component of the path is not valid UTF-8.
    NonUtf8Name,
    /// Canonicalization resolved the path outside the root.
    OutsideRoot,
    /// The file does not exist.
    NotFound,
    /// The path is not a regular file where one was expected.
    NotAFile,
    /// The path is not a directory where one was expected.
    NotADirectory,
    /// A write's parent directory does not exist. A capability does not create parents; the caller
    /// creates each level with `files.mkdir`.
    MissingParent,
    /// `mkdir`'s path names no final component (the root itself).
    NoName,
    /// `mkdir`'s destination already exists. Mkdiring is non-destructive: it never replaces a file
    /// or a directory, and it never deletes one.
    AlreadyExists,
    /// The path spells a name the filesystem stores differently — a Windows 8.3 short name or a
    /// trailing dot. Resolving it would touch a name `list` does not report, so it is refused.
    #[cfg_attr(not(windows), allow(dead_code))]
    AlternateSpelling,
    /// The directory holds more entries than [`list`] returns.
    TooManyEntries {
        /// The cap.
        max: usize,
    },
    /// The payload exceeds the caller's cap. A read is refused from metadata, before reading.
    TooLarge {
        /// The payload's size in bytes.
        len: u64,
        /// The cap the caller gave.
        max: usize,
    },
    /// The operating system refused the call for another reason.
    Io(io::Error),
}

impl fmt::Display for FilesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FilesError::Absolute => write!(f, "an absolute path cannot name a file under the root"),
            FilesError::Parent => write!(f, "a '..' component cannot name a file under the root"),
            FilesError::NonUtf8Name => write!(f, "a file name must be UTF-8"),
            FilesError::OutsideRoot => write!(f, "the path resolves outside the files root"),
            FilesError::NotFound => write!(f, "the file does not exist"),
            FilesError::NotAFile => write!(f, "the path is not a regular file"),
            FilesError::NotADirectory => write!(f, "the path is not a directory"),
            FilesError::MissingParent => {
                write!(
                    f,
                    "the file's parent directory does not exist; create it first with files.mkdir"
                )
            }
            FilesError::NoName => write!(f, "the path names no directory below the root"),
            FilesError::AlreadyExists => {
                write!(f, "the path already exists; files.mkdir does not overwrite")
            }
            FilesError::AlternateSpelling => write!(
                f,
                "the path is an alternate spelling of a stored name; use the spelling files.list reports"
            ),
            FilesError::TooManyEntries { max } => write!(
                f,
                "the directory holds more than {max} entries; files.list is capped at {max}"
            ),
            FilesError::TooLarge { len, max } => {
                write!(f, "the file is {len} bytes; the cap is {max}")
            }
            FilesError::Io(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for FilesError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            FilesError::Io(error) => Some(error),
            _ => None,
        }
    }
}

/// List `rel` (a directory relative to `root`), sorted by name, at most [`MAX_LIST_ENTRIES`].
///
/// A directory with more entries is [`FilesError::TooManyEntries`], not a truncated list: a client
/// cannot tell a capped prefix from the whole directory, so the cap is an error it can act on.
pub fn list(root: &Path, rel: &Path) -> Result<Vec<Entry>, FilesError> {
    let directory = resolve_dir(root, rel)?;

    let mut entries = Vec::new();
    for entry in fs::read_dir(&directory).map_err(FilesError::Io)? {
        if entries.len() >= MAX_LIST_ENTRIES {
            return Err(FilesError::TooManyEntries {
                max: MAX_LIST_ENTRIES,
            });
        }
        let entry = entry.map_err(FilesError::Io)?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| FilesError::NonUtf8Name)?;
        let file_type = entry.file_type().map_err(FilesError::Io)?;
        let (kind, len) = if file_type.is_file() {
            (Kind::File, entry.metadata().map_err(FilesError::Io)?.len())
        } else if file_type.is_dir() {
            (Kind::Directory, 0)
        } else {
            // A symlink is listed as itself, never as its target: following it here would report a
            // file that `read` may refuse.
            (Kind::Other, 0)
        };
        entries.push(Entry { name, kind, len });
    }
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(entries)
}

/// Resolve `rel` (a directory relative to `root`) to the canonical directory it names.
///
/// This is the sandbox check `runs.exec` reuses for a command's working directory: the same
/// refusals a `list` gets (absolute, `..`, non-UTF-8, an escaping symlink, a non-directory) apply
/// before a process starts. It decides where the process begins, not what it can reach afterwards.
pub fn resolve_dir(root: &Path, rel: &Path) -> Result<PathBuf, FilesError> {
    validate(rel)?;
    let root = canonical_root(root)?;
    #[cfg(windows)]
    ensure_spelling(&root, rel)?;
    let directory = canonicalize(&root.join(rel))?;
    ensure_inside(&root, &directory)?;
    if !fs::metadata(&directory).map_err(FilesError::Io)?.is_dir() {
        return Err(FilesError::NotADirectory);
    }
    Ok(directory)
}

/// Read `rel` (a file relative to `root`) into memory, refusing anything over `max_bytes`.
pub fn read(root: &Path, rel: &Path, max_bytes: usize) -> Result<Vec<u8>, FilesError> {
    validate(rel)?;
    let root = canonical_root(root)?;
    #[cfg(windows)]
    ensure_spelling(&root, rel)?;
    let path = canonicalize(&root.join(rel))?;
    ensure_inside(&root, &path)?;

    let metadata = fs::metadata(&path).map_err(FilesError::Io)?;
    if !metadata.is_file() {
        return Err(FilesError::NotAFile);
    }
    if metadata.len() > max_bytes as u64 {
        // The cap is enforced from metadata, before the file is opened for reading.
        return Err(FilesError::TooLarge {
            len: metadata.len(),
            max: max_bytes,
        });
    }

    let bytes = fs::read(&path).map_err(FilesError::Io)?;
    if bytes.len() > max_bytes {
        // The file grew between the metadata check and the read; the cap still holds.
        return Err(FilesError::TooLarge {
            len: bytes.len() as u64,
            max: max_bytes,
        });
    }
    Ok(bytes)
}

/// Write `bytes` to `rel` (a file relative to `root`), refusing anything over `max_bytes`.
///
/// The parent must exist. A `MissingParent` refusal names `files.mkdir`, so the client is told how
/// to create the tree rather than left to guess whether the write or the path was wrong.
pub fn write(root: &Path, rel: &Path, bytes: &[u8], max_bytes: usize) -> Result<(), FilesError> {
    if bytes.len() > max_bytes {
        return Err(FilesError::TooLarge {
            len: bytes.len() as u64,
            max: max_bytes,
        });
    }
    validate(rel)?;
    let root = canonical_root(root)?;

    let Some(name) = rel.file_name() else {
        return Err(FilesError::NotAFile);
    };
    let name = name.to_str().ok_or(FilesError::NonUtf8Name)?;

    // The spelling check covers every component, including the one being created, before the
    // parent is resolved or the destination is touched.
    #[cfg(windows)]
    ensure_spelling(&root, rel)?;

    // The parent is the part that must already exist and be inside the root. The destination itself
    // need not exist — that is what a write creates.
    let parent_rel = rel.parent().unwrap_or_else(|| Path::new(""));
    let parent = resolve_parent(&root, parent_rel)?;
    let destination = parent.join(name);

    // An existing destination may be a symlink. Writing through one that leaves the root would edit
    // a file the sandbox does not own, so the link is resolved and checked like a read's path.
    match fs::symlink_metadata(&destination) {
        Ok(metadata) => {
            if metadata.is_dir() {
                return Err(FilesError::NotAFile);
            }
            if metadata.file_type().is_symlink() {
                let resolved =
                    fs::canonicalize(&destination).map_err(|_| FilesError::OutsideRoot)?;
                ensure_inside(&root, &resolved)?;
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(FilesError::Io(error)),
    }

    fs::write(&destination, bytes).map_err(FilesError::Io)
}

/// Create one directory `rel` below `root`.
///
/// This is the tool that makes a fresh device's tree usable; the root itself is created at startup.
/// It creates exactly one level under an existing parent and is non-destructive: when the destination
/// already exists (as a file, a directory or a link) it refuses with [`FilesError::AlreadyExists`]
/// instead of replacing anything. That is why `files.mkdir` is not a destructive tool: there is no
/// byte of an existing entry it can change or remove.
pub fn mkdir(root: &Path, rel: &Path) -> Result<(), FilesError> {
    validate(rel)?;
    let root = canonical_root(root)?;
    let Some(name) = rel.file_name() else {
        return Err(FilesError::NoName);
    };
    let name = name.to_str().ok_or(FilesError::NonUtf8Name)?;

    // Every component, including the directory being created, is checked before the parent is
    // resolved or the destination is touched.
    #[cfg(windows)]
    ensure_spelling(&root, rel)?;

    let parent_rel = rel.parent().unwrap_or_else(|| Path::new(""));
    let parent = resolve_parent(&root, parent_rel)?;
    let destination = parent.join(name);

    match fs::symlink_metadata(&destination) {
        Ok(_) => return Err(FilesError::AlreadyExists),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(FilesError::Io(error)),
    }

    fs::create_dir(&destination).map_err(FilesError::Io)
}

/// The canonical parent of `rel`, checked to be inside `root` and to be a directory.
fn resolve_parent(root: &Path, parent_rel: &Path) -> Result<PathBuf, FilesError> {
    let parent = match fs::canonicalize(root.join(parent_rel)) {
        Ok(parent) => parent,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(FilesError::MissingParent)
        }
        Err(error) => return Err(FilesError::Io(error)),
    };
    ensure_inside(root, &parent)?;
    if !fs::metadata(&parent).map_err(FilesError::Io)?.is_dir() {
        return Err(FilesError::NotADirectory);
    }
    Ok(parent)
}

/// Reject the shapes a relative path may not have, before the filesystem is consulted.
fn validate(rel: &Path) -> Result<(), FilesError> {
    for component in rel.components() {
        match component {
            Component::ParentDir => return Err(FilesError::Parent),
            Component::RootDir | Component::Prefix(_) => return Err(FilesError::Absolute),
            Component::Normal(name) if name.to_str().is_none() => {
                return Err(FilesError::NonUtf8Name)
            }
            Component::Normal(_) | Component::CurDir => {}
        }
    }
    Ok(())
}

/// The root as the filesystem spells it: every later comparison is between canonical paths.
fn canonical_root(root: &Path) -> Result<PathBuf, FilesError> {
    canonicalize(root)
}

/// Canonicalize one path, naming the two ordinary failures.
fn canonicalize(path: &Path) -> Result<PathBuf, FilesError> {
    fs::canonicalize(path).map_err(|error| match error.kind() {
        io::ErrorKind::NotFound => FilesError::NotFound,
        _ => FilesError::Io(error),
    })
}

/// Whether `path` is the root or below it.
///
/// Both sides are canonical, so this is a component comparison rather than a string prefix: on
/// Windows it never mistakes `C:\root-other` for a child of `C:\root`, and it is not fooled by
/// case-insensitive spelling.
fn inside(root: &Path, path: &Path) -> bool {
    path == root || path.starts_with(root)
}

fn ensure_inside(root: &Path, path: &Path) -> Result<(), FilesError> {
    if inside(root, path) {
        Ok(())
    } else {
        Err(FilesError::OutsideRoot)
    }
}

/// Whether a Windows name could be an alternate spelling of a different stored name.
///
/// 8.3 short names carry `~` and a digit (`PROGRA~1`, `LONG-D~1`); the OS also strips trailing dots
/// and spaces. A normal component skips the directory scan entirely.
#[cfg(windows)]
fn looks_like_alternate(name: &str) -> bool {
    name.ends_with('.')
        || name.ends_with(' ')
        || name
            .rsplit_once('~')
            .is_some_and(|(_, suffix)| suffix.as_bytes().first().is_some_and(u8::is_ascii_digit))
}

/// Check one component against the names the directory actually stores, case-insensitively.
#[cfg(windows)]
fn ensure_entry_spelling(directory: &Path, requested: &str) -> Result<(), FilesError> {
    let folded = requested.to_lowercase();
    for entry in fs::read_dir(directory).map_err(FilesError::Io)? {
        let entry = entry.map_err(FilesError::Io)?;
        if entry.file_name().to_string_lossy().to_lowercase() == folded {
            return Ok(());
        }
    }
    Err(FilesError::AlternateSpelling)
}

/// Walk `rel` below `root`, checking the components that could be an alternate spelling.
///
/// This runs **before** the caller resolves anything: for each alternate-looking component the
/// parent directory's stored names are checked, so a trailing dot or an 8.3 alias is refused before
/// `canonicalize`, `metadata` or an open can let the OS choose a different name. `root` must already
/// be canonical (every caller gets it from [`canonical_root`]); each parent is canonicalized and
/// checked inside it before being listed, so the lookup never follows a prior component's symlink
/// out of the sandbox.
#[cfg(windows)]
fn ensure_spelling(root: &Path, rel: &Path) -> Result<(), FilesError> {
    let mut directory = root.to_path_buf();
    for component in rel.components() {
        let Component::Normal(name) = component else {
            continue;
        };
        let name = name.to_str().ok_or(FilesError::NonUtf8Name)?;
        if looks_like_alternate(name) {
            let parent = canonicalize(&directory)?;
            ensure_inside(root, &parent)?;
            ensure_entry_spelling(&parent, name)?;
        }
        directory.push(name);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::test_support::TempDir;

    /// A root directory that lives for as long as the caller keeps the [`TempDir`].
    fn rooted(label: &str) -> (TempDir, PathBuf) {
        let dir = TempDir::new(label);
        let root = dir.join("root");
        fs::create_dir_all(&root).expect("the root is creatable");
        (dir, root)
    }

    #[test]
    fn a_write_then_a_read_then_a_list_round_trips() {
        let (_dir, root) = rooted("files-round-trip");
        fs::create_dir(root.join("notes")).expect("the subdirectory is creatable");

        write(&root, Path::new("notes/todo.txt"), b"buy milk", 1024)
            .expect("a small write inside the root succeeds");

        assert_eq!(
            read(&root, Path::new("notes/todo.txt"), 1024).expect("the bytes come back"),
            b"buy milk"
        );
        assert_eq!(
            list(&root, Path::new("notes")).expect("the directory lists"),
            [Entry {
                name: "todo.txt".to_owned(),
                kind: Kind::File,
                len: 8,
            }]
        );

        let top = list(&root, Path::new("")).expect("the root itself lists");
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].name, "notes");
        assert_eq!(top[0].kind, Kind::Directory);

        write(&root, Path::new("notes/todo.txt"), b"done", 1024)
            .expect("a second write replaces the file");
        assert_eq!(
            read(&root, Path::new("notes/todo.txt"), 1024).expect("the new bytes come back"),
            b"done",
            "a write replaces the file's bytes rather than appending"
        );
    }

    #[test]
    fn a_parent_component_is_refused_by_every_operation() {
        let (dir, root) = rooted("files-parent");
        fs::write(dir.join("outside.txt"), b"outside").expect("the fixture outside the root");

        for relative in ["../outside.txt", "notes/../../outside.txt"] {
            let rel = Path::new(relative);
            assert!(
                matches!(read(&root, rel, 1024), Err(FilesError::Parent)),
                "read {relative}"
            );
            assert!(
                matches!(write(&root, rel, b"x", 1024), Err(FilesError::Parent)),
                "write {relative}"
            );
            assert!(
                matches!(list(&root, rel), Err(FilesError::Parent)),
                "list {relative}"
            );
            assert!(
                matches!(mkdir(&root, rel), Err(FilesError::Parent)),
                "mkdir {relative}"
            );
        }
    }

    #[test]
    fn an_absolute_path_is_refused_by_every_operation() {
        let (dir, root) = rooted("files-absolute");
        let outside = dir.join("outside.txt");
        fs::write(&outside, b"outside").expect("the fixture outside the root");

        assert!(
            matches!(read(&root, &outside, 1024), Err(FilesError::Absolute)),
            "an absolute read is refused before the filesystem is touched"
        );
        assert!(matches!(
            write(&root, &outside, b"x", 1024),
            Err(FilesError::Absolute)
        ));
        assert!(matches!(list(&root, &outside), Err(FilesError::Absolute)));
        assert!(matches!(mkdir(&root, &outside), Err(FilesError::Absolute)));
    }

    #[test]
    fn a_write_larger_than_the_cap_writes_nothing() {
        let (_dir, root) = rooted("files-write-cap");

        let error = write(&root, Path::new("big.bin"), b"12345", 4)
            .expect_err("five bytes do not fit a four-byte cap");

        assert!(
            matches!(error, FilesError::TooLarge { len: 5, max: 4 }),
            "{error:?}"
        );
        assert!(
            !root.join("big.bin").exists(),
            "a refused write must not create the file"
        );
    }

    #[test]
    fn a_read_larger_than_the_cap_is_refused_from_metadata_before_reading() {
        let (_dir, root) = rooted("files-read-cap");
        let path = root.join("huge.bin");
        // A sparse file: the test proves the size check happens before a read by making the read
        // itself impossible (a gigabyte) while the refusal is immediate.
        fs::File::create(&path)
            .expect("the sparse file is creatable")
            .set_len(1 << 30)
            .expect("the sparse file's length is settable");

        let error = read(&root, Path::new("huge.bin"), 4096)
            .expect_err("a gigabyte does not fit a four-kilobyte cap");

        assert!(
            matches!(
                error,
                FilesError::TooLarge {
                    len: 1_073_741_824,
                    max: 4_096
                }
            ),
            "{error:?}"
        );
    }

    #[test]
    fn a_write_refuses_a_parent_that_does_not_exist_and_names_the_fix() {
        let (_dir, root) = rooted("files-missing-parent");

        let error = write(&root, Path::new("missing/note.txt"), b"x", 1024)
            .expect_err("a capability does not create parent directories");

        assert!(matches!(error, FilesError::MissingParent), "{error:?}");
        assert!(
            error.to_string().contains("files.mkdir"),
            "the refusal names the tool that creates the parent: {error}"
        );
        assert!(!root.join("missing").exists());

        // The named fix works: mkdir one level, then the same write succeeds.
        mkdir(&root, Path::new("missing")).expect("mkdir creates the parent");
        write(&root, Path::new("missing/note.txt"), b"x", 1024)
            .expect("the write succeeds once the parent exists");
    }

    #[test]
    fn mkdir_creates_exactly_one_directory_and_never_overwrites() {
        let (_dir, root) = rooted("files-mkdir");
        fs::write(root.join("file.txt"), b"original").expect("the file fixture");
        fs::create_dir(root.join("directory")).expect("the directory fixture");
        fs::write(root.join("directory/keep.txt"), b"keep").expect("the fixture inside");

        mkdir(&root, Path::new("notes")).expect("a new directory below the root");
        assert!(root.join("notes").is_dir());
        mkdir(&root, Path::new("notes/sub")).expect("one level below an existing directory");

        for existing in ["file.txt", "directory", "notes"] {
            let error = mkdir(&root, Path::new(existing))
                .expect_err("mkdir never replaces an existing entry");
            assert!(
                matches!(error, FilesError::AlreadyExists),
                "{existing}: {error:?}"
            );
        }
        assert_eq!(
            fs::read(root.join("file.txt")).expect("the file is still readable"),
            b"original",
            "a refused mkdir leaves the file's bytes alone"
        );
        assert_eq!(
            fs::read(root.join("directory/keep.txt")).expect("the nested file is still readable"),
            b"keep"
        );

        let error = mkdir(&root, Path::new("missing/sub"))
            .expect_err("the parent has to exist; no recursive creation");
        assert!(matches!(error, FilesError::MissingParent), "{error:?}");
        assert!(!root.join("missing").exists());

        for no_name in ["", "."] {
            let error = mkdir(&root, Path::new(no_name)).expect_err("that names no directory");
            assert!(
                matches!(error, FilesError::NoName),
                "{no_name:?}: {error:?}"
            );
        }
    }

    #[test]
    fn list_is_capped_and_a_larger_directory_is_an_error_not_a_prefix() {
        let (_dir, root) = rooted("files-list-cap");
        // Deliberately created out of name order, so the returned list proves it is sorted.
        for index in (0..MAX_LIST_ENTRIES).rev() {
            fs::write(root.join(format!("note-{index:04}.txt")), b"x").expect("the fixture entry");
        }

        let entries = list(&root, Path::new("")).expect("exactly the cap still lists");
        assert_eq!(entries.len(), MAX_LIST_ENTRIES);
        assert!(
            entries.windows(2).all(|pair| pair[0].name < pair[1].name),
            "the list is sorted by name"
        );

        fs::write(root.join("one-more.txt"), b"x").expect("the entry over the cap");
        let error = list(&root, Path::new("")).expect_err("one entry over the cap is refused");
        assert!(
            matches!(
                error,
                FilesError::TooManyEntries {
                    max: MAX_LIST_ENTRIES
                }
            ),
            "{error:?}"
        );
        assert!(
            error.to_string().contains("1000"),
            "the refusal names the cap: {error}"
        );
    }

    #[test]
    fn the_wrong_kind_is_a_named_refusal() {
        let (_dir, root) = rooted("files-kind");
        fs::write(root.join("file.txt"), b"x").expect("the file fixture");
        fs::create_dir(root.join("directory")).expect("the directory fixture");

        assert!(matches!(
            read(&root, Path::new(""), 1024),
            Err(FilesError::NotAFile)
        ));
        assert!(matches!(
            read(&root, Path::new("directory"), 1024),
            Err(FilesError::NotAFile)
        ));
        assert!(matches!(
            list(&root, Path::new("file.txt")),
            Err(FilesError::NotADirectory)
        ));
        assert!(matches!(
            read(&root, Path::new("absent.txt"), 1024),
            Err(FilesError::NotFound)
        ));
        assert!(matches!(
            write(&root, Path::new("directory"), b"x", 1024),
            Err(FilesError::NotAFile)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_that_resolves_outside_the_root_is_refused() {
        use std::os::unix::fs::symlink;

        let (dir, root) = rooted("files-symlink-read");
        let outside = dir.join("outside.txt");
        fs::write(&outside, b"secret").expect("the fixture outside the root");
        symlink(&outside, root.join("escape")).expect("the escaping symlink");

        assert!(
            matches!(
                read(&root, Path::new("escape"), 1024),
                Err(FilesError::OutsideRoot)
            ),
            "canonicalization, not the string, decides where a path is"
        );

        // Staying inside is not the sin: an inside link still reads.
        fs::write(root.join("inside.txt"), b"inside").expect("the inside fixture");
        symlink(root.join("inside.txt"), root.join("inside-link")).expect("the inside symlink");
        assert_eq!(
            read(&root, Path::new("inside-link"), 1024).expect("an inside link reads"),
            b"inside"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_write_cannot_reach_through_a_parent_symlink() {
        use std::os::unix::fs::symlink;

        let (dir, root) = rooted("files-symlink-write-parent");
        let outside = dir.join("outside");
        fs::create_dir_all(outside.join("nested")).expect("the directory outside the root");
        symlink(&outside, root.join("escape")).expect("the escaping symlink");

        // The final parent (`escape`) resolves outside the root.
        let error = write(&root, Path::new("escape/note.txt"), b"x", 1024)
            .expect_err("the parent resolves outside the root");
        assert!(matches!(error, FilesError::OutsideRoot), "{error:?}");

        // An intermediate parent does too, even though the leaf parent exists under the target.
        let error = write(&root, Path::new("escape/nested/note.txt"), b"x", 1024)
            .expect_err("an intermediate symlink is still a path outside the root");
        assert!(matches!(error, FilesError::OutsideRoot), "{error:?}");

        // mkdir is a write of a directory and follows the same rule.
        let error = mkdir(&root, Path::new("escape/new-directory"))
            .expect_err("mkdir must not create a directory outside the root");
        assert!(matches!(error, FilesError::OutsideRoot), "{error:?}");

        assert!(
            !outside.join("note.txt").exists()
                && !outside.join("nested").join("note.txt").exists()
                && !outside.join("new-directory").exists(),
            "nothing may be created outside the root"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_write_onto_a_symlink_that_resolves_outside_the_root_is_refused() {
        use std::os::unix::fs::symlink;

        let (dir, root) = rooted("files-symlink-write-file");
        let outside = dir.join("outside.txt");
        fs::write(&outside, b"secret").expect("the fixture outside the root");
        symlink(&outside, root.join("link.txt")).expect("the escaping symlink");

        let error = write(&root, Path::new("link.txt"), b"replaced", 1024)
            .expect_err("writing through the link would edit a file outside the root");

        assert!(matches!(error, FilesError::OutsideRoot), "{error:?}");
        assert_eq!(
            fs::read(&outside).expect("the outside file is readable"),
            b"secret",
            "the refused write must not have reached the target"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_non_utf8_name_is_refused_rather_than_lossily_named() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let (_dir, root) = rooted("files-non-utf8");
        let name = OsStr::from_bytes(b"caf\xe9.txt");
        fs::write(root.join(name), b"x").expect("the non-UTF-8 fixture");

        let error = list(&root, Path::new("")).expect_err("a lossy name would be a different file");

        assert!(matches!(error, FilesError::NonUtf8Name), "{error:?}");

        let error = write(&root, Path::new(name), b"x", 1024)
            .expect_err("a write must not create a name that list cannot report");
        assert!(matches!(error, FilesError::NonUtf8Name), "{error:?}");
    }

    #[cfg(windows)]
    #[test]
    fn a_windows_drive_or_unc_path_is_refused_by_every_operation() {
        let (_dir, root) = rooted("files-windows-prefixes");

        // `C:foo` is drive-relative, not absolute, and is the same class of name: it starts at a
        // prefix the sandbox did not choose. `\\?\` is the verbatim form that bypasses Win32 path
        // normalization; it must not reach `canonicalize` at all.
        for relative in [
            r"C:\Windows\win.ini",
            r"C:relative.txt",
            r"\\server\share\file.txt",
            r"\\?\C:\Windows\win.ini",
        ] {
            let path = Path::new(relative);
            assert!(
                matches!(read(&root, path, 1024), Err(FilesError::Absolute)),
                "read {relative}"
            );
            assert!(
                matches!(write(&root, path, b"x", 1024), Err(FilesError::Absolute)),
                "write {relative}"
            );
            assert!(
                matches!(list(&root, path), Err(FilesError::Absolute)),
                "list {relative}"
            );
            assert!(
                matches!(mkdir(&root, path), Err(FilesError::Absolute)),
                "mkdir {relative}"
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn a_windows_alternate_spelling_is_refused() {
        let (_dir, root) = rooted("files-windows-short-names");
        let long = root.join("long-directory-name");
        fs::create_dir(&long).expect("the long-named directory");
        fs::write(long.join("todo.txt"), b"inside").expect("the fixture inside it");

        // A trailing dot always names the same file without one; the sandbox refuses the spelling
        // it was not told the file has.
        let error = read(&root, Path::new("long-directory-name/todo.txt."), 1024)
            .expect_err("a trailing dot is an alternate spelling");
        assert!(matches!(error, FilesError::AlternateSpelling), "{error:?}");

        // The refusal rule itself does not depend on this volume generating the alias: the
        // requested component is not the name any entry stores, so it is refused before resolution.
        let spelling_root = canonical_root(&root).expect("the fixture root canonicalizes");
        let error = ensure_spelling(&spelling_root, Path::new("LONG-D~1"))
            .expect_err("a short-name component must not match a stored long name");
        assert!(matches!(error, FilesError::AlternateSpelling), "{error:?}");
        ensure_spelling(&spelling_root, Path::new("long-directory-name"))
            .expect("the stored spelling is accepted");

        // NTFS generates `LONG-D~1` for `long-directory-name` when 8.3 names are enabled on the
        // volume. The probe records which environment this ran in, but the answer is the same on
        // both: the spelling check runs before the OS can resolve the alias, so an alias-looking
        // component is refused whether or not this volume would have generated it.
        let alias = Path::new("LONG-D~1");
        match fs::metadata(root.join(alias)) {
            Ok(_) => {}
            Err(error) => assert_eq!(
                error.kind(),
                io::ErrorKind::NotFound,
                "the 8.3 probe is either an existing alias or absence, not another failure: {error}"
            ),
        }

        let error = read(&root, &alias.join("todo.txt"), 1024)
            .expect_err("the 8.3 alias spelling must not read the long name");
        assert!(matches!(error, FilesError::AlternateSpelling), "{error:?}");
        let error = write(&root, &alias.join("todo.txt"), b"x", 1024)
            .expect_err("the 8.3 alias spelling must not write the long name");
        assert!(matches!(error, FilesError::AlternateSpelling), "{error:?}");
        let error = mkdir(&root, &alias.join("new"))
            .expect_err("the 8.3 alias spelling must not create under the long name");
        assert!(matches!(error, FilesError::AlternateSpelling), "{error:?}");
        assert_eq!(
            fs::read(long.join("todo.txt")).expect("the long name still reads"),
            b"inside",
            "a refused alias write leaves the target alone"
        );

        // A new file whose name ends in a dot would be stored without it; refuse before creating it.
        let error = write(&root, Path::new("long-directory-name/new."), b"x", 1024)
            .expect_err("a trailing dot would be stripped by the filesystem");
        assert!(matches!(error, FilesError::AlternateSpelling), "{error:?}");
        assert!(!long.join("new").exists());

        // A new name that only looks like an 8.3 alias is refused too: allowing it would mean
        // guessing whether the OS maps it to an existing long name.
        let error = write(&root, Path::new("backup~1.txt"), b"x", 1024)
            .expect_err("an alias-shaped new name is not created by guesswork");
        assert!(matches!(error, FilesError::AlternateSpelling), "{error:?}");
        assert!(!root.join("backup~1.txt").exists());
    }
}
