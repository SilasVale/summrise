#!/usr/bin/env bash
# release-lib.sh — testable stages of publish-release.sh, extracted verbatim
# (structure refactor: the last-5-per-minor prune and the version.json writer
# are pure file operations; scripts/test/release-lib.bash pins them so the
# round-309 keep-policy can never silently regress).

# Write the CDN manifest the agent_update tool consumes (round-119: sha256
# REQUIRED). $1 = version, $2 = packed tgz path, $3 = output dir,
# $4 = optional staged installer exe path (SummriseAgent-Setup-<ver>.exe).
# Echoes the sha256 so the caller keeps it for the reconcile stages.
# The installer fields are ADDITIVE (old readers ignore them): when $4 is
# given and exists, the manifest also carries installer (flat basename) +
# installer_sha256 so fresh installs are verifiable the same way tgz
# updates are. When absent (older flow / installer not rebuilt yet), the
# manifest keeps the tgz-only shape and every old consumer keeps working.
write_version_json() {
  local ver="$1" tgz="$2" out="$3" installer_exe="${4:-}"
  local sha
  sha=$(sha256sum "$tgz" | cut -d' ' -f1)
  # THE BOXED COMPONENTS, PINNED (grilling Q4, 2026-09-23). `summrise setup` fetches
  # cloudflared, the playwright bundle and the electron runtime from this host and
  # verified NONE of them: a worker serving different bytes would have been staged
  # without complaint. The manifest now carries a URL and a sha256 per component and
  # setup REFUSES a mismatch. The pins live in index/components.json, checked in
  # beside the worker that serves them; publish-release.sh cross-checks cloudflared's
  # against the agent's own CLOUDFLARED_SHA256, because the agent re-checks that one
  # at run time and the two must not drift. Absent file => the manifest keeps its old
  # shape and every old consumer keeps working.
  local comp=""
  if [[ -f index/components.json ]]; then
    comp=$(python3 - <<'PY'
import json
pins = {k: v for k, v in json.load(open("index/components.json")).items() if not k.startswith("_")}
if pins:
    print(',"components":' + json.dumps(pins, separators=(",", ":"), sort_keys=True), end="")
PY
)
  fi
  if [[ -n "$installer_exe" && -f "$installer_exe" ]]; then
    local ish
    ish=$(sha256sum "$installer_exe" | cut -d' ' -f1)
    printf '{"version":"%s","tarball":"summrise-agent-latest.tgz","updated":"%s","sha256":"%s","installer":"%s","installer_sha256":"%s"%s}\n' \
      "$ver" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$sha" "$(basename "$installer_exe")" "$ish" "$comp" > "$out/version.json"
  else
    printf '{"version":"%s","tarball":"summrise-agent-latest.tgz","updated":"%s","sha256":"%s"%s}\n' \
      "$ver" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$sha" "$comp" > "$out/version.json"
  fi
  echo "$sha"
}

