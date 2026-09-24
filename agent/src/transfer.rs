//! ONE WAY TO LAND A TRANSFER — the `.part` staging rule, the size cap, the
//! corpse, and nothing about HTTP, SFTP or paths.
//!
//! A TRANSFER DOOR lands a payload the device does not hold yet:
//! `system_file_download` receives one from a URL, and `terminal_sftp`'s
//! download arm has just read one whole off another machine. Both used to spell
//! the landing themselves, and the two spellings disagreed in exactly the way
//! that matters — the URL door staged a `.part` sibling and renamed it into
//! place under a cap; the SFTP door called `std::fs::write` on the caller's own
//! path with no cap and no staging at all, so an over-cap read landed anyway and
//! a write that died halfway left a file that LOOKS complete. The rule lives
//! here now, and it owns four things:
//!
//!   * the `.part` SIBLING — the path the caller named is never written to
//!     directly and never holds a partial file;
//!   * the cap, enforced WHILE the bytes arrive, so a payload over it is refused
//!     instead of filling a disk and then being reported;
//!   * the corpse removed on EVERY failure path, a failed rename included. That
//!     is the step a hand-rolled copy gets wrong, because by then the bytes are
//!     already on disk and a leftover `<name>.part` is indistinguishable from a
//!     transfer still in flight;
//!   * the rename LAST — the one step of a landing that can be atomic.
//!
//! WHY NOT [`crate::atomic`]. That module REPLACES a file whose whole body the
//! caller already holds, and its writer is a blocking closure run to
//! completion; a landing stages a payload that has NOT finished arriving, and
//! the streaming door must refuse a too-large transfer chunk by chunk and abort
//! mid-flight. The two share the temp+rename SHAPE and not the rule — which is
//! why the transfer landing was deferred by that module's header rather than
//! folded into it. ONE QUESTION TRAVELLED HERE WITH THE RULE, because the
//! deferral named it: a landing FLUSHES without ever `sync_all`ing, so a power
//! cut after the rename can still lose the payload. That is recorded as OPEN
//! rather than silently inherited (a transfer is not a store rewrite; the cost
//! of the answer is spelled out at `atomic::replace`'s `sync_all` step). The
//! next round to touch durability starts here.
//!
//! THE CALLER DECIDES WHAT TO SAY. [`TransferError`] reports what HAPPENED —
//! which cap was exceeded, which phase failed, in the words the doors already
//! used — and the sentence a person reads belongs to the door, which has been
//! saying it since before this rule had a name.

use std::io::Write;
use std::path::{Path, PathBuf};
use tokio::io::AsyncReadExt;

/// THE LARGEST PAYLOAD ANY TRANSFER DOOR WILL LAND. One declaration: the tool
/// prose promises "100 MB" and the code used to enforce it twice, in two
/// places, for two directions.
pub(crate) const MAX_TRANSFER_BYTES: u64 = 100 * 1024 * 1024;

/// WHY A LANDING FAILED — the caller decides what to say, this says what
/// happened.
#[derive(Debug)]
pub(crate) enum TransferError {
    /// The payload is larger than `cap`, and NOTHING was landed: the streamed
    /// door stops at the cap and removes the part it had staged, the buffered
    /// door refuses before it stages anything. The cap rides along so the door
    /// can name it in its own sentence.
    TooLarge { cap: u64 },
    /// A step of the landing failed. Its `Display` names the PHASE ("write:
    /// …", "rename <part> -> <dest>: …") in the sentences the doors produced
    /// before the sequence moved here, so a caller that renders it verbatim
    /// keeps saying what it always said.
    Io(std::io::Error),
}

/// LAND A PAYLOAD THAT IS STILL ARRIVING. Reads at most `cap` bytes from
/// `source`, staging into a `.part` sibling of `path` and renaming LAST, so the
/// caller's path never holds a partial file.
///
/// The bytes are consumed as they arrive: nothing about the transfer's size is
/// held in memory here, and a source that stops early (a connection that dies
/// mid-body) fails the landing with the destination untouched.
pub(crate) async fn land_streamed<R>(
    path: &Path,
    cap: u64,
    source: &mut R,
) -> Result<u64, TransferError>
where
    R: tokio::io::AsyncRead + Unpin,
{
    land(path, cap, source).await
}

