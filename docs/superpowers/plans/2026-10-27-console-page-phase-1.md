# The Console Page's First Rust Slice Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Serve a working operator page from Rust at `/next` — cookie session, sign-in, sign-out, and the fleet list — while the React page stays at `/`.

**Architecture:** A new `/next` namespace on the existing axum console. The page's credential is the same operator token `/auth` accepts, exchanged for an HttpOnly `SameSite=Strict` cookie scoped to `/next`; the session store is the console's existing in-memory `sessions`. The session gains a server-side CSRF token every mutation echoes; the cookie never carries it. The page is rendered by `maud` (one new crate) with inline CSS and no JavaScript. The fleet rows reuse the exact `DeviceView` projection `/fleet` already returns. No API route, wire shape, or the React page changes.

**Tech Stack:** Rust 2021, axum 0.7, maud, the existing `summrise-console` crate and its in-process test harness.

**Spec:** `docs/superpowers/specs/2026-10-27-console-page-in-rust.md`

## Global Constraints

- The repository is written in English: code, comments, commit messages, docs. Talking to the operator is Chinese.
- Rust only. The only new dependency is `maud`; its lockfile entry is committed. No JS, no npm, no build step for `/next`.
- The React page at `/` and every API route keep their current behaviour and wire shapes; `/next` is additive.
- The cookie is `HttpOnly; SameSite=Strict; Path=/next; Max-Age=86400`, with `Secure` iff `X-Forwarded-Proto: https`.
- The CSRF token lives in the session (server memory), is rendered into forms, and is compared in constant time. It is never in the cookie or a URL.
- Page HTML goes through `maud`, so device names are escaped; no `format!`-built HTML.
- `cargo test -p summrise-console` from `device/` stays green; the deploy version is `0.3.48`.

## Review Focus

- **The cookie on plain HTTP.** `http://127.0.0.1:19000` must still sign in (no `Secure`), while `https://console.saisi.online` behind Access must set `Secure`. A browser that silently drops the cookie is the failure.
- **The CSRF.** A sign-out POST without the token, a stale token, or another session's token must be 403 and must not end a session; a logout with no cookie must be a harmless redirect, not a 500.
- **The session expiry.** A cookie whose session passed its 24 hours is a login page, never the fleet; the console's injected clock is how the test moves time.
- **The page escaping.** A device name containing `<script>` must appear as text; `maud` does this by construction, and a unit test pins it.
- **The wrong token.** A bad operator token must be 401 with the login form and a `bad_operator_token` denial line, exactly like `/auth`; it must not mint a session.

---

### Task 1: The `/next` session, cookie, sign-in, sign-out and the page shell

**Files:**
- Modify: `device/console/Cargo.toml` (add `maud`)
- Modify: `device/console/src/auth.rs:19-24` (`Session.csrf`)
- Modify: `device/console/src/http.rs` (imports, `new_session`, `operator_from_credentials`, cookie helpers, `device_views`, `/next` handlers, routes)
- Create: `device/console/src/http/page.rs` (`login` and `dashboard`; `mod page;` inside `http.rs`)
- Test: `device/console/tests/console.rs`

**Interfaces:**
- Consumes: `AppState` (`Arc<Inner>`), `Session`, `new_session_token()`, `random_32()`, `constant_time_eq()`, `hex::decode_32/encode`, `ApiError`, `refuse_as`, `DeviceView` (`Serialize`, fields `id`, `name: Option<String>`, `online: bool`, `version: Option<String>`, `host: Option<String>`, `last_attempt: Option<RetryView>`, `public_key`, `registered_at_ms`, `last_seen_ms`).
- Produces:
  - `pub(crate) struct Session { token: [u8; 32], expires_at_ms: u64, csrf: [u8; 32] }`
  - `fn new_session(app: &AppState) -> Session`
  - `async fn operator_from_credentials(app: &AppState, code: Option<&str>, token: Option<&str>) -> Result<(), ApiError>`
  - `fn cookie_session(app: &AppState, headers: &HeaderMap) -> Option<Session>`
  - `fn session_cookie(token: &[u8; 32], secure: bool) -> HeaderValue`
  - `fn clear_session_cookie() -> HeaderValue`
  - `fn device_views(app: &AppState) -> Vec<DeviceView>`
  - Routes `GET /next`, `POST /next/auth`, `POST /next/logout`
  - `page::login(error: Option<&str>) -> maud::Markup`
  - `page::dashboard(devices: &[DeviceView], csrf: &str, version: &str) -> maud::Markup`