# Last-5-per-minor prune (round-309): keep the newest 5 of EACH major.minor
# line + the latest alias; delete every other summrise-agent-1.*.*.tgz. A flat
# last-5 across all 1.x would evict the previous minor line the moment the
# new line ships 5 releases and break pinned installs. $1 = asset dir.
prune_last5_per_minor() {
  local dir="$1"
  # Captured BEFORE the shopt below: capturing after it would "restore" the state this function
  # just imposed (the first version did exactly that, and the leak survived its own fix).
  local _nullglob_was="$(shopt -p nullglob || true)"
  shopt -s nullglob
  mapfile -t KEEP < <(ls "$dir"/summrise-agent-1.*.*.tgz 2>/dev/null | grep -v latest | sort -V | awk '
    { ver = $0; sub(/.*summrise-agent-/, "", ver); sub(/\.tgz$/, "", ver); n = split(ver, a, "."); key = a[1] "." a[2]; c[key]++; line[key, c[key]] = $0 }
    END { for (k in c) { from = (c[k] > 5 ? c[k] - 4 : 1); for (i = from; i <= c[k]; i++) print line[k, i] } }
  ')
  local f k keep=0
  for f in "$dir"/summrise-agent-1.*.*.tgz; do
    keep=0
    for k in "${KEEP[@]}"; do [ "$k" = "$f" ] && keep=1 && break; done
    if [ "$keep" -eq 0 ]; then rm -f "$f"; echo "pruned $(basename "$f")"; fi
  done

  # A PRUNE THAT ONLY KNOWS THE CURRENT NAME CANNOT CLEAN UP AFTER A RENAME, which is exactly when it is
  # needed (round 132). Every glob above matches `summrise-agent-*`, so the six installers this repo
  # published while the product was called `vale` were never candidates: they sat in the asset directory
  # (~40 MB, dated months earlier, including a `vale-agent-latest.tgz` alias pointing at 1.2.451) and were
  # still answering HTTP 200 from the CDN long after the rename. Nothing in this repo named them — no
  # route, no test, no doc — which is why nothing removed them either.
  #
  # NAMED EXPLICITLY RATHER THAN MATCHED LOOSELY. A `*-agent-*.tgz` sweep would delete a FUTURE product's
  # assets the day it shares this directory; a declared list of superseded names is the same shape this
  # repo uses for retired colours and production hosts, and a rename adds one word here.
  local SUPERSEDED_NAMES="vale-agent"
  local s
  for s in $SUPERSEDED_NAMES; do
    for f in "$dir"/"$s"-*.tgz; do
      rm -f "$f"
      echo "pruned $(basename "$f") (superseded product name)"
    done
  done
  eval "$_nullglob_was"
}

# Installer prune: keep the newest $2 (default 5) versioned
# SummriseAgent-Setup-1.*.*.exe PER MAJOR.MINOR, plus the versionless
# SummriseAgent-Setup.exe alias (never pruned — the landing page links it when the
# manifest advertises one).
#
# PER MINOR, because this comment used to CALL ITSELF the companion to the tgz
# last-5-per-minor policy above while doing something else: a flat last-5 across
# all 1.x. Round 127 measured what that costs — with five 1.2.x installers and a
# 1.3 line at the cap, the flat policy deleted EVERY 1.2.x installer. A versioned
# installer pins the tgz it was built against, so the previous line's rollback
# path went with them — the exact loss the tgz policy exists to prevent, and its
# own comment says so 20 lines above. The one-minor test fixture could not tell
# the two policies apart; there is a two-minor one now.
prune_installers() {
  local dir="$1" keep_n="${2:-5}"
  # Captured BEFORE the shopt below: capturing after it would "restore" the state this function
  # just imposed (the first version did exactly that, and the leak survived its own fix).
  local _nullglob_was="$(shopt -p nullglob || true)"
  shopt -s nullglob
  mapfile -t KEEP_EXE < <(ls "$dir"/SummriseAgent-Setup-1.*.*.exe 2>/dev/null | sort -V | awk -v keep="$keep_n" '
    { ver = $0; sub(/.*SummriseAgent-Setup-/, "", ver); sub(/\.exe$/, "", ver); n = split(ver, a, "."); key = a[1] "." a[2]; c[key]++; line[key, c[key]] = $0 }
    END { for (k in c) { from = (c[k] > keep ? c[k] - keep + 1 : 1); for (i = from; i <= c[k]; i++) print line[k, i] } }
  ')
  local f k keep=0
  for f in "$dir"/SummriseAgent-Setup-1.*.*.exe; do
    keep=0
    for k in "${KEEP_EXE[@]}"; do [ "$k" = "$f" ] && keep=1 && break; done
    if [ "$keep" -eq 0 ]; then rm -f "$f"; echo "pruned $(basename "$f")"; fi
  done
  eval "$_nullglob_was"
}

# ── hashing a DOWNLOAD that may not exist (round 27) ─────────────────────────
# `curl … | sha256sum | cut -d' ' -f1` LOOKS like "hash it, or empty on failure" and is not:
# when curl fails (a 404, a timeout) it writes nothing, sha256sum still hashes the EMPTY input
# and prints e3b0c442…, and the pipeline's status is `cut`'s — so the caller sees a HASH, not
# an absence. Measured on the live CDN: after the retired installer alias was deleted (404
# confirmed by hand) the smoke still reported "the alias still serves a build", i.e. the check
# could never say "absent" and would have warned forever.
#
# Download to a file first, then hash only what actually arrived. Empty output = no download.
sha256_of_url() {
  local url="$1" tmp
  tmp="$(mktemp)"
  if curl -fsSL -m 120 -o "$tmp" "$url" 2>/dev/null; then
    sha256sum <"$tmp" | cut -d' ' -f1
  fi
  rm -f "$tmp"
}

# ── retiring the NSIS installer (round 27) ───────────────────────────────────
# The installer stopped being a channel on 2026-08-28 (npm only), but its artifacts kept
# being STAGED in the asset dir and uploaded by every deploy. Measured 2026-09-15: six exes
# from 1.2.358 to 1.2.365 (40 MB) were still there, and the versionless alias among them
# answered 200 on the CDN serving **1.2.365** while the release was 1.2.406 — a publicly
# downloadable build from 41 releases ago, which the smoke could only WARN about on every
# publish. A warning that cannot be cleared is noise, so the publish flow now removes them
# when it is not building one: the next deploy drops them from the manifest and the URLs
# 404, which the worker already handles by design ("a missing exe must 404, never the
# landing page as 200 HTML").
#
# Echoes the count and the bytes it freed, so a publish log says what happened to them.
retire_installers() {
  local dir="$1"
  # Captured BEFORE the shopt below: capturing after it would "restore" the state this function
  # just imposed (the first version did exactly that, and the leak survived its own fix).
  local _nullglob_was="$(shopt -p nullglob || true)"
  shopt -s nullglob
  local files=("$dir"/SummriseAgent-Setup*.exe)
  if [ "${#files[@]}" -eq 0 ]; then
    echo "no staged installer to retire (npm is the channel)"
    eval "$_nullglob_was"
    return 0
  fi
  local bytes
  bytes="$(du -ch "${files[@]}" 2>/dev/null | tail -1 | cut -f1)"
  rm -f "${files[@]}"
  echo "retired ${#files[@]} staged installer(s) ($bytes) — installer URLs will 404 after this deploy"
  eval "$_nullglob_was"
}

# ── the reconcile ledger (round 123) ─────────────────────────────────────────
# WHAT THIS IS FOR. `--skip-reconcile` publishes to the CDN with no GitHub
# release to audit against, and until round 123 that fact lived ONLY in the
# operator's scrollback: no file recorded it, so nothing could refuse the next
# one. Measured 2026-09-14: the newest GitHub release was v1.2.361 while the CDN
# had shipped 1.2.363 and 1.2.364 — the debt was three versions old, invisible,
# and `scripts/lib/release-audit.sh` had never once run against a real release.
#
# One line per version, `#` comments allowed, TRACKED in git so the debt
# survives this machine and shows up in review. The alternative — keeping it on
# the CDN next to version.json — was rejected: a state file that only exists
# where the publish puts it cannot fail a publish that has not happened yet.
# NOT TRACKED, and the code tolerates that on purpose (`[ -f … ] || return 0` below) — the file records which
# versions still owe a CDN-vs-GitHub reconcile, and it lives on this machine. An earlier comment here called it
# "TRACKED in git"; `git ls-files docs/agents/` has never listed it.
RECONCILE_LEDGER="${RECONCILE_LEDGER:-docs/agents/release-reconcile.txt}"

# Echo the versions that owe a reconcile, one per line, in file order.
#
# ONE awk, NOT `grep -v | awk`. The pipeline version returned grep's status —
# 1 when the ledger held only comments — and under `set -e -o pipefail` that
# killed publish-release.sh SILENTLY the first time the ledger existed with every
# debt settled. Its own test caught it (the suite died with no output, which is
# what a `set -e` death inside a command substitution looks like). awk exits 0 on
# empty input, so the question "what is owed?" always has an answer.
reconcile_pending() {
  [ -f "$RECONCILE_LEDGER" ] || return 0
  awk '!/^[[:space:]]*#/ && NF {print $1}' "$RECONCILE_LEDGER"
}

# Record <ver> as published-but-unreconciled. Idempotent: a re-run must not grow
# the ledger, or "how many versions owe a reconcile" becomes unanswerable.
reconcile_record() {
  local ver="$1" note="${2:-published with no GitHub release to audit against}"
  # here-string, not `producer | grep -q`: under pipefail an early-exiting grep
  # SIGPIPEs the producer and the pipeline reports failure even on a match
  # (round-288, learned the same way in publish-release.sh).
  if grep -qx "$ver" <<<"$(reconcile_pending)"; then return 0; fi
  mkdir -p "$(dirname "$RECONCILE_LEDGER")"
  if [ ! -f "$RECONCILE_LEDGER" ]; then
    printf '# Versions on the CDN with no GitHub release to audit against.\n' > "$RECONCILE_LEDGER"
    printf '# Written by scripts/publish-release.sh --skip-reconcile; cleared by --audit-only <ver>.\n' >> "$RECONCILE_LEDGER"
  fi
  printf '%s %s %s\n' "$ver" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$note" >> "$RECONCILE_LEDGER"
}

# Clear <ver> — its audit finally ran and passed. Comments are preserved.
reconcile_clear() {
  local ver="$1" tmp
  [ -f "$RECONCILE_LEDGER" ] || return 0
  tmp="$(mktemp)"
  awk -v v="$ver" '/^[[:space:]]*#/ {print; next} $1 != v {print}' "$RECONCILE_LEDGER" > "$tmp"
  mv "$tmp" "$RECONCILE_LEDGER"
}

# ── the reconcile gate's DIAGNOSIS: ask, then name only what was observed ────
# THE GATE NAMED A CAUSE IT HAD NOT CHECKED. Measured 2026-09-28 against its own ledger,
# which held 1.2.453: it refused with "these versions are on the CDN with no GitHub release
# to audit against", and EVERY clause of that sentence was false — GET
# /releases/tags/v1.2.453 answers 200 (the release exists, and did at publish time), and the
# CDN answers 404 for summrise-agent-1.2.453.tgz, so the version is not "on the CDN" either.
# The sentence sent a reader to CREATE A RELEASE THAT ALREADY EXISTED, while the road that
# works — `--acknowledge-unreconciled`, accepting that the entry can never be settled because
# the tarball it would be audited against is gone — went unnamed. And the script already knew
# how to say it: `--audit-only 1.2.453`, the road THIS message recommends, prints
# "cannot download the CDN tgz". Two sentences about one version, one of them wrong, and the
# wrong one is the one a refusing gate prints first.
#
# So the gate ASKS before it names: GitHub for the release, the CDN for the tgz. Each answer
# has THREE states, not two — present, absent, and `unknown` for a probe that could not answer
# at all (no token, offline, timeout, DNS, an unrecognised status). The third is the one this
# repository keeps having to add: a check that fails open and reports a confident wrong cause
# is worse than one that says it does not know, so an unanswerable probe claims NEITHER cause.

# http_asset_verdict <http-status-code> → present | absent | unknown
#
# PURE, so the judgement is unit-tested without a network:
#   2xx        the object is served.
#   404 / 410  the object is NOT there. 410 Gone is the same fact stated permanently, and it
#              means exactly what 404 means here — there is nothing left to audit.
#   everything else — 3xx, 5xx, an empty answer, curl's own 000 — is `unknown`. A redirect is
#   not an answer to "is it there" without following it, and following it is a second request
#   this gate does not need: saying "could not tell" beats guessing in either direction.
http_asset_verdict() {
  case "${1:-}" in
    2*) echo present ;;
    404|410) echo absent ;;
    *) echo unknown ;;
  esac
}

