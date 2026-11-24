# Review: the launcher's Windows lifecycle

> **Scope:** `device/launch/src/win.rs` (SCM service, supervisor loop, child job, swap, PID
> termination), the pure half in `lib.rs`, `paths.rs`, `self_check.rs`, and
> `tests/windows_lifecycle.rs`. Two independent subagents ran the **Standards** (AGENTS.md + Fowler
> baseline) and **Spec** (the module/design claims and `docs/RELEASE.md`/`device/README.md`) axes
> read-only. This file is the disposition record.

## Standards

| Finding | Disposition |
|---|---|
| **Documented claim broken:** "terminates a PID after checking it is really our binary" compared only the **file name**, so another install's `summrise-device.exe` passed | Fixed: `same_image` compares the **full image path** (case-insensitive, verbatim `\\?\` prefix stripped), with the install's app path and, for the old supervisor, the install's launcher path or the current launcher's path. The pure helper has a Linux test; the lifecycle test covers the normal path |
| **Claim too strong:** a failed `AssignProcessToJobObject` is a warning, yet the job doc said a dead supervisor "cannot leave" the app behind | Fixed the doc: the guarantee holds **when the assignment succeeded**; a failure is logged and the guarantee is lost. The training behaviour is unchanged (a failed assignment must not brick a device at boot) |
| **Child stop unproven:** `stop_child` discarded `kill()`/`wait()` and `Child::wait` was unbounded | Fixed: a bounded (5 s) stop returns an error, puts the handle back for a retry, and the swap refuses to install over a surviving app; the supervisor's own stop logs and lets the kill-on-close job finish it |
| **Stop reported without STOP_PENDING; a stop during the 10 s health wait can exceed the SCM's 5 s timeout and kill the supervisor mid-swap** | **Deferred, top follow-up:** the next start deletes `pending` and repairs from the old baseline, so the outcome is a lost update, not a broken device. A correct fix needs the status handle in the control handler, checkpoints/wait hints, and an interruptible health/swap path — a dedicated round with the Windows lifecycle test |
| **The baseline's trust anchor is an unchecked install directory ACL;** a local writer to `<install>` could plant a self-consistent baseline that start-up repair copies over the app as SYSTEM | **Deferred:** the default install (Program Files, SYSTEM service) is admin-only; hardening needs an install-ACL/signature-at-start design, and the flat-pair migration has no launcher signature to check. Recorded with the migration finding |
| Smells: spawn→assign→pid-file repeated, `Finish` threaded but unread, unreachable `Action::Start`, `token_action` enum over a bool, `(version, sha256)` clumps, `next_restart_delay` middle-man, 1808-line win.rs | **Deferred, judgement:** a large mechanical refactor of the most critical Windows path is not justified by this review; the empty "Install / uninstall" banner and the dead `Finish` arm are noted for a future cleanup |

## Spec

| Finding | Disposition |
|---|---|
| **Give-up reported as a clean stop:** `status.code().unwrap_or(1).max(0)` mapped zero and negative crash codes to 0, so the SCM saw `Win32(0)` and skipped its failure actions | Fixed: `failure_code` maps zero/None/negative to 1, keeps a positive code; used by the supervisor; tested on Linux |
| **`--apply` marked an unsupervised child** with `SUMMRISE_SUPERVISED=1`, so a later remote restart could exit a child nothing would restart | Fixed: `spawn_app` takes `supervised`; only the supervised starts set the mark; the manual `--apply` child does not |
| "It is the only other copy of the app" was false (previous/pending/staged copies exist) | Fixed the lifecycle test's comment: `current` is the generation the repair trusts, not the only copy |
| `poll_swap` mapped `SwapError::Failed` to `rolled_back` **without a rollback** | Fixed: the supervisor's `Failed` is the pre-install stop failure, so the outcome is `refused` (nothing was replaced); the comment says a future fallible `finish` must not reuse the mapping |
| An upgrade must pass `--replace-baseline`, but no doc said so; without it the start-up repair rewrites the new installer binary back to the old generation | Fixed in `device/README.md` with the failure mode named |
| Flat-pair migration promotes on sha256 agreement alone, against "the launcher key signs the first baseline generation" | **Deferred:** the flat pair is the legacy pre-generation form; verifying a launcher signature there would refuse the very installs the migration exists for. Recorded with the ACL finding |
| SCM non-crash failure recovery is best-effort (the flag's failure is a warning) | **Deferred:** making it fatal would refuse to install on a machine where the SCM refuses the flag; the warning is the honest fallback |
| `--service-name` exists for the test's `sc.exe` registration; `--self-check`/mesh persistence exceed the design sections | **Accepted:** test-only, read-only, or operator-facing, all documented in the module |
| The Windows test proves the restore/apply/rollback/SCM/kill-on-close/token/seed/lock paths directly; the two probes, restart budget, failure-action *behaviour*, console-link re-arm and installer migration are indirect | Recorded; no code change. The test's header already states the failure-action limit |

## Gates

- `cargo test -p summrise-launch`: **118 lib tests** (2 new: the give-up code mapping and the
  path-not-name image check); clippy `-D warnings` 0; fmt 0; `cargo xwin check -p summrise-launch
  --tests` compiles the Windows half.
- The Windows lifecycle job (the real SCM, ACLs, swap) is the CI proof for the Windows-side changes;
  this record is committed with the branch and the PR must see that job green.
