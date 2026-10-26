//! The unauthenticated-refusal log at `<data>/denials.jsonl`.
//!
//! A request that fails authentication never reaches an action: a bad operator token, a missing or
//! expired session, a bad device token, or a registration proof that does not verify. Those
//! attempts are worth recording — they are how an operator sees someone probing the console — but
//! they are the one kind of traffic an unauthenticated caller can generate at will, so they must
//! not be able to push the authenticated audit out of its file.
//!
//! Two coarse defences:
//!
//! * a separate file, `denials.jsonl`, with its own 2 MiB cap and one rotation, so `audit.jsonl`
//!   is untouchable by unauthenticated traffic;
//! * at most [`DENIALS_PER_WINDOW`] lines in any [`DENIAL_WINDOW_MS`] window. The first denial after
//!   a window ends writes one `denied` line with status 429 and `detail: "suppressed N"` for the
//!   N refusals the previous window suppressed, then starts the next window. The summary is
//!   written lazily, with the next denial: this log has no clock of its own, and the console's
//!   clock is what every line records.
//!
//! The line shape is the audit [`Entry`]: the same facts, a different audience.

use std::path::Path;

use serde_json::Value;

use crate::audit::Entry;
use crate::jsonl::JsonLines;

/// The largest the denial file may become before it is rotated to `denials.1.jsonl`.
pub(crate) const DENIALS_MAX_BYTES: u64 = 2 * 1024 * 1024;

/// How many denial lines one window may hold before the rest are suppressed.
pub(crate) const DENIALS_PER_WINDOW: u32 = 100;

/// The length of the rate-limit window.
pub(crate) const DENIAL_WINDOW_MS: u64 = 60_000;

/// The current file's name under the data directory.
const DENIALS_FILE: &str = "denials.jsonl";

/// The denial log and the window its writer is counting in.
pub(crate) struct Denials {
    file: JsonLines,
    window: tokio::sync::Mutex<Window>,
}

/// The current minute's count. `start_ms` is the window's first denial; the window rolls forward
/// to the time of the first denial after it ends.
struct Window {
    start_ms: u64,
    written: u32,
    suppressed: u64,
}

impl Denials {
    /// The denial file under `data`.
    pub(crate) fn new(data: &Path) -> Denials {
        Denials {
            file: JsonLines::new(data.join(DENIALS_FILE), DENIALS_MAX_BYTES),
            window: tokio::sync::Mutex::new(Window {
                start_ms: 0,
                written: 0,
                suppressed: 0,
            }),
        }
    }

    /// Record one refusal, or count it as suppressed when the current window is full.
    ///
    /// A refusal that arrives after the window ends first writes the previous window's summary, so
    /// the suppressed count is never silently lost while the console keeps receiving traffic.
    pub(crate) async fn append(&self, entry: &Entry<'_>) {
        let mut window = self.window.lock().await;

        if entry.ts_ms.saturating_sub(window.start_ms) >= DENIAL_WINDOW_MS {
            if window.suppressed > 0 {
                let detail = format!("suppressed {}", window.suppressed);
                self.file
                    .append(&Entry {
                        ts_ms: entry.ts_ms,
                        kind: "denied",
                        device_id: None,
                        status: 429,
                        detail: Some(detail.as_str()),
                        tool: None,
                        token_id: None,
                    })
                    .await;
            }
            *window = Window {
                start_ms: entry.ts_ms,
                written: 0,
                suppressed: 0,
            };
        }

        if window.written < DENIALS_PER_WINDOW {
            window.written += 1;
            self.file.append(entry).await;
        } else {
            window.suppressed += 1;
        }
    }

    /// The newest `limit` denial lines, newest first.
    pub(crate) async fn newest(&self, limit: usize) -> Vec<Value> {
        self.file.newest(limit).await
    }
}
