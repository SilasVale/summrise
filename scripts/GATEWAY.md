# THE GATEWAY CUTOVER — moving the console's hostnames onto the Rust worker

**STATUS: READY, NOT EXECUTED.** Every route the console serves is `gateway/wasm/src`'s (see the tail of
`git log --oneline -- gateway/src`), the front door's own gates run in the worker (`lib.rs`'s `front_gates`),
host isolation and the static page do too, and `verify.mjs` compares the worker against the shipping TypeScript
byte for byte on every corpus section. What remains is a DEPLOY — and a deploy is the operator's call, not a
commit's.

This file exists so that the deploy is a checklist rather than a memory. The traps in it were each measured;
where a number appears it came from Cloudflare's own analytics or from a worker's logged env.

## THE TOPOLOGY TODAY

| worker | script | owns | serves |
|---|---|---|---|
| console | `summrise-gate` (`gateway/wrangler.jsonc`) | `api.saisi.online`, `ai.saisi.online`, the DO classes `BreakerDO`/`RouteDO`, the `ASSETS` upload | the static page, and hands `/api/*`, `/mcp`, `/v1/*` to the worker below through the `WASM_GATE` binding |
| API | `vale-gate-wasm` (`gateway/wasm/wrangler.jsonc`) | nothing (`workers_dev: false`, no routes) | every `WASM_GATE` call: the whole API, `/v1` included |

**`vale-gate` (the pre-cutover TypeScript, no `summrise` prefix) is still deployed and is NOT cleanup**: it is
the DO owner the console worker binds by `script_name`, and a Durable Object's storage is keyed by the script
that defines its class — a new owner starts EMPTY (`RouteDO` holds `route:{uid}`, a user's chosen model). It is
also the rollback.

## THE ORDER, AND WHY IT IS THE ORDER

**1. SECRETS AND VARS BEFORE HOSTNAMES.** The measured gap: `vale-gate-wasm` is missing four channel secrets
(`AMD_API_KEY`, `CMD_API_KEY`, `GMI_API_KEY`, `R4_API_KEY`), so those four prefixes answer
`502 config_error: <NAME> not configured` to a user who has no key of their own — while `/api/health` reports
every channel `ok: true`, because it reports whether the MODELS answer, not whether the KEYS are present.

```
for k in AMD_API_KEY CMD_API_KEY GMI_API_KEY R4_API_KEY; do
  wrangler secret put "$k" --name vale-gate-wasm
done
```

**2. THE VARS THE FRONT DOOR'S GATES READ**, mirrored from `gateway/wrangler.jsonc`: `CONSOLE_HOST` (the
host isolation — without it EVERY host is a non-console host and the console answers 404 to itself),
`CONSOLE_ORIGINS` (the CORS allowlist; origins carry a scheme), `DO_AUTH` (the Durable Object shared secret),
`DEVICE_HOST_SUFFIX`, and the KV namespace the console's data lives in. **A BINDING THAT IS ABSENT IS NOT A
MISSING FEATURE, IT IS A 404 OR A 500**: `ASSETS` absent means no page, `KEYS` absent means no data.

**3. DEPLOY THE WORKER WITH THE CUSTOM DOMAINS.** Add the two hostnames to `gateway/wasm/wrangler.jsonc`'s
`routes` (as custom domains, `"custom_domain": true`) — or repoint them in the dashboard — and deploy. The
worker's `workers_dev: false` stays: nothing should reach it by a `*.workers.dev` name.

**4. VERIFY, IN THIS ORDER — EACH STEP PROVES A DIFFERENT LAYER:**

```
curl -sS https://api.saisi.online/api/health | head -c 200      # the worker answers at all
curl -sS -o /dev/null -w '%{http_code}\n' https://ai.saisi.online/   # the PAGE arm: 200 + text/html
curl -sS -H "Authorization: Bearer <token>" https://api.saisi.online/v1/models | head -c 200
curl -sS -o /dev/null -w '%{http_code}\n' -X POST https://api.saisi.online/api/auth/login \
  -H 'sec-fetch-site: cross-site' -H 'cookie: ag_session=x' -H 'content-type: application/json' -d '{}'   # 403
```

The last one is the CSRF gate, and it is the one worth running by hand: it is the only step whose failure is a
SECURITY answer rather than a broken page.

## THE ROLLBACK, AND IT IS ONE API CALL

Move the two custom domains back to `summrise-gate`. **No redeploy of either worker is needed**: the console
worker's deployed code is unchanged by this cutover (it keeps serving whenever a request reaches it), and the
Rust worker answers the binding exactly as it does today. Data is untouched — both read the same KV namespace
and write the same Durable Objects.

## WHAT THIS CUTOVER DOES *NOT* DELETE

`gateway/src/index.ts` stays, and so does the console worker: it is the DO host (a Durable Object class cannot
be defined in Rust) and the rollback. Deleting it is a SEPARATE decision, and it needs the DO classes moved to
a script that owns them — a scripted step (read `route:{uid}` through `RouteDO`'s own HTTP API, write it into a
self-owned class), not a rename. **`scripts/RELEASE.md`'s whole-tree rule applies to any of these deploys:
a worktree deploy uploads `ASSETS` wholesale, `.gitignore` unread.**