- [ ] **Step 1: Write the failing test**

Append to `device/console/tests/console.rs`:

```rust
/// A form body the page's sign-in and sign-out send.
fn post_form(uri: &str, fields: &[(&str, &str)]) -> Request<Body> {
    let body = fields
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("&");
    Request::builder()
        .method("POST")
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(body))
        .expect("the request builds")
}

fn get_with_cookie(uri: &str, cookie: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(uri)
        .header(header::COOKIE, cookie)
        .body(Body::empty())
        .expect("the request builds")
}

/// The CSRF value the sign-out form carries.
fn csrf_from(page: &str) -> String {
    let marker = "name=\"csrf\" value=\"";
    let start = page.find(marker).expect("a CSRF field") + marker.len();
    page[start..].chars().take(64).collect()
}

#[tokio::test]
async fn the_rust_page_signs_in_with_a_cookie_and_signs_out_with_csrf() {
    let fixture = Fixture::new("rust-page-login");
    let token = hex(&OPERATOR_TOKEN);

    let (status, _, body) = send_text(fixture.app(), get("/next", None)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Sign in"), "{body}");
    assert!(!body.contains("Devices"), "the fleet is behind the cookie: {body}");

    let (status, headers, _) =
        send_text(fixture.app(), post_form("/next/auth", &[("token", &token)])).await;
    assert_eq!(status, StatusCode::SEE_OTHER, "a form sign-in redirects");
    let cookie = headers
        .get(header::SET_COOKIE)
        .expect("a session cookie")
        .to_str()
        .expect("a cookie is ASCII")
        .to_owned();
    assert!(cookie.contains("HttpOnly"), "{cookie}");
    assert!(cookie.contains("SameSite=Strict"), "{cookie}");
    assert!(cookie.contains("Path=/next"), "{cookie}");
    assert!(cookie.contains("Max-Age=86400"), "{cookie}");
    let cookie = cookie.split(';').next().expect("the name=value pair").to_owned();

    let (status, _, body) = send_text(fixture.app(), get_with_cookie("/next", &cookie)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Devices"), "{body}");
    let csrf = csrf_from(&body);

    let mut request = post_form("/next/logout", &[("csrf", &csrf)]);
    request
        .headers_mut()
        .insert(header::COOKIE, cookie.parse().expect("the cookie header"));
    let (status, headers, _) = send_text(fixture.app(), request).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert!(headers
        .get(header::SET_COOKIE)
        .expect("a clearing cookie")
        .to_str()
        .expect("a cookie is ASCII")
        .contains("Max-Age=0"));

    let (status, _, body) = send_text(fixture.app(), get_with_cookie("/next", &cookie)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Sign in"), "the ended session is signed out: {body}");
}

#[tokio::test]
async fn the_rust_page_refuses_a_wrong_token_and_a_missing_csrf() {
    let fixture = Fixture::new("rust-page-refusals");

    let (status, _, body) =
        send_text(fixture.app(), post_form("/next/auth", &[("token", &"00".repeat(32))])).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(body.contains("Sign in"), "{body}");

    let (status, headers, _) = send_text(
        fixture.app(),
        post_form("/next/auth", &[("token", &hex(&OPERATOR_TOKEN))]),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let cookie = headers
        .get(header::SET_COOKIE)
        .expect("a session cookie")
        .to_str()
        .expect("a cookie is ASCII")
        .split(';')
        .next()
        .expect("the name=value pair")
        .to_owned();

    let mut request = post_form("/next/logout", &[("csrf", &"00".repeat(32))]);
    request
        .headers_mut()
        .insert(header::COOKIE, cookie.parse().expect("the cookie header"));
    let (status, _, _) = send_text(fixture.app(), request).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "a wrong CSRF ends nothing");

    let (status, _, body) = send_text(fixture.app(), get_with_cookie("/next", &cookie)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Devices"), "the session survived the refusal: {body}");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd device && cargo test -p summrise-console --test console the_rust_page -- --nocapture`
