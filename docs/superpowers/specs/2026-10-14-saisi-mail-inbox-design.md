# The operator's mail address, readable by the agent — design

**2026-10-14 · status: proposed, awaiting the operator's approval before any code is written.**

## Intent

The operator wants one address — `dsh@saisi.online` — whose mail the AI agent (DSH) can read. The agent's only
inbound channel is HTTP: it cannot log into a mailbox and it has no browser session. So the mail has to become a
URL the agent can fetch, and the agent has to be able to say what arrived, including what an attachment contains.

The stated usage, in the operator's words: *"你改好后我让你查一下 summrise-mail 是不是你就能知道收到了什么邮件"* —
the operator sends mail (with attachments) to that address and then asks the agent to look.

Today nothing that arrives is reachable this way.

## What was decided, and by whom

| Decision | Value | Who |
|---|---|---|
| Address scope | ONE rule, `dsh@saisi.online` → worker. The existing catch-all keeps sending everything else to 163, untouched. | operator |
| Retention | **7 days.** The operator first said one day, then "30 天也可以", then decided against any write cap ("也不设置限制吧，即使清理就行"), which makes the retention window the ONLY bound; 7 days prices the worst case at ≈$0.90/month and still lets a message sent days ago be found. | operator (approved 2026-10-14) |
| Write cap | **None.** Nothing refuses mail. | operator (approved 2026-10-14) |
| Storage location | A NEW dedicated R2 bucket, not a prefix inside an existing one. | this design |
| Read-path protection | An unguessable path segment, **not** Cloudflare Access — Access would lock the agent out. The operator was told this is the trade. | this design |
| Where MIME is parsed | On the operator's Linux box, never in the Worker. | this design, forced by measurement (below) |
| Read hostname | `mail.saisi.online`, as a **Route** (`mail.saisi.online/*`) declared in `wrangler.jsonc` over a proxied placeholder record — the shape `relay/` already proves — rather than a dashboard-managed Custom Domain. | this design |
| Who sends the test mail | **The operator, from 163**, two messages (one plain text, one ≈17 MiB attachment) while the agent tails the worker's log. Sending from this box via postfix was offered and not taken. | operator (approved 2026-10-14) |
| A copy to 163 | **Yes.** Cloudflare routes one pattern to ONE destination, so the handler itself calls `message.forward("13122231591@163.com")` — the only way a message the handler drops becomes visible to the operator, since the read side cannot tell "nothing arrived" from "it bounced". | operator (approved 2026-10-14) |

## Measured facts this design rests on (2026-10-14, from this box)

- Zone `saisi.online` = `79860b60a8681147d0ce38294d404ead`, account = `8e9ea6cb01f2336a2f00039a51f96c6d`.
- **Email Routing is already enabled** (created 2026-09-02, `status: ready`). Exactly ONE rule exists: the
  **catch-all** (`matcher: all`, priority 2147483647) → `forward ["13122231591@163.com"]`, the account's only
  verified destination. There are no per-address rules.
- **Plans:** the zone is on Cloudflare Free; the account has **no Workers Paid subscription** → Workers Free.
  `R2 Paid` is subscribed at $0/month (R2 is usage-billed, not plan-gated).
- **Workers Free limits:** **CPU 10 ms per request**, 100 000 requests/day, 64 vars/worker, 128 MB memory,
  50 subrequests/request. The docs add that "heavy workloads that … parse large payloads typically use 10–20 ms".
- **Email Routing limits:** inbound message ≤ **25 MiB**; 200 routing rules/domain; 200 verified destinations.
- **R2:** free 10 GB-month storage, 1M Class A ops, 10M Class B ops, egress free. Overage $0.015/GB-month
  (Standard), rounded up to the next billing unit. `DeleteObject` is free. Object lifecycles are **bucket-level**,
  delete-by-age, and Cloudflare states objects are "typically removed within 24 hours of the `x-amz-expiration`
  value". Managing them needs the `Workers R2 Storage Write` permission group.
