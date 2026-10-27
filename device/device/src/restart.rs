//! A supervised app's restart: the one hook the panel's enrollment and `host.restart` ask for.
//!
//! The launcher sustains this process: the app exits, and the launcher starts it again with the
//! release facts and the console link it reads at boot. A standalone process has no such parent, so
//! no handler calls this hook for it — the handlers read `Config.supervised` and refuse with a
//! sentence instead, because an exit would not bring the app back.
//!
//! The hook returns immediately. The exit happens on a thread after two seconds so the answer that
//! asked for the restart can reach the page or the MCP client before the socket disappears; the
//! delay is the stated mechanism, not a guarantee that every client has read the answer.

use std::sync::Arc;
use std::time::Duration;

/// What a handler calls after a supervised enrollment or a successful `host.restart`.
pub(crate) type Hook = Arc<dyn Fn() + Send + Sync>;

/// The hook a test, or a directly built server, uses: a restart is never requested.
pub(crate) fn none() -> Hook {
    Arc::new(|| {})
}

/// The production hook: leave after two seconds, so the answer is written first.
pub(crate) fn exit_soon() -> Hook {
    Arc::new(|| {
        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_secs(2));
            std::process::exit(0);
        });
    })
}
