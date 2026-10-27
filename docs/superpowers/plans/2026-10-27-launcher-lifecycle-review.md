# Review: the launcher's Windows lifecycle

> **For agentic workers:** a review round, not a feature. Two independent subagents read the launcher's
> Windows half and its design claims (Standards + Spec), report findings, and this session fixes the
> confirmed ones. No code is edited by a reviewer.

**Scope:** `device/launch/src/win.rs` (the SCM service, supervisor loop, child job, swap transaction,
PID termination, ACL/install), the pure functions in `device/launch/src/lib.rs` it calls, `paths.rs`,
`self_check.rs`, and `device/launch/tests/windows_lifecycle.rs`. The app and console are out of scope
except where the launcher's contract with them is the claim under review.

**Spec sources:** the module docs (design §4.1/§4.4, F1/F2/F4/F5) and `docs/RELEASE.md`; AGENTS.md is
the standards source, plus the Fowler baseline.

**Output:** a review record at `docs/superpowers/reviews/2026-10-27-launcher-lifecycle.md` with the two
axes, dispositions, and the gates. Fixes only for confirmed findings; judgement calls may be deferred
with a reason.

- [x] Both axes report
- [x] Confirmed findings fixed (or deferred with a reason)
- [x] Launcher gates: `cargo test -p summrise-launch` (118 lib tests), clippy, fmt, `cargo xwin check -p
  summrise-launch --tests`; Windows CI is the PR gate
- [x] Review record committed