- **R2 usage today**, all four buckets (GraphQL, `r2StorageAdaptiveGroups`): `summrise-temp-files` 147.4 MB /
  2 objects · `vale-temp-files` 31.4 MB / 1 · `summrise-relay-files` 6.9 MB / 3 · `vale-releases` 0 —
  **185.7 MB total, 1.9 % of the free tier.**
- **The repo already runs exactly this shape of worker:** `relay/` — workers-rs 0.8.7, built by
  `worker-build --release worker`, `main: worker/build/index.js`, with `build.command` in `wrangler.jsonc`
  because `agent/tests/worker_build_commands.rs` requires an untracked `main` to carry one.
- **`worker` 0.8.7 supports the email event**: `worker-macros` 0.8.7 expands `#[event(email)]` into an exported
  `email(message: ForwardableEmailMessage, env, ctx)` wrapper, and `ForwardableEmailMessage::raw_bytes()` exists
  (read in the local cargo registry, not assumed).
- **The account token WRITES Email Routing rules — probed, not assumed.** On 2026-10-14 a rule with matcher
  `literal probe-<random>@saisi.online`, `enabled: false` was created (`success: true`), read back, and deleted;
  the zone's final state is again exactly ONE rule, the original catch-all. A disabled rule routes nothing, and
  the probe's matcher cannot match real mail, so the probe was inert by construction. **The rule is an API call,
  not a dashboard click.** Its three sibling writes (bucket, lifecycle, hostname/route) rest on permissions the
  same token already exercises for reads plus `#dns_records:edit`/`#worker:edit`; `wrangler secret put` rides
  `#worker:edit` on the deployed script. The one write with no probe yet is the bucket creation itself, which
  waits for the operator's approval.
- **This box's toolchain, measured:** `worker-build` is **NOT installed** (`worker-build` absent; `cargo install`
  costs minutes and disk on the HDD the repo names as its constraint), and the installed wrangler is **4.147.0**
  while `ci.yml`/`build.sh` pin **4.127.0** — a local `--dry-run` does not exercise the pinned tool. `postfix`
  is active and listening on `0.0.0.0:25`, and TCP/25 egress to `route1.mx.cloudflare.net` connects. The zone has
  **no DMARC record** and its SPF is `~all` (softfail), so mail sent from this box's residential IP is
  deliverability-unknown.
- **`mail.saisi.online` has no DNS record today** (no A/AAAA, and no wildcard on the zone), so the read host
  needs a record plus a worker attachment.
- **Both fallbacks for the CPU risk are implementable in this stack**: `R2Bucket::put` accepts a
  `web_sys::ReadableStream`, so the raw message can be streamed into R2 without buffering it, and truncation is
  the cruder alternative. Verified in the local `worker` 0.8.7 source.
- **The "declared place" for production hostnames EXISTS**: `agent/tests/production_host.rs`, with its
  `MAX_ALLOWED` ratchet (53 today) and `relay/` as the precedent entry for a worker directory.
- The repo's own retention precedent: bucket `vale-temp-files` already carries rule `vale-temp-24h-expiry`
  (`conditions.prefix = "files/"`, `deleteObjectsTransition.condition = {type: Age, maxAge: 86400}`).

## The constraint that shapes everything: 10 ms of CPU

Workers Free gives 10 ms of CPU per request, and the Email Routing docs warn that a Worker handling mail may
exceed it (`EXCEEDED_CPU`). Parsing a multi-megabyte MIME message costs far more than 10 ms. Cloudflare also
states that "complex handlers may exceed these limits and fail to process a message".

**Therefore the Worker does not parse anything.** It is a dumb, bounded store: it copies the raw message into R2
and writes an index record from values the runtime hands it for free (`message.from`, `message.to`,
`message.headers`, `message.rawSize`). Every expensive step — MIME structure, `text/plain` extraction, base64
decoding, attachment names — happens on the operator's box, which has `python3` (stdlib `email`), `pdftotext`,
`libreoffice`, `unzip` and no CPU limit at all.

This is a correction to what was said earlier in the conversation ("the link opens as plain text"): the link
serves the raw message, and the agent renders it locally.

## The flow

