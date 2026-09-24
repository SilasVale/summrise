//! ONE WAY TO REPLACE A FILE ATOMICALLY — the mechanics, and nothing else.
//!
//! `File::create` truncates IN PLACE, so a crash (a Windows service kill, power
//! loss) between the truncate and the rewrite loses the whole file. Every durable
//! "rewrite this file" in this crate therefore does the same dance: write a
//! SIBLING temp, flush + `sync_all` it, then rename it over the target — atomic
//! on both platforms, so a reader sees the old file or the new one and never a
//! half-written one.
//!
//! SEVEN SPELLINGS OF THAT DANCE, and the drift was measured rather than
//! imagined:
//!
//!   * three of the seven never called `sync_all` at all, so a power cut after
//!     the rename could leave the REPLACED file empty;
//!   * FOUR of the seven left the temp behind when the RENAME (not the write)
//!     failed — `bootstrap`, `connections`, `secrets` and `ssh` each returned the
//!     rename error with `<name>.tmp` still on disk, and the other three cleaned
//!     up only on the write failure they had thought of. The bytes are on disk by
//!     then, so a leftover `<name>.tmp` beside a store is indistinguishable from
//!     a write still in flight;
//!   * the hardening step was spelled four ways (a bare `let _ =`, a logged
//!     failure, a fail-closed refusal, and a `#[cfg(unix)]` chmod), so WHICH
//!     protection a file got was an accident of which copy it was written
//!     against rather than a decision anyone made;
//!   * the "no residue" / "a failed write leaves the original intact" properties
//!     were pinned only for the one implementation that had a name
//!     ([`crate::jsonl`], with `evidence`/`runs` asserting residue through it).
//!     They are pinned here once now, and every caller inherits them.
//!
//! The posture is a PARAMETER ([`Hardening`]) rather than a policy of this
//! module, because the callers genuinely differ: a session audit trail wants no
//! permission work, a metadata inventory must not cost the operator their saved
//! connections over an unavailable ACL, and the file that holds SSH passwords
//! must not land unprotected. What this module owns is that the posture the
//! caller stated is applied to the TEMP before the rename, exactly once,
//! wherever the bytes came from.
//!
//! STILL SPELLING ITS OWN: `plugins/memory/store.rs`'s tombstone compaction
//! (`MemoryStore::compact`) — same mechanics, its own temp+sync+rename. It is
//! the one member left to move, and it is named here so the next round does not
//! have to re-measure to find it.

use std::io::Write;
use std::path::{Path, PathBuf};

/// HOW A REPLACEMENT FILE IS PROTECTED — stated by the caller, applied by the
/// one writer.
///
/// The distinction is about FAILURE, not about strength: every posture calls the
/// same hardener, and they differ only in what happens when it cannot do its
/// job. That is the part a call site must decide, because only it knows what the
/// file holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Hardening {
    /// No permission work.
    None,
    /// Try to harden; a failure is logged and the write continues.
    BestEffort,
    /// A failure to harden FAILS the write — the file must not land unprotected.
    FailClosed,
}