/// LAND A PAYLOAD ALREADY IN HAND. Refuses a buffer larger than `cap` BEFORE
/// staging it.
///
/// THE REFUSAL PRECEDES THE FIRST WRITE, so an over-cap buffer leaves no trace
/// at all — not the destination, not a `.part` sibling, not a directory that
/// had to be created for it.
///
/// WHY THIS IS SYNCHRONOUS and still goes through the same landing:
/// `futures::executor::block_on` is not a runtime smuggled in through the back
/// door. The landing this drives awaits exactly ONE thing — reads from the
/// caller's buffer, and `AsyncRead for &[u8]` is always `Ready` — while its
/// file work is synchronous. The future therefore completes on its FIRST poll
/// and never parks a worker, and because no step of it reaches for a reactor,
/// this call is legal outside a runtime as well as inside one (which is what
/// lets its tests be plain `#[test]`s, and what would fail loudly rather than
/// quietly if a `tokio::fs` step were ever added to the landing).
///
/// The door with a payload already in hand is `terminal_sftp`, which exists
/// only in the `terminal` configuration; the item is kept in the headless
/// configuration too, because the module's surface does not change between
/// configurations (the same rule the other tools follow).
#[cfg_attr(not(feature = "terminal"), allow(dead_code))]
pub(crate) fn land_buffered(path: &Path, cap: u64, bytes: &[u8]) -> Result<u64, TransferError> {
    if bytes.len() as u64 > cap {
        return Err(TransferError::TooLarge { cap });
    }
    let mut source = bytes;
    futures::executor::block_on(land(path, cap, &mut source))
}

/// THE ONE LANDING both doors go through — and with the clean-up half of the
/// rule wrapped around it, so a caller cannot reach the sequence without it.
async fn land<R>(path: &Path, cap: u64, source: &mut R) -> Result<u64, TransferError>
where
    R: tokio::io::AsyncRead + Unpin,
{
    match stage_and_rename(path, cap, source).await {
        Ok(total) => Ok(total),
        Err(e) => {
            // ANY failure removes the part, the rename included. The bytes are
            // on disk by the time a rename fails, so leaving them is precisely
            // the litter an operator misreads as a transfer in progress.
            let _ = std::fs::remove_file(part_path(path));
            Err(e)
        }
    }
}

/// The PARENTS / OPEN / READ / WRITE / FLUSH / RENAME sequence [`land`] wraps
/// in its clean-up: every step up to and including the rename, with no rollback
/// of its own. A caller must not reach past [`land`] to it — removing the part
/// on failure is half of what this module guarantees.
async fn stage_and_rename<R>(path: &Path, cap: u64, source: &mut R) -> Result<u64, TransferError>
where
    R: tokio::io::AsyncRead + Unpin,
{
    // Parents are created HERE rather than left to the caller: a transfer's
    // destination is often a directory that does not exist yet (the tool prose
    // says so: "parents are auto-created"), and a landing that failed on a
    // missing parent would make every door spell the same `create_dir_all`.
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| phase(format!("create parent {}: {e}", parent.display()), e))?;
    }
    let part = part_path(path);
    let mut file = std::fs::File::create(&part)
        .map_err(|e| phase(format!("open {}: {e}", part.display()), e))?;
    let mut buf = vec![0u8; READ_WINDOW];
    let mut total: u64 = 0;
    loop {
        let n = source
            .read(&mut buf)
            .await
            .map_err(|e| phase(format!("read chunk: {e}"), e))?;
        if n == 0 {
            break;
        }
        total += n as u64;
        // THE CAP IS CHECKED BEFORE THE BYTES ARE WRITTEN, and the chunk that
        // would cross it is never staged: a refusal must not need the disk
        // space it is refusing.
        if total > cap {
            return Err(TransferError::TooLarge { cap });
        }
        file.write_all(&buf[..n])
            .map_err(|e| phase(format!("write: {e}"), e))?;
    }
    file.flush().map_err(|e| phase(format!("flush: {e}"), e))?;
    // Close before the rename: a rename over a path this process still holds
    // open is a platform-dependent refusal, and one fewer open handle is one
    // fewer way to fail. (Same reason, and the same step, as `atomic::replace`.)
    drop(file);
    std::fs::rename(&part, path).map_err(|e| {
        phase(
            format!("rename {} -> {}: {e}", part.display(), path.display()),
            e,
        )
    })?;
    Ok(total)
}