# cdn_asset_verdict <url> → prints "<verdict> <http-code>"
#
# ONE HEAD REQUEST, NEVER A DOWNLOAD. The audit path downloads the 6.7 MB tgz because it has to
# hash it; this probe only has to learn whether the object is THERE, and the script already asks
# the CDN that question this way in its own post-publish checklist
# (`curl -sI $CDN_BASE/summrise-agent/SummriseAgent-Setup.exe | grep -i etag`). Measured on the
# live CDN: HEAD /summrise-agent/summrise-agent-1.2.453.tgz → 404, …-1.2.492.tgz → 200.
#
# THE TIMEOUT IS BOUNDED AND SHORT ON PURPOSE, and it is the whole of the risk this probe adds:
# it runs INSIDE A REFUSAL — only when the ledger is non-empty and not acknowledged — and a
# refusal that hangs is worse than one that names less. The audit's 300 s belongs to a 6.7 MB
# body with a 20 KB/s stall floor; a HEAD carries no body, so 20 s is already generous, and
# `--connect-timeout` bounds the case that actually hangs a dry run (a SYN to a host that never
# answers). A probe that hits either bound returns `unknown`, which is a verdict, not a failure.
CDN_PROBE_TIMEOUT="${CDN_PROBE_TIMEOUT:-20}"
CDN_PROBE_CONNECT_TIMEOUT="${CDN_PROBE_CONNECT_TIMEOUT:-10}"
cdn_asset_verdict() {
  local url="$1" code=""
  code="$(curl -sS -o /dev/null -w '%{http_code}' -I \
            -m "$CDN_PROBE_TIMEOUT" --connect-timeout "$CDN_PROBE_CONNECT_TIMEOUT" \
            "$url" 2>/dev/null)" || code=""
  printf '%s %s\n' "$(http_asset_verdict "$code")" "${code:-000}"
}

