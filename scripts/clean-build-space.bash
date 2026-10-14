#!/usr/bin/env bash
# clean-build-space.bash — GIVE THE DISK BACK AFTER A BUILD, WITHOUT LOSING THE CACHE THAT MAKES THE
# NEXT ONE CHEAP.
#
# WHY THIS EXISTS. This box is a single HDD whose partition is 83-99% full, and a cargo target directory is
# the largest thing a build leaves behind: measured 2026-10-13, `agent/target` was 13G (11G of it
# `debug/deps`), `gateway/wasm/target` 2.9G, and the four proxy/relay/index crates another 6G between them —
# none of which is source, and all of which is regenerated on demand. Nothing cleaned it, so it accumulated
# until the disk was the constraint rather than the code.
#
# WHAT IT DELETES, IN THREE TIERS — the tiers exist because "clean the build" means three different things
# and only one of them is free:
#
#   incremental  `target/*/incremental/`            ALWAYS SAFE. Regenerated on the next build; it is the
#                                                   per-crate cache for the edit-test loop, so deleting it
#                                                   costs one slower rebuild, nothing else. This is the
#                                                   default tier.
#   dirs         whole `target/` directories        SAFE BUT EXPENSIVE: the next `cargo test` in that crate
#                                                   recompiles it and its dependencies (the registry cache in
#                                                   ~/.cargo is NOT touched, so nothing is re-downloaded).
#                                                   `--targets` selects them; crates in flight can be kept.
#   all          every target directory in the repo `--all`, for when the disk is full and time is not.
#
# WHAT IT NEVER TOUCHES: `~/.cargo/registry` (the dependency cache — deleting it costs network, not disk
# hygiene), anything tracked by git, and any directory a cargo process is currently writing.
#
# THE ONE THING THAT MAKES THIS PROFILE-SENSITIVE: since 2026-10-13 `~/.cargo/config.toml` sets
# `debug = 0` for the dev and test profiles, so these directories are ~3x smaller than they were. A tree
# built BEFORE that change still holds debug info that nothing will ever read again — `--targets` is how
# you get it back, and it is why the default tier is not enough on its own.
set -uo pipefail
cd "$(dirname "$0")/.." || exit 1

TIER=incremental
DRY=0
for arg in "$@"; do
  case "$arg" in
    --incremental) TIER=incremental ;;
    --targets) TIER=targets ;;
    --all) TIER=all ;;
    --dry-run) DRY=1 ;;
    -h | --help)
      sed -n '2,30p' "$0" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *)
      echo "clean-build-space: unknown argument '$arg' (try --help)" >&2
      exit 2
      ;;
  esac
done

# ── A BUILD IN FLIGHT OWNS ITS TARGET DIRECTORY ──────────────────────────────────────────────────────────
# `cargo` holds a lock file, and deleting the directory under it turns a running build into a confusing
# failure that looks like a compiler bug. Refuse rather than race: this script is run by hand, and "wait a
# minute" is a better answer than a corrupted artifact.
# A DRY RUN DELETES NOTHING, so the guard does not apply to it: measuring under a live build is exactly
# when a reader wants the number.
if [ "$DRY" != "1" ] && { pgrep -x cargo >/dev/null 2>&1 || pgrep -x rustc >/dev/null 2>&1; }; then
  echo "clean-build-space: a cargo/rustc process is running — refusing to delete under a live build." >&2
  echo "  (wait for it to finish, or kill it deliberately, then re-run)" >&2
  exit 1
fi

# THE REPOSITORY'S OWN TARGET DIRECTORIES, DERIVED RATHER THAN LISTED: every `target/` a tracked
# `Cargo.toml` implies, so a crate added tomorrow is covered without editing this file. `git ls-files`
# rather than a filesystem walk: a worktree's copy, a vendored tree and an editor's scratch dir are not
# this repository's build output, and a walk would find all three.
mapfile -t MANIFESTS < <(git ls-files '*Cargo.toml' | grep -v '^vendor/')
TARGETS=()
for m in "${MANIFESTS[@]}"; do
  d="$(dirname "$m")/target"
  [ -d "$d" ] && TARGETS+=("$d")
done

# THE TIER DECIDES WHAT IS A CANDIDATE. `nullglob` is deliberately not set: an unmatched glob must stay a
# literal here, because the loop below tests it with `[ -d ]` and reports what it found.
CANDIDATES=()
case "$TIER" in
  incremental)
    for t in "${TARGETS[@]}"; do
      for triple in "$t"/*/incremental; do
        [ -d "$triple" ] && CANDIDATES+=("$triple")
      done
    done
    ;;
  targets)
    # THE CRATES IN FLIGHT ARE THE ONES THE MIGRATION IS TOUCHING: the pure core and both of its hosts.
    # Everything else (proxies, relay, index, the wasm policy crates) is rebuilt only when its own gate
    # runs, which is rare — and it is 6G of the 22G this repository was holding.
    for t in "${TARGETS[@]}"; do
      case "$t" in
        agent/target | gateway/wasm/target | core/summrise-core/target) continue ;;
      esac
      CANDIDATES+=("$t")
    done
    ;;
  all)
    CANDIDATES=("${TARGETS[@]}")
    ;;
esac

if [ "${#CANDIDATES[@]}" -eq 0 ]; then
  echo "clean-build-space: nothing to remove at tier '$TIER' — already clean."
  du -sh "${TARGETS[@]}" 2>/dev/null | sort -rh || true
  exit 0
fi

# MEASURE BEFORE, SO THE COMMAND REPORTS A RESULT RATHER THAN AN INTENTION. `du` on this disk is the
# expensive half of this script; it runs once per candidate and once for the total, and `df` brackets it.
BEFORE_AVAIL=$(df --output=avail -k . | tail -1 | tr -d ' ')
FREED_K=0
for c in "${CANDIDATES[@]}"; do
  size_k=$(du -sk "$c" 2>/dev/null | cut -f1)
  size_k=${size_k:-0}
  FREED_K=$((FREED_K + size_k))
  if [ "$DRY" = "1" ]; then
    printf '  would remove  %6s  %s\n' "$(numfmt --to=iec --suffix=B "$((size_k * 1024))" 2>/dev/null || echo "${size_k}K")" "$c"
  else
    rm -rf "$c" || {
      echo "clean-build-space: could not remove $c" >&2
      exit 1
    }
    printf '  removed       %6s  %s\n' "$(numfmt --to=iec --suffix=B "$((size_k * 1024))" 2>/dev/null || echo "${size_k}K")" "$c"
  fi
done
AFTER_AVAIL=$(df --output=avail -k . | tail -1 | tr -d ' ')

echo ""
if [ "$DRY" = "1" ]; then
  echo "clean-build-space: DRY RUN at tier '$TIER' — $(numfmt --to=iec --suffix=B "$((FREED_K * 1024))") would be freed."
else
  echo "clean-build-space: tier '$TIER' — $(numfmt --to=iec --suffix=B "$((FREED_K * 1024))") removed;"
  echo "  free space $(numfmt --to=iec --suffix=B "$((BEFORE_AVAIL * 1024))") -> $(numfmt --to=iec --suffix=B "$((AFTER_AVAIL * 1024))")"
fi
