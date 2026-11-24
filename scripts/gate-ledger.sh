#!/usr/bin/env bash
# Print the gate ledger: every suite this product's two surfaces are held by, with its count.
#
#   bash scripts/gate-ledger.sh [--console|--panel]
#
# The backlog's ledger (`docs/superpowers/specs/2026-11-05-console-polish-backlog.md`, "THE GATE LEDGER") is a SNAPSHOT of a
# day. This script is how a reader re-takes it: it runs the commands the ledger names and prints their counts in the same
# order, so a number that moved is visible rather than remembered.
#
# IT RUNS WHAT CI RUNS. The Rust suites are the console's own (`-p summrise-console`, which is what the console job runs), the
# web suite is `npm test`, the browser gate drives the BUILT `dist/index.html` (so `npm run build` runs first), and the panel's
# suites run in the agent checkout — which this repository's tree does not carry, so that half prints `n/a` with the path it
# looked for rather than failing.
#
# Exit code is 0 only when every suite that ran passed. A skipped half is not a failure; a red one is.

set -uo pipefail

cd "$(dirname "$0")/.." || exit 1
REPO="$(pwd)"
PANEL="$(cd .. && pwd)/summrise/agent/resources/panel-react"
WANT="${1:-all}"
FAILED=0
LINE=""

say() { printf '%s\n' "$*"; }
row() { printf '  %-52s %s\n' "$1" "$2"; }

# A count or a red mark: the suites print their own words, so the ledger reads them rather than counting tests itself.
run_rust() {
  local manifest="$1" pkg="$2" label="$3"
  if [ ! -f "$manifest" ]; then row "$label" "n/a (no $manifest)"; return; fi
  local out
  out="$(PATH="${HOME}/.cargo/bin:${PATH}" cargo test --manifest-path "$manifest" -p "$pkg" 2>&1)"
  if [ $? -ne 0 ]; then
    row "$label" "FAILED"
    printf '%s\n' "$out" | grep -E "^(test result|error)" | head -4 | sed 's/^/      /'
    FAILED=1
    return
  fi
  local passed
  passed="$(printf '%s\n' "$out" | grep -oE "^test result: ok\. [0-9]+ passed" | awk '{sum += $4} END {print sum + 0}')"
  row "$label" "${passed:-0} passed"
}

run_node() {
  local dir="$1" label="$2" script="$3"
  if [ ! -d "$dir/node_modules" ]; then row "$label" "n/a (no node_modules in $dir)"; return; fi
  local out
  out="$(cd "$dir" && eval "$script" 2>&1)"
  if [ $? -ne 0 ]; then
    row "$label" "FAILED"
    printf '%s\n' "$out" | tail -8 | sed 's/^/      /'
    FAILED=1
    return
  fi
  local count
  count="$(printf '%s\n' "$out" | grep -oE "Tests +[0-9]+ passed" | tail -1 | grep -oE "[0-9]+")"
  if [ -z "$count" ]; then
    count="$(printf '%s\n' "$out" | grep -oE "PASS — [0-9]+ steps" | tail -1 | grep -oE "[0-9]+")"
    [ -n "$count" ] && count="${count} steps"
  fi
  row "$label" "${count:-ok}"
}

if [ "$WANT" = "all" ] || [ "$WANT" = "--console" ]; then
  say "console ($REPO)"
  run_rust device/Cargo.toml summrise-console "cargo test -p summrise-console"
  run_rust device/Cargo.toml summrise-ui "cargo test -p summrise-ui"
  run_node device/console/web "npm test (vitest)" "npm test"
  run_node device/console/web "npm run gate:browser (BUILT page)" "npm run build >/dev/null && npm run gate:browser"
fi

if [ "$WANT" = "all" ] || [ "$WANT" = "--panel" ]; then
  say "panel ($PANEL)"
  run_node "$PANEL" "npx vitest run (panel)" "npx vitest run"
  # The panel's three Rust gates live in the agent repository's own `all-gates.bash`; this repository can only name them.
  if [ -d "$PANEL" ]; then
    row "cargo test --test panel_sheet_freshness / spacing_scale / stylesheet_hygiene" \
      "in the agent repo: bash scripts/all-gates.bash"
  else
    row "the panel's three Rust gates" "n/a (no agent checkout at $PANEL)"
  fi
fi

say ""
if [ "$FAILED" -ne 0 ]; then
  say "LEDGER: RED — a suite above failed; the ledger's row for it is wrong until it is fixed."
  exit 1
fi
say "LEDGER: GREEN — every suite that ran passed. Update the snapshot's numbers if they moved."