# reconcile_entry_limb <release-verdict> <cdn-verdict> → the limb, ONE classification
#
#   no_release   the GitHub release does not exist      → the original sentence is true
#   tgz_gone     release present, CDN tgz absent        → can never be settled by --audit-only
#   settleable   release present, CDN tgz present       → not this condition at all; settle it
#   cdn_unknown  release present, CDN did not answer    → claim neither cause
#   rel_unknown  GitHub did not answer                  → claim neither cause
#
# THE TWO UNANSWERABLE LIMBS ARE SEPARATE ON PURPOSE: "which question went unanswered" is the
# thing a reader needs, and collapsing them printed a sentence that could not say. Both are
# read as "cannot claim" by the fold below.
reconcile_entry_limb() {
  case "${1:-}" in
    absent) echo no_release ;;
    present)
      case "${2:-}" in
        absent) echo tgz_gone ;;
        present) echo settleable ;;
        *) echo cdn_unknown ;;
      esac
      ;;
    *) echo rel_unknown ;;
  esac
}

# reconcile_settleable_verdict <limb…> → yes | no | maybe
#   yes    at least one entry is on a road --audit-only can travel (no_release once its tag
#          exists, or settleable now), and every entry was classified.
#   no     EVERY entry is tgz_gone: the footer's "Settle one: --audit-only" line would send a
#          reader down the dead road this whole change exists to stop recommending.
#   maybe  something was unanswerable, and a footer must not claim to know about it.
#
# WHY THE FOOTER NEEDS THIS. Naming the cause per version is not enough while the advice two
# lines below still recommends the dead road to every one of them — that is the brief's own
# complaint ("two sentences, two different roads, one of them dead") and a fixed pair is one
# where the footer stops recommending a road that cannot be travelled.
reconcile_settleable_verdict() {
  local l any_road=0 any_dead=0 any_unknown=0
  for l in "$@"; do
    case "$l" in
      no_release|settleable) any_road=1 ;;
      tgz_gone) any_dead=1 ;;
      *) any_unknown=1 ;;
    esac
  done
  if [ "$any_unknown" -eq 1 ]; then echo maybe; return 0; fi
  if [ "$any_road" -eq 1 ]; then echo yes; return 0; fi
  if [ "$any_dead" -eq 1 ]; then echo no; return 0; fi
  echo maybe   # no entries at all: nothing was classified, so nothing may be claimed
}