```
sender
  → route1/2/3.mx.cloudflare.net (the zone's existing MX)
  → Email Routing rule: literal dsh@saisi.online → worker summrise-mail
  → worker :: email()
        raw bytes   → R2  mail/raw/<id>.eml        (one byte unchanged, attachments included)
        index json  → R2  mail/idx/<id>.json       (from/to/subject/date/message-id/rawSize only)
        copy        → message.forward("13122231591@163.com")   (so a drop is visible in 163)
  → operator: "查一下 summrise-mail"
  → agent:  curl https://mail.saisi.online/s/<secret>/          → index, newest first
            curl https://mail.saisi.online/s/<secret>/m/<id>    → the raw .eml to /tmp
            python3 -c '…email…' → body + attachment list
            pdftotext / libreoffice / read_image for the attachments; delete the temp copy
  → agent reports: who, when, subject, body, attachments
```

## The storage bound: retention, and nothing else

**The operator's decision (2026-10-14): no write cap.** Nothing refuses mail; the only lever is how long a
message lives. That is a defensible call under R2's billing model, which is worth writing down because it is
what makes it defensible:

- Storage is billed as **GB-month, the average of the daily peak**. A single day's flood of 10 GB is therefore
  `10 × 1/30 ≈ 0.33 GB-month` — inside the free tier by itself. **What costs money is occupancy over time**,
  which is exactly what the retention window controls. A per-day write budget would have throttled the part
  that is nearly free.
- Per message the ceiling is Cloudflare's own inbound limit, **25 MiB**, so **no attachment is ever truncated**.

| Retention | Steady state under a sustained 10 GB/day flood | Beyond the 10 GB free tier | Monthly cost |
|---|---|---|---|
| 30 days | 300 GB | 290 GB | ≈ $4.35 |
| **7 days (proposed)** | 70 GB | 60 GB | ≈ $0.90 |
| 1 day | 10 GB | 0 | $0 |

Normal use — the operator sends a few messages with attachments over a week — is **single-digit megabytes**, and
the account sits at 185.7 MB today.

**The retained decision, in the operator's words: "不该删的别删."** Two kinds of "should not be deleted", and
they have different answers:

1. **Anything that is not this bucket's mail.** Answered by scope, and proven rather than promised — see risk 4.
2. **A message the operator wants to keep forever.** Scope cannot answer this one: when the window closes, R2
   deletes it, and there is no undelete. Either it is accepted (tell the agent while the message is still there
   and it keeps a copy outside R2), or the design grows a `keep/` prefix that no lifecycle rule matches, fed by a
   sender allowlist in the worker's vars. **This is an open question for the operator.**

**What this design cannot bound**: the other buckets. Measured 2026-10-14, the account held 185.7 MB — of which
178.8 MB is the installer's component cache (`electron-win32-x64.zip` 115.0 MB, `summrise-playwright.zip`
32.4 MB, `vale-playwright.zip` 31.4 MB). That cache is **legitimate, not garbage, and must NOT be given a
blanket expiry rule**: it is what installs and updates fetch. (Side finding: `vale-temp-files`' existing 24 h
rule is scoped to prefix `files/`, so `vale-playwright.zip` was never touched by it — the rule is fine, it
simply does not match that key.) So the honest claim is: **the mail bucket's size is `one retention window of
inbound mail`, in the tens of megabytes under normal use; "the account stays under 10 GB" is an observation,
not a guarantee** — and the observation is available on demand (`mail/scripts/usage.sh`).

## The pieces

1. **`mail/worker` — a Rust crate** (workers-rs 0.8.7), split like `relay/worker`:
   `src/lib.rs` holds the pure, host-testable decisions (id minting, path parsing, constant-time secret compare,
   index-record rendering, the size/ceiling guard); `src/worker.rs` is `#[cfg(target_arch = "wasm32")]` and holds
   `#[event(email)]` + `#[event(fetch)]`. The four modules the gate needs: `lib` is a plain host build that
   `cargo test` runs, `worker.rs` is checked by `cargo clippy --target wasm32-unknown-unknown`.
