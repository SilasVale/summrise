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
use std::sync::{Arc, Mutex};
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

/* ─────────────────────────── the KEYED table ─────────────────────────── */

/// **`withKeyLock(key, fn)`'s `__locks` MAP, WHICH IS WHAT THE USER STORE NEEDS AND THE DEVICE STORE DOES NOT.**
///
/// The two device blobs are known at compile time, so one `KeyLock` static each is the whole mechanism. The user
/// store's keys are not: `createUser` serializes on `user:<name>` and then, INSIDE that section, on
/// `invclaim:<code>` — two different keys held at once, so a single lock would DEADLOCK and no per-key static can
/// be written for a name that arrives in a request. This is the keyed form, and it is the form the source has.
///
/// **THE 512-KEY BOUND IS THE SOURCE'S, AND IT EVICTS THE OLDEST** — with the cost its round-124 comment already
/// records: *"a pending chain there loses its queue (its own RMW still completes; only a NEW caller for that key
/// can now race it)"*. A `KeyLock` evicted from this table is still the lock its holder is using, so the section
/// in flight is serialized exactly as before; what is lost is a queue, not a section.
pub struct KeyedLocks {
    locks: Mutex<Vec<(String, Arc<KeyLock>)>>,
}

impl Default for KeyedLocks {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyedLocks {
    pub const fn new() -> Self {
        Self {
            locks: Mutex::new(Vec::new()),
        }
    }

    /// The lock for `key`, created on first use. It answers an `Arc` so the caller holds the lock itself across
    /// its awaits rather than borrowing this table — borrowing it would hold THIS mutex for the whole section.
    pub fn lock_for(&self, key: &str) -> Arc<KeyLock> {
        let Ok(mut locks) = self.locks.lock() else {
            // A poisoned table cannot happen on this single-threaded runtime (nothing panics while holding it),
            // and refusing to serialize would be worse than running the section unserialized — the same arm
            // `Acquire::poll` takes.
            return Arc::new(KeyLock::new());
        };
        if let Some((_, lock)) = locks.iter().find(|(k, _)| k == key) {
            return lock.clone();
        }
        let lock = Arc::new(KeyLock::new());
        locks.push((key.to_string(), lock.clone()));
        if locks.len() > 512 {
            locks.remove(0);
        }
        lock
    }
}

/// `withKeyLock(key, fn)` — the source's call shape, as one awaitable section: the guard is held for the whole
/// of `section`. (It takes the future rather than a closure so a caller can write
/// `with_key_lock(&LOCKS, &key, async { … }).await` around its own borrows.)
pub async fn with_key_lock<T, F>(locks: &KeyedLocks, key: &str, section: F) -> T
where
    F: Future<Output = T>,
{
    let lock = locks.lock_for(key);
    let _guard = lock.lock().await;
    section.await
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

    /// **TWO DIFFERENT KEYS MUST NOT BLOCK EACH OTHER, OR `createUser` DEADLOCKS.** It holds `user:<name>` and
    /// takes `invclaim:<code>` INSIDE that section; a keyed table that answered one lock for every key would
    /// hang every registration on the second acquisition.
    ///
    /// MUTATION: `KeyedLocks::lock_for` returns the same lock for every key (a single `Arc` field).
    /// RESULT:   `different_keys_are_different_locks` fails — `invclaim:a` is pending while `user:x` is held.
    #[test]
    fn different_keys_are_different_locks() {
        let locks = KeyedLocks::new();
        let waker = noop_waker();
        let mut cx = Context::from_waker(&waker);
        let user = locks.lock_for("user:x");
        let mut held = Box::pin(user.lock());
        // **THE GUARD MUST BE HELD, NOT MERELY OBSERVED** — the same trap the reuse test above records:
        // `assert!(…is_ready())` DROPS the poll's guard, which releases the lock and makes the assertions below
        // measure nothing.
        let guard = match held.as_mut().poll(&mut cx) {
            Poll::Ready(guard) => guard,
            Poll::Pending => panic!("the first acquires"),
        };
        // A different key acquires immediately...
        let claim = locks.lock_for("invclaim:c");
        let mut other = Box::pin(claim.lock());
        let other_guard = match other.as_mut().poll(&mut cx) {
            Poll::Ready(guard) => guard,
            Poll::Pending => panic!("a different key must not wait"),
        };
        // ...and the SAME key waits, which is the property the section needs.
        let again = locks.lock_for("user:x");
        let mut same = Box::pin(again.lock());
        assert!(same.as_mut().poll(&mut cx).is_pending(), "the same key waits");
        drop(guard);
        assert!(same.as_mut().poll(&mut cx).is_ready());
        drop(other_guard);
    }

    /// The 512-key bound evicts the OLDEST key. A test that only counted would pass on a table that evicted the
    /// NEWEST, which is the one entry a burst is about to use again.
    #[test]
    fn the_keyed_table_evicts_the_oldest() {
        let locks = KeyedLocks::new();
        for i in 0..512 {
            let _ = locks.lock_for(&format!("k{i}"));
        }
        assert_eq!(locks.locks.lock().unwrap().len(), 512);
        let _ = locks.lock_for("fresh");
        let keys: Vec<String> = locks
            .locks
            .lock()
            .unwrap()
            .iter()
            .map(|(k, _)| k.clone())
            .collect();
        assert_eq!(keys.len(), 512);
        assert_eq!(keys[0], "k1", "the oldest went, not the newest");
        assert_eq!(keys[511], "fresh");
    }
}