# reconcile_entry_lines <ver> <limb> <release-detail> <cdn-code>
#
# ONE LINE PER OBSERVED STATE, and the limb decides which. The old message asserted the first
# limb for every version, whatever was true.
#   no_release   the ORIGINAL SENTENCE, kept verbatim — this is the limb it is true for, and
#                there is genuinely nothing to audit against.
#   tgz_gone     name the observation, name the version, and name the only road left: this
#                entry can never be settled, because --audit-only has no tarball to download.
#   settleable   say that the condition does not hold, and point at the road that works.
#   cdn_unknown  the release is there; the CDN did not answer. Name which of the two facts is
#   rel_unknown  missing, and claim neither cause.
#
# <release-detail> is the unanswerable probe's own words, printed as ITS line rather than
# folded into this gate's sentence — the probe's message is not this gate's message, and a
# reader has to be able to tell which one is guessing. <cdn-code> is the status the CDN
# answered with (000 when nothing answered).
reconcile_entry_lines() {
  local ver="$1" limb="$2" rel_detail="${3:-}" cdn_code="${4:-}"
  case "$limb" in
    no_release)
      printf '  %s  on the CDN with no GitHub release to audit against   (GitHub release v%s: does not exist)\n' \
        "$ver" "$ver"
      ;;
    tgz_gone)
      printf '  %s  release v%s EXISTS but the CDN tgz is GONE (HTTP %s)\n' "$ver" "$ver" "${cdn_code:-404}"
      printf '        there is no tarball left to download, so --audit-only can never settle this entry.\n'
      printf '        The only road left:  rerun with --acknowledge-unreconciled\n'
      ;;
    settleable)
      printf '  %s  release v%s EXISTS and the CDN serves its tgz (HTTP %s) — settleable, not blocked\n' \
        "$ver" "$ver" "${cdn_code:-200}"
      printf '        settle it now:  ./scripts/publish-release.sh --audit-only %s\n' "$ver"
      ;;
    cdn_unknown)
      printf '  %s  release v%s EXISTS, but whether its CDN tgz is there could not be determined (HTTP %s)\n' \
        "$ver" "$ver" "${cdn_code:-000}"
      printf '        — this run does not claim either cause\n'
      ;;
    *)
      printf '  %s  could not check whether release v%s exists — this run does not claim either cause\n' \
        "$ver" "$ver"
      # `if`, not `[ … ] &&`: a false test as the last command of a function is a non-zero
      # status, and `set -e` in the caller would end the whole publish over a printed line.
      if [ -n "$rel_detail" ]; then printf '        the probe said: %s\n' "$rel_detail"; fi
      ;;
  esac
}