2. **`mail/wrangler.jsonc`** — `name: summrise-mail`, `main: worker/build/index.js`,
   `build.command: worker-build --release worker`, `r2_buckets` → `summrise-mail`, a **route**
   `mail.saisi.online/*` (zone_name `saisi.online`) over a proxied placeholder DNS record, vars for the retention
   constant, secret `MAIL_PATH_SECRET`. Building locally needs `cargo install worker-build` (absent on this box),
   and a deploy must run the pinned wrangler (4.127.0), not the 4.147.0 installed here.
3. **Bucket `summrise-mail`** with one lifecycle rule: prefix `mail/`, `maxAge` = the retention window
   (**proposed: 7 days = 604800 s**). This is the only bound on the bucket's size, and it does not depend on any
   code of ours being correct or running.
4. **One Email Routing rule** — literal matcher `dsh@saisi.online`, action `worker: summrise-mail`, enabled,
   priority below the catch-all. Created through the API by a documented, idempotent script; if the token can only
   read, the same call is one dashboard action and the script says so.
5. **The deploy path** — a branch in `scripts/build.sh` beside `deploy_worker`/`deploy_proxy`, and **two CI steps
   in the EXISTING `proxies` job** (it already carries the relay's `cargo test` + wasm32 clippy, the wasm target
   and the pinned wrangler): `cargo test --manifest-path mail/worker/Cargo.toml` and
   `cargo clippy --manifest-path mail/worker/Cargo.toml --target wasm32-unknown-unknown -- -D warnings`.
   `scripts/test/all-gates.bash` derives them from `ci.yml` automatically. A deploy smoke needs **no secret**:
   `GET /s/<wrong-secret>/` must answer 404, which proves both that the worker is live and that the path guard is
   on.
6. **The secret** — `wrangler secret put MAIL_PATH_SECRET` into the worker, and the same value written to
   `~/.saisi-mail-secret` (`600`) on this box so the agent can read it without it ever appearing in a command
   line or a transcript (`~/.cloudflare-token` is the existing precedent for this pattern).
7. **`mail/scripts/usage.sh`** — the per-bucket usage read (`r2StorageAdaptiveGroups` over the account's buckets,
   the same query that measured every number in this document), so "how full are we" is a command rather than a
   guess.

## URL surface

| Route | Answer |
|---|---|
| `GET /s/<secret>/` | `text/plain` index, newest first: `time · from · subject · bytes · id`. Capped (e.g. `?n=20`). |
| `GET /s/<secret>/m/<id>` | The raw message, `message/rfc822`. Pure R2 streaming — no parsing, no CPU. |
| anything else | `404`, empty body, no hint that the worker exists. |

`GET` only. A wrong secret answers `404`, not `403`, and the compare is constant-time. No directory listing of R2,
no index without the secret. The id is 128 bits of randomness, so a link cannot be guessed even if one leaks.

**An empty index is ambiguous, and that is a design gap, not an edge case.** The read path is pull-only, so
"nothing arrived" and "your message was rejected by a handler that died" produce the same answer from here. Two
answers exist: the agent can read the worker's logs before reporting nothing (Workers Logs record `EXCEEDED_CPU`),
and/or the handler forwards a copy to 163, so a dropped message still lands where the operator will see it. The
second is an open item; the first adds a step to the read flow and the Cloudflare token to what the agent needs.

## Non-goals

