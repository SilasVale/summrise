//! The page role: the built page, its assets, and the live stream.
//!
//! The console may host both roles in one process, but the split is a code boundary first
//! (architecture of record, §0.4 and §4.2): this module serves the static page, the icon and the
//! operator's event stream, and it is handed no secret to hold. Enrollment, the device registry,
//! grant signing and permanent-token management stay in `http.rs`; that control role builds a
//! [`Page`] and mounts [`router`].
//!
//! `/events` is the one route here that is not public. This module cannot ask who the caller is and
//! deliberately does not learn: the control router wraps this router with its operator gate
//! (`http::page_operator_gate`) before serving it. The page still never holds a way to mint or check
//! a credential, which is the invariant [`tests::the_page_holds_nothing_secret`] makes executable.

use std::convert::Infallible;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use axum::extract::State;
use axum::http::{header, HeaderMap, HeaderValue, Uri};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::get;
use axum::Router;
use serde_json::{json, Value};

use crate::audit::Audit;
use crate::running::Running;

/// The React console page, embedded as one self-contained file.
///
/// `/` and `/app` both serve it. The build is committed and gated, so a stale page cannot compile
/// and ship.
const REACT_PAGE: &str = include_str!("../web/dist/index.html");

/// **THE PAGE KNOWS WHICH BUILD IT IS (the hundred-and-ninety-ninth list).** A tab left open across a deploy kept calling the routes of
/// an older page — measured: `monitor.add`, `memory.list` and `monitor.status` refusals in the audit, all `caller: console`, all from a
/// bundle that had already been fixed. The page is stamped at start with this build's version, so it can compare itself with `/health`
/// and offer a Reload rather than keep asking in an older language.
fn stamped_page() -> &'static str {
    static STAMPED: OnceLock<String> = OnceLock::new();
    STAMPED.get_or_init(|| {
        let version = PAGE_VERSION.get().map(String::as_str).unwrap_or("");
        if version.is_empty() {
            // A console built without a version (a test binary) serves the page as it was: a placeholder in a `<meta>` is not a claim.
            return REACT_PAGE.replace("__CONSOLE_VERSION__", "");
        }
        REACT_PAGE.replace("__CONSOLE_VERSION__", version)
    })
}

/// The build this process serves; set once, when the console is constructed.
static PAGE_VERSION: OnceLock<String> = OnceLock::new();

/// Stamp the page with the build the console is serving.
///
/// Called by the control role at load: the page's copy of the version is fixed before the first
/// request, and a later call is a no-op rather than a second claim.
pub(crate) fn stamp_version(version: &str) {
    let _ = PAGE_VERSION.set(version.to_owned());
}

/// The static assets behind the page's own paths.
mod assets {
    /// The brand mark at 128 px: the browser tab's icon and the console's face in a task switcher.
    /// The React build already points at `/assets/icon.png`, so this route is the page's, not the
    /// deleted surface's.
    pub(super) const ICON: &[u8] = include_bytes!("../assets/icon.png");
}

/// The page's clock, in Unix milliseconds: the console's injected clock, never a second wall clock.
pub(crate) type Clock = Arc<dyn Fn() -> u64 + Send + Sync>;

/// The page role's state.
///
/// It holds only what the page's routes read: the built page and its icon, the live index it
/// follows, the calls still in flight, and the clock. There is deliberately no signing key, no
/// operator credential, no device registry and no channel here; the executable form of that rule is
/// [`tests::the_page_holds_nothing_secret`].
pub(crate) struct Page {
    /// The built page, stamped with this build's version before the first request.
    page: &'static str,
    /// The brand mark, served at `/favicon.ico` and `/assets/icon.png`.
    icon: &'static [u8],
    /// The live index `/events` follows and reads its snapshot from.
    audit: Audit,
    /// The calls the proxy is still waiting on, for the snapshot's `running` line.
    running: Arc<Running>,
    /// The console's clock.
    now: Clock,
}

impl Page {
    /// Build the page role from the control role's shared handles.
    pub(crate) fn new(audit: Audit, running: Arc<Running>, now: Clock) -> Page {
        Page {
            page: stamped_page(),
            icon: assets::ICON,
            audit,
            running,
            now,
        }
    }
}

/// The page role's routes.
///
/// The caller must mount this router behind the control role's operator gate for `/events`. The page
/// cannot authenticate a caller and must not be given the means to: the gate lives in `http.rs`,
/// with the keys.
pub(crate) fn router(page: Arc<Page>) -> Router {
    Router::new()
        .route("/", get(root))
        // The conventional favicon path: a browser asks for it on every page, and a 404 there is a
        // console error in every page that does not declare an icon. It is the same PNG.
        .route("/favicon.ico", get(favicon))
        .route("/assets/icon.png", get(icon))
        .route("/app", get(react_page))
        .route("/events", get(event_stream))
        .with_state(page)
}

