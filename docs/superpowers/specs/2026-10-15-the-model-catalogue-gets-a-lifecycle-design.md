# The model catalogue gets a lifecycle · design (2026-10-15)

> **The operator's decision (2026-10-15, this session):** *"按 opencode 这条路走(目录数据化 + 状态字段)"* —
> take the path opencode takes: the catalogue is data, and a row carries its own status. This document is
> that decision's buildable form.
>
> It follows `docs/superpowers/plans/2026-10-06-the-provider-catalogue-becomes-data.md` (provider records became
> data; this one is about **model rows**). It does not reopen that plan's §五 (A5 / A6 / B4).

---

## 1. Intent, and the measurement that forced it

A model row must stop being advertised the moment upstream stops serving it, and a limited-time free tier must be
adoptable **without a Rust release**.

Measured on 2026-10-15 (operator's key, `zen/go`):

| Probe | Result |
|---|---|
| `POST zen/go/v1/chat/completions` with `ox-alpha-free` | **400** `Upstream request failed: Model is unavailable.` |
| the same call with `omen-alpha` | **200** — upstream renamed the model |
| `GET zen/go/v1/models` | 37 ids; the free ones are `longcat-2.5-preview-free`, `step-5-preview-free`, `cat-jam-free` — **none of which we advertise** |
| `POST inference/openai/v1` with `deepseek-v4.1-flash-free`, from curl, **no key / bogus key / valid key** | **403 `FreeTierError`** — the free tier is gated on being the OpenCode client, not on a credential |

So we advertise a model that answers 400, we advertise none of the three free models that do answer, and the one
free model worth wanting is unreachable by design (we do not forge a client identity to reach it).

**Why now, in one line:** the rows that rot are exactly the ones whose nature is ephemeral — free tiers and stealth
models are marketing artifacts, renamed on reveal and withdrawn on a schedule. Hand-maintaining them in a Rust
constant guarantees this failure recurs, and it recurs **silently**: the row stays selectable in the console and the
400 only appears when someone picks it.

## 2. Prior art: what opencode actually does (read, not assumed)

| Layer | Mechanism | Source |
|---|---|---|
| Catalogue | one TOML per model in a separate data repo: `providers/<provider>/models/<model>.toml`; `opencode` 125 rows, `opencode-go` 38 | [anomalyco/models.dev](https://github.com/anomalyco/models.dev) (`providers/opencode/models/deepseek-v4.1-flash-free.toml` exists already) |
| Row schema | `cost` (context tiers), `limit{context,input,output}`, `modalities`, `tool_call`, `release_date`, **`status: alpha \| beta \| deprecated`**, **`provider.shape: "responses" \| "completions"`**, `experimental.modes` | `packages/core/src/models-dev.ts`'s `SourceModel` |
| Freshness | runtime fetch of `https://models.opencode.ai` with KV cache, exponential-backoff retry and a `Schedule.spaced(ttl)` refresh; a 5.3 MB `snapshot.txt` committed by `script/update-models-snapshot.ts` is the "boot-time floor" | `packages/core/src/models-dev.ts` |
| Availability | a catalogue row becomes an integration only when a credential exists (`type: "key"` or `type: "env"`, `names: [...provider.environment]`); "the selector only shows **enabled** models whose provider is **available**" | `packages/core/src/plugin/models-dev.ts`, docs `Models` |
| Control | config `disabled` / `modelID` / `capabilities` / `limit` / `variants`; Console per-workspace **Enabled**, **New models** (auto-enable new upstream models), **Free models** (one switch to block them all), editable prices/limits, policy push | docs `Models`, `Console → Providers` |

Two things worth stealing, and one worth refusing:

- **Steal:** the catalogue does not filter by "can we keep serving this" — `-free` rows sit in it as ordinary rows.
  Filtering happens at the **enablement** layer.
- **Steal:** `status` and `shape` are **data fields**, so lifecycle and routing dialect travel with the row.
- **Refuse:** their discovery path accepts fallback assumptions ("OpenCode cannot detect whether the custom-model
  fallback values are accurate"). We accept uncertainty too, but our failure mode is worse: a wrong facet does not
  degrade, it **routes to the wrong arm silently** — the class `request_shape.rs` exists to prevent. Assumptions must
  therefore be *declared* on the row and *refused* where they cannot be derived (§4, D4).

## 3. What already exists here (measured, so the build stays narrow)

| Half of "data-ization" | Status | Evidence |
|---|---|---|
| A row's **existence** can be data | **exists** | `models:custom` (`model_store::CUSTOM_MODELS_KEY`), POST `/api/admin/models`, merged into the advertised set by `admin_routes::advertised_ids_for` |
| A store row on a **built-in prefix routes** | **exists** | `routing.rs:472` — a built-in prefix never consults the store; `pick_route(prefix)`; the wire name is `wire_model_name`, whose `og` remap **falls back to the stripped id** (`registry.rs:846`) |
| **Facets** can be data | **exists** | `models:overrides` + `PINNED_FACETS` (`wire`, `usEgress`, `search`, `responsesOnly`) + `reasoningEfforts` |
| **Hiding** a row can be data | **exists** | `models:disabled` |
| **Drift detection** | **exists, manual** | `models_probe` returns `offered` / `advertised` / `not_advertised` / `not_offered`; its own comment: *"ADVERTISED BUT NOT LISTED — printed as a CHECK, never a verdict"* (the router normalises names, so a raw diff reports false drift) |
| Per-row **lifecycle** | **missing** | nothing records whether a row was ever seen upstream, or that it is ephemeral |
| Probe result **written back** | **missing** | the probe reports; nothing changes state |
| **Policy** surface | **missing** | adoption is a human clicking 采纳 per model; there is no per-channel switch |

**Therefore the build is three pieces, not a migration.** `MODEL_REGISTRY` stays a Rust constant: its order is a
byte contract (`/v1/models` serves it verbatim, `registry.rs` header), and it is the set we *promise*. The rows that
must be data are the ephemeral ones.

## 4. The design

### D1 — Two classes of row, and only one of them is ours to keep

| Class | Lives in | Examples | Who removes it |
|---|---|---|---|
| **Curated** | `MODEL_REGISTRY` (Rust const, ordered) | `og/deepseek-v4.1-flash`, `og/gpt-5.6-luna` | a human, in a commit |
| **Opportunistic** | the store (`models:custom`) | `og/longcat-2.5-preview-free`, `og/omen-alpha` on reveal | the probe (§D3), or the operator |

The two classes differ in one promise: a curated row is one we intend to serve next month. An opportunistic row is
one that answers **today**, and the console says so.

### D2 — Lifecycle fields, on the store row and in the admin view

Added to a store row (all optional; absent means the conservative default):

| Field | Values | Meaning |
|---|---|---|
| `status` | `active` \| `alpha` \| `beta` \| `deprecated` (default `active`) | lifecycle, opencode's spelling |
| `ephemeral` | bool (default `false`) | a limited-time free tier or a stealth model — *expected* to disappear |
| `verifiedAt` | unix ms | when the probe last saw this row in an upstream listing |
| `verifiedBy` | probe run id | which run wrote `verifiedAt` — the same "since_ms" instinct as the device monitor |

**`/v1/models` bytes do not change.** The OpenAI model object has no status field, and that response is a pinned
byte contract (`models-corpus.json`). Lifecycle is served by `GET /api/admin/models`, which already assembles a
per-id facet map (`admin_routes.rs:543`) — this is an extension of that map, not a new endpoint.

### D3 — The probe observes on a timer; a reader decides; only store rows can be demoted

**Carrier and credential.** The timer fetches the upstream listing with the **worker's** key: a scheduled run has no
caller, and today's probe authenticates with the *clicking admin's* BYOK keys (`models_probe.rs:201`). It is a
**listing** call only — never a billed inference call.

**Scope: `og/` only, in this design.** Listing derivation is a pure suffix swap (`decisions.rs:94`), and only channels
with a chat-hardcoded arm are listable at all: `og/ nv/ gmi/ cm/`. `or/ qw/ r4/ amd/` answer CANNOT CHECK (the admin
corpus already pins that for `qw/`), and `nv/ gmi/` are pure BYOK with no deployment key. `og/` is both listable and
the only channel in use (operator, 2026-10-06). **D4's switches therefore exist for `og/` only**, and the console says
why for every other channel: a switch on a channel nothing can check is a switch that lies.

**Clean evidence has a floor.** "Clean" means the listing parsed into a recognised shape (`offered_ids` returns `[]`
for anything but a bare array or `{data:[…]}`, `models_probe.rs:30`) **and is non-empty** **and contains at least one
curated row of that channel**. A 200 with `{"data":[]}` — an entitled key with no models, or a changed upstream shape
— is **inconclusive**, not clean, and must deprecate nothing. Without this floor, `K` runs against a degenerate
success would hide every advertised row of the channel: anti-goal 1 wearing a different hat.

**Order-free state.** Nothing is read-modify-written across isolates: KV is eventually consistent and this worker's
locks are per-isolate by construction (`key_lock.rs:1`; `device_store.rs:381` re-reads inside its lock for exactly
this reason). Each run writes **its own key** — one entry `{runId, at, conclusive, seen, missing}` — into a bounded
ring of the last `K + 1` runs per channel. The verdict is then a **read**: an id is demoted when the last `K` entries
are all conclusive and all list it as missing. Nothing to serialize, replayable, and a half-written run changes no
verdict. A Durable Object would supply a lock; this worker binds only the relay's `BreakerDO` and defines no class, so
class ownership and migrations are a cost this design does not pay.

**One writer.** One decision function, two callers: the manual button stays what it is today — **a report** — because
its evidence comes from the clicking admin's key, which may see a different list than the deployment key, and one
click must not hide rows for everyone. Only the timer writes `status` / `verifiedAt` / `verifiedBy`.

**Demotion applies to store rows only.** A curated row that upstream stops listing becomes a **console 待查 entry**
for a human commit (D1). A single KV write must not hide a row we promise (`store.rs:728` already filters registry
ids through the disabled list — `is_built_in` is the predicate that keeps this honest), and it must not be able to
make a health card lie (D5).

**How a demoted store row is hidden: `status`, not `models:disabled`.** `models:disabled` is defined for built-ins
only: the advertised set filters registry ids and appends store ids unfiltered (`store.rs:728`), and the console's own
rule is "console-owned = REMOVED, built-in = DISABLED" (`admin_routes.rs:289`). The lifecycle therefore rides on the
row, and `advertised_ids_for` skips store rows whose `status == "deprecated"`. **Three pinned contracts move on
purpose**: that function, its emitted twin (`gateway/src/store/models-logic.ts`, byte-compared) and the corpus case
pinning "one disabled id is removed AND the created values shift". A demoted row stays in the console with its
history, and returns to `active` — and back into `/v1/models` — the moment a conclusive run sees it again.

**The comparison is normalised** before it concludes anything: the row's id through `wire_model_name`, with the
`[1m]`-style bracket markers stripped. The naive diff is what `models_probe` already refuses to treat as a verdict,
and that refusal is the thing to preserve. The raw, un-normalised report stays exactly what it is today: a report.

### D4 — Adoption is a switch, off by default, and refuses what it cannot derive

For `og/` (D3's scope), the console gets two switches (opencode's **New models** and **Free models**, in our vocabulary):

- **auto-adopt** (default **off**): rows in `not_advertised` are adopted as `ephemeral: true`, `status: "alpha"`.
- **free tier** (default **off**): when off, nothing whose upstream price is zero is adopted, and existing
  `ephemeral` rows are hidden but kept.

**What auto-adopt refuses**, and lists for a human instead:

- a row whose routing dialect cannot be derived (a `responses`-only id on a channel whose arm is chat/completions);
- a row that would need a non-default facet to work (the `usEgress` class: `cat-jam-free` is region-limited by
  upstream's own documentation) — adopting it blind converts a loud 400 into a silent wrong route.

An adopted row is born with an explicit `assumed: [...]` list naming every facet that was defaulted rather than
derived. The console renders that list. *This is the one place we deliberately diverge from opencode's posture:
they accept a silent assumption, we accept a declared one.*

### D5 — Health cards never carry an opportunistic row

`HEALTH_CHANNELS` is a Rust constant of `(channel, model)` labels; `build_health` is `ok` by construction and only
the `og` circuit can turn a card red (`lib.rs:106`). So a dead free model was never *reported* — it was a label
claiming we serve something we do not. Rule: **a card's model must be a curated row**, enforced by a test that walks
`HEALTH_CHANNELS` against the registry, not by a comment.

The immediate consequence is part of the cleanup (§5): the `("og", "og/ox-alpha-free")` card goes, 20 cards → 19.

### D6 — Model listing bytes stay frozen

`/v1/models` and `/api/health` bytes are compared against corpora. Nothing in this design adds, removes or reorders
a curated row; the only byte-affecting change is the one §5 cleanup, whose corpus is regenerated and diffed
deliberately.

### D7 — The metadata catalogue (operator's decision, §11)

**Decision (operator, 2026-10-15): take B — integrate the catalogue.** So D4's "derive or refuse" gains a source of
facts, and the one thing it still cannot derive is named below.

| | |
|---|---|
| Endpoint | `https://models.dev/api.json` — **2,006,627 B measured 2026-10-15** (the `models.opencode.ai` mirror is 5,665,821 B). There is **no per-provider endpoint**: `/opencode.json` and `/opencode-go.json` both 302 to the site root (measured). One document, the whole world. |
| What we take per row | `provider.shape` (dialect → D4's refusal test), `cost` (price → the free switch), `limit{context,output}` (the `contextWindow`/`maxTokens` facets we hand-declare today), `status` (`alpha`/`beta`/`deprecated` — a **hint**, never evidence: demotion still comes from D3), `modalities` (vision), `tool_call`, `reasoning_options` |
| What it does **not** carry | **region policy** — the `usEgress` class is ours alone (`cat-jam-free`'s China/HK/Macau block is in opencode's prose, not in the catalogue). That facet stays **declared** on adoption, and D4 keeps refusing to guess it. Also ours, not theirs: the `og` wire aliases, search capability, and the curated set itself |
| Freshness | the committed **slice is the floor**; the runtime refresh is a Cron that fetches the document, extracts our two providers, validates, and writes the slice to KV with a TTL. A request path **never** parses the 2 MB document. If the fetch, the extraction or the validation fails, the KV value stands and the floor applies |

Three hard rules, because this is a third-party document arriving at runtime:

1. **The slice is extracted, not the document.** `scripts/update-model-catalogue.sh` writes
   `gateway/wasm/src/catalogue/{opencode,opencode-go}.json` and commits it — a *data* commit, the same shape as
   opencode's `update-models-snapshot.ts` (§2), and the reason this design can say "no Rust release".
2. **The catalogue may only describe opportunistic rows.** It can fill a facet on a store row. It can never create a
   curated row, change `MODEL_REGISTRY`, or touch a health card (D5). Validation drops unknown fields, and an id that
   does not match our prefix rules is dropped, not adopted.
3. **Absence is not evidence.** A catalogue that does not list a row changes nothing (anti-goal 1, third hat); D3's
   conclusive upstream *listing* remains the only thing that may demote.

**Consequence for D4:** the refusal list shrinks to exactly one entry — a row needing a region facet we cannot derive.
Everything else is derived or dropped.

## 5. What lands first, and what lands with the project

**Step 0 (small, can land alone, unblocks "stop pretending"):** hide the dead row, and drop its card.

- **Hide as data**: disable `og/ox-alpha-free` (`models:disabled`). The corpus already pins the shape of this — the
  case named "one disabled id is removed AND the created values shift" — so the byte diff is the one the fixture was
  built for. No Rust edit, and it is reversible.
- **Drop the card in the same commit** (`HEALTH_CHANNELS` 20 → 19): a public card naming a model that answers 400 is
  the exact lie this design exists to end. That moves the `/api/health` body, and the fixtures recording it are
  **hand-diffed, deliberately, in the commit** — the generators those fixtures name are retired (`route-oracle.mjs`,
  `oracle-translate.mjs`), and `verify.mjs`'s record mode records from a shipping TS side that no longer exists.
  Hand-diffing an **expectation** when the product changes on purpose is the sanctioned move; hand-editing a
  **recording** is not — which is why the registry row itself stays until the project builds a fixture emitter.
- The three live free tiers are opportunistic rows: they enter through D1/D4's store path, never through the registry.

**The project:** D2 (fields), D3 (scheduled observation + the three contract moves it names), D4 (switches), D5
(test), D7 (the catalogue slice, its extractor and its runtime cache), a fixture emitter, the registry row's own
removal, and the console surface.

## 6. Non-goals

- Moving `MODEL_REGISTRY` into KV. Its order is a byte contract and its contents are our promise.
- Any change to `/v1/models` response shape or GitHub-style "status" in the public listing.
- Adopting anything automatically with the switch off, or from a probe that failed.
- Reaching opencode's Console free tier (`deepseek-v4.1-flash-free`). It is gated on being the OpenCode client; we
  do not forge that identity, and this design does not try.
- Reopening the 2026-10-06 plan's §五 (A5 sweep-to-Rust, A6 console-to-wasm, B4 channel deletions).

## 7. Anti-goals (each of these makes the change a failure even if it works)

1. **A failed fetch deprecates a row.** A single 500 from a listing endpoint must not hide a model.
2. **A status that only exists in the UI.** If `status` does not sit in the path that decides what is advertised, it
   is decoration.
3. **An adopted row that routes wrong.** Silently wrong route ≫ silently dead model; where a facet cannot be
   derived, refuse and list.
4. **A second source of truth.** Anything derived must be derived from the registry or the store, never mirrored
   into a new table that can drift (the repo deleted a "twin wearing a freshness test" once already — see
   `registry.rs`'s `CHANNEL_CARDS` header).

## 8. Constraints (this repository's rules, not preferences)

- **Rust first.** A change to `.js`/`.mjs`/`.cjs` needs a reason; `agent/tests/js_boundary_inventory.rs` classifies
  every tracked JS/TS file and caps the LOGIC count.
- **The gate list is `ci.yml`.** The suites do not share a reporter: `cargo test` → `test result: ok`; `npm test` in
  `gateway/` → `ℹ pass N`; `npx vitest run` in `panel-react/` → `Tests N passed`.
- **Every gate carries its own `MUTATION:`/`RESULT:` header**, and the commit message carries the measurement and a
  `VERIFIED:` line. Where the long form lives: the commit message.
- **Byte contracts**: `/v1/models`, `/api/health`, and the corpora that pin them.
- **Deploys need the operator**; this design produces a commit, not a deploy.
- No undeclared production hostname may appear in a tracked file (`agent/tests/production_host.rs` is a ratchet).

## 9. Left to the builder (decisions a competent builder may make)

- The timer's period, and where the two switches are stored and named.
- The exact spelling of the lifecycle fields, and whether `assumed` rides on the row or beside it.
- `K`'s constant name and the mutation test's shape.
- The console copy (Chinese and English; `gateway/ui/src/i18n.ts` is the only place strings live) and whether a
  "restore" action is needed next to 采纳.
- Whether Step 0's two edits ride in one commit or two.

## 10. The tests this design requires (each must be able to fail)

1. A failed listing fetch changes **no** row's status (the anti-goal 1 mutation).
2. `K-1` clean probes missing a row change nothing; the `K`th deprecates it; the row leaves `/v1/models` and is
   still returned by `GET /api/admin/models`.
3. A deprecated row seen again returns to `active` and back into `/v1/models`.
4. The comparison ignores a wire-remapped name and its bracket markers (two cases; without them the naive diff
   would deprecate a live row).
5. No id in `HEALTH_CHANNELS` is a non-curated row (walk the constant against the registry).
6. An adoption whose dialect cannot be derived is refused and listed, not adopted (D4).
7. A **degenerate success** is inconclusive: a 200 carrying `{"data":[]}`, or an unrecognised body, hides nothing
   after `K` runs (anti-goal 1's second hat — the one §10 test 1 does not cover).
8. A conclusive listing that is missing **every** curated row of the channel is inconclusive too (the D3 floor).
9. A curated registry row is never demoted by the machine: one KV write cannot hide a row we promise.
10. The manual probe writes no status (one writer; D3).
11. The catalogue is unreachable → the committed slice serves, and **no** row changes state (D7 rule 3).
12. A catalogue entry can never create a curated row or alter `MODEL_REGISTRY` (D7 rule 2).
13. An unrecognised shape in the catalogue is dropped, not adopted (D7 rule 2, the validation half).

## 11. The decision this design was missing (answered)

**Operator, 2026-10-15: "B. 接元数据目录(models.dev / models.opencode.ai)".** Where the facts come from is therefore
the catalogue, and D7 is that decision's buildable form. The alternative below is kept for the record, because the
part B cannot answer — region policy — is exactly why the option existed.

| | Where those facts come from | Cost |
|---|---|---|
| A. Declared on adoption *(not taken)* | whoever adopts (the 采纳 button gains the fields, and refuses to guess) | small: fields on an existing route, plus console inputs |
| **B. A metadata catalogue** *(taken)* | `models.dev`, sliced and committed, refreshed by a Cron — see **D7** | a new upstream integration: cache, TTL, a committed slice, and a new outbound host the repo's declarations must account for |