# ── the installer manifest verdict (round 130) ───────────────────────────────
# A DEPLOY THAT LEAVES THE MANIFEST BEHIND IS NOT A SUCCESS.
# `build-installer.sh`'s default path staged the exe, deployed the worker and
# printed "== done ==" with the new URLs — while `/api/version` still described
# the PREVIOUS installer (or none), because nothing rewrote version.json. The
# landing page asks the manifest before offering the button (round 125), so that
# build was invisible at best and advertised a stale digest at worst, and the run
# said it succeeded. `publish-release.sh` tells operators to run this script
# standalone, so the path is not hypothetical.
#
# Echoes one verdict for <manifest-json> <sha256-of-the-exe-just-deployed>:
#   ok               the manifest advertises exactly this build
#   absent           no installer is advertised at all — this door will not link it
#   mismatch:<sha>   the manifest advertises a DIFFERENT installer
installer_manifest_verdict() {
  local json="$1" want="$2" got
  got="$(printf '%s' "$json" | grep -oP '"installer_sha256":"\K[^"]+' | head -1)" || got=""
  if [ -z "$got" ]; then echo "absent"; return 0; fi
  if [ "$got" = "$want" ]; then echo "ok"; return 0; fi
  echo "mismatch:$got"
}

# worktree_mode_verdict <repo_root> <npm_dir> [<pathspec>...]
# Prints one line per file whose WORKTREE mode differs from the mode git records,
# and returns 0 when none do. Extracted from publish-release.sh (round 154) so the
# gate has BEHAVIOURAL tests: `npm pack` preserves worktree modes, so a 0600 file
# packs a tarball that differs from a fresh checkout's by its tar HEADER alone —
# the measured cause of twenty consecutive "packaging metadata" WARNs and of the
# 3-byte drift on the 1.2.348 pair (README.md's mode field plus the header
# checksum, with every file's sha256 matching, the exe included). The gate sat
# mid-chain in the orchestrator, behind the reconcile gate, the version check and
# the exe check, so no test could reach it; this is that logic, reachable.
#
# THE SUBJECT IS AN ARGUMENT, AND THE PACK INPUTS ARE ONLY ITS DEFAULT (measured
# 2026-09-28). The publish gate needs the twenty-four files npm packs; the RULE is
# wider than that, because this box's umask is 0002 and git compares only the
# owner-execute bit — ANY tracked file rewritten here can land as 664 (or 775)
# while the index records 644 (755), `git status` says nothing and `git diff` is
# empty. A reader who counts that class with `awk '$1=="100644"'` cannot see the
# 0755 half of it AT ALL: fifteen files were drifted that way on this checkout
# when this parameter was added, EVERY one of them outside the pack inputs. So a
# pathspec REPLACES the subject (`.` is the whole tree, which is what the ordinary
# local gate run asks for) and the pack inputs stay the default, leaving the
# publish path's subject — and its refusal — unchanged.
#
# ONE PASS, NOT ONE `git` PER FILE. Measured over this tree's 985 tracked files:
# the per-file `git ls-files -s` + `stat` shape this started as took 10.6 s, one
# `git` with a `stat` per file took 4.4 s, and this — one `git`, one `xargs stat`,
# one `awk` join — takes 0.05 s, which is what lets it run on every commit instead
# of at publish time.
#
# IT REFUSES; IT DOES NOT REPAIR. A verdict that chmod-ed would be a gate that
# cannot fail, and this repository's standard is that a gate that cannot be broken
# is worse than no gate. So it names each file, the mode it has, the mode git
# records, and the exact command that repairs the tree.
#
# The line format is kept verbatim: the 1.2.348 investigation quoted
# `<file> (worktree 664, this checkout should be 644)`.
worktree_mode_verdict() {
  local root="${1:?repo root}" npm_dir="${2:?npm dir}"
  shift 2
  local -a spec=("$@")
  if [ "${#spec[@]}" -eq 0 ]; then
    spec=("$npm_dir/bin" "$npm_dir/src" "$npm_dir/test" "$npm_dir/README.md" \
          "$npm_dir/summrise-desktop-electron" "agent/summrise-desktop-electron")
  fi
  local idx wt bad="" n=0 k=0
  idx=$(mktemp) || return 1
  wt=$(mktemp) || { rm -f "$idx"; return 1; }
  # The index side: <mode> TAB <path>, in ONE `git` call for the whole subject.
  git -C "$root" ls-files -s -- "${spec[@]}" |
    awk -F'\t' '{split($1, a, " "); print a[1] "\t" $2}' > "$idx"
  # AN EMPTY SUBJECT PROVES NOTHING, and answering 0 would be the vacuity this
  # suite reports elsewhere as "exited 0 having printed NOTHING".
  if [ ! -s "$idx" ]; then
    rm -f "$idx" "$wt"
    echo "no tracked file matched the subject (${spec[*]}) — this proves nothing"
    return 1
  fi
  n=$(wc -l < "$idx")
  # The worktree side, from $root so the index's relative paths resolve. A path
  # that no longer exists (a deletion in progress) prints nothing and is skipped,
  # exactly as the per-file `[ -f ]` guard it replaces did.
  git -C "$root" ls-files -z -- "${spec[@]}" |
    (cd "$root" && xargs -0 -r stat -c '%a %n' 2>/dev/null) > "$wt"
  if [ ! -s "$wt" ]; then
    rm -f "$idx" "$wt"
    echo "no worktree mode could be read for the subject (${spec[*]}) — this proves nothing"
    return 1
  fi
  # 100644 and 100755 are the modes this repository's index carries; 120000 (a
  # symlink) is 777 in the worktree; anything else (a gitlink) has no worktree mode
  # to compare and is skipped. A path containing a TAB would not join here — this
  # repository tracks none (measured: 0 of 985), and `stat -c` offers no separator
  # that survives one.
  bad=$(awk -F'\t' '
    NR == FNR {
      want[$2] = ($1 == "100644") ? 644 : ($1 == "100755") ? 755 : ($1 == "120000") ? 777 : 0
      next
    }
    {
      p = $0; sub(/^[0-9]+ /, "", p)
      m = $0; sub(/ .*$/, "", m)
      w = want[p]
      if (w != "" && w != 0 && m != w) printf "%s (worktree %s, this checkout should be %s)\n", p, m, w
    }' "$idx" "$wt")
  rm -f "$idx" "$wt"
  if [ -n "$bad" ]; then
    printf '%s\n' "$bad"
    k=$(printf '%s\n' "$bad" | wc -l)
    printf '%s\n' "$k of $n checked file(s) disagree with the worktree mode git records."
    # ONE line, so a reader of all-gates — which shows the LAST FOUR lines of this
    # gate's output — sees the repair as well as the refusal.
    printf '%s\n' "fix (whole tree, one line): git ls-files -s | awk '\$1==\"100644\"{print \$4}' | xargs -r chmod 644 && git ls-files -s | awk '\$1==\"100755\"{print \$4}' | xargs -r chmod 755"
    return 1
  fi
  return 0
}

# component_route_verdict <repo_root>
# Prints one line per copy of a boxed component's CDN path that names something the index
# worker does NOT serve, and returns 0 when every declared copy agrees with the route table.
#
# WHY THIS EXISTS (2026-09-25). Measured on the live CDN: version.json carried a `url` per
# component BESIDE its sha256, every consumer read the DIGEST, and the url was read by NOBODY —
# while the same CDN path was retyped in four places. Two of those readers (the npm CLI's
# resolveComponent, the online installer's component blocks) already fetched the manifest for the
# digest and now take the ADDRESS from the same body; the copies that cannot fetch anything keep
# the path and are compared HERE against the one derivation that is not a copy: the routes
# index/src/index.js answers. A copy that moves without the route, or a route that moves without
# the copies, is the drift this refuses.
#
# PATHS, NOT HOSTS. The worker REBUILDS every published url against the origin of the request
# that asked for /api/version, so a host spelled in these files belongs to whatever mirror they
# point at; the path is the part that must be served. That is also why reading the manifest in
# those two readers changes no bytes today: same path, same origin.
#
# THE EXTRACTION IS PER FILE because the copies spell different things (a full url, a $CdnBase
# expression, a call argument, an OUT default) — and every declared copy must yield at least one
# path: a rule that silently stops matching proves nothing, which is the failure all-gates
# reports as a gate that "exited 0 having printed NOTHING". A copy that is genuinely gone gets
# removed from the list below, in the commit that removes it.
component_route_verdict() {
  local root="${1:?repo root}"
  local worker="$root/index/src/index.js"
  local report="" served
  [ -f "$worker" ] || { echo "the index worker is missing ($worker) — the addresses cannot be compared to anything"; return 1; }
  # The served paths, read from the worker's own route table rather than from any of the copies.
  served="$(grep -oE 'pathname === "/summrise-agent/[A-Za-z0-9._-]+"' "$worker" | grep -oE '/summrise-agent/[A-Za-z0-9._-]+' | sort -u)"
  if [ -z "$served" ]; then
    echo "no /summrise-agent route could be read from index/src/index.js — the route table moved, so this proves nothing"
    return 1
  fi

  # 1. THE PIN FILE. publish-release.sh copies index/components.json verbatim into version.json's
  # `components` block, so its url is what the static manifest publishes to the world.
  local pins="$root/index/components.json" u p n=0
  if [ -f "$pins" ]; then
    while IFS= read -r u; do
      [ -n "$u" ] || continue
      n=$((n + 1))
      p="${u#*://}"; p="/${p#*/}"
      grep -qxF "$p" <<<"$served" || report="${report}index/components.json publishes $p, which index/src/index.js does not serve"$'\n'
    done < <(python3 -c "
import json
try:
    pins = json.load(open('$pins'))
except Exception:
    pins = {}   # an unreadable pin file reports BELOW as 'no component url could be read'
for k, v in sorted(pins.items()):
    if not k.startswith('_') and isinstance(v, dict):
        print(v.get('url') or '')
")
    [ "$n" -gt 0 ] || report="${report}no component url could be read from index/components.json — the pin file moved, so this proves nothing"$'\n'
  fi

  # 2. THE INSTALLER'S FALLBACKS. `summrise-online-setup.ps1` now takes each component's address
  # from the manifest it reads (Get-ComponentUrl) and keeps this path for the arm where that
  # manifest cannot be read at all — the one copy that is genuinely offline, so it is the one that
  # most needs a pin.
  local ps1="$root/agent/deploy/summrise-online-setup.ps1"
  if [ -f "$ps1" ]; then
    n=0
    while IFS= read -r p; do
      [ -n "$p" ] || continue
      n=$((n + 1))
      grep -qxF "/summrise-agent/$p" <<<"$served" || report="${report}agent/deploy/summrise-online-setup.ps1 spells /summrise-agent/$p, which index/src/index.js does not serve"$'\n'
    done < <(grep -oP '\$CdnBase/summrise-agent/[A-Za-z0-9._-]+(?=")' "$ps1" | sed 's|.*/summrise-agent/||' | sort -u)
    [ "$n" -gt 0 ] || report="${report}no \$CdnBase/summrise-agent path could be read from agent/deploy/summrise-online-setup.ps1 — the fallbacks moved, so this proves nothing"$'\n'
  fi

  # 3. THE CLI'S CALL SITES. resolveComponent's first argument is the file the release host is
  # asked for when the manifest is silent, so it must be a name the worker serves — the manifest
  # url wins while it is there, which is exactly what would hide this drift until the first device
  # that cannot read the manifest.
  local ts="$root/agent/summrise-agent-npm/src/summrise.ts"
  if [ -f "$ts" ]; then
    n=0
    while IFS= read -r p; do
      [ -n "$p" ] || continue
      n=$((n + 1))
      grep -qxF "/summrise-agent/$p" <<<"$served" || report="${report}agent/summrise-agent-npm/src/summrise.ts asks setup for /summrise-agent/$p, which index/src/index.js does not serve"$'\n'
    done < <(python3 -c "
import re
src = open('$ts').read()
print('\n'.join(sorted(set(re.findall(r'resolveComponent\(\s*\"([A-Za-z0-9._-]+)\"', src)))))
")
    [ "$n" -gt 0 ] || report="${report}no resolveComponent call site could be read from agent/summrise-agent-npm/src/summrise.ts — the loader moved, so this proves nothing"$'\n'
  fi

  # 4. THE BUNDLE PRODUCER. build-playwright-bundle.sh writes the artefact the worker serves at
  # the playwright route; its OUT default is the last place that path is spelled.
  local bundle="$root/scripts/build-playwright-bundle.sh"
  if [ -f "$bundle" ]; then
    n=0
    while IFS= read -r p; do
      [ -n "$p" ] || continue
      n=$((n + 1))
      grep -qxF "/summrise-agent/$p" <<<"$served" || report="${report}scripts/build-playwright-bundle.sh writes index/public/summrise-agent/$p, which index/src/index.js does not serve"$'\n'
    done < <(grep -oP 'index/public/summrise-agent/[A-Za-z0-9._-]+' "$bundle" | sed 's|.*/summrise-agent/||' | sort -u)
    [ "$n" -gt 0 ] || report="${report}no index/public/summrise-agent path could be read from scripts/build-playwright-bundle.sh — the OUT default moved, so this proves nothing"$'\n'
  fi

  if [ -n "$report" ]; then printf '%s' "$report"; return 1; fi
  return 0
}

# cf_token — the Cloudflare API token: env first, then ~/.cloudflare-token.
#
# ONE OWNER. It used to be written out four times, byte-identical, in build.sh,
# publish-release.sh, build-installer.sh and publish-cdn-from-ci.sh — and the copies had already
# started to diverge in their COMMENTS, which is how a fifth copy gets written with a fifth idea.
#
# AND IT TRIMS, which the copies did not. The npm path learned this the expensive way on
# 2026-09-23: a token file written with a trailing newline authenticates as nothing, and "401
# while the file looks right" cost an hour. There is no reason the Cloudflare token file is a
# different kind of file, and every consumer here wants the token, not the bytes.
#
# Pure and side-effect free: reads ${CLOUDFLARE_API_TOKEN} or $HOME/.cloudflare-token, prints the
# token or nothing. unit-tested with a fixture HOME.
cf_token() {
  if [[ -n "${CLOUDFLARE_API_TOKEN:-}" ]]; then printf '%s' "$CLOUDFLARE_API_TOKEN";
  elif [[ -f "$HOME/.cloudflare-token" ]]; then tr -d ' \t\r\n' < "$HOME/.cloudflare-token";
  else printf ''; fi
}