/// Name a failure after the PHASE that produced it, keeping its kind.
///
/// The phases name themselves because a landing has six steps and a bare
/// "access is denied" cannot tell an operator WHICH one refused; the door that
/// renders this adds the destination it was asked for.
fn phase(what: String, e: std::io::Error) -> TransferError {
    TransferError::Io(std::io::Error::new(e.kind(), what))
}

/// The staging path a landing writes through: a SIBLING of `path`, named by
/// APPENDING ".part" to the target's file name (`fw.bin` -> `fw.bin.part`).
///
/// APPENDED, and never a hidden dotfile, for the reason `atomic::temp_path`
/// spells out for its own suffix: litter left by a transfer that died is
/// something an operator has to be able to SEE, and a filter that selects
/// `*.bin` must not walk past the corpse of a `.bin` that never landed.
/// `.part` rather than `.tmp` on purpose — a staging file whose payload has not
/// finished arriving and a whole-file replacement are two rules, and the name
/// says which one an operator is looking at.
fn part_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(".part");
    path.with_file_name(name)
}

/// HOW MUCH IS READ FROM THE SOURCE AT A TIME. A landing's contract is about
/// the BYTES it accepts, not about the window they arrive in — so this number
/// is free to change, and the cap is not.
const READ_WINDOW: usize = 64 * 1024;

#[cfg(test)]
mod tests {
    //! THE TRUTH TABLE for the one landing, in one place, for both doors:
    //! success lands the bytes and leaves no part; a source that dies mid-flight
    //! leaves NO destination and NO corpse; the cap refuses at exactly `cap + 1`
    //! and passes at exactly `cap`; parents are created; a failed RENAME — the
    //! phase hand-rolled copies got wrong — cleans up too.
    //!
    //! The buffered half is deliberately driven by plain `#[test]`s: the
    //! synchronous door must work outside a runtime (see `land_buffered`), and a
    //! dev-dependency on a reactor would hide the day that stops being true.
    use super::*;
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use tokio::io::ReadBuf;