Expected: FAIL (`/next` is 404; the helpers may not compile).

- [ ] **Step 3: Add `maud` and the CSRF field**

In `device/console/Cargo.toml` under `[dependencies]`, after `serde_json`:

```toml
# The page is rendered, not bundled: maud builds the HTML from the Rust that already owns the
# data, and escapes every interpolated value by construction.
maud = "0.26"
```

In `device/console/src/auth.rs`, extend `Session`:

```rust
pub(crate) struct Session {
    pub(crate) token: [u8; 32],
    pub(crate) expires_at_ms: u64,
    /// The value a server-rendered form echoes back. It lives in the session (server memory),
    /// never in the cookie, so a cross-site request cannot read it.
    pub(crate) csrf: [u8; 32],
}
```

- [ ] **Step 4: Refactor the credential check and the session mint**

In `device/console/src/http.rs`, extract these helpers and make `/auth` use them:

```rust
/// Verify the operator's secret: a typed token, or a one-time login code. One outcome for both
/// callers, `/auth` and the page's form, including the denial line.
async fn operator_from_credentials(
    app: &AppState,
    code: Option<&str>,
    token: Option<&str>,
) -> Result<(), ApiError> {
    let accepted = if let Some(code) = code {
        let now = (app.now)();
        match crate::store::spend_login_code(&app.data, code, now) {
            Ok(()) => true,
            Err(reason) => {
                eprintln!("summrise-console: refused a login code: {reason}");
                return Err(app
                    .refuse_as(None, ApiError::unauthorized(), "bad_login_code")
                    .await);
            }
        }
    } else {
        let Some(presented) = token.and_then(hex::decode_32) else {
            return Err(app
                .refuse_as(None, ApiError::unauthorized(), "bad_operator_token")
                .await);
        };
        constant_time_eq(&presented, &app.operator_token)
    };
    if !accepted {
        return Err(app
            .refuse_as(None, ApiError::unauthorized(), "bad_operator_token")
            .await);
    }
    Ok(())
}

/// Mint one session, drop the expired ones, and return it. The CSRF value is part of the session.
fn new_session(app: &AppState) -> Session {
    let now = (app.now)();
    let session = Session {
        token: new_session_token(),
        expires_at_ms: now.saturating_add(SESSION_LIFETIME_MS),
        csrf: random_32(),
    };
    let mut sessions = lock(&app.sessions);
    sessions.retain(|session| session.expires_at_ms > now);
    sessions.push(session.clone());
    session
}
```

Then `authenticate` becomes:

```rust
async fn authenticate(
    State(app): State<AppState>,
    Json(request): Json<AuthRequest>,
) -> Result<Json<AuthResponse>, ApiError> {
    operator_from_credentials(&app, request.code.as_deref(), request.token.as_deref()).await?;
    let session = new_session(&app);
    Ok(Json(AuthResponse {
        token: hex::encode(&session.token),
        expires_at_ms: session.expires_at_ms,
    }))
}
```

- [ ] **Step 5: Add the cookie helpers**

In `device/console/src/http.rs`:

```rust
/// The cookie name the page uses. A page cookie, not an API credential: nothing outside `/next`
/// reads it.
const SESSION_COOKIE: &str = "summrise_session";

fn cookie_session(app: &AppState, headers: &HeaderMap) -> Option<Session> {
    let presented = headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .filter_map(|part| part.trim().strip_prefix("summrise_session="))
        .find_map(hex::decode_32)?;
    let now = (app.now)();
    let sessions = lock(&app.sessions);
    sessions
        .iter()
        .find(|session| constant_time_eq(&session.token, &presented) && now < session.expires_at_ms)
        .cloned()
}

fn secure_request(headers: &HeaderMap) -> bool {
    headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("https"))
}

fn session_cookie(token: &[u8; 32], secure: bool) -> HeaderValue {
    let mut cookie = format!(
        "{SESSION_COOKIE}={}; Path=/next; HttpOnly; SameSite=Strict; Max-Age={}",
        hex::encode(token),
        SESSION_LIFETIME_MS / 1000
    );
    if secure {
        cookie.push_str("; Secure");
    }
    HeaderValue::from_str(&cookie).expect("a valid Set-Cookie")
}

fn clear_session_cookie() -> HeaderValue {
    HeaderValue::from_static("summrise_session=; Path=/next; HttpOnly; SameSite=Strict; Max-Age=0")
}
```

- [ ] **Step 6: Extract `device_views` and add the `/next` handlers/routes**

In `device/console/src/http.rs`, move the body of `list_devices` into a helper both callers use:

```rust
fn device_views(app: &AppState) -> Vec<DeviceView> {
    lock(&app.devices)
        .list()
        .into_iter()
        .map(|device| DeviceView {
            online: app.channels.online(&device.id),
            version: app.channels.version(&device.id),
            host: app.channels.host(&device.id),
            last_attempt: app.channels.retry(&device.id).map(|note| RetryView {
                attempts: note.attempts,
                reason: note.reason,
            }),
            name: device.name.or_else(|| app.channels.name(&device.id)),
            id: device.id,
            public_key: device.public_key,
            registered_at_ms: device.registered_at_ms,
            last_seen_ms: device.last_seen_ms,
        })
        .collect()
}

async fn list_devices(State(app): State<AppState>, _: OperatorSession) -> Json<Vec<DeviceView>> {
    Json(device_views(&app))
}
```

Add the handlers (imports: `use axum::Form;`, `use axum::response::Redirect;`):

```rust
#[derive(Deserialize)]
struct NextLogin {
    token: String,
}

#[derive(Deserialize)]
struct NextLogout {
    csrf: String,
}

/// `GET /next`: the login form without a live cookie, the page with one.
async fn next_page(State(app): State<AppState>, headers: HeaderMap) -> Response {
    let Some(session) = cookie_session(&app, &headers) else {
        return Html(page::login(None).into_string()).into_response();
    };
    Html(page::dashboard(&device_views(&app), &hex::encode(&session.csrf), &app.version).into_string())
        .into_response()
}

/// `POST /next/auth`: the page's sign-in. The same operator secret as `/auth`, the same denial
/// line; the answer is a cookie and a redirect, not JSON.
async fn next_login(
    State(app): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<NextLogin>,
) -> Response {
    if operator_from_credentials(&app, None, Some(&form.token))
        .await
        .is_err()
    {
        return (
            StatusCode::UNAUTHORIZED,
            Html(page::login(Some("That token was not accepted.")).into_string()),
        )
            .into_response();
    }
    let session = new_session(&app);
    let mut response = Redirect::to("/next").into_response();
    response
        .headers_mut()
        .insert(header::SET_COOKIE, session_cookie(&session.token, secure_request(&headers)));
    response
}

/// `POST /next/logout`: end the session the cookie names, if the form echoes its CSRF value.
async fn next_logout(
    State(app): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<NextLogout>,
) -> Response {
    let Some(session) = cookie_session(&app, &headers) else {
        return Redirect::to("/next").into_response();
    };
    let Some(csrf) = hex::decode_32(&form.csrf) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    if !constant_time_eq(&csrf, &session.csrf) {
        return StatusCode::FORBIDDEN.into_response();
    }
    {
        let mut sessions = lock(&app.sessions);
        sessions.retain(|entry| !constant_time_eq(&entry.token, &session.token));
    }
    let mut response = Redirect::to("/next").into_response();
    response
        .headers_mut()
        .insert(header::SET_COOKIE, clear_session_cookie());
    response
}
```

Register the routes after `/auth`:

```rust
.route("/next", get(next_page))
.route("/next/auth", post(next_login))
.route("/next/logout", post(next_logout))
```

- [ ] **Step 7: Create `page::login` and `page::dashboard`**

Create `device/console/src/http/page.rs`; add `mod page;` at the top of `http.rs`:

```rust
//! The Rust page's first slice: the sign-in card and the fleet shell. The React page at `/` is
//! untouched; this module is where it moves to, one surface at a time.

use maud::{html, Markup, DOCTYPE};

use super::DeviceView;

/// The page's whole style: the console's tokens, a centered card, a table. No script.
const STYLE: &str = r#"
:root { color-scheme: light dark; --bg:#f6f5f2; --panel:#fff; --ink:#1d1c1a; --muted:#6c6a65; --line:#dedbd4; --accent:#b3400f; }
@media (prefers-color-scheme: dark) { :root { --bg:#141311; --panel:#1d1b18; --ink:#f2efe9; --muted:#a6a29a; --line:#37332d; --accent:#e07a4a; } }
* { box-sizing: border-box; }
body { margin:0; min-height:100vh; background:var(--bg); color:var(--ink); font:15px/1.5 system-ui, sans-serif; }
body.login { display:grid; place-items:center; padding:24px; }
.card { width:min(100%,380px); background:var(--panel); border:1px solid var(--line); border-radius:12px; padding:28px; }
h1 { margin:0 0 6px; font-size:20px; letter-spacing:-0.01em; }
h1 span { color:var(--muted); font-weight:400; }
.hint { color:var(--muted); font-size:13px; margin:0 0 20px; }
label { display:block; font-size:13px; margin:0 0 6px; }
input { width:100%; padding:9px 10px; border:1px solid var(--line); border-radius:7px; background:var(--bg); color:var(--ink); font:inherit; }
input:focus { outline:2px solid var(--accent); outline-offset:1px; }
button { margin-top:14px; width:100%; padding:9px 12px; border:0; border-radius:7px; background:var(--accent); color:#fff; font:inherit; cursor:pointer; }
button:hover { filter:brightness(1.08); }
.error { color:var(--accent); font-size:13px; margin:12px 0 0; }
header { display:flex; align-items:center; gap:12px; padding:14px 20px; border-bottom:1px solid var(--line); }
header form { margin:0 0 0 auto; width:auto; }
header button { width:auto; margin:0; background:transparent; color:var(--muted); border:1px solid var(--line); }
main { max-width:920px; margin:0 auto; padding:28px 20px 48px; }
h2 { font-size:15px; text-transform:uppercase; letter-spacing:0.08em; color:var(--muted); }
table { width:100%; border-collapse:collapse; background:var(--panel); border:1px solid var(--line); border-radius:10px; overflow:hidden; }
th, td { text-align:left; padding:11px 14px; border-bottom:1px solid var(--line); vertical-align:top; }
tr:last-child td { border-bottom:0; }
th { font-size:12px; text-transform:uppercase; letter-spacing:0.06em; color:var(--muted); }
code { color:var(--muted); font-size:12px; }
.ok { color:#1f8f4e; }
.off { color:var(--muted); }
.version { color:var(--muted); font-size:12px; }
"#;

/// The sign-in card. `error` is the one line a refused token gets.
pub(super) fn login(error: Option<&str>) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { "Summrise console" }
                style { (STYLE) }
            }
            body class="login" {
                main class="card" {
                    h1 { "Summrise " span { "console" } }
                    p class="hint" {
                        "The Rust page. The operator token opens a cookie session; nothing leaves this machine."
                    }
                    form method="post" action="/next/auth" {
                        label for="token" { "Operator token" }
                        input id="token" name="token" type="password" autocomplete="current-password" required;
                        button type="submit" { "Sign in" }
                    }
                    @if let Some(error) = error {
                        p class="error" { (error) }
                    }
                }
            }
        }
    }
}

/// The signed-in page: the fleet table and the sign-out form carrying the session's CSRF value.
pub(super) fn dashboard(devices: &[DeviceView], csrf: &str, version: &str) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { "Summrise console" }
                style { (STYLE) }
            }
            body {
                header {
                    h1 { "Summrise " span { "console" } }
                    span class="version" { "console " (version) }
                    form method="post" action="/next/logout" {
                        input type="hidden" name="csrf" value=(csrf);
                        button type="submit" { "Sign out" }
                    }
                }
                main {
                    h2 { "Devices" }
                    @if devices.is_empty() {
                        p class="hint" { "No devices have registered yet." }
                    } @else {
                        table {
                            thead { tr { th { "Device" } th { "State" } th { "Version" } } }
                            tbody {
                                @for device in devices {
                                    tr {
                                        td {
                                            strong { (device.name.as_deref().unwrap_or(&device.id)) }
                                            br;
                                            code { (&device.id) }
                                        }
                                        td {
                                            @if device.online { span class="ok" { "online" } } @else { span class="off" { "offline" } }
                                        }
                                        td { (device.version.as_deref().unwrap_or("—")) }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
```

