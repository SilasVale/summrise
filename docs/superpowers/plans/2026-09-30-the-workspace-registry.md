# The Workspace Registry Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A remote SSH workspace can be added from DSH's own UI — pick one of the agent's saved connections, type a
remote path — and every later call against that path reaches that host with those credentials.

**Architecture:** The agent becomes the only authority for "which connection serves this path" (a mapping store
resolved by longest prefix) and for its credentials (the keychain it already owns). The two workspace doors resolve
a request's path before connecting, and fall back to the inline connection parameters when nothing matches, so
nothing that works today breaks. The shim stops pinning a host and forwards paths; the DSH plugin gains a host half
(loopback calls to the agent) and a client half that registers a component into DSH's own
`sidebar.workspaces.directoryFlow` slot.

**Tech Stack:** Rust (the agent: `axum`-free hand-rolled routing in `agent/src/web/mod.rs`, `serde_json`,
`summrise_agent_core::DeviceError`), Node ESM (the shim and the plugin), React via `createElement` (the client half
ships as browser-ready JS, so it is not bundled).

**Spec:** `docs/superpowers/specs/2026-09-30-the-workspace-registry-design.md` — read it with this plan; the plan
argues from it.

## Global Constraints

- **English** in code, comments, commit messages, `AGENTS.md`; **Chinese** when talking to the operator.
- **Prefer Rust.** A change to a `.mjs`/`.cjs`/`.js` file needs a reason, not a habit. The reasons here: the shim
  and the plugin run inside DSH's Node process, and the client half runs in a browser.
- `cargo fmt --all -- --check` and `cargo clippy -p summrise-agent --all-targets -- -D warnings` (also with
  `--features terminal,keyring`) must stay clean; `cargo test -p summrise-agent` is the suite.
- Every tracked file's **worktree mode is 644** (`100755` where it is a script) — the `publish-release` gate checks
  the whole tree, and a file *created* with this shell's umask 0002 lands 664/600. After creating a file:
  `chmod 644 <file>`.
- `bash scripts/test/all-gates.bash` before any push, and read the exit code, not the output.
- The doors' existing request fields keep their meaning; `host`/`user` stop being **required** and become the
  inline fallback. A request that carries them behaves exactly as it does today.
- Refusals are **named**: `workspace/unknown-path`, `workspace/unknown-connection`. Never a timeout, never prose.

## Review Focus

The spec is a vision document; its silence on an input is not permission for that input to break the program.
These are the five most likely to bite, and each is pinned by a test in the task that owns the code:

1. **A path spelled the Windows way** (`D:\home\zhengsaisi\summrise`) must resolve to the same mapping as
   `/home/zhengsaisi/summrise` — this deployment really does send both spellings (commit `b3eda13d`). → Task 1.
2. **`/home/z` must not claim `/home/zeta`** — prefix matching is segment-wise, never `startsWith`. → Task 1.
3. **An `exec` with no `cwd`** has no path to match: it must use its inline connection, never guess a workspace.
   → Task 2.
4. **A mapping whose connection was forgotten** must refuse by name (`workspace/unknown-connection`) rather than
   connect with something else or hang. → Task 2.
5. **The agent being down** in the form must be *said* — an empty connection list is not "no connections exist",
   it is "nobody checked". → the UI plan (this plan ships the endpoints that form will call; it has no form).

---

## File Structure

| file | responsibility |
|---|---|
| `agent/src/workspace_mappings.rs` (new) | the store: path normalization, register/unregister/list, longest-prefix resolve, atomic persistence. Pure logic + one file, testable without a network |
| `agent/src/lib.rs` (modify) | declares the module |
| `agent/src/web/mod.rs` (modify) | the two doors resolve a path before connecting; four new endpoints |
| `agent/summrise-workspace-dsh/lib/transport.js` (modify) | `host`/`user` become optional in the exec transport |
| `agent/summrise-workspace-dsh/lib/fs-transport.js` (modify) | the same for the fs transport |
| `agent/summrise-workspace-dsh/lib/index.js`, `lib/fs.js` (modify) | `#requireHost` stops refusing when no host is configured |
| `agent/summrise-workspace-dsh/lib/workspace-registry.js` (new) | the plugin's **host half**: the agent's connections/mappings over loopback, exposed to the client half |
| `agent/summrise-workspace-dsh/client.js` (new) | the plugin's **client half**: a component registered into `sidebar.workspaces.directoryFlow` |
| `agent/summrise-workspace-dsh/package.json` (modify) | `exports["./client"]` + the new host entry |

---

### Task 1: The mapping store

**Files:**
- Create: `agent/src/workspace_mappings.rs`
- Modify: `agent/src/lib.rs` (add `pub mod workspace_mappings;` beside the other module declarations)
- Test: inline `#[cfg(test)] mod tests` in the new file (the shape `agent/src/tools/terminal/connections.rs` uses,
  including its `TEST_DIR` thread-local so tests never touch the real data dir)

