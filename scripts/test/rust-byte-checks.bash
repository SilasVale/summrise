#!/usr/bin/env bash
# rust-byte-checks — RUN EVERY RUST<->JS BYTE COMPARISON, WHICH NOTHING RAN.
#
# ── WHY THIS EXISTS (measured 2026-10-05) ───────────────────────────────────────────────────────
#
# `verify.mjs` harnesses compare a BUILT Rust worker against a reference on the same request and compare
# the BYTES — the plan's own P3 criterion, "同请求新旧响应字节可比":
#
#     gateway/wasm/verify.mjs                 the console's /api/health route
#     index/worker/verify.mjs                 the CDN worker's four routes
#     index/landing/verify.mjs                the landing page, rendered by both renderers
#
# **THE TWO SATELLITES ARE NOT IN THIS LIST ANY MORE (2026-10-08), AND THEIR COMPARISON DID NOT GO
# ANYWHERE — IT MOVED INTO RUST.** Their harnesses were ~400-line `.mjs` files doing three jobs at once:
# running the built wasm module needs a JS engine, but the case list, the upstream answers and the byte
# comparison do not. Those live in `proxies/zen-{us,go}-proxy/worker/tests/differential.rs` now, run by
# the `cargo test --manifest-path …` steps the workflow already had, and the only JavaScript left is
# `gateway/wasm/run-cases.mjs` — a runner that takes a spec and prints what the module did. **SO THIS
# SCRIPT NO LONGER RUNS THEM, AND THAT IS NOT A GAP**: a cargo test in a named CI step is run by the job,
# while a `verify.mjs` named in no step was run by nobody — which is why this file exists at all.
#
# **AND ONE THAT IS DELIBERATELY NOT HERE**: `index/spike-rust-landing/verify.mjs` also runs and also
# passes, but its own README opens "A spike, not a migration" — it answered whether the landing COULD be
# Rust, and `index/landing/` is the answer. **A retired spike's fidelity check is a gate on an artifact
# nobody ships**, so it is named here rather than run.
#
# **NOT ONE OF THEM WAS INVOKED BY `ci.yml`.** The only mention of `verify.mjs` in the workflow is a
# COMMENT (line 58) describing the gateway's Rust half. So four checks that pass by hand had never run
# once on a runner — which is this repository's own rule turned on itself: "a test nobody runs is the
# same defect class as a check that cannot fail".
#
# ── AND ONE OF THEM COULD NOT HAVE RUN ANYWAY ───────────────────────────────────────────────────
#
# `gateway/wasm/verify.mjs` wrote `build/index_bg.wasm.mjs` WITHOUT BUILDING FIRST, so on a clean tree it
# died with `ENOENT: no such file or directory, open '…/build/index_bg.wasm.mjs'`. It builds now, the way
# the other three do. **A HARNESS THAT CANNOT RUN ON A CLEAN TREE IS A HARNESS THAT ONLY ITS AUTHOR HAS
# EVER RUN.**
#
# ── WHAT IT NEEDS, AND WHAT IT DOES WHEN IT CANNOT ──────────────────────────────────────────────
#
# `worker-build` (the same 0.8.7 the `worker` crate is pinned to) and the `wasm32-unknown-unknown`
# target. **IT INSTALLS `worker-build` IF IT IS MISSING** — that is a several-minute build, which is why
# the CI job caches `~/.cargo/bin`; on this box it is already installed.
#
# **IT EXITS 2 WHEN IT CANNOT RUN AT ALL**, which `all-gates.bash` maps to "n/a": a host with no `cargo`
# or no wasm32 target cannot make this judgement, and saying so is better than a green that means nothing.
#
# ── AND THE ORDER OF THOSE TWO QUESTIONS IS A FIX, NOT A STYLE (measured 2026-10-05) ────────────
#
# The first version asked `command -v worker-build`, INSTALLED it when missing — a several-minute build —
# and only THEN checked the wasm target. `pack-chain` ("npm artifact gates, no exe") has no Rust toolchain
# at all, so the gate spent minutes building a tool it could not use and then failed that job. **THE CHEAP
# QUESTION COMES FIRST.** Verified both ways: `env -i PATH=/usr/bin:/bin` -> exit 2 naming cargo, and a
# stubbed `rustup` that answers nothing -> exit 2 naming the target.
set -uo pipefail