/// REPLACE `path` WITH WHAT `write` PRODUCES, ATOMICALLY.
///
/// The temp is a SIBLING of the target — same directory, so the rename stays on
/// one filesystem — named by APPENDING ".tmp" to the target's file name
/// (`x.jsonl` -> `x.jsonl.tmp`). Appended rather than a hidden dotfile, and
/// never by replacing the extension: litter left by a crashed write is
/// something an operator should be able to SEE, and a filter that selects
/// `*.jsonl` must not silently hide it.
///
/// THE SEQUENCE IS FIXED, and each step's position is a rule:
///
///   1. create the temp — never the target, which must keep its old bytes until
///      the instant the rename lands;
///   2. run `write`, which also means the caller's own encoding (the DPAPI seal
///      in `secrets.rs`) happens with the plaintext never touching the target;
///   3. `flush`, then `sync_all` — the rename must not outrun the data. A power
///      cut after rename without the sync can leave the replaced file EMPTY;
///   4. HARDEN THE TEMP, BEFORE THE RENAME — the whole point of a posture. After
///      the rename there is a window (a crash, a concurrent reader, another
///      local account) in which the real file name exists with the temp's
///      inherited permissions. Doing it before means the target only ever
///      appears already protected;
///   5. rename the temp onto the target — the atomic step, and the only one
///      that can be;
///   6. remove the temp on ANY failure, including a failed rename. A caller that
///      finds `<name>.tmp` beside its store cannot tell a crashed write from a
///      live one, and four of the hand-rolled copies this replaced left it there.
pub(crate) fn replace(
    path: &std::path::Path,
    hardening: Hardening,
    write: impl FnOnce(&mut std::fs::File) -> std::io::Result<()>,
) -> std::io::Result<()> {
    let tmp = temp_path(path);
    match attempt(path, &tmp, hardening, write) {
        Ok(()) => Ok(()),
        Err(e) => {
            // ANY failure removes the temp (step 6 above). The bytes are already
            // on disk by the time a rename fails, so leaving them is precisely
            // the litter an operator misreads as a write in progress.
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

/// Did `replace` refuse because the [`Hardening::FailClosed`] hardening failed?
///
/// The three failure phases report sentences an operator already knows (the temp
/// could not be written / could not be secured / could not be renamed into
/// place), and a caller with its own sentence for ONE of them — `secrets.rs`
/// refuses in its own words — must be able to tell which phase it got. Parsing a
/// message would be the alternative, and a message is not an interface.
pub(crate) fn hardening_failed(e: &std::io::Error) -> bool {
    e.get_ref()
        .is_some_and(|inner| inner.downcast_ref::<HardeningFailure>().is_some())
}

/// The FailClosed refusal, carried as the PAYLOAD of the `io::Error` [`replace`]
/// returns. Its `Display` is the hardener's own message, so a caller that
/// already had a sentence for this case keeps it word for word.
#[derive(Debug)]
struct HardeningFailure(std::io::Error);

impl std::fmt::Display for HardeningFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for HardeningFailure {}

fn attempt(
    path: &Path,
    tmp: &Path,
    hardening: Hardening,
    write: impl FnOnce(&mut std::fs::File) -> std::io::Result<()>,
) -> std::io::Result<()> {
    // The three phases name themselves in the error, and the temp path is in
    // there too: "write" alone cannot tell an operator WHICH file a full disk
    // refused. These are the sentences call sites used to build by hand.
    let mut out = std::fs::File::create(tmp).map_err(|e| write_failed(tmp, e))?;
    write(&mut out).map_err(|e| write_failed(tmp, e))?;
    out.flush().map_err(|e| write_failed(tmp, e))?;
    out.sync_all().map_err(|e| write_failed(tmp, e))?;
    // Close before the rename: a rename over a path this process still holds
    // open is a platform-dependent refusal (Windows needs the handle to have
    // been opened FILE_SHARE_DELETE — Rust's is, but the guarantee is cheaper to
    // not depend on), and one fewer open handle is one fewer way to fail.
    drop(out);
    harden(tmp, hardening)?;
    std::fs::rename(tmp, path)
        .map_err(|e| std::io::Error::new(e.kind(), format!("rename to {path:?}: {e}")))
}

fn write_failed(tmp: &Path, e: std::io::Error) -> std::io::Error {
    std::io::Error::new(e.kind(), format!("write {tmp:?}: {e}"))
}

/// Apply the caller's posture to the temp (step 4 of [`replace`]).
fn harden(tmp: &Path, hardening: Hardening) -> std::io::Result<()> {
    match hardening {
        // No permission work at all — not a call whose failure is ignored.
        Hardening::None => Ok(()),
        Hardening::BestEffort => match harden_outcome(tmp) {
            Ok(()) => Ok(()),
            Err(e) => {
                // A volume without ACL support (or a path whose ACL cannot be
                // read back) must not fail the write the caller asked for. The
                // posture is stated at the call site; this line is what keeps the
                // failure visible instead of swallowed. One wording, because the
                // alternative was a module-specific spelling of the same event at
                // every site that needed one.
                tracing::debug!(
                    "[summrise-agent] atomic: ACL hardening unavailable for {tmp:?}: {e}"
                );
                Ok(())
            }
        },
        Hardening::FailClosed => {
            harden_outcome(tmp).map_err(|e| std::io::Error::new(e.kind(), HardeningFailure(e)))
        }
    }
}

fn harden_outcome(tmp: &Path) -> std::io::Result<()> {
    #[cfg(test)]
    {
        if HARDENING_FAILS.with(|c| c.get()) {
            return Err(std::io::Error::other(
                "hardening forced to fail (atomic.rs test seam)",
            ));
        }
    }
    crate::paths::harden_file(tmp)
}

/// The temp [`replace`] writes through: a SIBLING of `path`, named by APPENDING
/// ".tmp" to the target's file name.
///
/// APPENDED, not `with_extension("tmp")`: replacing the extension is how a
/// `monitors.json` rewrite once landed its temp at `monitors.jsonl.tmp` — a name
/// that looks like the log of a DIFFERENT file, and that a `*.json` filter walks
/// straight past. The suffix is therefore derived from the whole file name, and
/// that is what makes the name predictable to the sweepers that clean up after a
/// crash (`session_log::prune_stale` filters `<sid>.jsonl.tmp`).
fn temp_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(".tmp");
    path.with_file_name(name)
}

#[cfg(test)]
thread_local! {
    /// TEST SEAM: while armed, EVERY hardening attempt fails.
    static HARDENING_FAILS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Run `f` with the hardening step forced to fail (test seam).
///
/// WHY A SEAM RATHER THAN A PATH TRICK: `paths::harden_file` is a chmod on unix
/// (applied to a file this process just created in its own temp dir — it
/// succeeds) and an `icacls` subprocess on Windows (which cannot be made to
/// reject a path the test owns). The one hardening failure a test could produce
/// without a seam — a DIRECTORY sitting at the temp path — makes `File::create`
/// fail instead, which is the WRITE phase and proves nothing about either
/// posture. The fail-closed refuse path in `secrets.rs` used to reach this
/// through an injectable hardener passed to it; the hardener belongs to this
/// module now, so the instrument moved here with it.
///
/// Thread-local and panic-safe: the guard disarms on unwind, so a failing
/// assertion cannot leak the seam into the next test on the same thread.
#[cfg(test)]
pub(crate) fn with_failing_hardening<T>(f: impl FnOnce() -> T) -> T {
    struct Disarm;
    impl Drop for Disarm {
        fn drop(&mut self) {
            HARDENING_FAILS.with(|c| c.set(false));
        }
    }
    HARDENING_FAILS.with(|c| c.set(true));
    let _disarm = Disarm;
    f()
}

#[cfg(test)]
mod tests {
    //! THE TRUTH TABLE, in one place, for every caller. These are the properties
    //! the hand-rolled copies were supposed to have and only `jsonl.rs` tested:
    //! success leaves the bytes and NO temp; a failed write leaves the original
    //! intact; a failed RENAME leaves no litter either (the phase two copies got
    //! wrong); FailClosed does not land when it cannot secure the file, and
    //! BestEffort does.
    use super::*;
    use std::io::Write;

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("summrise-atomic-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("temp dir");
        d
    }

    fn tmp_of(target: &Path) -> PathBuf {
        let mut n = target.file_name().unwrap().to_os_string();
        n.push(".tmp");
        target.with_file_name(n)
    }

    #[test]
    fn replace_lands_the_bytes_and_leaves_no_temp() {
        let d = dir("ok");
        let p = d.join("store.jsonl");
        std::fs::write(&p, "old\n").expect("seed");
        replace(&p, Hardening::None, |f| f.write_all(b"new\n")).expect("replace");
        assert_eq!(std::fs::read_to_string(&p).expect("read"), "new\n");
        assert!(!tmp_of(&p).exists(), "a completed replace leaves no temp");
    }

    /// The naming rule, measured at the only moment the temp is observable: while
    /// `write` runs. `<name>.tmp`, APPENDED — not the extension replaced (which
    /// turned a `monitors.json` rewrite's temp into `monitors.jsonl.tmp`), and
    /// not a hidden dotfile (litter must be visible).
    #[test]
    fn the_temp_is_a_visible_sibling_named_by_appending_tmp() {
        let d = dir("name");
        let p = d.join("monitors.json");
        replace(&p, Hardening::None, |f| {
            f.write_all(b"[]")?;
            assert!(
                d.join("monitors.json.tmp").exists(),
                "the temp is the file name with .tmp appended"
            );
            assert!(
                !d.join("monitors.jsonl.tmp").exists(),
                "never the extension replaced — that name looks like another file's log"
            );
            assert!(
                !d.join(".monitors.json.tmp").exists(),
                "and never a hidden dotfile: an operator has to be able to see it"
            );
            Ok(())
        })
        .expect("replace");
        assert!(!tmp_of(&p).exists());
    }

    #[test]
    fn a_failed_write_leaves_the_original_intact_and_no_temp() {
        let d = dir("write-fail");
        let p = d.join("store.json");
        std::fs::write(&p, "keep\n").expect("seed");
        let err = replace(&p, Hardening::None, |f| {
            // PARTIAL bytes first: this is the shape a disk-full kill leaves, and
            // the reason the temp exists at all.
            f.write_all(b"half a record")?;
            Err(std::io::Error::other("disk full"))
        })
        .expect_err("a failed write must report failure");
        assert!(
            err.to_string().starts_with("write "),
            "the phase is named: {err}"
        );
        assert_eq!(
            std::fs::read_to_string(&p).expect("read"),
            "keep\n",
            "a FAILED write must leave the original untouched"
        );
        assert!(!tmp_of(&p).exists(), "no temp residue after a failed write");
    }

    /// THE PHASE FOUR OF THE HAND-ROLLED COPIES GOT WRONG: the temp writes fine,
    /// so nothing in the write path reports a problem, and the rename is what
    /// fails. They left `<name>.tmp` beside the store; this is the regression
    /// test for that.
    #[test]
    fn a_failed_rename_removes_the_temp() {
        let d = dir("rename-fail");
        let p = d.join("store.json");
        // A DIRECTORY at the target: the rename cannot land on it.
        std::fs::create_dir_all(&p).expect("blocker dir");
        let err = replace(&p, Hardening::None, |f| f.write_all(b"new"))
            .expect_err("a rename that cannot land must report failure");
        assert!(
            err.to_string().starts_with("rename to "),
            "the callers' own rename sentence is built here: {err}"
        );
        assert!(
            !tmp_of(&p).exists(),
            "a failed RENAME must not leave the temp behind — the litter an \
             operator cannot tell from a write in progress"
        );
    }

    #[test]
    fn fail_closed_does_not_land_when_hardening_fails() {
        let d = dir("fail-closed");
        let p = d.join("secrets.json");
        std::fs::write(&p, "keep\n").expect("seed");
        let err = with_failing_hardening(|| {
            replace(&p, Hardening::FailClosed, |f| f.write_all(b"secret"))
        })
        .expect_err("FailClosed must refuse");
        assert!(
            hardening_failed(&err),
            "the refuse phase must be readable as a FACT, not parsed out of a \
             message: {err}"
        );
        assert!(
            !err.to_string().contains("write "),
            "and it must not be reported as a write failure: {err}"
        );
        assert_eq!(
            std::fs::read_to_string(&p).expect("read"),
            "keep\n",
            "the unprotected file must NOT land"
        );
        assert!(!tmp_of(&p).exists(), "the refused temp is cleaned up");
    }

    #[test]
    fn best_effort_lands_anyway_when_hardening_fails() {
        let d = dir("best-effort");
        let p = d.join("known-hosts.json");
        std::fs::write(&p, "keep\n").expect("seed");
        with_failing_hardening(|| replace(&p, Hardening::BestEffort, |f| f.write_all(b"new")))
            .expect("an unavailable ACL must not fail a BestEffort write");
        assert_eq!(std::fs::read_to_string(&p).expect("read"), "new");
        assert!(!tmp_of(&p).exists());
        assert!(
            !hardening_failed(&std::io::Error::other("x")),
            "and only a FailClosed refuse is a hardening failure"
        );
    }

    /// `None` means NO PERMISSION WORK — not "a failure that is ignored".
    ///
    /// Measured without depending on the process umask: the temp is pre-created
    /// with a known mode, `File::create` truncates it IN PLACE (keeping that
    /// mode), and the final file's mode then says whether the hardener ran.
    /// `paths::harden_file` is a 0o600 chmod on unix.
    #[cfg(unix)]
    #[test]
    fn none_does_no_permission_work_and_best_effort_does() {
        use std::os::unix::fs::PermissionsExt;
        let d = dir("posture");
        let mode = |p: &Path| std::fs::metadata(p).expect("stat").permissions().mode() & 0o777;

        let untouched = d.join("none.json");
        std::fs::write(tmp_of(&untouched), b"x").expect("seed temp");
        std::fs::set_permissions(tmp_of(&untouched), std::fs::Permissions::from_mode(0o644))
            .expect("chmod temp");
        replace(&untouched, Hardening::None, |f| f.write_all(b"new")).expect("replace");
        assert_eq!(
            mode(&untouched),
            0o644,
            "None must not touch permissions at all"
        );

        let hardened = d.join("best-effort.json");
        std::fs::write(tmp_of(&hardened), b"x").expect("seed temp");
        std::fs::set_permissions(tmp_of(&hardened), std::fs::Permissions::from_mode(0o644))
            .expect("chmod temp");
        replace(&hardened, Hardening::BestEffort, |f| f.write_all(b"new")).expect("replace");
        assert_eq!(mode(&hardened), 0o600, "BestEffort hardens the TEMP");
    }
}