/// `GET /`: the React workspace. A `code` query still goes to `/next`, because `--login-link`
/// puts a one-time code there and the React page does not redeem it; dropping the query would
/// turn a working link into a page with no explanation. Every other query stays on the React
/// page: a cache-bust or a shared link must not become a redirect.
async fn root(State(page): State<Arc<Page>>, uri: Uri) -> Response {
    let login_code = uri.query().is_some_and(|query| {
        query
            .split('&')
            .any(|part| part == "code" || part.starts_with("code="))
    });
    if login_code {
        Redirect::to(&format!("/next?{}", uri.query().unwrap_or_default())).into_response()
    } else {
        Html(page.page).into_response()
    }
}

/// `GET /app`: the React console page, one embedded file. It carries no data of its own; the page
/// authenticates with the console's own session and calls the JSON routes that already exist.
async fn react_page(State(page): State<Arc<Page>>) -> Html<&'static str> {
    Html(page.page)
}

/// `GET /favicon.ico`: the brand mark, as PNG, at the path a browser asks for on its own.
async fn favicon(State(page): State<Arc<Page>>) -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "image/png")],
        axum::body::Bytes::from_static(page.icon),
    )
}

/// `GET /assets/icon.png`: the brand mark the built page points at.
async fn icon(State(page): State<Arc<Page>>) -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "image/png"),
            (header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        axum::body::Bytes::from_static(page.icon),
    )
}

/// `GET /events`: the audit as a stream of server-sent events.
///
/// A `snapshot` carries the newest lines so the page opens with context, then each appended line is
/// an `activity` event. A slow client is told it `lagged` rather than given a growing backlog: it
/// refetches `/audit`, which is the same index from the same file.
async fn event_stream(State(page): State<Arc<Page>>, headers: HeaderMap) -> Response {
    // Subscribe BEFORE reading the newest lines: a line written between the two would otherwise be in
    // neither the snapshot nor the stream.
    let live = page.audit.follow();
    // The calls in flight are not in the file, so a page that reconnects reads them here. The same
    // cut the page applies is applied by the registry: an entry whose clear was dropped expires.
    let running = page.running.current((page.now)());
    let recent: Vec<Value> = page
        .audit
        .newest(40)
        .await
        .into_iter()
        .map(|mut line| {
            let sentence = crate::activity::line_sentence(&line);
            line["sentence"] = json!(sentence);
            line
        })
        .collect();
    // Oldest first, like the file: a page prepends each live event and gets a newest-first list.
    let recent: Vec<Value> = recent.into_iter().rev().collect();
    let snapshot = futures_util::stream::once(async move {
        let body = json!({ "lines": recent, "running": running });
        Ok::<Event, Infallible>(
            Event::default()
                .event("snapshot")
                .id(format!("{}:0", boot_epoch()))
                .data(body.to_string()),
        )
    });
    // A page that reconnects with a device cursor gets the buffered rows it missed before the live
    // stream. The audit side needs no replay: the snapshot re-describes its history.
    let replayed = match headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .and_then(parse_device_cursor)
    {
        Some((device, seq)) => futures_util::stream::iter(
            page.audit
                .events_after(device, seq)
                .into_iter()
                .map(|value| {
                    let device = value["device_event"]["device"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned();
                    let seq = value["device_event"]["event"]["seq"].as_u64().unwrap_or(0);
                    Ok::<Event, Infallible>(
                        Event::default()
                            .event("device")
                            .id(format!("device:{device}:{seq}"))
                            .data(value.to_string()),
                    )
                })
                .collect::<Vec<_>>(),
        ),
        None => futures_util::stream::iter(Vec::new()),
    };
    let live_events = futures_util::stream::unfold(live, |mut live| async move {
        match live.recv().await {
            Ok(value) => {
                // A call's announcement and clear share the audit's broadcast, but they are not
                // activity lines: the page listens for their own names and keeps them out of the
                // record. A retention reset is likewise its own event, not a device row. Everything
                // else is an activity line.
                let (name, id, data) = if let Some(reset) = value.get("device_reset") {
                    let device = reset
                        .get("device")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let snapshot_seq = reset
                        .pointer("/reset/snapshot_seq")
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
                    let body = json!({
                        "device": device,
                        "snapshot_seq": snapshot_seq,
                        "lost": reset.pointer("/reset/lost").cloned().unwrap_or(Value::Null),
                    });
                    // The id names the reset itself: the same reset replayed to a reconnecting
                    // page is not a new row, and anything else is an ordinary stream id.
                    (
                        "reset",
                        format!("reset:{device}:{snapshot_seq}"),
                        body.to_string(),
                    )
                } else if let Some(device_event) = value.get("device_event") {
                    let device = device_event
                        .get("device")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let seq = device_event
                        .get("event")
                        .and_then(|event| event.get("seq"))
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
                    (
                        "device",
                        format!("device:{device}:{seq}"),
                        value.to_string(),
                    )
                } else {
                    let name = if value.get("started").is_some() {
                        "started"
                    } else if value.get("finished").is_some() {
                        "finished"
                    } else {
                        "activity"
                    };
                    (
                        name,
                        format!("{}:{}", boot_epoch(), next_event_id()),
                        value.to_string(),
                    )
                };
                Some((
                    Ok::<Event, Infallible>(Event::default().event(name).id(id).data(data)),
                    live,
                ))
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(dropped)) => {
                let body = json!({ "dropped": dropped });
                Some((
                    Ok::<Event, Infallible>(
                        Event::default().event("lagged").data(body.to_string()),
                    ),
                    live,
                ))
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => None,
        }
    });
    let tail = futures_util::StreamExt::chain(replayed, live_events);
    let mut response = Sse::new(futures_util::StreamExt::chain(snapshot, tail))
        .keep_alive(KeepAlive::default().interval(Duration::from_secs(15)))
        .into_response();
    response
        .headers_mut()
        .insert("x-accel-buffering", HeaderValue::from_static("no"));
    response
}

/// Parse `device:<device>:<seq>` from a `Last-Event-ID`; anything else is not a device cursor.
fn parse_device_cursor(id: &str) -> Option<(&str, u64)> {
    let rest = id.strip_prefix("device:")?;
    let (device, seq) = rest.rsplit_once(':')?;
    Some((device, seq.parse().ok()?))
}

/// This boot's prefix for SSE ids.
///
/// A page that reconnects with a `Last-Event-ID` from another boot is told to start over by the
/// epoch mismatch instead of being handed the wrong range.
fn boot_epoch() -> &'static str {
    static EPOCH: OnceLock<String> = OnceLock::new();
    EPOCH.get_or_init(|| {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_millis() as u64)
            .unwrap_or(0);
        format!("{:x}{:x}", std::process::id(), now_ms)
    })
}