cd "$(dirname "$0")/../.." || exit 1
ROOT="$PWD"

if ! command -v cargo >/dev/null 2>&1; then
  echo "  n/a no cargo on PATH — the byte comparisons build a wasm module first"
  exit 2
fi

# **THE CHEAP QUESTIONS COME FIRST, AND THE ORDER IS THE FIX FOR A RED CI (measured 2026-10-05).**
# The first version asked `command -v worker-build`, INSTALLED it when missing — a several-minute build —
# and only THEN checked the wasm target. In `pack-chain` ("npm artifact gates, no exe") there is no Rust
# toolchain at all, so the script spent minutes building a tool it could not use and then failed the job.
# **A HOST THAT CANNOT MAKE THIS JUDGEMENT SHOULD SAY SO IN SECONDS, NOT AFTER AN INSTALL.**
if ! rustup target list --installed 2>/dev/null | grep -qx wasm32-unknown-unknown; then
  echo "  n/a the wasm32-unknown-unknown target is not installed, so nothing here can be built"
  exit 2
fi

if ! command -v worker-build >/dev/null 2>&1; then
  echo "  installing worker-build (a few minutes; the CI job caches ~/.cargo/bin) …"
  cargo install worker-build --locked >/dev/null 2>&1 || {
    echo "  n/a could not install worker-build, so the byte comparisons cannot build" >&2
    exit 2
  }
fi

fail=0
dirs=(gateway/wasm index/worker index/landing)
for d in "${dirs[@]}"; do
  # **THE STATUS IS KEPT BY EXITING, NOT BY PRINTING** — `cmd && echo ok || echo FAIL` throws it away.
  # **`--build`, ALWAYS, AND THE FIRST RUN OF THIS SCRIPT IS WHY.** Without it the harness reuses
  # whatever `build/` happens to hold — and on 2026-10-05 that was a MUTATED build left by a mutation
  # test, so `zen-go` reported "1 of 17 DIFFER" against a source tree that was correct. It is AGENTS.md's
  # own rule one level down: "when a test reads a built artifact, `build` must run before `test`", because
  # "a `test` that runs first therefore validates the PREVIOUS build".
  if (cd "$ROOT/$d" && node verify.mjs --build > /tmp/verify-$$.log 2>&1); then
    echo "  ok    $d/verify.mjs — $(grep -aoE '[0-9]+ case\(s\) byte-identical|[0-9]+ of [0-9]+ case\(s\) DIFFER|byte-identical' /tmp/verify-$$.log | tail -1)"
  else
    echo "  FAIL  $d/verify.mjs"
    tail -20 /tmp/verify-$$.log | sed 's/^/        /'
    fail=$((fail + 1))
  fi
done
rm -f /tmp/verify-$$.log

if [ "$fail" -gt 0 ]; then
  echo ""
  echo "  $fail of ${#dirs[@]} byte comparison(s) run HERE differ — the implementations do not answer the same bytes."
  exit 1
fi
# **THE COUNT IS DERIVED, AND THE TWO THAT MOVED ARE NAMED.** It read "5" as a literal, which stopped being
# the number of comparisons this script runs the moment the satellites' moved into Rust — a summary that
# states a number nothing produces is the defect this suite exists to catch in others.
echo "rust-byte-checks: ${#dirs[@]} comparison(s) hold here, and the two satellites' hold as cargo tests:"
echo "  cargo test --manifest-path proxies/zen-us-proxy/worker/Cargo.toml   (the same for zen-go)"
exit 0
