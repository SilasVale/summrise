# Review: the token era (rounds 10–15)

> **Scope:** `git diff d536841...13c931a` — 42 commits, 36 files, +4749/−127: monitor notifications and
> the quick start, token last-use/attribution, rotation and audit names, the live browser view and
> cross-platform end facts, the Windows ConPTY test and logout, expiring tokens.
>
> **Axes:** Standards (AGENTS.md + the Fowler smell baseline) and Spec (the six round plans, plus
> CONNECT-MCP/QUICKSTART). Both ran as independent subagents; no file was edited by either.

## Standards

| Finding | Disposition |
|---|---|
| `audit.rs` said "one line per authenticated action", but `POST /logout` writes none and the closed kind set has no member | Fixed: the module doc now says device/credential actions, and that session sign-in/sign-out is deliberately not recorded |
| `create`/`rotate` duplicated the mint (id, `srm_`, Token build, clone/save/adopt) | Fixed: `Scope`, `issue`, `save_and_adopt` |
| `allow_destructive=true/false` repeated three times | Fixed: `consent_detail` |
| `record_tool`'s six positionals, three interchangeable `Option`s | Fixed: `AuditFacts` struct with named fields; forward's five calls use it |
| `mark_used` re-implemented `find_by_hash`'s constant-time scan | Fixed: `same_hash` predicate |
| BrowserPane's capture and live tick repeated the paint/status pair | Fixed: `paint` |
| App's `probe` and `load` repeated the three-call fetch and three setters | Fixed: `refreshData` |
| Token/TokenMeta/Draft repeat the scope fields; `last_used_ms`/`saved_last_used_ms` are a twin; the Windows test fixture duplicates the Unix one; `forward` re-derives `audit_id` | Deferred, judgement calls: the shapes serve the store/API/TS boundary; the twin is the persisted-value mirror the throttle needs; the two fixtures serve different test styles and the Windows one is CI-only; the forwarded device id is the path's canonical form |

## Spec

| Finding | Disposition |
|---|---|
| The last-use 60 s write throttle was in source comments only, though the plan says it is stated | Fixed: CONNECT-MCP says the list is live in memory and a reload may lag by up to a minute |
| Notifications card used the same "permission is asked" sentence for default and granted-but-off | Fixed: granted/off has its own sentence and button ("Turn notifications on"); tests updated |
| The live view stopped on **any** target change, not just a change to none, and silently | Fixed: a target change keeps watching on the new target (test added); a grant change stops with "Live stopped: the grant changed."; the empty-target branch still turns live off |
| QUICKSTART said "revocation is the only withdrawal" even for expiring tokens; the 401 row omitted expiry | Fixed: "for a permanent token"; the 401 row names expired tokens |
| ConnectCard's intro and doc still said a token works until revoked | Fixed: permanent vs expiring stated in both |

## Verification

- Console: 75 lib + 1 bin + 120 integration (2 ignored); clippy `-D warnings` 0; fmt 0.
- Web: 29 files, **295 tests**; typecheck 0; the committed bundle is current.
- No conflict markers; the two axes' reports are the basis for every fix above.