    fn dir(tag: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("summrise-transfer-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("temp dir");
        d
    }

    /// The staging path the PRODUCTION rule produces — asked of the module
    /// rather than re-spelled here, so a "no corpse" assertion cannot end up
    /// looking at a path nothing writes. The literal name is pinned once, in
    /// `the_part_is_a_visible_sibling_named_by_appending_part`.
    fn part_of(dest: &Path) -> PathBuf {
        part_path(dest)
    }

    fn names_in(d: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(d)
            .expect("read_dir")
            .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// A source that hands over `head` and then FAILS — the shape of a
    /// connection that dies mid-body.
    struct DiesMidStream {
        head: Vec<u8>,
        sent: bool,
    }

    impl tokio::io::AsyncRead for DiesMidStream {
        fn poll_read(
            mut self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            if self.sent {
                return Poll::Ready(Err(std::io::Error::other("connection reset mid-stream")));
            }
            self.sent = true;
            let head = std::mem::take(&mut self.head);
            buf.put_slice(&head);
            Poll::Ready(Ok(()))
        }
    }

    /// A source that LOOKS at the destination directory on its first poll —
    /// the only moment the staging file is observable — then ends.
    struct Peek {
        dir: PathBuf,
        seen: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
        first: bool,
    }

    impl tokio::io::AsyncRead for Peek {
        fn poll_read(
            mut self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            if self.first {
                self.first = false;
                *self.seen.lock().expect("lock") = names_in(&self.dir);
                buf.put_slice(b"payload");
                return Poll::Ready(Ok(()));
            }
            Poll::Ready(Ok(()))
        }
    }

    #[tokio::test]
    async fn a_streamed_payload_lands_and_leaves_no_part() {
        let d = dir("stream-ok");
        let dest = d.join("fw.bin");
        let payload: Vec<u8> = (0u16..=255).map(|b| b as u8).cycle().take(70_000).collect();
        let mut source = payload.as_slice();
        let landed = land_streamed(&dest, MAX_TRANSFER_BYTES, &mut source)
            .await
            .expect("land");
        assert_eq!(landed, payload.len() as u64, "the count is what landed");
        assert_eq!(
            std::fs::read(&dest).expect("read"),
            payload,
            "byte for byte"
        );
        assert!(
            !part_of(&dest).exists(),
            "a completed landing leaves no .part"
        );
    }

    #[test]
    fn a_buffered_payload_lands_outside_any_runtime_and_leaves_no_part() {
        let d = dir("buffer-ok");
        let dest = d.join("fw.bin");
        let payload: Vec<u8> = (0u16..=255).map(|b| b as u8).cycle().take(70_000).collect();
        let landed = land_buffered(&dest, MAX_TRANSFER_BYTES, &payload).expect("land");
        assert_eq!(landed, payload.len() as u64);
        assert_eq!(std::fs::read(&dest).expect("read"), payload);
        assert!(!part_of(&dest).exists());
    }

    /// The naming rule, measured WHILE the payload is still arriving: the part
    /// is `<name>.part` (appended, visible, never the extension replaced), and
    /// the destination the caller named does not exist yet.
    #[tokio::test]
    async fn the_part_is_a_visible_sibling_named_by_appending_part() {
        let d = dir("name");
        let dest = d.join("fw.bin");
        let mut source = Peek {
            dir: d.clone(),
            seen: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            first: true,
        };
        land_streamed(&dest, MAX_TRANSFER_BYTES, &mut source)
            .await
            .expect("land");
        let seen = source.seen.lock().expect("lock").clone();
        assert_eq!(
            seen,
            vec!["fw.bin.part".to_string()],
            "the part is the file name with .part appended — visible, and the \
             destination is absent while the bytes arrive"
        );
        assert_eq!(names_in(&d), vec!["fw.bin".to_string()]);
    }

    #[tokio::test]
    async fn a_failure_mid_stream_leaves_no_file_and_no_corpse() {
        let d = dir("dies");
        let dest = d.join("fw.bin");
        let mut source = DiesMidStream {
            head: vec![7u8; 4096],
            sent: false,
        };
        let err = land_streamed(&dest, MAX_TRANSFER_BYTES, &mut source)
            .await
            .expect_err("a source that dies must fail the landing");
        match &err {
            TransferError::Io(e) => assert!(
                e.to_string().starts_with("read chunk: "),
                "the phase is named: {e}"
            ),
            other => panic!("a dead source is an I/O failure, not {other:?}"),
        }
        assert!(
            !dest.exists(),
            "the caller's path must never hold a partial file"
        );
        assert!(
            names_in(&d).is_empty(),
            "and the staged part is removed: {:?}",
            names_in(&d)
        );
    }

    #[tokio::test]
    async fn an_over_cap_stream_is_refused_and_leaves_no_corpse() {
        let d = dir("stream-cap");
        let dest = d.join("fw.bin");
        let payload = vec![1u8; 200];
        let mut source = payload.as_slice();
        let err = land_streamed(&dest, 64, &mut source)
            .await
            .expect_err("over the cap must be refused");
        assert!(
            matches!(err, TransferError::TooLarge { cap: 64 }),
            "the refusal names the cap it applied: {err:?}"
        );
        assert!(!dest.exists());
        assert!(
            names_in(&d).is_empty(),
            "the staged part is removed even though the landing refused: {:?}",
            names_in(&d)
        );
    }

    #[test]
    fn an_over_cap_buffer_is_refused_before_anything_is_staged() {
        let d = dir("buffer-cap");
        let dest = d.join("fw.bin");
        let err = land_buffered(&dest, 64, &[0u8; 65]).expect_err("over the cap must be refused");
        assert!(matches!(err, TransferError::TooLarge { cap: 64 }));
        assert!(
            names_in(&d).is_empty(),
            "not even a .part: the refusal precedes the first write: {:?}",
            names_in(&d)
        );
    }

    #[tokio::test]
    async fn the_cap_boundary_is_exact() {
        let d = dir("boundary");
        let exact = vec![0u8; 64];
        let mut source = exact.as_slice();
        assert_eq!(
            land_streamed(&d.join("at.bin"), 64, &mut source)
                .await
                .expect("exactly the cap must land"),
            64
        );
        let over = d.join("over.bin");
        let over_buf = vec![0u8; 65];
        let mut source = over_buf.as_slice();
        assert!(matches!(
            land_streamed(&over, 64, &mut source).await,
            Err(TransferError::TooLarge { cap: 64 })
        ));
        assert!(!over.exists(), "cap + 1 lands nothing");
        assert!(!part_of(&over).exists(), "…and leaves no corpse");
        // The buffered door draws the line in the same place — one rule, not
        // two boundaries that happen to agree today.
        assert_eq!(
            land_buffered(&d.join("buffered-at.bin"), 64, &[0u8; 64])
                .expect("exactly the cap lands"),
            64
        );
        assert!(matches!(
            land_buffered(&d.join("buffered-over.bin"), 64, &[0u8; 65]),
            Err(TransferError::TooLarge { cap: 64 })
        ));
        assert!(!d.join("buffered-over.bin").exists());
    }

    #[tokio::test]
    async fn parents_are_created_on_the_way_to_the_destination() {
        let d = dir("parents");
        let deep = d.join("nested").join("deeper").join("fw.bin");
        let buf = b"streamed".to_vec();
        let mut source = buf.as_slice();
        land_streamed(&deep, MAX_TRANSFER_BYTES, &mut source)
            .await
            .expect("land");
        assert_eq!(std::fs::read(&deep).expect("read"), b"streamed");
        let deep_buffered = d.join("other").join("sub").join("b.bin");
        land_buffered(&deep_buffered, MAX_TRANSFER_BYTES, b"buffered").expect("land");
        assert_eq!(std::fs::read(&deep_buffered).expect("read"), b"buffered");
    }

    /// THE PHASE THE HAND-ROLLED COPIES GOT WRONG: the bytes write fine, so
    /// nothing in the write path reports a problem, and the RENAME is what
    /// fails. They left `<name>.part` beside the destination.
    #[tokio::test]
    async fn a_failed_rename_removes_the_part() {
        let d = dir("rename-fail");
        let dest = d.join("fw.bin");
        // A DIRECTORY at the destination: the rename cannot land on it.
        std::fs::create_dir_all(&dest).expect("blocker dir");
        let buf = b"new".to_vec();
        let mut source = buf.as_slice();
        let err = land_streamed(&dest, MAX_TRANSFER_BYTES, &mut source)
            .await
            .expect_err("a rename that cannot land must fail the landing");
        match &err {
            TransferError::Io(e) => assert!(
                e.to_string().starts_with("rename "),
                "the phase is named: {e}"
            ),
            other => panic!("a failed rename is an I/O failure, not {other:?}"),
        }
        assert!(
            !part_of(&dest).exists(),
            "a failed RENAME must not leave the part behind — the litter an \
             operator cannot tell from a transfer in progress"
        );
        assert!(dest.is_dir(), "and the blocker is untouched");
    }

    /// ONE DECLARATION, and this is the number the tool prose and the relay's
    /// own ceiling were written against.
    #[test]
    fn the_transfer_cap_is_the_100_mb_the_prose_promises() {
        assert_eq!(MAX_TRANSFER_BYTES, 100 * 1024 * 1024);
    }
}