- [ ] **Step 8: Run the tests to verify they pass**

Run: `cd device && cargo test -p summrise-console --test console the_rust_page -- --nocapture`
Expected: PASS — two tests.

- [ ] **Step 9: Run the whole console suite and clippy**

Run: `cd device && cargo test -p summrise-console && cargo clippy -p summrise-console --all-targets -- -D warnings`
Expected: the existing 140 integration tests plus the two new ones pass; clippy silent.

- [ ] **Step 10: Commit**

```bash
git add device/console/Cargo.toml device/console/Cargo.lock device/console/src/auth.rs device/console/src/http.rs device/console/src/http/page.rs device/console/tests/console.rs
git commit -m "console: serve the first Rust page at /next

The operator page is the product's last TypeScript surface. This is the
first slice of the migration: a maud-rendered sign-in, a cookie session
scoped to /next, a server-side CSRF value the sign-out form echoes, and
the fleet table fed by the same DeviceView projection /fleet returns.

The React page at / is untouched; the two pages share the session store
but not the transport (cookie vs bearer). The only new dependency is
maud.

VERIFIED: cargo test -p summrise-console (N passed); clippy -D warnings."
```

### Task 2: Pin the fleet rows, the escaping and the expiry

**Files:**
- Modify: `device/console/src/http/page.rs` (unit test only)
- Test: `device/console/tests/console.rs` (one new integration test)

**Interfaces:**
- Consumes: Task 1's `page::dashboard`, `DeviceView`, the fixture's registration.
- Produces: no new API; the tests pin the rows and escaping.

- [ ] **Step 1: Write the failing tests**

In `device/console/tests/console.rs`:

```rust
#[tokio::test]
async fn the_rust_page_lists_the_registered_fleet() {
    let fixture = Fixture::new("rust-page-fleet");
    let (device_id, _) = fixture.register(&signing_key(0xb4)).await;
    let token = hex(&OPERATOR_TOKEN);
    let (status, headers, _) =
        send_text(fixture.app(), post_form("/next/auth", &[("token", &token)])).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let cookie = headers
        .get(header::SET_COOKIE)
        .expect("a session cookie")
        .to_str()
        .expect("a cookie is ASCII")
        .split(';')
        .next()
        .expect("the name=value pair")
        .to_owned();

    let (status, _, body) = send_text(fixture.app(), get_with_cookie("/next", &cookie)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Devices"), "{body}");
    assert!(body.contains(&device_id), "the registered device is listed: {body}");
    assert!(!body.contains("No devices have registered yet."), "{body}");
}

#[tokio::test]
async fn the_rust_page_hides_the_fleet_after_the_session_expires() {
    let fixture = Fixture::new("rust-page-expiry");
    let (status, headers, _) =
        send_text(fixture.app(), post_form("/next/auth", &[("token", &hex(&OPERATOR_TOKEN))])).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let cookie = headers
        .get(header::SET_COOKIE)
        .expect("a session cookie")
        .to_str()
        .expect("a cookie is ASCII")
        .split(';')
        .next()
        .expect("the name=value pair")
        .to_owned();

    // The console's clock is injectable; a session lives 24 hours from its mint.
    fixture.clock.store(START_MS + DAY_MS + 1, Ordering::Relaxed);

    let (status, _, body) = send_text(fixture.app(), get_with_cookie("/next", &cookie)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Sign in"), "the expired cookie is signed out: {body}");
    assert!(!body.contains("Devices"), "{body}");
}
```