**Interfaces:**
- Consumes: `summrise_agent_core::{recover_guard, DeviceError}`; `crate::paths::data_dir()`.
- Produces (exact, used by Tasks 2, 3 and 5):
  - `pub fn normalize(path: &str) -> String`
  - `pub fn register(path: &str, connection_id: &str) -> Result<(), DeviceError>`
  - `pub fn unregister(path: &str) -> Result<bool, DeviceError>`
  - `pub fn list() -> Vec<Entry>` where `pub struct Entry { pub path: String, pub connection_id: String }`
  - `pub fn resolve(path: &str) -> Option<Entry>`
  - `pub fn contains(parent: &str, child: &str) -> bool` (segment-wise, used by `resolve` and by the tests)

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    thread_local! {
        static TEST_DIR: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
    }

    /// Each test gets its own store file; nothing here can touch the real data dir.
    fn with_dir<T>(f: impl FnOnce() -> T) -> T {
        let dir = std::env::temp_dir().join(format!("sm-mappings-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        TEST_DIR.with(|d| *d.borrow_mut() = Some(dir.clone()));
        let out = f();
        TEST_DIR.with(|d| *d.borrow_mut() = None);
        out
    }

    #[test]
    fn a_path_has_one_spelling_and_windows_spelling_is_the_same_path() {
        assert_eq!(normalize("/home/zhengsaisi/summrise/"), "/home/zhengsaisi/summrise");
        assert_eq!(normalize("/home//zhengsaisi/./summrise"), "/home/zhengsaisi/summrise");
        assert_eq!(normalize("/home/zhengsaisi/x/../summrise"), "/home/zhengsaisi/summrise");
        // The deployment really sends both spellings (commit b3eda13d): the drive letter carries no
        // information on the far side of the seam, so it must fold to the same key.
        assert_eq!(normalize("D:\\home\\zhengsaisi\\summrise"), "/home/zhengsaisi/summrise");
        assert_eq!(normalize("d:/home/zhengsaisi\\summrise"), "/home/zhengsaisi/summrise");
    }

    #[test]
    fn containment_is_segment_wise_so_z_never_claims_zeta() {
        assert!(contains("/home/z", "/home/z"));
        assert!(contains("/home/z", "/home/z/a"));
        assert!(!contains("/home/z", "/home/zeta"));
        assert!(!contains("/home/z", "/home/zeta/a"));
        assert!(contains("/", "/anything"));
    }

    #[test]
    fn the_longest_prefix_wins() {
        with_dir(|| {
            register("/home/zhengsaisi", "ssh:a:22").unwrap();
            register("/home/zhengsaisi/summrise", "ssh:b:22").unwrap();
            assert_eq!(resolve("/home/zhengsaisi/other").unwrap().connection_id, "ssh:a:22");
            assert_eq!(resolve("/home/zhengsaisi/summrise/agent").unwrap().connection_id, "ssh:b:22");
            assert!(resolve("/tmp").is_none());
        });
    }

    #[test]
    fn register_is_idempotent_and_unregister_says_whether_it_removed_anything() {
        with_dir(|| {
            register("/srv/x", "ssh:a:22").unwrap();
            register("/srv/x", "ssh:a:22").unwrap();
            assert_eq!(list().len(), 1);
            register("/srv/x", "ssh:c:22").unwrap();
            assert_eq!(resolve("/srv/x").unwrap().connection_id, "ssh:c:22", "a re-register replaces");
            assert!(unregister("/srv/x").unwrap());
            assert!(!unregister("/srv/x").unwrap(), "removing nothing is a fact, not an error");
        });
    }

    #[test]
    fn a_windows_spelled_request_finds_a_posix_mapping() {
        with_dir(|| {
            register("/home/zhengsaisi/summrise", "ssh:a:22").unwrap();
            assert_eq!(resolve("D:\\home\\zhengsaisi\\summrise\\agent").unwrap().connection_id, "ssh:a:22");
        });
    }

    #[test]
    fn a_relative_or_empty_path_is_refused_by_name() {
        with_dir(|| {
            assert!(register("relative/path", "ssh:a:22").is_err());
            assert!(register("", "ssh:a:22").is_err());
            assert!(resolve("relative/path").is_none());
        });
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd agent && cargo test -p summrise-agent workspace_mappings 2>&1 | tail -20`
Expected: FAIL — `error[E0425]: cannot find function 'normalize' in this scope` (the module does not exist yet).

- [ ] **Step 3: Write the implementation**

```rust
//! WHICH HOST A PATH LIVES ON. One store, one rule: the longest matching prefix over normalized absolute
//! paths names the saved connection that serves a path — and nothing else in the system has to know.
//!
//! WHY IT LIVES IN THE AGENT. The alternative (a table in the DSH provider's config) makes adding a workspace a
//! config edit on a machine the operator is not sitting at, and it splits "which host" from "which credentials"
//! across two owners. The agent already owns the credentials (`tools/terminal/secrets.rs`, the keychain) and the
//! saved connections (`tools/terminal/connections.rs`), so it owns this too.
//!
//! NORMALIZATION MIRRORS THE PROVIDER'S, deliberately: `agent/summrise-workspace-dsh/lib/fs-transport.js`
//! `normalizePath` folds the Windows spelling of a POSIX path (measured 2026-09-30: every `resolve()` in a live
//! session arrived as `D:\home\zhengsaisi\summrise\…`). A store that normalized differently would answer
//! "unknown path" for a path the provider had just resolved.

use std::io::Write;
use std::path::PathBuf;

use summrise_agent_core::{recover_guard, DeviceError};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Entry {
    pub path: String,
    pub connection_id: String,
}

fn store_path() -> PathBuf {
    #[cfg(test)]
    if let Some(d) = TEST_DIR.with(|d| d.borrow().clone()) {
        return d.join("summrise-workspace-mappings.json");
    }
    crate::paths::data_dir().join("summrise-workspace-mappings.json")
}

#[cfg(test)]
thread_local! {
    pub(crate) static TEST_DIR: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

/// One spelling per path: POSIX rules, and a Windows-spelled absolute path folded to the POSIX one.
/// `None` when the input cannot name an absolute path — a relative path is refused rather than guessed.
pub fn normalize(path: &str) -> Option<String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return None;
    }
    let folded = match trimmed.as_bytes() {
        [drive, b':', b'\\' | b'/', ..] if drive.is_ascii_alphabetic() => trimmed[2..].replace('\\', "/"),
        _ => trimmed.replace('\\', "/"),
    };
    if !folded.starts_with('/') {
        return None;
    }
    let mut out: Vec<&str> = Vec::new();
    for segment in folded.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    Some(format!("/{}", out.join("/")))
}

/// Segment-wise containment: `/home/z` must NOT claim `/home/zeta`.
pub fn contains(parent: &str, child: &str) -> bool {
    if parent == child {
        return true;
    }
    let base = if parent == "/" { "/" } else { parent };
    let base = base.trim_end_matches('/');
    child.len() > base.len() && child.starts_with(base) && child.as_bytes()[base.len()] == b'/'
}

fn read_all() -> Vec<Entry> {
    match std::fs::read_to_string(store_path()) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

/// Serialize writers: two registrations that read the same file would otherwise drop one (the same
/// round-109 defect `tools/terminal/connections.rs` records for its own store).
static STORE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn write_all(entries: &[Entry]) -> Result<(), DeviceError> {
    let _guard = recover_guard();
    let path = store_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let tmp = path.with_extension("json.tmp");
    // temp + rename: a crash mid-write must not leave a torn store an operator cannot tell from a real one.
    {
        let mut f = std::fs::File::create(&tmp)
            .map_err(|e| DeviceError::Io(format!("write {}: {e}", tmp.display())))?;
        f.write_all(
            serde_json::to_string(entries)
                .map_err(|e| DeviceError::Io(format!("encode mappings: {e}")))?
                .as_bytes(),
        )
        .map_err(|e| DeviceError::Io(format!("write {}: {e}", tmp.display())))?;
    }
    std::fs::rename(&tmp, &path)
        .map_err(|e| DeviceError::Io(format!("rename to {}: {e}", path.display())))
}

pub fn list() -> Vec<Entry> {
    let _guard = STORE_LOCK.lock().unwrap();
    read_all()
}

pub fn register(path: &str, connection_id: &str) -> Result<(), DeviceError> {
    let key = normalize(path)
        .ok_or_else(|| DeviceError::BadRequest(format!("not an absolute path: {path:?}")))?;
    let _guard = STORE_LOCK.lock().unwrap();
    let mut entries = read_all();
    entries.retain(|e| e.path != key);
    entries.push(Entry { path: key, connection_id: connection_id.to_string() });
    write_all(&entries)
}

pub fn unregister(path: &str) -> Result<bool, DeviceError> {
    let Some(key) = normalize(path) else { return Ok(false) };
    let _guard = STORE_LOCK.lock().unwrap();
    let mut entries = read_all();
    let before = entries.len();
    entries.retain(|e| e.path != key);
    if entries.len() == before {
        return Ok(false);
    }
    write_all(&entries)?;
    Ok(true)
}

/// The longest matching prefix wins, so `/home/zhengsaisi` and `/home/zhengsaisi/summrise` can be two workspaces
/// on two hosts. `None` means "no mapping" — which the door turns into the inline fallback, not into an error.
pub fn resolve(path: &str) -> Option<Entry> {
    let key = normalize(path)?;
    let _guard = STORE_LOCK.lock().unwrap();
    read_all()
        .into_iter()
        .filter(|e| contains(&e.path, &key))
        .max_by_key(|e| e.path.len())
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd agent && cargo test -p summrise-agent workspace_mappings 2>&1 | tail -20`
Expected: PASS — `test result: ok. 6 passed; 0 failed`.

- [ ] **Step 5: Run the suite, fmt and clippy**

```bash
cd agent
cargo fmt --all -- --check
cargo clippy -p summrise-agent --all-targets -- -D warnings
cargo test -p summrise-agent 2>&1 | tail -3      # expect: test result: ok
```

- [ ] **Step 6: Commit**

```bash
cd /home/zhengsaisi/summrise
chmod 644 agent/src/workspace_mappings.rs
git add agent/src/workspace_mappings.rs agent/src/lib.rs
git commit -F - <<'MSG'
feat(agent): the mapping store — which host a path lives on

One rule: the longest matching prefix over normalized absolute paths names the saved
connection that serves a path. It lives in the agent because the agent already owns the
credentials (the keychain) and the saved connections; the alternative splits "which host"
from "which credentials" across two owners and makes adding a workspace a config edit on a
machine nobody is sitting at.

Normalization mirrors the provider's on purpose: this deployment sends both spellings of one
path (measured: every resolve() in a live session arrived as `D:\home\zhengsaisi\…`), so a
store that normalized differently would answer "unknown path" for a path the provider had
just resolved. Containment is segment-wise, so `/home/z` never claims `/home/zeta`.
MSG
```

---

### Task 2: The doors resolve a path before they connect

**Files:**
- Modify: `agent/src/web/mod.rs` — `struct WorkspaceTarget` and its `from_request` (around line 2319), the exec
  handler (around line 2600) and the fs handler
- Test: the existing route tests in `agent/src/web/mod.rs` (`#[cfg(test)] mod tests`), plus the new cases below

**Interfaces:**
- Consumes: `crate::workspace_mappings::{resolve, Entry}` (Task 1); `crate::tools::terminal::connections::list()`
  (existing) for the connection lookup.
- Produces: the doors accept a request **either** with `host`+`user` (today's shape) **or** with a path that
  matches a mapping. Refusal bodies: `{"ok":false,"error":…,"code":"workspace/unknown-path"}` and
  `…"code":"workspace/unknown-connection"`.

- [ ] **Step 1: Write the failing tests**

```rust
    /// A path with no mapping and no inline connection is refused BY NAME, not by a connect timeout.
    #[tokio::test]
    async fn an_exec_with_neither_a_mapping_nor_a_host_is_refused_by_name() {
        let answer = api_workspace_exec(r#"{"cwd":"/nowhere/at/all","argv":["true"]}"#).await;
        assert_eq!(answer["ok"], false);
        assert_eq!(answer["code"], "workspace/unknown-path", "got: {answer}");
    }

    /// A mapping whose connection has been forgotten is refused BY NAME — never connected with something else.
    #[tokio::test]
    async fn a_mapping_whose_connection_is_gone_is_refused_by_name() {
        crate::workspace_mappings::register("/gone/soon", "ssh:192.0.2.9:22").unwrap();
        let answer = api_workspace_exec(r#"{"cwd":"/gone/soon/x","argv":["true"]}"#).await;
        assert_eq!(answer["ok"], false);
        assert_eq!(answer["code"], "workspace/unknown-connection", "got: {answer}");
    }

    /// An exec with NO cwd has no path to match: it must use the inline connection it was given.
    /// This one cannot connect in a test, so it asserts the REFUSAL IT DOES NOT GET.
    #[tokio::test]
    async fn an_exec_without_a_cwd_uses_its_inline_connection_and_not_a_guess() {
        let answer = api_workspace_exec(r#"{"host":"192.0.2.1","user":"u","argv":["true"]}"#).await;
        assert_ne!(answer["code"], "workspace/unknown-path", "got: {answer}");
        assert_ne!(answer["code"], "workspace/unknown-connection", "got: {answer}");
    }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cd agent && cargo test -p summrise-agent --features terminal,keyring workspace 2>&1 | tail -20`
Expected: FAIL — the first asserts on `code` and gets `invalid_params` ("a host is required").

- [ ] **Step 3: Implement the resolution**

In `agent/src/web/mod.rs`, replace `WorkspaceTarget::from_request`'s required-host head with a resolution step, and
add the two named refusals. The shape (keep the existing `port`/`password`/`key_path` reads):

```rust
impl WorkspaceTarget {
    /// THE PATH DECIDES FIRST. A request may carry a connection inline (today's shape, unchanged) or name a path
    /// that the mapping store knows; only when neither holds is it refused — and then by NAME.
    fn from_request(v: &serde_json::Value) -> Result<Self, serde_json::Value> {
        let bad = |message: &str| serde_json::json!({"ok": false, "error": message, "code": "invalid_params"});
        let named = |code: &str, message: String| serde_json::json!({"ok": false, "error": message, "code": code});

        // The path: `cwd` for exec, `path` for fs — the only fields that name a place.
        let path = v
            .get("cwd")
            .or_else(|| v.get("path"))
            .and_then(|x| x.as_str())
            .filter(|p| !p.is_empty());

        let inline = v.get("host").and_then(|x| x.as_str()).filter(|h| !h.is_empty());
        if inline.is_none() {
            let Some(path) = path else {
                return Err(named(
                    "workspace/unknown-path",
                    "no connection was given and this request names no path to look one up by".into(),
                ));
            };
            let Some(entry) = crate::workspace_mappings::resolve(path) else {
                return Err(named(
                    "workspace/unknown-path",
                    format!("no workspace is registered for {path:?}; add one, or pass host and user inline"),
                ));
            };
            let saved = crate::tools::terminal::connections::find(&entry.connection_id)
                .ok_or_else(|| named(
                    "workspace/unknown-connection",
                    format!(
                        "workspace {:?} is registered to connection {:?}, which is no longer saved",
                        entry.path, entry.connection_id
                    ),
                ))?;
            return Self::from_saved(&saved);
        }

        // …the existing inline path, unchanged, including "a host is required"/"a user is required" for the
        // fields that remain required once a host IS present.
        let Some(user) = v.get("user").and_then(|x| x.as_str()) else {
            return Err(bad("a user is required"));
        };
        Ok(Self {
            host: inline.unwrap().to_string(),
            user: user.to_string(),
            port: v.get("port").and_then(|x| x.as_u64()).unwrap_or(22) as u16,
            password: v.get("password").and_then(|x| x.as_str()).unwrap_or_default().to_string(),
            key_path: v.get("key_path").and_then(|x| x.as_str()).unwrap_or_default().to_string(),
        })
    }

    /// A saved connection's original open params, as a target. The entry's shape is the one
    /// `terminal_connect_saved` replays — `{"id": …, "params": {host, user, port, key_path, password?}}` — and the
    /// password is NOT in that file when it is a secret (the store's own comment records why). An empty password
    /// keeps the meaning it already has at the connect site: use the key.
    fn from_saved(saved: &serde_json::Value) -> Result<Self, serde_json::Value> {
        let named = |code: &str, message: String| serde_json::json!({"ok": false, "error": message, "code": code});
        let params = saved.get("params").and_then(|p| p.as_object()).cloned().unwrap_or_default();
        let text = |key: &str| params.get(key).and_then(|x| x.as_str()).unwrap_or_default().to_string();
        let (host, user) = (text("host"), text("user"));
        if host.is_empty() || user.is_empty() {
            return Err(named(
                "workspace/unknown-connection",
                format!(
                    "the saved connection {} carries no host/user to connect with",
                    saved.get("id").and_then(|x| x.as_str()).unwrap_or("?")
                ),
            ));
        }
        Ok(Self {
            host,
            user,
            port: params.get("port").and_then(|x| x.as_u64()).unwrap_or(22) as u16,
            password: text("password"),
            key_path: text("key_path"),
        })
    }
}
```

`agent/src/tools/terminal/connections.rs` gains one function, because "is this id still saved" is its question:

```rust
/// The saved entry for `id`, or `None` when it was forgotten. `terminal_saved_connections` lists them; this is
/// the same map, read by id, so a workspace can say WHICH connection it lost.
pub fn find(id: &str) -> Option<serde_json::Value> {
    let _guard = STORE_LOCK.lock().unwrap();
    read_all().get(id).cloned()
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd agent && cargo test -p summrise-agent --features terminal,keyring workspace 2>&1 | tail -20`
Expected: PASS. Then the full agent suite: `cargo test -p summrise-agent --features terminal,keyring` → `test result: ok`.

- [ ] **Step 5: fmt, clippy, commit**

```bash
cd agent && cargo fmt --all && cargo clippy -p summrise-agent --features terminal,keyring --all-targets -- -D warnings
cd /home/zhengsaisi/summrise
git add agent/src/web/mod.rs agent/src/tools/terminal/connections.rs
git commit -F - <<'MSG'
feat(agent): the workspace doors resolve a path before they connect

A request may carry its connection inline (unchanged) or name a path the mapping store knows.
Only when neither holds is it refused — and then by NAME: `workspace/unknown-path` when
nothing matched, `workspace/unknown-connection` when a mapping's connection has been
forgotten. A timeout is not an answer an operator can act on.

An exec with NO cwd has no path to match and uses its inline connection: guessing a workspace
would be worse than saying nothing, and the test asserts the refusal it does NOT get.
MSG
```

---

### Task 3: The registry endpoints

**Files:**
- Modify: `agent/src/web/mod.rs` — the route table (`Pattern::Exact("/api/workspace/exec")` sits in it) and four new
  handlers beside `api_workspace_exec`
- Test: the route-table test that already asserts unknown paths are not found, plus the new cases

**Interfaces:**
- Consumes: `crate::workspace_mappings::{list, register, unregister}` (Task 1);
  `crate::tools::terminal::connections::list()` (existing).
- Produces, for Task 5 (the plugin's host half) — every body is `{"ok":true, …}` or `{"ok":false,"error":…,"code":…}`:
  - `GET /api/workspace/connections` → `{"ok":true,"connections":[{"id":"ssh:10.10.61.83:22122","kind":"ssh","target":"10.10.61.83:22122","label":…}]}`
  - `GET /api/workspace/mappings` → `{"ok":true,"mappings":[{"path":"/home/…","connection_id":"ssh:…"}]}`
  - `POST /api/workspace/register` `{path, connection_id}` → `{"ok":true}`; unknown connection →
    `{"ok":false,"code":"workspace/unknown-connection"}`
  - `POST /api/workspace/unregister` `{path}` → `{"ok":true,"removed":true|false}`

- [ ] **Step 1: Write the failing tests**

```rust
    #[tokio::test]
    async fn registering_a_mapping_names_an_unknown_connection() {
        let answer = api_workspace_register(r#"{"path":"/srv/x","connection_id":"ssh:192.0.2.9:22"}"#).await;
        assert_eq!(answer["ok"], false);
        assert_eq!(answer["code"], "workspace/unknown-connection", "got: {answer}");
    }

    #[tokio::test]
    async fn unregistering_nothing_is_a_fact_not_an_error() {
        let answer = api_workspace_unregister(r#"{"path":"/never/registered"}"#).await;
        assert_eq!(answer["ok"], true);
        assert_eq!(answer["removed"], false);
    }

    #[test]
    fn the_registry_endpoints_are_routed() {
        assert!(route_of("GET", "/api/workspace/connections").is_some());
        assert!(route_of("GET", "/api/workspace/mappings").is_some());
        assert!(route_of("POST", "/api/workspace/register").is_some());
        assert!(route_of("POST", "/api/workspace/unregister").is_some());
    }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cd agent && cargo test -p summrise-agent --features terminal,keyring registry 2>&1 | tail -15`
Expected: FAIL — `route_of("GET", "/api/workspace/connections")` is `None`.

- [ ] **Step 3: Implement the four handlers**

```rust
/// The agent's saved connections, so the form can offer them. The password is not in the store (it lives in the
/// keychain), and this answer carries the same fields `terminal_saved_connections` already returns.
async fn api_workspace_connections() -> serde_json::Value {
    serde_json::json!({"ok": true, "connections": crate::tools::terminal::connections::list()})
}

async fn api_workspace_mappings() -> serde_json::Value {
    serde_json::json!({"ok": true, "mappings": crate::workspace_mappings::list()})
}

/// Registering is idempotent, and an unknown connection is refused BY NAME — a mapping to a connection that does
/// not exist would be a workspace that fails much later, somewhere else.
async fn api_workspace_register(body: &str) -> serde_json::Value {
    let v: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return serde_json::json!({"ok": false, "error": format!("bad json: {e}"), "code": "invalid_params"}),
    };
    let (Some(path), Some(connection_id)) = (
        v.get("path").and_then(|x| x.as_str()),
        v.get("connection_id").and_then(|x| x.as_str()),
    ) else {
        return serde_json::json!({"ok": false, "error": "path and connection_id are required", "code": "invalid_params"});
    };
    if crate::tools::terminal::connections::find(connection_id).is_none() {
        return serde_json::json!({
            "ok": false,
            "error": format!("no saved connection {connection_id:?}"),
            "code": "workspace/unknown-connection",
        });
    }
    match crate::workspace_mappings::register(path, connection_id) {
        Ok(()) => serde_json::json!({"ok": true}),
        Err(e) => serde_json::json!({"ok": false, "error": e.to_string(), "code": "invalid_params"}),
    }
}

async fn api_workspace_unregister(body: &str) -> serde_json::Value {
    let v: serde_json::Value = serde_json::from_str(body).unwrap_or(serde_json::Value::Null);
    let Some(path) = v.get("path").and_then(|x| x.as_str()) else {
        return serde_json::json!({"ok": false, "error": "path is required", "code": "invalid_params"});
    };
    match crate::workspace_mappings::unregister(path) {
        Ok(removed) => serde_json::json!({"ok": true, "removed": removed}),
        Err(e) => serde_json::json!({"ok": false, "error": e.to_string(), "code": "invalid_params"}),
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd agent && cargo test -p summrise-agent --features terminal,keyring 2>&1 | tail -3`
Expected: `test result: ok`.

- [ ] **Step 5: fmt, clippy, commit**

```bash
cd agent && cargo fmt --all && cargo clippy -p summrise-agent --features terminal,keyring --all-targets -- -D warnings
cd /home/zhengsaisi/summrise
git add agent/src/web/mod.rs
git commit -F - <<'MSG'
feat(agent): the workspace registry endpoints

`GET /api/workspace/connections` (the saved connections, for the form), `GET /api/workspace/mappings`,
`POST /api/workspace/register` and `POST /api/workspace/unregister`. Registering is idempotent and an
unknown connection is refused by name: a mapping to a connection that does not exist would be a
workspace that fails much later, somewhere else. Unregistering nothing answers `removed:false`,
which is a fact and not an error.
MSG
```

---

### Task 4: The provider stops pinning a host

**Files:**
- Modify: `agent/summrise-workspace-dsh/lib/transport.js` — add `connectionFields` and use it in the body
- Modify: `agent/summrise-workspace-dsh/lib/fs-transport.js` — the same rule for the fs body
- Modify: `agent/summrise-workspace-dsh/lib/index.js`, `lib/fs.js` — `__requireHost()` calls the shared rule instead
  of refusing on its own
- Test: `agent/summrise-workspace-dsh/test/transport.test.mjs`, `test/fs-transport.test.mjs`

**Interfaces:**
- Produces: `export function connectionFields({ host, user, port })` in both transports —
  `{}` when neither is set (delegated: the agent resolves the path), `{host, user, port}` when both are, and a
  thrown `Error` when exactly one is (a half-written connection is a mistake, not a delegation).

**Why the rule lives in the transport and not in the provider:** `lib/index.js` imports
`@deepseek-ai/dsh-subprocess`, so it cannot be loaded without DSH installed — which is exactly why this package's
tests cover the transports and nothing else. Putting the rule in the provider would make it untestable here; putting
it in the transport makes it a pure function with a test, and leaves the provider a one-line caller.

- [ ] **Step 1: Write the failing tests**

```js
test("a connection with no host is DELEGATED, not refused", async () => {
  // The agent resolves the path against its mapping store, so a provider with no host is a
  // legitimate configuration — it is the whole point of the registry.
  let seen = null;
  await execOnAgent({
    endpoint: "http://127.0.0.1:18080",
    token: "t",
    argv: ["uname", "-a"],
    cwd: "/home/zhengsaisi/summrise",
    fetchImpl: async (_url, init) => {
      seen = JSON.parse(init.body);
      return { ok: true, status: 200, json: async () => ({ ok: true, stdout: "Linux", stderr: "", exit_code: 0 }) };
    },
  });
  assert.equal("host" in seen, false, "no host must be sent when none is configured");
  assert.equal("user" in seen, false);
  assert.equal(seen.cwd, "/home/zhengsaisi/summrise", "the path is what the agent resolves");
});

test("half a connection is still a mistake", () => {
  assert.throws(() => connectionFields({ host: "10.0.0.1" }), /needs a user/);
  assert.throws(() => connectionFields({ user: "root" }), /needs a host/);
  assert.deepEqual(connectionFields({ host: "10.0.0.1", user: "root", port: 22122 }), {
    host: "10.0.0.1",
    user: "root",
    port: 22122,
  });
  assert.deepEqual(connectionFields({}), {});
});
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cd agent/summrise-workspace-dsh && npm test 2>&1 | tail -12`
Expected: FAIL — `connectionFields is not a function`, and the body carries `host: undefined` as a key.

- [ ] **Step 3: Implement**

```js
/**
 * The connection fields to send with a request. NO host is a legitimate configuration: the agent resolves the
 * request's path against its mapping store and uses the connection registered for it. A host WITHOUT a user is
 * still a mistake — that is a half-written connection, not a delegated one — and it fails here, where it is cheap,
 * rather than as an SSH error later.
 */
export function connectionFields({ host, user, port = 22 } = {}) {
  if (!host && !user) return {};
  if (!host) throw new Error('summrise-workspace: a connection needs a host as well as a user');
  if (!user) throw new Error('summrise-workspace: a connection needs a user as well as a host');
  return { host, user, port };
}
```

`execOnAgent` builds its body from it (`const body = { ...connectionFields({ host, user, port }), argv };`) and
`lib/index.js`'s `__requireHost()` becomes `connectionFields(this.__config)` — the same rule, one place. `describe()`
keeps reporting exactly what the config holds, so a diagnostic on a delegated provider says no host rather than
inventing one.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd agent/summrise-workspace-dsh && npm test 2>&1 | tail -5`
Expected: `ℹ pass N` with `ℹ fail 0` (read the exit code).

- [ ] **Step 5: Commit**

```bash
cd /home/zhengsaisi/summrise
git add agent/summrise-workspace-dsh/lib agent/summrise-workspace-dsh/test
git commit -F - <<'MSG'
feat(workspace-dsh): a provider with no host is delegated, not broken

The provider pinned ONE host, so every workspace landed on it whatever its path said. Now a
config with no host sends no host, and the agent resolves the request's path against its
mapping store; a config with a host and no user still fails here, where it is cheap.

The rule lives in the transports as a pure function because `lib/index.js` imports
`@deepseek-ai/dsh-subprocess` and cannot be loaded without DSH — which is why this package's
tests cover the transports and nothing else.
MSG
```

---

### Task 5: The device acceptance — the proof the feature exists

**Files:** none. This task changes no code; it is the measurement that says the four tasks above work on the
machine the operator uses. **Its evidence goes in the commit message of the release that carries it.**

**Preconditions:** the agent is deployed to `desktop-14rjcr8` with Tasks 1-4 in it (`summrise status` shows the new
release); DSH is running; the Linux connection is saved (`terminal_saved_connections` lists
`ssh:10.10.61.83:22122`).

- [ ] **Step 1: Register the workspace the deployment already uses**

```bash
# on the device, with the agent's token (the same one the panel and the plugin use)
curl -s -X POST http://127.0.0.1:18080/api/workspace/register \
  -H "authorization: Bearer $TOKEN" -H 'content-type: application/json' \
  -d '{"path":"/home/zhengsaisi/summrise","connection_id":"ssh:10.10.61.83:22122"}'
```
Expected: `{"ok":true}`. Then `GET /api/workspace/mappings` → the entry, normalized to
`/home/zhengsaisi/summrise`.

- [ ] **Step 2: Take the host OUT of the provider's config**

Remove `host`/`user` from the `summrise-workspace` entry's config in the DSH profile and restart DSH. **This is the
step that proves routing rather than a lucky pin** — with the host gone, nothing but the mapping store can make the
workspace work.

- [ ] **Step 3: The positive control — a real command in a real session**

In a DSH session in that workspace, ask for `uname -a` and `pwd`. Expected: the Linux kernel string
(`Linux … x86_64 GNU/Linux`) and `/home/zhengsaisi/summrise`, with a `toolMs` in the thousands, not the tens of a
local run — the same evidence shape as the turn that first proved the seam.

- [ ] **Step 4: The negative control — an unregistered path is refused BY NAME**

```bash
curl -s -X POST http://127.0.0.1:18080/api/workspace/exec \
  -H "authorization: Bearer $TOKEN" -H 'content-type: application/json' \
  -d '{"cwd":"/nowhere/at/all","argv":["true"]}'
```
Expected: `{"ok":false,…,"code":"workspace/unknown-path"}` — **not** a timeout, and not a connect attempt.

- [ ] **Step 5: The mutation — unregister and watch the SAME call change its answer**

```bash
curl -s -X POST http://127.0.0.1:18080/api/workspace/unregister -H "authorization: Bearer $TOKEN" \
  -H 'content-type: application/json' -d '{"path":"/home/zhengsaisi/summrise"}'
```
Expected: `{"ok":true,"removed":true}`, and the call from Step 3 now answers `workspace/unknown-path`. **That pair is
what proves the mapping was the thing making it work**; without it, Step 3 is a green that could have come from
anywhere. Re-register afterwards.

- [ ] **Step 6: A second path on the same host (and a second host if one is reachable)**

Register `/home/zhengsaisi` as well, then prove the **longest prefix wins** from a session: a call in
`/home/zhengsaisi/summrise/agent` must use the more specific mapping. If a second SSH host is reachable from the
device (its own `sshd` on `127.0.0.1:22122` is one candidate, if a connection can be saved for it), register a path
for it too and prove two hosts coexist. **If no second host is reachable, say so in the report** — a claim of
"two hosts" that was never measured is the kind of sentence this repository has paid for before.

- [ ] **Step 7: Record the evidence**

The measurements above go into the commit message of the release that ships Tasks 1-4, as the `VERIFIED:` block,
with the exact strings the device answered. No new file, no ledger.

---

## What this plan deliberately does NOT include — and why

**The DSH UI half is a second plan, not a missing task.** The spec's §7 asks for a form in DSH's UI, and the
mechanism it needs is not settled by measurement yet:

- a third-party package **can** ship a client half (`clientExportOf` is generic — measured), and the workspace flow
  **does** have a slot to register into (`sidebar.workspaces.directoryFlow` — measured);
- but the channel from that client half back to the host half is `ctx.remote` (typert), and typert discovers remote
  methods through a **compiler-injected prototype descriptor** (`remoteMethods` reads a symbol-keyed descriptor;
  `bindTypertRemote` binds a service but does not author methods). A hand-written JS plugin would have to write that
  descriptor by hand, and no plugin-facing route seam on DSH's own HTTP server was found in the packages either.

So the UI plan starts with that question and answers it by measurement, the way this one did. **The registry this
plan ships is what makes the UI plan small**: adding a workspace becomes one authenticated HTTP call
(`POST /api/workspace/register`), and everything else — the form, the slot, the connection picker — is a client of it.
That is also why the operator can use the feature the moment this plan lands: the call above is the feature, and the
form is its convenience.

## Execution

- Every task ends green: `cargo test -p summrise-agent --features terminal,keyring` for Tasks 1-3,
  `npm test` in `agent/summrise-workspace-dsh` for Task 4, and Task 5 is the device.
- `bash scripts/test/all-gates.bash` before the push that carries them, then `main` by `--no-ff` merge, then the
  release (`release: 1.2.N`) — and the device acceptance (Task 5) runs **after** the device is updated, because its
  whole point is the deployed machine.