/// A console-wide monotonic id for events that are not a device's own.
fn next_event_id() -> usize {
    static NEXT: AtomicUsize = AtomicUsize::new(1);
    NEXT.fetch_add(1, Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{StatusCode, Uri};

    /// A directory this test owns, removed with it.
    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir()
                .join(format!("summrise-page-unit-{}-{label}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("the test directory is creatable");
            TempDir(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// The page role with a throwaway index and a fixed clock. The routes under test read only the
    /// static assets and the clock's values; nothing here writes a line.
    fn test_page(label: &str) -> (TempDir, Arc<Page>) {
        let dir = TempDir::new(label);
        let page = Arc::new(Page::new(
            Audit::new(&dir.0, 64 * 1024),
            Arc::new(Running::new(Duration::from_secs(30))),
            Arc::new(|| 0u64),
        ));
        (dir, page)
    }

    #[tokio::test]
    async fn root_sends_only_login_codes_to_the_login_link() {
        let (_dir, page) = test_page("root");

        let answer = root(State(page.clone()), Uri::from_static("/")).await;
        assert_eq!(answer.status(), StatusCode::OK);

        // A cache-bust or any other query is still the React page.
        let cache_bust = root(State(page.clone()), Uri::from_static("/?v=236")).await;
        assert_eq!(cache_bust.status(), StatusCode::OK);

        let code = root(State(page), Uri::from_static("/?code=abc&next=%2F")).await;
        assert_eq!(code.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            code.headers().get("location").unwrap(),
            "/next?code=abc&next=%2F"
        );
    }

    #[test]
    fn a_device_cursor_parses_and_anything_else_does_not() {
        assert_eq!(parse_device_cursor("device:9vh:42"), Some(("9vh", 42)));
        assert_eq!(parse_device_cursor("device:9vh"), None);
        assert_eq!(parse_device_cursor("9vh:42"), None);
        assert_eq!(parse_device_cursor("device:9vh:not-a-number"), None);
    }

    /// **THE PAGE HOLDS NONE.** The page role is a code boundary, so this reads its own source and
    /// proves the names of the control role's state never enter it: no signing key, no operator
    /// credential, no session list, no permanent-token store, no device channel and no proxy trust.
    /// The needles are assembled from fragments so this test does not itself mention them.
    #[test]
    fn the_page_holds_nothing_secret() {
        let source = include_str!("page.rs");
        let forbidden = [
            ["grant", "_key"].concat(),
            ["operator", "_", "token"].concat(),
            ["ses", "sions"].concat(),
            ["to", "kens"].concat(),
            ["chan", "nels"].concat(),
            ["trusted", "_proxy"].concat(),
        ];
        for needle in forbidden {
            assert!(
                !source.contains(&needle),
                "page.rs names `{needle}`: the page role must hold none of the control role's state"
            );
        }
    }
}
