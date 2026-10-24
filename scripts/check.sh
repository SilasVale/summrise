#!/usr/bin/env bash
# The local gate: the commands ci.yml's `device` job runs, read out of that file instead of copied
# beside it. A change to CI cannot leave this script testing yesterday's commands.
#
#   scripts/check.sh              # the same commands CI runs, from the repository root
#   scripts/check.sh --fast       # skip the test run (format and lints only)
#   scripts/check.sh --windows    # also cross-compile the Windows target (needs cargo-xwin)
#
# It is the *portable* half. The Windows lifecycle suite only runs on Windows, in CI's
# `device-windows` job, and `--windows` proves that half compiles rather than that it passes.
set -euo pipefail

cd "$(dirname "$0")/.."

fast=0
windows=0
for argument in "$@"; do
    case "$argument" in
        --fast) fast=1 ;;
        --windows) windows=1 ;;
        -h | --help) sed -n '2,11p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *)
            echo "check: unknown argument $argument" >&2
            exit 2
            ;;
    esac
done

if ! command -v cargo >/dev/null; then
    echo "check: cargo is not on PATH" >&2
    exit 2
fi

workflow=.github/workflows/ci.yml
if [[ ! -f "$workflow" ]]; then
    echo "check: $workflow is missing; run from a checkout of the repository" >&2
    exit 2
fi

# The `device` job only: from its `  device:` header to the next job header. Its steps name their
# workspace by `--manifest-path`, which is what lets them run here unchanged.
job="$(awk '/^  device:/{inside=1; next} /^  [a-z][a-z0-9-]*:/{inside=0} inside' "$workflow")"
mapfile -t commands < <(
    printf '%s\n' "$job" |
        sed -n 's/^        run: //p' |
        grep -E '^cargo (test|fmt|clippy)( |$)'
)

if [[ ${#commands[@]} -eq 0 ]]; then
    echo "check: no cargo commands found in $workflow's device job" >&2
    exit 2
fi

for command in "${commands[@]}"; do
    if ((fast)) && [[ "$command" == cargo\ test* ]]; then
        echo "== skipped (--fast): $command"
        continue
    fi
    echo "== $command"
    # The line is CI's own `run:` value, unquoted by the workflow parser; word splitting is exactly
    # how GitHub's runner would hand it to the shell.
    eval "$command"
done

if ((windows)); then
    echo "== cargo xwin check --workspace --target x86_64-pc-windows-msvc"
    cargo xwin check --workspace --target x86_64-pc-windows-msvc
fi

echo "check: every command passed"
