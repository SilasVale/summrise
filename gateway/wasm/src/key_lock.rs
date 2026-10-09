//! A PER-KEY ASYNC LOCK — `store/cache.ts`'s `withKeyLock`, AS THE PRIMITIVE THE DEVICE STORE NEEDS.
//!
//! WHY IT EXISTS. Three KV records in this worker are read-modify-write blobs (`devices:v1`, `plugins:v1`), and
//! the source's own comments record what happened without serialization: *"two concurrent writes used to both
//! read [] and the second save clobbered the first's device (registry loss → proxy 404)"*, and *"a stale cached
//! blob rewritten inside the lock resurrects links another isolate revoked"*. A Workers isolate is
//! single-threaded but its requests INTERLEAVE AT EVERY `await`, so two handlers can both read the pre-write
//! value and the second put then loses the first's record. The lock is what bridges the awaits.
//!
//! WHY IT IS HAND-ROLLED RATHER THAN `futures_util::lock::Mutex`. That type is behind `futures-util`'s `std`
//! feature, and this crate takes the dependency with `default-features = false` — so using it would mean
//! enabling a feature for the whole wasm build to get one primitive. The lock below is thirty lines, has no
//! dependency, and is testable on the host.
//!
//! **IT IS A SINGLE-WAITER-GROUP LOCK, NOT A FAIR QUEUE, AND THAT IS ENOUGH HERE.** On release every waiter is
//! woken and exactly one wins the next acquisition — the source's queue is FIFO and this is not, so a handler
//! can in principle overtake another. What the source needs is that the read-modify-write sections do not
//! INTERLEAVE, not that they run in arrival order, and that property holds: a waiter that is woken without
//! winning re-registers. (The source's own `withKeyLock` is not fair either in the case its round-124 comment
//! describes — the 512-key bound evicts a pending chain and a new caller then runs concurrently with it.)
//!
//! THE WAKE IS THE PART THAT IS EASY TO GET WRONG: registration happens under the SAME mutex as the
//! held-check, so there is no window in which a release can miss a waiter that has decided to sleep.

use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;
use std::task::{Context, Poll, Waker};

#[derive(Default)]
struct LockState {
    held: bool,
    waiters: Vec<Waker>,
}

/// One lock. The crate declares one per KV blob (`DEVICES_LOCK`, `PLUGIN_LOCK`) rather than keying a map,
/// because the source has exactly two keys that are read-modify-written and a map would add its own state to
/// get wrong.
pub struct KeyLock {
    state: Mutex<LockState>,
}

impl Default for KeyLock {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyLock {
    pub const fn new() -> Self {
        Self {
            state: Mutex::new(LockState {
                held: false,
                waiters: Vec::new(),
            }),
        }
    }

    /// `withKeyLock(key, fn)` — holding the guard across the caller's awaits is what serializes the section.
    pub async fn lock(&self) -> KeyGuard<'_> {
        Acquire { lock: self }.await;
        KeyGuard { lock: self }
    }

    fn release(&self) {
        let waiters = {
            let Ok(mut state) = self.state.lock() else {
                return;
            };
            state.held = false;
            std::mem::take(&mut state.waiters)
        };
        for waker in waiters {
            waker.wake();
        }
    }
}

/// The guard. Dropping it is the release, exactly as leaving the source's `withKeyLock` callback is.
pub struct KeyGuard<'a> {
    lock: &'a KeyLock,
}

impl Drop for KeyGuard<'_> {
    fn drop(&mut self) {
        self.lock.release();
    }
}

struct Acquire<'a> {
    lock: &'a KeyLock,
}

impl Future for Acquire<'_> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let Ok(mut state) = self.lock.state.lock() else {
            // A poisoned lock cannot happen on this single-threaded runtime (nothing panics while holding it),
            // and refusing to run the section would be worse than running it unserialized.
            return Poll::Ready(());
        };
        if !state.held {
            state.held = true;
            return Poll::Ready(());
        }
        state.waiters.push(cx.waker().clone());
        Poll::Pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noop_waker() -> Waker {
        std::task::Waker::noop().clone()
    }

    /// **THE PROPERTY THE SOURCE NEEDS: A SECOND ACQUISITION CANNOT COMPLETE WHILE THE FIRST IS HELD.**
    ///
    /// MUTATION: make `Acquire::poll` answer `Poll::Ready` without checking `state.held`.
    /// RESULT:   `a_second_lock_waits_for_the_first` fails on its first assertion — both futures complete and
    ///           the read-modify-write they guard interleaves, which is the registry loss the source records.
    #[test]
    fn a_second_lock_waits_for_the_first() {
        let lock = KeyLock::new();
        let waker = noop_waker();
        let mut cx = Context::from_waker(&waker);

        let mut first = Box::pin(lock.lock());
        let mut second = Box::pin(lock.lock());

        // **THE GUARD MUST BE HELD, NOT MERELY OBSERVED.** `assert!(…is_ready())` DROPS the guard the poll
        // returned, which releases the lock — the first version of this test did exactly that and reported the
        // second acquisition as succeeding while the first was "held".
        let guard = match first.as_mut().poll(&mut cx) {
            Poll::Ready(guard) => guard,
            Poll::Pending => panic!("the first acquires"),
        };
        assert!(
            second.as_mut().poll(&mut cx).is_pending(),
            "the second waits"
        );

        // Dropping the guard releases, and the waiter that registered completes on its next poll.
        drop(guard);
        let second_guard = match second.as_mut().poll(&mut cx) {
            Poll::Ready(guard) => guard,
            Poll::Pending => panic!("the second acquires"),
        };
        // ...and a THIRD has to wait for the second.
        let mut third = Box::pin(lock.lock());
        assert!(third.as_mut().poll(&mut cx).is_pending());
        drop(second_guard);
        assert!(third.as_mut().poll(&mut cx).is_ready());
    }

    /// The lock is re-usable: a guard that is dropped re-arms it (a one-shot lock would deadlock the second
    /// request through a warm isolate).
    #[test]
    fn the_lock_is_reusable() {
        let lock = KeyLock::new();
        let waker = noop_waker();
        let mut cx = Context::from_waker(&waker);
        for _ in 0..3 {
            let mut guard = Box::pin(lock.lock());
            assert!(guard.as_mut().poll(&mut cx).is_ready());
            drop(guard);
        }
    }
}