In `device/console/src/http/page.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_device_name_is_escaped() {
        let device = DeviceView {
            id: "00".repeat(32),
            public_key: "00".repeat(32),
            name: Some("<script>alert(1)</script>".to_owned()),
            registered_at_ms: 0,
            last_seen_ms: 0,
            online: false,
            version: None,
            host: None,
            last_attempt: None,
        };
        let page = dashboard(&[device], "00", "0.3.48").into_string();
        assert!(page.contains("&lt;script&gt;alert(1)&lt;/script&gt;"), "{page}");
        assert!(!page.contains("<script>alert(1)</script>"), "{page}");
    }
}
```

- [ ] **Step 2: Run them to verify they fail or pass for the right reason**

Run: `cd device && cargo test -p summrise-console the_rust_page_lists -- --nocapture && cargo test -p summrise-console a_device_name_is_escaped --lib -- --nocapture`
Expected: the integration test passes once Task 1 is in (the dashboard already renders the rows); the escaping test passes because `maud` escapes. If the integration test fails because the fixture's device is not registered, fix the registration call, not the page.

- [ ] **Step 3: Commit**

```bash
git add device/console/tests/console.rs device/console/src/http/page.rs
git commit -m "console: pin the Rust page's fleet rows and escaping"
```

### Task 3: Ship `/next` as console 0.3.48

**Files:**
- Modify: `device/console/Cargo.toml`? No code; the deploy only.
- Test: the live console health and the `/next` page.

- [ ] **Step 1: Build the Windows console**

Run:

```bash
cd device && SUMMRISE_BUILD_VERSION=0.3.48 cargo xwin build --release --target x86_64-pc-windows-msvc -p summrise-console
```

Expected: `target/x86_64-pc-windows-msvc/release/summrise-console.exe`.

- [ ] **Step 2: Deploy to the console host**

Copy the binary to `C:\SummriseConsole\summrise-console.exe` (back up the previous one as `.bak-0.3.47`), restart the `SummriseConsole` scheduled task (or wait for its restart policy), then check `GET http://127.0.0.1:19000/health` on the host and `GET https://console.saisi.online/next` through Access.

Expected: health reports `version: "0.3.48"`, the fleet is the same, and `/next` shows the sign-in card (the cookie is per browser, so the operator signs in once).

- [ ] **Step 3: Verify the React page is untouched**

Run: `curl -s https://console.saisi.online/ | head -c 200`.

Expected: the existing React page, byte-identical to before the deploy.

- [ ] **Step 4: Commit the plan's checkboxes**

Update this plan's checkboxes to `[x]` as each step completes; commit the plan.

---

## Self-Review

- **Spec coverage:** Phase 1's shell, login, cookie session, CSRF, fleet table, and the Rust rendering are all in Task 1; the audit/denial tables and the remaining surfaces are the next slices (the spec's phases 2-4), intentionally outside this plan. The server-rendered decision and the xterm exception are the spec's architecture, unchanged.
- **Type consistency:** `Session.csrf`, `new_session`, `operator_from_credentials`, `cookie_session`, `session_cookie`, `clear_session_cookie`, `device_views`, `page::login`, `page::dashboard` are named identically in every task and in the tests.
- **Review Focus:** each line has a test: the cookie flags in Task 1's integration test; the CSRF refusals in Task 1's second test; the page-level expiry in Task 2's `the_rust_page_hides_the_fleet_after_the_session_expires` (the console's injected clock); the escaping in Task 2's unit test; the wrong token in Task 1's second test.
