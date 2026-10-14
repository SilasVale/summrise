# The mail worker

One address's inbound mail, stored in R2 and served to the agent over an unguessable path.

**Design:** `docs/superpowers/specs/2026-10-14-saisi-mail-inbox-design.md` · **Plan:**
`docs/superpowers/plans/2026-10-14-saisi-mail-inbox.md`

## The URL contract

| Route | Answer |
|---|---|
| `GET /s/<secret>/` | `text/plain` index, newest first: `uploaded · from · subject · bytes · id`. `?n=` caps the count (default 20, hard cap 40 — one listing plus one `get` per row has to stay under the Free plan's 50 subrequests). |
| `GET /s/<secret>/m/<id>` | The message's original bytes, `message/rfc822`. |
| anything else, including a wrong secret | `404`, empty body. |

The secret is the whole protection: **no Cloudflare Access** sits in front of this, because the agent has no
browser session. It lives in the worker as the secret `MAIL_PATH_SECRET` and is mirrored on this box at
`~/.saisi-mail-secret` (mode `600`). Read it into a variable rather than onto a command line:

```bash
SECRET="$(cat ~/.saisi-mail-secret)"
curl -s "https://mail.saisi.online/s/${SECRET}/"
```

## What happens to a message

```
sender → route1/2/3.mx.cloudflare.net (the zone's MX)
       → the ONE Email Routing rule: literal dsh@… → worker summrise-mail
       → email():  forward a copy to 163, then stream the raw bytes to R2, then write the index row
       → agent:    the index, then the raw message, parsed on the agent's own box
```

- **Forward first, store second.** Cloudflare routes one pattern to ONE destination, so this worker is the only
  thing between the sender and the 163 copy; if the store fails afterwards, the operator still has the message.
- **Nothing is parsed here.** Workers Free allows 10 ms of CPU per request; MIME parsing happens on the box that
  reads the mail (`python3 -c 'import email…'`, `pdftotext`, `libreoffice`).
- **The message is streamed, never buffered** — through a `FixedLengthStream`, because `R2Bucket::put` refuses a
  stream whose length it does not know (`rawSize` gives us the length).

## Retention, and the one bound on storage

The bucket `summrise-mail` carries ONE lifecycle rule: prefix `mail/`, delete after **7 days**. The number has a
single source, `RETENTION_DAYS` in `wrangler.jsonc`; `mail/scripts/provision.sh` reads it and
`mail/scripts/test/provision-stub.bash` fails if the two ever disagree. Nothing else expires, and the other
buckets in the account are not touched by anything here — in particular the installer's component cache, which
must NOT be given an expiry rule because installs and updates fetch from it.

## Operating it

```bash
bash mail/scripts/provision.sh --dry-run   # print the bucket + lifecycle plan
bash mail/scripts/provision.sh             # create the bucket if absent, put + re-read the rule
bash mail/scripts/route.sh --dry-run       # print the DNS + routing plan
bash mail/scripts/route.sh                 # create what is missing, re-read every rule in the zone
./scripts/build.sh mail                    # deploy + the 404 smoke (a wrong secret must answer 404)
bash mail/scripts/usage.sh                 # per-bucket objects and bytes, and the total against the 10 GB tier
cargo test --manifest-path mail/worker/Cargo.toml
cargo clippy --manifest-path mail/worker/Cargo.toml --target wasm32-unknown-unknown -- -D warnings
```

Local end to end (no Cloudflare, no token): `cd mail && wrangler dev`, with `MAIL_PATH_SECRET` and an empty
`FORWARD_TO` in `mail/.dev.vars`, then POST a raw RFC 5322 message (it must carry a `Message-ID`) to the dev
server's own email endpoint:

```bash
curl -s --request POST 'http://localhost:8787/cdn-cgi/local/email' \
  --url-query 'from=operator@example.com' --url-query 'to=dsh@saisi.online' \
  --data-binary @mail/worker/tests/fixtures/latin1.eml
curl -s 'http://localhost:8787/s/localdev/'            # MAIL_PATH_SECRET=localdev in .dev.vars
```

## Rollback

Delete the one Email Routing rule for `dsh@…` (the catch-all is untouched and keeps serving every other
address), then delete the worker and, if wanted, the bucket. `route.sh` prints every rule in the zone after it
runs, which is how "the catch-all was not touched" is checked rather than asserted.

## Files

| Path | What it is |
|---|---|
| `wrangler.jsonc` | the worker: its route, its R2 binding, `RETENTION_DAYS`, `FORWARD_TO` |
| `worker/src/lib.rs` | every decision, host-testable: path routing, the constant-time secret, ids, keys, index rows |
| `worker/src/worker.rs` | the two events (`email`, `fetch`) — wasm only, I/O only |
| `worker/tests/pure.rs` | the decisions under test, including the near-miss and empty-secret cases |
| `worker/tests/fixtures/latin1.eml` | a message with an 8-bit body and no `Date:` header, for the local run |
| `scripts/provision.sh` | the bucket and its lifecycle rule |
| `scripts/route.sh` | the read host's DNS record and the one Email Routing rule |
| `scripts/usage.sh` | how full the account's buckets are |