- No sending on the operator's behalf and no replying. **One exception, decided:** every message is forwarded to
  `13122231591@163.com` (the account's only verified destination) so a dropped message is visible in the
  operator's own inbox — Cloudflare routes one pattern to ONE destination, so only the handler can do this.
- No MIME parsing, no HTML rendering, no search, no web UI in the Worker.
- No push or notification to the agent: it looks when the operator asks. That is the stated usage.
- Not a product feature of Summrise — it is operator infrastructure for one address, and the charter's
  cost/blast-radius table applies.

## Anti-goals — what would make this a failure

- **Any change to where other addresses' mail goes.** The catch-all rule must be byte-identical after the change;
  only a new, more specific rule is added.
- **Deleting something that should not be deleted.** The lifecycle rule touches one new bucket, on prefix
  `mail/`, and no other bucket's configuration is modified at all (risk 4 is the proof).
- **A read path that dies on a large attachment.** Hence the CPU probe below, and a size policy that is chosen
  after the measurement rather than before it.

## Risks, and the probe that settles each

1. **Workers Free's 10 ms CPU against a real attachment.** The probe must be written in **raw message bytes**,
   not attachment bytes: a 20 MiB binary attachment base64-encodes to ≈27 MiB and is rejected at Cloudflare's
   25 MiB inbound cap *before the worker runs*. The largest testable message is **≈24 MiB raw ≈ 17 MiB
   attachment**, and the plan should record which was actually used. Probe: one message at that size, with the
   worker's log tail open, watching for `exceededCpu` / `Error 1102`. The preferred fallback is **streaming the
   R2 put** (`R2Bucket::put` accepts a `ReadableStream`, so the message never has to be buffered in the isolate);
   truncation-with-a-marker is the cruder second choice. Which one ships is decided by that measurement.
2. ~~Whether the token may write Email Routing rules.~~ **ANSWERED 2026-10-14: it can** (probe recorded under
   "Measured facts"). What remains for the plan is ordering, not authority: bucket → deploy → secret → rule.
3. **R2 overage.** With no write cap, the only bound is `retention × inbound rate`; the table above prices the
   worst case (a sustained 10 GB/day flood: ≈$0.90/month at 7 days). Normal use is single-digit megabytes. A
   billing notification (threshold alert) is the operator's own backstop and is worth turning on.
4. **The lifecycle rule deleting something that should not be deleted ("不该删的别删").** Answered by scope, and
   the proof is a diff rather than a promise:
   - the rule lives on a **new, dedicated bucket** that contains nothing but mail, so the blast radius is a
     bucket that did not exist before this change;
   - the rule's condition is **prefix `mail/`**, so anything later placed in that bucket outside `mail/`
     (config, a kept message, a log) is provably outside its reach;
   - **the other four buckets are not touched.** Their lifecycle configuration was read BEFORE the change
     (2026-10-14: all four carry only R2's default multipart-abort rule, except `vale-temp-files` with
     `vale-temp-24h-expiry` on prefix `files/`), and the probe is to read all four again afterwards and require
     a byte-identical diff. In particular the installer's component cache gets no rule: it is what installs and
     updates fetch;
   - the case scope cannot fix is content: a message the operator wants to keep is still deleted when the window
     closes, and R2 has no undelete. That is the `keep/` question in "Open items".

## Open items for the operator

- **Where a future session learns the URL contract.** The secret lives in `~/.saisi-mail-secret`; the contract
  (path shape, index fields) needs a home. `AGENTS.md` has headroom under `agent/tests/docs_budget.rs` and is
  already a declared production-host location, so a short pointer there is the cheapest option — noted because
  the design otherwise calls this operator infrastructure rather than a product feature.

Everything else this document once listed here is settled above: retention is **7 days**, there is **no write
cap**, **no `keep/` prefix** (a message worth keeping is kept by telling the agent while it is still there), the
test messages come **from the operator's 163**, and every message is **copied to 163** by the handler.

## Settled while checking (recorded so the plan inherits them)

- **`production_host.rs` is the declared place** (`ideas.md` row 23 said it did not exist — wrong). Adding
  `mail/` to its `ALLOWED` list means raising `MAX_ALLOWED` 53 → 54 **in the same commit**, because the ratchet
  fails in both directions; `relay/` is the precedent entry. `docs/` is not declared wholesale, so a new runbook
  that spells the hostname costs its own entry plus another bump — the cheap shape is to spell the host only in
  the wrangler config and the build smoke.
- **Newest-first needs no time-sortable id**: R2's list returns `uploaded` per object, so the 128-bit random id
  stays.
- **The crate sits outside the agent workspace**, like `relay/worker`; `scripts/clean-build-space.bash` derives
  its target directories from tracked `Cargo.toml` files, so it needs no edit.

## Sizing

One new crate (two files plus tests), one wrangler config, one bucket plus one lifecycle rule, one routing rule,
two CI steps, one deploy branch. **Cost delta: $0/month at the measured volume** — inside the charter's ≤$5
"decide alone" band, but the operator's approval is what this document waits for.
