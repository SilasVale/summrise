# The mail inbox for the agent — implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `dsh@saisi.online` receives mail, the worker stores each raw message in R2 and copies it to 163, and the agent reads what arrived over an unguessable HTTP path.

**Architecture:** One new Rust worker (`mail/`, `summrise-mail`, workers-rs 0.8.7) with two events: `email()` copies the raw message into a dedicated R2 bucket and forwards a copy to the operator's 163, `fetch()` serves an index and the raw messages. One Email Routing rule for the single address; the catch-all that sends everything else to 163 is untouched. Nothing parses MIME inside the worker — Workers Free gives 10 ms of CPU per request, so parsing lives on the operator's box.

**Tech Stack:** Rust (workers-rs 0.8.7, `worker-build --release`), Cloudflare Workers + R2 + Email Routing, bash provisioning scripts against the Cloudflare API, `wrangler` 4.127.0 (the repo's pin).

**Spec:** `docs/superpowers/specs/2026-10-14-saisi-mail-inbox-design.md` — read it before Task 1. Every number below is copied from it.

## Global Constraints

- **Retention: 7 days**, enforced by the bucket's lifecycle rule (`mail/` prefix, `maxAge` 604800 s). The worker stores no expiry of its own.
- **No write cap.** Nothing refuses mail. The retention window is the only bound on the bucket's size.
- **No MIME parsing, no HTML rendering, no sending and no replying inside the worker.** The only outbound action is `message.forward(FORWARD_TO)`.
- **No Cloudflare Access on the read path** — the agent has no browser session. Protection is the unguessable secret path segment only.
- **Rust only.** No new `.js`/`.mjs`/`.cjs` file may carry logic (`agent/tests/js_boundary_inventory.rs` caps them; Rust-backed workers point `main` at the untracked `worker/build/index.js` glue, which is what `agent/tests/worker_language.rs` expects).
- **The repository is written in English** — commits, comments, docs. The operator is addressed in Chinese in chat only.
- **Run the command the other end runs**: `cargo test`, `cargo clippy --target wasm32-unknown-unknown -- -D warnings`, `wrangler deploy --dry-run` — CI's commands, not convenient ones.
- **Work on a branch; `main` advances only by merge** (`agent/tests/main_shape.rs`).
- **A new production hostname must be declared** in `agent/tests/production_host.rs` **and `MAX_ALLOWED` raised in the same commit** (53 → 56 in Task 1). The ratchet fails in both directions.
- **The deploy tool is the global `wrangler` (4.127.0 pin), never `npx wrangler`** (`scripts/build.sh`'s `deploy_relay` records why), and every `wrangler deploy` in `scripts/build.sh` must carry `CLOUDFLARE_API_TOKEN=` or `scripts/test/release-lib.bash` refuses it.
- Values used verbatim from the spec: inbound limit **25 MiB**, worker CPU ceiling **10 ms**, R2 free tier **10 GB-month**, overage **$0.015/GB-month**, verified destination **13122231591@163.com**.

## Review Focus

These are the input classes and failure modes the spec leaves to the code. Each one gets its test in the task named beside it.

1. **Headers the sender omitted** (`Subject`, `Date`, `Message-ID` absent — legal in email) → the index row shows empty fields and the message is still stored and listed. (Task 2 tests, Task 3 uses the same accessor.)
2. **A body that is not UTF-8** (8-bit or Latin-1 text) → the worker never decodes the raw bytes; the index is built from headers only and the raw object is served byte-for-byte. (Task 3's local check posts a Latin-1 fixture and compares stored bytes.)
3. **Two messages arriving in the same second** → two distinct ids, two raw objects, two index rows; nothing is overwritten. (Task 2 tests the id/key shapes; Task 3's local check posts twice.)
4. **A near-miss secret** (right length, one byte different, or the secret as a prefix of the path) → 404 with an empty body, no listing, no hint. (Task 2 tests `parse_route`; Task 5 smokes the deployed 404.)
5. **A message larger than 25 MiB** → rejected by Cloudflare before the worker runs, so neither the bucket nor 163 sees it. The plan records this as expected rather than defended against. (Task 6 records what the operator actually sent.)

---

### Task 1: Declare the new hostname surface (the gate drives the edit)

**Files:**
- Modify: `agent/tests/production_host.rs` (`ALLOWED` list and `MAX_ALLOWED`)
- Add to the index: `docs/superpowers/specs/2026-10-14-saisi-mail-inbox-design.md`, `docs/superpowers/plans/2026-10-14-saisi-mail-inbox.md`

**Interfaces:**
- Consumes: nothing.
- Produces: the three declared paths every later task relies on being allowed to name the host.

- [ ] **Step 1: Put the two documents in the index so the gate can see them**

The gate reads the working tree through `git ls-files`, so an untracked file is invisible to it.

```bash
git add docs/superpowers/specs/2026-10-14-saisi-mail-inbox-design.md docs/superpowers/plans/2026-10-14-saisi-mail-inbox.md
```

- [ ] **Step 2: Run the gate and watch it fail**

Run: `cd agent && cargo test -p summrise-agent --test production_host`
(`agent/` is the workspace root — there is no `Cargo.toml` at the repository root, so every agent gate runs from
there.)
Expected: FAIL — `FAIL 2 file(s) name a production host outside the declared list`, naming the two documents, plus the fix block.

- [ ] **Step 3: Declare `mail/`, the design doc and the plan; raise the ratchet**

In `ALLOWED`, beside the other worker entries (the `relay/` entry is the model — one prefix per worker directory,
and the reason travels with the path):

```rust
("mail/", "the mail worker's own config: its route pattern and the read host must name the host"),
("docs/superpowers/specs/2026-10-14-saisi-mail-inbox-design.md", "the design that records this hostname and the routing rule that serves it"),
("docs/superpowers/plans/2026-10-14-saisi-mail-inbox.md", "the plan that implements it, including the route pattern"),
```

and change `const MAX_ALLOWED: usize = 53;` to `56`, with the raise recorded in the comment block above it in the
same style the file already uses ("53 → 56 ON …, three entries, all one feature: a new worker directory and the
two documents that specify it").

- [ ] **Step 4: Run the gate and watch it pass**

Run: `cd agent && cargo test -p summrise-agent --test production_host`
Expected: PASS, and the two printed lines report `56 declared location(s)`.

- [ ] **Step 5: Commit**

```bash
git add agent/tests/production_host.rs docs/superpowers/
git commit -m "docs: design and plan for the agent's mail inbox, and the hostname declarations they need"
```

---

### Task 2: The pure decisions

**Files:**
- Create: `mail/worker/Cargo.toml`
- Create: `mail/worker/src/lib.rs`
- Create: `mail/worker/tests/pure.rs`
- Create: `mail/worker/.gitignore`

**Interfaces:**
- Consumes: Task 1's declarations.
- Produces (all of `lib.rs`, used verbatim by Task 3):
  - `pub const ID_HEX_LEN: usize = 32;`
  - `pub const DEFAULT_INDEX_LIMIT: usize = 20;`
  - `pub const MAX_INDEX_LIMIT: usize = 50;`
  - `pub enum Route { Index { limit: usize }, Message { id: String } }`
  - `pub fn parse_route(path: &str, secret: &str) -> Option<Route>`
  - `pub fn secret_eq(a: &str, b: &str) -> bool`
  - `pub fn new_id(bytes: &[u8; 16]) -> String`
  - `pub fn date_partition(iso: &str) -> String`
  - `pub fn raw_key(date: &str, id: &str) -> String`
  - `pub fn idx_key(id: &str) -> String`
  - `pub struct IndexRow { pub id: String, pub uploaded: String, pub from: String, pub to: String, pub subject: String, pub date: String, pub bytes: u64 }`
  - `pub fn index_json(id: &str, from: &str, to: &str, subject: &str, date: &str, bytes: u64) -> String`
  - `pub fn parse_index_row(json: &str, id: &str, uploaded: &str) -> Option<IndexRow>`
  - `pub fn render_index(rows: &[IndexRow]) -> String`

- [ ] **Step 1: Write the failing tests**

`mail/worker/tests/pure.rs` — one test per row of the table below, each asserting the exact values shown:

```rust
use summrise_mail_worker::{parse_route, Route, ...};

#[test] fn a_well_formed_index_path_parses_with_the_default_limit() {
    assert!(matches!(parse_route("/s/s3cret/", "s3cret"), Some(Route::Index { limit: 20 })));
}
#[test] fn a_well_formed_message_path_parses_to_its_id() { /* "/s/s3cret/m/<32 hex>" → Route::Message */ }
#[test] fn a_near_miss_secret_is_refused() { /* one byte different, a prefix of the real secret, and "" */ }
#[test] fn a_secret_of_the_right_length_but_wrong_bytes_is_refused() { /* constant-time path, same shape */ }
#[test] fn only_the_documented_limit_query_is_honoured_and_it_is_capped() { /* ?n=5 → 5; ?n=999 → 50; ?n=x → 20 */ }
#[test] fn ids_are_lowercase_hex_of_the_required_length() { /* new_id(&[0xab; 16]) == "abab…" (32 chars) */ }
#[test] fn keys_partition_by_date_and_id() {
    assert_eq!(raw_key("2026-10-14", "ab"), "mail/raw/2026-10-14/ab.eml");
    assert_eq!(idx_key("ab"), "mail/idx/ab.json");
}
#[test] fn a_raw_date_header_is_never_used_as_the_partition() { /* date_partition("2026-10-14T05:57:00Z") == "2026-10-14" */ }
#[test] fn an_index_row_round_trips_through_json() { /* index_json → parse_index_row → identical fields */ }
#[test] fn a_row_with_missing_headers_parses_to_empty_strings_rather_than_failing() { /* Review Focus 1 */ }
#[test] fn render_index_lists_newest_first_with_the_fields_the_agent_reads() { /* fixed rows → exact text */ }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --manifest-path mail/worker/Cargo.toml`
Expected: FAIL — the crate does not exist yet (`error: could not find Cargo.toml`), then `cannot find function parse_route` after Step 3's skeleton.

- [ ] **Step 3: Create the crate and implement the decisions**

`mail/worker/Cargo.toml` — modeled on `relay/worker/Cargo.toml`:

```toml
[package]
name = "summrise-mail-worker"
version = "0.1.0"
edition = "2021"
publish = false
description = "The mail worker: stores one address's inbound mail in R2 and serves it to the agent over an unguessable path. No MIME parsing — Workers Free gives 10 ms of CPU."

[lib]
crate-type = ["cdylib", "rlib"]
name = "summrise_mail_worker"
path = "src/lib.rs"

[dependencies]
worker = "0.8.7"
serde_json = { version = "1", features = ["preserve_order"] }
```

`mail/worker/.gitignore`: `build/` and `/target` (the deploy entry point is generated, exactly as `relay/worker/.gitignore` does for its own).

`src/lib.rs`: the constants and signatures above. Bodies are the implementer's; the two that carry a decision are
`secret_eq` — compare over a fixed length without an early return (`a.len() == b.len()` gates it, then fold bytes
with `|`) — and `parse_route`, which must reject `/s/<secret>` (no trailing slash), any extra segment, an id that
is not exactly `ID_HEX_LEN` lowercase hex, and any path not beginning `/s/`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --manifest-path mail/worker/Cargo.toml`
Expected: PASS, `test result: ok. N passed`.

- [ ] **Step 5: Commit**

```bash
git add mail/worker
git commit -m "feat(mail): the pure decisions — path routing, the constant-time secret, ids and index rows"
```

---

### Task 3: The two handlers and the worker config, proven locally

**Files:**
- Create: `mail/worker/src/worker.rs`
- Create: `mail/wrangler.jsonc`
- Create: `mail/.dev.vars` (gitignored)
- Modify: `.gitignore` (the root file has no `.dev.vars` rule — verified 2026-10-14)

**Interfaces:**
- Consumes: every signature from Task 2.
- Produces: the deployed contract — `GET /s/<secret>/` (index, `text/plain`), `GET /s/<secret>/m/<id>` (the raw message, `message/rfc822`), anything else `404`; `email()` writing `mail/raw/<date>/<id>.eml` + `mail/idx/<id>.json` and forwarding to `FORWARD_TO`.

- [ ] **Step 1: Write the worker config**

`mail/wrangler.jsonc`, modeled on `relay/wrangler.jsonc` (its comments record why each key is shaped this way):

```jsonc
{
  "$schema": "node_modules/wrangler/config-schema.json",
  "name": "summrise-mail",
  "main": "worker/build/index.js",
  "build": { "command": "worker-build --release worker" },
  "compatibility_date": "2026-08-01",
  "workers_dev": false,
  "r2_buckets": [{ "binding": "MAIL", "bucket_name": "summrise-mail" }],
  "routes": [{ "pattern": "mail.saisi.online/*", "zone_name": "saisi.online" }],
  "vars": {
    "RETENTION_DAYS": "7",
    "FORWARD_TO": "13122231591@163.com"
  },
  "observability": { "enabled": true }
}
```

`RETENTION_DAYS` is read by `mail/scripts/provision.sh` (Task 4) so the lifecycle age and this value cannot drift.
`FORWARD_TO` is empty in `mail/.dev.vars` so the local run exercises the store path without a real forward.

- [ ] **Step 2: Write the handlers**

`mail/worker/src/worker.rs`, `#[cfg(target_arch = "wasm32")]`, two events:

```rust
#[event(email)]
async fn email(message: ForwardableEmailMessage, env: Env, _ctx: Context) -> Result<()>

#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response>
```

The email handler's order is a decision, not a style: **forward first, then read `raw_bytes()`, then store.** A
failure after the forward still leaves the operator a copy in 163 — the only signal that survives a dead worker —
whereas storing first would leave a message whose loss is silent. `message.forward()` runs only when
`FORWARD_TO` is non-empty. Ids come from `worker::crypto::Crypto::get_random_values` into a `[u8; 16]`; the date
partition comes from `worker::Date::now()` through `date_partition`.

The fetch handler: secret from `env.var("MAIL_PATH_SECRET")`; `parse_route` or 404; index = `MAIL.list()` over
prefix `mail/idx/`, newest first by each object's `uploaded`, then one `get` per row (at most 50 subrequests, the
Free-plan ceiling); message = `MAIL.get(raw_key)` streamed into the response with **no decoding**.

- [ ] **Step 3: Install the tool this box is missing and build**

Run: `cargo install worker-build`
Run: `cd mail && wrangler dev` (the global 4.127.0 wrangler, not `npx`)
Expected: the build succeeds and the dev server listens on `http://localhost:8787`.

- [ ] **Step 4: Prove the store path locally**

With `wrangler dev` running and `MAIL_PATH_SECRET=localdev` in `mail/.dev.vars`:

```bash
curl --request POST 'http://localhost:8787/cdn-cgi/local/email' \
  --url-query 'from=operator@example.com' --url-query 'to=dsh@saisi.online' \
  --data-raw "$(cat mail/worker/tests/fixtures/latin1.eml)"
curl -s 'http://localhost:8787/s/localdev/'
```

Expected: the index lists one row with the fixture's subject and byte count; `GET /s/localdev/m/<id>` returns the
fixture's bytes unchanged. The fixture is a small RFC 5322 message with a `Message-ID`, a Latin-1 body and no
`Date` header, so this one run covers Review Focus 2 and the missing-header half of Review Focus 1. Post the same
fixture twice and expect two rows with different ids (Review Focus 3).

- [ ] **Step 5: Record the local evidence and commit**

Paste the four observed outputs (build, index line, byte comparison, second row) into the commit message.

```bash
git add mail/wrangler.jsonc mail/worker/src/worker.rs mail/worker/tests/fixtures .gitignore
git commit -m "feat(mail): the email and fetch handlers, and the worker config — proven against wrangler dev"
```

---

### Task 4: Provisioning, usage, and the lifecycle's single source

**Files:**
- Create: `mail/scripts/provision.sh`
- Create: `mail/scripts/usage.sh`
- Create: `mail/scripts/test/provision-stub.bash`

**Interfaces:**
- Consumes: `mail/wrangler.jsonc`'s `RETENTION_DAYS` var.
- Produces: `bash mail/scripts/provision.sh [--dry-run]` (creates the bucket and its lifecycle rule; idempotent) and
  `bash mail/scripts/usage.sh` (prints per-bucket objects and bytes for the account).

- [ ] **Step 1: Write the failing stub test**

`mail/scripts/test/provision-stub.bash`, in the style of `scripts/test/relay-deploy-stub.bash`: it runs
`provision.sh --dry-run` with the token unset and greps the printed plan for exactly two facts — the lifecycle
condition is `{"type":"Age","maxAge":604800}` and its `prefix` is `mail/`. Then it edits a copy of
`mail/wrangler.jsonc` with `"RETENTION_DAYS": "3"` and asserts the printed `maxAge` becomes `259200`. It also
asserts that `mail/worker/src/lib.rs` carries **no** second copy of the number (`grep -c RETENTION_DAYS` → `0`),
so the config stays the single source. Exit non-zero on any mismatch.

- [ ] **Step 2: Run it to verify it fails**

Run: `bash mail/scripts/test/provision-stub.bash`
Expected: FAIL — `provision.sh` does not exist.

- [ ] **Step 3: Implement both scripts**

`provision.sh`: reads `${CLOUDFLARE_API_TOKEN:-$(cat "$HOME/.cloudflare-token")}` through the same shape
`scripts/lib/release-lib.sh`'s `cf_token` uses; `--dry-run` prints the calls it would make and exits 0; the real
path GETs the bucket first (an existing bucket is success, not an error), PUTs the lifecycle rule, and re-reads it
so the run's output IS the proof. `usage.sh` runs the `r2StorageAdaptiveGroups` GraphQL query and prints one line
per bucket (`name objects bytes`) plus a total, so "how full are we" is a command.

- [ ] **Step 4: Run the stub test to verify it passes**

Run: `bash mail/scripts/test/provision-stub.bash`
Expected: PASS (`provision-stub: ok`).

- [ ] **Step 5: Commit**

```bash
git add mail/scripts
git commit -m "feat(mail): idempotent bucket + lifecycle provisioning, and a usage read, with the retention age sourced from the worker's own var"
```

---

### Task 5: The deploy path, CI, and the runbook

**Files:**
- Modify: `scripts/build.sh` (add `mail) deploy_mail ;;` to the dispatch and a `deploy_mail()` beside `deploy_relay()`)
- Modify: `.github/workflows/ci.yml` (the `proxies` job)
- Create: `mail/scripts/route.sh`
- Create: `mail/README.md`
- Modify: `AGENTS.md` (one pointer line, under the existing structure)
- Modify: `docs/superpowers/plans/2026-10-14-saisi-mail-inbox.md` (tick the boxes; add the live numbers in Task 6)

**Interfaces:**
- Consumes: Tasks 3 and 4.
- Produces: `./scripts/build.sh mail` (deploy + smoke), the CI steps, and `bash mail/scripts/route.sh` (the literal Email Routing rule + the placeholder DNS record for the read host, both idempotent).

- [ ] **Step 1: Add the deploy branch**

`deploy_mail()` copies `deploy_relay()`'s shape: `require_cf_token`, the global `wrangler deploy` with
`CLOUDFLARE_API_TOKEN="$CF_TOKEN"` in front, then a smoke that needs no secret —
`curl -s -o /dev/null -w '%{http_code}'` against `https://mail.saisi.online/s/deadbeefdeadbeef/` must print
**404**, which proves both that the route reached this worker and that the path guard is on. The dispatch gains
`mail)      deploy_mail ;;` and the `deploy)` arm lists it.

- [ ] **Step 2: Wire CI**

In the `proxies` job, beside the relay's steps:

```yaml
      - name: The mail worker's decisions (Rust)
        run: cargo test --manifest-path mail/worker/Cargo.toml
      - name: The mail worker's entry point, for the target it serves
        run: cargo clippy --manifest-path mail/worker/Cargo.toml --target wasm32-unknown-unknown -- -D warnings
      - name: Dry-run the mail worker
        run: wrangler deploy --dry-run
        working-directory: mail
```

and add `bash mail/scripts/test/provision-stub.bash` to the job's script-test steps.

- [ ] **Step 3: Write the route script and the runbook**

`route.sh` creates the proxied placeholder record for the read host (an `A` record to `192.0.2.1`, proxied — the
Cloudflare-documented shape for a Worker route on a custom hostname) and the Email Routing rule
(`matcher: {type: literal, field: to, value: dsh@saisi.online}` → `action: {type: worker, value: ["summrise-mail"]}`,
`enabled: true`, `priority` 0 so it precedes the catch-all), then re-reads both and prints them. `mail/README.md`
is the runbook: the URL contract, where the secret lives, the deploy command, the probes, and the rollback (delete
the one rule; the catch-all is untouched).

- [ ] **Step 4: Verify the whole gate set locally**

Run: `cargo test --manifest-path mail/worker/Cargo.toml && cargo clippy --manifest-path mail/worker/Cargo.toml --target wasm32-unknown-unknown -- -D warnings`
Run: `bash mail/scripts/test/provision-stub.bash`
Run: `cd agent && cargo test -p summrise-agent --test production_host && cargo test -p summrise-agent --test worker_build_commands && cargo test -p summrise-agent --test worker_language && cargo test -p summrise-agent --test docs_budget`
Expected: every command exits 0. The four agent gates are the ones a new worker directory can break.

- [ ] **Step 5: Commit**

```bash
git add scripts/build.sh .github/workflows/ci.yml mail/scripts/route.sh mail/README.md AGENTS.md
git commit -m "feat(mail): deploy branch, CI steps, the routing script and the runbook"
```

---

### Task 6: Bring it up live, then measure (operator hand-off)

**Files:**
- Modify: `mail/README.md` (the observed numbers), `docs/superpowers/plans/2026-10-14-saisi-mail-inbox.md` (the ticks)

**Interfaces:**
- Consumes: everything above, and the operator's two messages.
- Produces: a live address, and the measurement that decides Task 7.

- [ ] **Step 1: Provision and deploy**

Run: `bash mail/scripts/provision.sh` (expect the re-read lifecycle rule in its output)
Run: `bash mail/scripts/route.sh`
Run: `./scripts/build.sh mail` → expect the 404 smoke line and no error.
Run: `printf '%s' "$(openssl rand -hex 24)" | wrangler secret put MAIL_PATH_SECRET --name summrise-mail`, then write
the same value to `~/.saisi-mail-secret` (mode 600) so the agent can read it without it entering a command line.

- [ ] **Step 2: Ask the operator for the two messages**

One plain-text message and one carrying an attachment of about **17 MiB** (≈24 MiB once base64-encoded — the
largest that clears Cloudflare's 25 MiB inbound limit; a 20 MiB attachment is rejected before the worker runs).
Keep a worker log tail open: `wrangler tail summrise-mail --format pretty`.

- [ ] **Step 3: Read the results through the same path the agent will use**

```bash
SECRET="$(cat ~/.saisi-mail-secret)"
curl -s "https://mail.saisi.online/s/${SECRET}/"
curl -s -o /tmp/probe.eml "https://mail.saisi.online/s/${SECRET}/m/<id>" && wc -c /tmp/probe.eml
```

Expected: both messages listed; the large one's byte count equal to what the operator sent (plus headers); the
copy present in 163. Record the numbers in `mail/README.md`.

- [ ] **Step 4: Decide Task 7 from the log, not from hope**

Look for `exceededCpu` / `Error 1102` in the tail during the large message. If it appears, Task 7 is required.
If it does not, Task 7 is dropped and the commit message says which evidence dropped it.

- [ ] **Step 5: Commit the evidence**

```bash
git add mail/README.md docs/superpowers/plans/2026-10-14-saisi-mail-inbox.md
git commit -m "feat(mail): live on dsh@…, with the measured size, CPU and copy results"
```

---

### Task 7 (only if Task 6 saw `exceededCpu`): stream the put instead of buffering

**Files:**
- Modify: `mail/worker/src/worker.rs`

**Interfaces:**
- Consumes: Task 3's handler.
- Produces: the same store contract, with the raw body never held in the isolate.

- [ ] **Step 1: Reproduce the failure locally** — post a ≈24 MiB fixture to `wrangler dev` and confirm the same resource error appears. If it cannot be reproduced locally, stop: the shipped design stays buffered and the plan records why.
- [ ] **Step 2: Hand the raw stream to R2** — `R2Bucket::put` accepts a `web_sys::ReadableStream` (verified in `worker` 0.8.7's source), so pass `message.raw()` through instead of `raw_bytes()`, keeping the index row's `bytes` from `message.raw_size()`.
- [ ] **Step 3: Re-run Task 6's Step 3 and Step 4** — same two messages, same log check, and the recorded result decides whether the change ships.
- [ ] **Step 4: Commit** — with both measurements (before/after) in the message.

---

## Self-review

- **Spec coverage:** the dedicated bucket + `mail/` lifecycle (Task 4), the one routing rule and the untouched catch-all (Task 5), the 404-guarded read path (Tasks 3, 5), the raw-store/no-parse split (Task 3), no write cap (nothing implements one), the 163 copy (Task 3), the CPU probe and its fallback (Tasks 6, 7), the ratchet and the CI steps (Tasks 1, 5). The spec's "where a future session learns the URL contract" is `mail/README.md` + the `AGENTS.md` pointer in Task 5.
- **Type consistency:** every name in Task 3 comes from Task 2's Interfaces block; no later task renames one.
- **Review Focus:** rows 1–3 are pinned in Task 2's tests and Task 3's local run; row 4 in Task 2's `parse_route` tests and Task 5's smoke; row 5 is recorded, not defended.
