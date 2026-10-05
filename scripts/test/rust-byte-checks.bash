#!/usr/bin/env bash
# rust-byte-checks — RUN EVERY RUST<->JS BYTE COMPARISON, WHICH NOTHING RAN.
#
# ── WHY THIS EXISTS (measured 2026-10-05) ───────────────────────────────────────────────────────
#
# Four `verify.mjs` harnesses compare a BUILT Rust worker against the shipping JavaScript on the same
# request and compare the BYTES — the plan's own P3 criterion, "同请求新旧响应字节可比":
#
#     gateway/wasm/verify.mjs                 the console's /api/health route
#     index/worker/verify.mjs                 the CDN worker's four routes
#     proxies/zen-us-proxy/worker/verify.mjs  the zen-us satellite, 17 cases
#     proxies/zen-go-proxy/worker/verify.mjs  the zen-go satellite, 17 cases
#     index/landing/verify.mjs                the landing page, rendered by both renderers
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
# cannot make this judgement, and saying so is better than a green that means nothing.
set -uo pipefail

cd "$(dirname "$0")/../.." || exit 1
ROOT="$PWD"

if ! command -v cargo >/dev/null 2>&1; then
  echo "  n/a no cargo on PATH — the byte comparisons build a wasm module first"
  exit 2
fi

if ! command -v worker-build >/dev/null 2>&1; then
  echo "  installing worker-build (a few minutes; the CI job caches ~/.cargo/bin) …"
  cargo install worker-build --locked >/dev/null 2>&1 || {
    echo "  n/a could not install worker-build, so the byte comparisons cannot build" >&2
    exit 2
  }
fi

# The target the four modules are built for. Missing it is the same class of "cannot judge here".
if ! rustup target list --installed 2>/dev/null | grep -qx wasm32-unknown-unknown; then
  echo "  n/a the wasm32-unknown-unknown target is not installed"
  exit 2
fi

fail=0
for d in gateway/wasm index/worker proxies/zen-us-proxy/worker proxies/zen-go-proxy/worker index/landing; do
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
  echo "  $fail of 5 byte comparison(s) differ — the two implementations do not answer the same bytes."
  exit 1
fi
echo "rust-byte-checks: 5 comparison(s) hold"
exit 0
