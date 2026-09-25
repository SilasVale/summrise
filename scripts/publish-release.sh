#!/usr/bin/env bash
# Summrise agent release publisher — the ONE command for a CDN release.
#
#   ./scripts/publish-release.sh <1.2.N> [--skip-reconcile] [--with-installer] [--npm [--npm-tag alpha|next|latest]]
#
# Assumes the exe is already built and staged (cargo xwin build + cp into
# agent/summrise-agent-npm/summrise-agent.exe) and package.json version == 1.2.N.
#
# Steps:
#   1. npm pack in agent/summrise-agent-npm -> summrise-agent-1.2.N.tgz
#   2. stage tgz + versionless latest alias into index/public/summrise-agent
#   2b. [--with-installer] build the SELF-CONTAINED installer (NSIS bundles
#      the staged tgz; --no-deploy here — the single deploy in step 6 covers
#      everything, so the manifest and the installer never disagree)
#   3. write version.json {version, tarball, updated, sha256} (sha256 of the
#      packed tgz — agent_update REQUIRES it, round-119) + installer fields
#      when the exe is staged
#   4. LAST-5-PER-MINOR PRUNE (round-309 lesson): delete every
#      summrise-agent-1.*.*.tgz older than the newest 5 OF ITS minor line, so
#      defective releases are not downloadable (this policy was never
#      enforced on manual publishes and 46 old tgz accumulated on the CDN)
#      without evicting the previous minor line (pinned installs keep
#      working while the new line ramps)
#   5. wrangler deploy (CDN sync — deletes pruned assets too) + the post-publish smoke
#   6. [--npm] publish THE SAME PACK to the npm registry — BEFORE THE AUDIT, on purpose:
#      the audit cannot pass on a first publish (the asset is built after the tag) and its
#      failure branch exits, so a step ordered after it never runs at all — measured on
#      1.2.454, where the CDN deployed, the smoke went green, --npm was passed, and the
#      registry still answered `latest: 1.2.453`.
#   7. P0 AUDIT: CDN tgz vs GitHub release asset (scripts/lib/release-audit.sh).
#   THEN A HUMAN — this is where the command ends. Commit the two tracked files, push, WAIT
#   for CI to go green on that commit, tag through the API, let release.yml attach the
#   asset, then `--audit-only <ver>` to settle the debt. Every one of those orderings was
#   paid for by a failed release; the transcripts are in docs/agents/design-ledger.md.
#      Every SOURCE-DERIVED file must match byte-for-byte; only summrise-agent.exe
#      may differ, because the two builders do not share a toolchain (local
#      rustc stable + hand-built llvm18 vs release.yml's floating stable +
#      distro llvm). Whole-tarball equality was the old rule and could never
#      pass here, which is how the gate became a skip. --skip-reconcile covers
#      only a genuine first publish (no asset built yet) and refuses once one
#      exists.
#
#   8. [--npm] the dist-tag: `latest` by default, `alpha|next` for a deliberate
#      prerelease. NOT alpha-by-default — this product's CDN `-latest.tgz` alias moves on
#      every release, so the registry's `latest` has to move with it, or `npm i -g
#      summrise-agent` installs a CLI OLDER than the release the agent is being asked to
#      take (the 1.2.453 deadlock). dsh can afford alpha-first because its `latest` is a
#      release candidate, not an alias a device already follows. Without --npm the run
#      prints a ::warning:: naming the consequence rather than drifting in silence.
# After this: push main, create the GitHub tag v1.2.N via the API, and let
# release.yml build the GitHub release asset (keep-latest manual).

set -euo pipefail
# P2-4: nullglob so the prune globs below iterate zero times on an empty
# asset dir instead of rm-ing the literal pattern. The 1.*.* pattern only
# needs revisiting at 2.x (major bump = revisit the keep policy anyway).
shopt -s nullglob
cd "$(dirname "$0")/.."
# Testable stages (prune + manifest writer) live in the sourced lib —
# scripts/test/release-lib.bash pins them.
source "scripts/lib/release-lib.sh"

# The npm package directory, defined ONCE and early: the --check-modes-only entry
# point below needs it before any guard runs, and a second literal would be the
# "two things that must agree" shape this loop keeps closing.
NPM_DIR=agent/summrise-agent-npm

# --check-modes-only: run ONLY the pack-input permission gate and exit. That gate
# exists because `npm pack` preserves worktree modes — a 0600 file packs a tarball
# that differs from a fresh checkout's by its tar HEADER alone, the measured cause
# of twenty consecutive "packaging metadata" WARNs and of the 3-byte drift on the
# 1.2.348 pair. It sits mid-chain (reconcile gate -> version -> exe timestamp ->
# THIS), so no test could reach it; this entry point is that gate, reachable — the
# same move --audit-only made for the dual-builder audit. The verdict prints the
# offending file per line itself; the long explanation stays at the mid-chain call
# site, where a publish is actually being refused, so the text is not duplicated.
if [ "${1:-}" = "--check-modes-only" ]; then
  pack_input_mode_verdict "$PWD" "$NPM_DIR" || exit 1
  echo "pack input modes match a fresh checkout OK"
  exit 0
fi

# --audit-only <ver>: (re)run the dual-builder audit against an ALREADY
# published release, without repacking or deploying anything. This is the
# entry point the post-publish checklist names — the asset only exists after
# the tag/CI, so the audit for a fresh release is always run separately.
if [ "${1:-}" = "--audit-only" ]; then
  VER="${2:?usage: ./scripts/publish-release.sh --audit-only <1.2.N>}"
  # shellcheck source=lib/release-audit.sh
  source "scripts/lib/release-audit.sh"
  if audit_release_asset "$VER" "${SMOKE_BASE_URL:-https://agent.saisi.online}"; then
    # The audit ran and passed for this version: whatever the reconcile ledger carried for it is settled.
    # CLEARING IT IS THE POINT OF THIS BRANCH, and the first version only SAID so — it echoed "settled" and
    # left the line in place, so the debt outlived the audit that discharged it and the NEXT publish refused
    # with "these versions are on the CDN with no GitHub release to audit against" (measured 2026-09-22, on
    # 1.2.450: the audit passed, the ledger still named it). A record that says a version owes something it
    # has already paid is the kind of stale sentence this repository keeps having to unpick.
    if grep -qx "$VER" <<<"$(reconcile_pending)"; then
      reconcile_clear "$VER"
      echo "audit: v$VER settled"
    fi
    exit 0
  fi
  exit 1
fi

VER="${1:?usage: ./scripts/publish-release.sh <1.2.N> [--skip-reconcile] [--with-installer]}"
case "$VER" in -*) echo "::error::usage: ./scripts/publish-release.sh <1.2.N> [--skip-reconcile] [--with-installer]" >&2; exit 1;; esac
shift
SKIP_RECONCILE=0
WITH_INSTALLER=0
ACK_UNRECONCILED=0
PUBLISH_NPM=0
# `latest`, NOT `alpha` -- corrected after the first real two-channel release
# (1.2.453) deadlocked. The CDN's `-latest.tgz` alias moves on EVERY release, so
# npm's `latest` must move with it: publish to alpha only and `npm i -g
# summrise-agent` installs an OLDER CLI than the release the agent is being asked
# to take, which the CLI's own guard then refuses ("this CLI is 1.2.452 and the
# release channel has 1.2.453") -- a loop with no exit. `alpha` stays available
# via --npm-tag for a deliberate prerelease channel.
NPM_TAG="latest"
DRY_RUN=0
# THE SEQUENCE, AS DATA. A release's order used to exist in four places — this file's layout, its
# header comment, AGENTS.md, and tests that asserted it by LINE NUMBER — and the header had already
# drifted from the code once (it listed the audit before npm, which is the order a FIRST publish
# never survives: the audit cannot pass until the asset exists, and its failure branch exits).
# Declared here so a test can hold the source to it, and printed by --dry-run so an operator can
# read what a run would do before it does anything.
SEQUENCE="pack stage prune deploy smoke npm audit"
while [ $# -gt 0 ]; do
  case "$1" in
    --skip-reconcile) SKIP_RECONCILE=1 ;;
    --with-installer) WITH_INSTALLER=1 ;;
    --acknowledge-unreconciled) ACK_UNRECONCILED=1 ;;
    --npm) PUBLISH_NPM=1 ;;
    --npm-tag) NPM_TAG="${2:?--npm-tag needs a dist-tag: alpha | next | latest}"; PUBLISH_NPM=1; shift ;;
    --dry-run) DRY_RUN=1 ;;
    *) echo "::error::unknown flag: $1 (usage: ./scripts/publish-release.sh <1.2.N> [--skip-reconcile] [--with-installer] [--acknowledge-unreconciled] [--npm] [--npm-tag alpha|next|latest] [--dry-run])" >&2; exit 1 ;;
  esac
  shift
done

# THE RECONCILE GATE (round 123). A version published with no GitHub release to
# audit against leaves a debt in the reconcile ledger, and the next publish
# REFUSES until it is settled. It used to be silence: measured 2026-09-14, the
# newest GitHub release that existed at all was v1.2.361 while the CDN served
# 1.2.361-1.2.364 — three versions published with no release, no tag and no audit.
PENDING_RECONCILE="$(reconcile_pending | tr '\n' ' ')"
if [ -n "${PENDING_RECONCILE// /}" ] && [ "$ACK_UNRECONCILED" -eq 0 ]; then
  echo "::error::refusing to publish: these versions are on the CDN with no GitHub release to audit against:" >&2
  echo "  $PENDING_RECONCILE" >&2
  echo "  Settle one:      ./scripts/publish-release.sh --audit-only <ver>   (after its tag/release exists)" >&2
  echo "  Or acknowledge:  rerun with --acknowledge-unreconciled  (this run ADDS to the ledger; it does not clear it)" >&2
  exit 1
fi
ASSET_DIR=index/public/summrise-agent
PKG="$NPM_DIR/package.json"

# P2-3 token (same logic as scripts/build.sh cf_token): env first,
# ~/.cloudflare-token fallback — never a bare `cat` (missing file used to
# die with an opaque cat error deep in the deploy step).


# D7 (docs/design/0010): the npm registry is the SECOND channel, and it follows
# the same shape as cf_token — env first, then a file — with one difference that
# cost an hour on 2026-09-23: the file is TRIMMED. A token written with a
# trailing newline authenticates as nothing, and "401 while the file looks right"
# is a bad hour to spend. Publishing also needs 2FA, or a token with Bypass 2FA
# enabled; a granular token without it is refused with exactly that sentence.
npm_token() {
  if [[ -n "${NPM_TOKEN:-}" ]]; then echo "$NPM_TOKEN";
  elif [[ -f "$HOME/.npm-token" ]]; then tr -d ' \t\r\n' < "$HOME/.npm-token";
  else echo ""; fi
}

# Guard: package.json version must already be bumped to $VER.
PKG_VER=$(node -p "require('./$PKG').version")
if [ "$PKG_VER" != "$VER" ]; then
  echo "::error::package.json version is $PKG_VER, want $VER — bump it first" >&2
  exit 1
fi

# D7 fail-fast: if npm publishing was ASKED for, prove the credential BEFORE a
# single artifact is packed, deployed or pruned. Checking it at the publish step
# instead would fail AFTER the CDN was already updated — a release half-published,
# which is the one state this script must never leave behind.
NPM_TOKEN_VAL="$(npm_token)"
if [ "$PUBLISH_NPM" = "1" ] && [ -z "$NPM_TOKEN_VAL" ]; then
  echo "::error::--npm was asked for but there is no token — set NPM_TOKEN or write ~/.npm-token (npmjs.com > Access Tokens; publishing needs 2FA, or a granular token with 'Bypass 2FA' enabled)" >&2
  exit 1
fi

# Guard: the exe must be staged (built from the current source).
if [ ! -f "$NPM_DIR/summrise-agent.exe" ]; then
  echo "::error::missing $NPM_DIR/summrise-agent.exe — build + stage it first" >&2
  exit 1
fi

# P1-1 exe provenance (fail-closed): a stale exe used to sail through this
# script straight onto the CDN. The canonical cross-compile output must
# exist, the staged copy must BE that output (byte-identical — a hand
# cp from elsewhere aborts), and the build must postdate the newest commit
# touching any exe input (rust src + embedded panel + cargo manifests).
# Minimum bar on top: older than 30 days always aborts (WARN past 7 days).
EXE_BUILD="agent/target/x86_64-pc-windows-msvc/release/summrise-agent.exe"
if [ ! -f "$EXE_BUILD" ]; then
  echo "::error::missing $EXE_BUILD — cross-compile first: ./scripts/build.sh agent" >&2
  exit 1
fi
if ! cmp -s "$EXE_BUILD" "$NPM_DIR/summrise-agent.exe"; then
  echo "::error::staged $NPM_DIR/summrise-agent.exe != fresh $EXE_BUILD — re-stage and retry:" >&2
  echo "  cp $EXE_BUILD $NPM_DIR/summrise-agent.exe" >&2
  exit 1
fi
SRC_TS=$(git log -1 --format=%ct -- agent/src agent/build.rs agent/resources/panel-react agent/resources/panel agent/Cargo.toml agent/Cargo.lock)
# AN EMPTY ANSWER IS "I CANNOT DATE THE INPUTS", NOT "ZERO" (round 127).
# `git log <path>` exits 0 with NO OUTPUT when the path list matches no commit at
# all, and `${SRC_TS:-0}` then made `EXE_TS -lt 0` false for every exe: a rename or
# removal under agent/src turned "the exe must postdate its inputs" into a no-op
# WITH NO MESSAGE, leaving the 30-day cap as the only staleness bound.
if [ -z "$SRC_TS" ]; then
  echo "::error::cannot date the exe inputs: 'git log -- <paths>' returned nothing (did a path move?). Refusing to disable the exe-staleness gate silently." >&2
  exit 1
fi
# ...and the working tree of those inputs must be clean: build.sh bakes
# the CURRENT panel SPA into the exe (include_str!), so uncommitted panel
# or rust changes mean the exe matches neither HEAD nor CI. build.rs counts:
# it sets link args (/Brepro) and the embedded PANEL_BUNDLE_HASH env.
DIRTY_EXE=$(git status --porcelain -- agent/src agent/build.rs agent/resources/panel-react agent/resources/panel agent/Cargo.toml agent/Cargo.lock)
if [ -n "$DIRTY_EXE" ]; then
  echo "::error::exe inputs have uncommitted changes (the exe embeds them, CI never sees them) — commit (or stash), rebuild, re-stage:" >&2
  echo "$DIRTY_EXE" >&2
  exit 1
fi
EXE_TS=$(stat -c %Y "$EXE_BUILD")
NOW_TS=$(date +%s)
if [ "$EXE_TS" -lt "$SRC_TS" ]; then
  echo "::error::$EXE_BUILD predates the newest exe-input commit ($(date -u -d "@$SRC_TS" +%Y-%m-%dT%H:%M:%SZ)) — rebuild, re-stage, retry:" >&2
  echo "  ./scripts/build.sh agent && cp $EXE_BUILD $NPM_DIR/summrise-agent.exe" >&2
  exit 1
fi
if [ "$EXE_TS" -lt "$((NOW_TS - 30*24*3600))" ]; then
  echo "::error::$EXE_BUILD is older than 30 days — rebuild regardless of source changes" >&2
  exit 1
fi
if [ "$EXE_TS" -lt "$((NOW_TS - 7*24*3600))" ]; then
  echo "-- WARN: $EXE_BUILD is older than 7 days (still newer than every exe-input commit — proceeding)"
fi
echo "exe provenance OK ($EXE_BUILD newer than all exe inputs)"

# CHEAP artifact gates replicated from release.yml (a bare `npm pack` here
# used to bypass all three CI gates and ship stale files to the CDN).
# Fail fast before packing. tsc comes from the repo's own
# summrise-agent-npm/node_modules (P1-2 pins typescript@5, same as CI) — no
# network install here; a missing tsc fails with the install command.
# (a) round-298 marker presence in bin/summrise.js — the exact grep the CI step
# runs post-compile. A missing marker means src/summrise.ts changed without
# recompiling (the 1.2.274 stale-bin lesson).
if ! grep -q "summrise-release" "$NPM_DIR/bin/summrise.js"; then
  echo "::error::$NPM_DIR/bin/summrise.js missing round-298 marker — recompile src/summrise.ts first:" >&2
  echo "  (cd $NPM_DIR && npm install --no-save --ignore-scripts --force typescript@5 @types/node@22 && ./node_modules/.bin/tsc -p tsconfig.json && cp dist/summrise.js bin/summrise.js)" >&2
  exit 1
fi
echo "bin/summrise.js marker check OK"
# P1-2 bin/summrise.js freshness (same gate as release.yml:140-143 + the CI
# pack-chain step): recompile src/summrise.ts with the repo tsconfig into a
# tmp dir and cmp against the committed bin/summrise.js. The marker grep above
# only proves SOME build happened — this proves it was built from the
# CURRENT source. No tsc here FAILS with the install command (never skip:
# an uncheckable bin is an unshippable bin).
TSC="$NPM_DIR/node_modules/.bin/tsc"
if [ ! -x "$TSC" ]; then
  echo "::error::no tsc in $NPM_DIR (bin/summrise.js freshness uncheckable) — install and retry:" >&2
  echo "  (cd $NPM_DIR && npm install --no-save --ignore-scripts --force typescript@5 @types/node@22)" >&2
  exit 1
fi
if ! "$TSC" --version | grep -q "Version 5\."; then
  echo "::error::$TSC is not typescript@5 (CI compiles with v5 — a foreign major emits different bytes and false-fails the cmp) — reinstall:" >&2
  echo "  (cd $NPM_DIR && npm install --no-save --ignore-scripts --force typescript@5 @types/node@22)" >&2
  exit 1
fi
"$TSC" -p "$NPM_DIR/tsconfig.json" --outDir /tmp/summrise-fresh-bin
if ! cmp -s /tmp/summrise-fresh-bin/summrise.js "$NPM_DIR/bin/summrise.js"; then
  echo "::error::$NPM_DIR/bin/summrise.js is stale (src/summrise.ts changed without recompiling) — recompile, commit, retry:" >&2
  echo "  (cd $NPM_DIR && ./node_modules/.bin/tsc -p tsconfig.json && cp dist/summrise.js bin/summrise.js)" >&2
  rm -rf /tmp/summrise-fresh-bin
  exit 1
fi
rm -rf /tmp/summrise-fresh-bin
echo "bin/summrise.js freshness check OK (tsc recompile + cmp)"
# (c) electron freshness: COMMITTED-clean (as before — CI compiles the
# committed state, so any local modification means this pack may not match
# what CI builds) PLUS the fresh-emit compare, same gate as
# release.yml:155-162. The old comment here claimed the emit comparison
# needed a network install CI does in ~15s — P1-2 above already guarantees
# a pinned tsc, so run the real gate instead of waving through.
if [ -n "$(git status --porcelain -- agent/summrise-desktop-electron/src/)" ]; then
  echo "::error::agent/summrise-desktop-electron/src/ has uncommitted changes — commit (or stash) them first so this pack matches what CI will compile:" >&2
  git status --porcelain -- agent/summrise-desktop-electron/src/ >&2
  exit 1
fi
echo "electron src committed-clean OK"
(cd "$NPM_DIR" && ./node_modules/.bin/tsc -p ../summrise-desktop-electron/tsconfig.json \
  --typeRoots ./node_modules/@types --outDir /tmp/electron-fresh-pub --noCheck)
for F in main.js preload.js url-policy.js; do
  if ! cmp -s "/tmp/electron-fresh-pub/${F}" "$NPM_DIR/summrise-desktop-electron/src/${F}"; then
    echo "::error::$NPM_DIR/summrise-desktop-electron/src/${F} is stale (ts source changed without recompiling) — run tsc and commit the fresh output" >&2
    rm -rf /tmp/electron-fresh-pub
    exit 1
  fi
done
rm -rf /tmp/electron-fresh-pub
echo "electron src freshness check OK (fresh tsc emit + cmp)"
# (d) source-tree copy vs npm-packaged copy: release.yml's freshness gate
# compiles the TS and cmps ONLY the npm copy (summrise-agent-npm/.../src/),
# while tsc's input tree (agent/summrise-desktop-electron/src/) holds its OWN
# committed main.js that nothing pins — the two drifted silently once
# already (hand-edit reached only the npm copy). cmp all three shipped
# files; pure local, no toolchain needed.
for F in main.js preload.js url-policy.js; do
  if ! cmp -s "agent/summrise-desktop-electron/src/${F}" "agent/summrise-agent-npm/summrise-desktop-electron/src/${F}"; then
    echo "::error::electron src copy drift: agent/summrise-desktop-electron/src/${F} != agent/summrise-agent-npm/summrise-desktop-electron/src/${F} — sync them (tsc emit) and commit both" >&2
    exit 1
  fi
done
echo "electron src copies in sync OK"

# P1-6 pack-input committed-clean: everything npm packs EXCEPT the
# gitignored exe/tgz/dist (invisible to git status) and the in-progress
# package.json bump (committed in step 5 below) must already be committed
# — CI packs the committed tree, so any local delta here means this tgz
# may not match what CI builds.
DIRTY_INPUTS=$(git status --porcelain -- "$NPM_DIR/bin" "$NPM_DIR/src" "$NPM_DIR/test" "$NPM_DIR/README.md" "$NPM_DIR/summrise-desktop-electron" "agent/summrise-desktop-electron")
if [ -n "$DIRTY_INPUTS" ]; then
  echo "::error::pack inputs have uncommitted changes — commit (or stash) them first so this pack matches CI:" >&2
  echo "$DIRTY_INPUTS" >&2
  exit 1
fi
echo "pack inputs committed-clean OK"

# P1-6b MODE PARITY — the check `git status` STRUCTURALLY CANNOT MAKE.
#
# Git tracks only the EXECUTABLE BIT (100644 vs 100755), so a file whose content
# is unchanged reports clean however its permissions sit on disk. `npm pack`
# preserves each file's WORKTREE mode, while CI packs a fresh checkout at 0644.
# So a 0600 worktree file packs a tarball that differs from CI's by its HEADER
# alone — and the P1-6 gate above says the tree is clean.
#
# THAT IS THE MEASURED CAUSE OF TWENTY CONSECUTIVE "packaging metadata" WARNs.
# On the live 1.2.348 pair the two tarballs differ by exactly 3 bytes out of
# 17,774,080 — README.md's mode field and its header checksum — while EVERY
# file's sha256 matches, the 17.5 MB summrise-agent.exe included. Blaming "the
# unreproducible-build long tail" for that was wrong, and this is where it is
# actually decided.
# The gate itself lives in scripts/lib/release-lib.sh (round 154) so it has
# behavioural tests; publish-release.sh has no harness of its own, which is why
# this check had none for as long as it existed. Same wording, same refusal, same
# exit.
if ! MODE_BAD=$(pack_input_mode_verdict "$PWD" "$NPM_DIR"); then
  echo "::error::pack inputs have WORKTREE PERMISSIONS that differ from a fresh checkout — npm pack preserves them, so this tgz would differ from CI's by its tar headers alone:" >&2
  printf '%s' "$MODE_BAD" >&2
  echo "  Fix: chmod each file to the mode git records (e.g. \`chmod 644 <file>\`)." >&2
  exit 1
fi
echo "pack input modes match a fresh checkout OK"

# ── the component pins must AGREE with the agent's own (grilling Q4) ──────────
# cloudflared's sha256 appears TWICE: index/components.json (published in the
# manifest so `summrise setup` can verify what it fetched) and agent/src/tunnel.rs
# (the pin the agent re-checks at run time). If they drift, a fresh install verifies
# against one value and the running agent against another — so this refuses to pack.
if [ -f index/components.json ] && [ -f agent/src/tunnel.rs ]; then
  MANIFEST_CF=$(python3 -c "import json;print(json.load(open('index/components.json')).get('cloudflared',{}).get('sha256',''))")
  RUST_CF=$(grep -oE 'CLOUDFLARED_SHA256: &str = "[0-9a-f]{64}"' agent/src/tunnel.rs | grep -oE '[0-9a-f]{64}')
  if [ -n "$RUST_CF" ] && [ "$MANIFEST_CF" != "$RUST_CF" ]; then
    echo "::error::cloudflared pin drift: index/components.json says ${MANIFEST_CF:0:12}… and agent/src/tunnel.rs says ${RUST_CF:0:12}… — update BOTH in one commit" >&2
    exit 1
  fi
  echo "  component pins: cloudflared agrees with the agent's CLOUDFLARED_SHA256 (${RUST_CF:0:12}…)"
fi

# ── every copy of a component's ADDRESS must be a path the worker SERVES (2026-09-25) ─────────
# The digest above has two pins and a cross-check; the ADDRESS had four copies and no comparison at
# all — version.json published a `url` per component that NOBODY read. The two readers that fetch
# the manifest for the digest now take the address from the same body (the npm CLI's
# resolveComponent, the installer's component blocks); the copies that cannot fetch anything are
# compared here against the routes index/src/index.js answers, so a rename on one side of that chain
# cannot leave the other side publishing a dead address. The gate lives in scripts/lib/release-lib.sh
# so it has behavioural tests (round 154's rule for every refusal in this block).
if [ -f index/components.json ] && [ -f index/src/index.js ]; then
  if ! ROUTE_BAD=$(component_route_verdict "$PWD"); then
    echo "::error::component address drift — a published url and the route that serves it disagree:" >&2
    printf '%s' "$ROUTE_BAD" >&2
    echo "  Fix: index/src/index.js is the derivation; move the copy to the route it serves." >&2
    exit 1
  fi
  echo "  component addresses: every declared copy names a path index/src/index.js serves"
fi

# ── --dry-run: every gate that can REFUSE, and nothing that can CHANGE anything ────────────────
# The 30 refusals above are this module's real interface, and until now the only way to exercise
# one was to attempt a release: the whole gate block needs no credential and touches nothing, but
# it sat inline above the first effect, so no test could reach it. This is the seam that makes it
# reachable — one adapter runs the release, this one runs the gates and stops.
#
# WHAT IT DOES NOT COVER, said out loud rather than implied: the credential-presence checks that
# live INSIDE their steps, below this line — the Cloudflare token (checked where it is used) and the
# npm token is NOT one of them (that refusal is a gate, above). A dry run that claimed to have
# verified everything would be the exact failure mode the rest of this file exists to prevent.
if [ "$DRY_RUN" -eq 1 ]; then
  echo "== dry run =="
  echo "   every gate above passed; nothing was packed, staged, pruned, deployed, published or committed"
  echo "   sequence: $SEQUENCE"
  echo "   NOT verified here: the Cloudflare token (its check lives in the deploy step)"
  exit 0
fi

echo "== pack =="
(cd "$NPM_DIR" && npm pack >/dev/null)
TGZ="$NPM_DIR/summrise-agent-$VER.tgz"
[ -f "$TGZ" ] || { echo "::error::pack did not produce $TGZ" >&2; exit 1; }

# (b) packed-tgz content gate — mirror release.yml's list exactly (a
# missing file silently keeps the stale one on devices, round-278/282
# lesson). Same SIGPIPE-safe pattern as release.yml (round-288 lesson:
# never `tar tzf | grep -q` under pipefail — list to a temp file first,
# then grep with basename-tolerant anchors).
tar tzf "$TGZ" > "/tmp/tgz-list-${VER}.txt"
# THE LIST HAS ONE OWNER: agent/summrise-agent-npm/required-in-tgz.txt, which release.yml reads
# too. It used to be written out verbatim in both files — two owners of one fact as long as nobody
# touched it, and two DIFFERENT release gates the moment somebody did.
REQUIRED_IN_TGZ="agent/summrise-agent-npm/required-in-tgz.txt"
[ -f "$REQUIRED_IN_TGZ" ] || { echo "::error::the packed-tgz content list is missing: $REQUIRED_IN_TGZ (a missing owner would silently disable this gate in BOTH builders)" >&2; exit 1; }
while IFS= read -r F; do
  case "$F" in ''|'#'*) continue ;; esac
  if ! grep -qE "(^|/)${F}$" "/tmp/tgz-list-${VER}.txt"; then
    echo "::error::tgz missing required file: $F" >&2
    exit 1
  fi
done < "$REQUIRED_IN_TGZ"
echo "tgz content check OK ($TGZ)"

echo "== stage =="
cp "$TGZ" "$ASSET_DIR/"
cp "$TGZ" "$ASSET_DIR/summrise-agent-latest.tgz"
# Installer 同版同发：--with-installer 在这里打自包含安装器（--no-deploy，
# 单次 deploy 在下面统一做，manifest 和安装器不可能互相滞后）。tgz 已在
# 上面 stage 好，正好满足 build-installer.sh 的前置。
if [ "$WITH_INSTALLER" -eq 1 ]; then
  echo "== installer (self-contained, staged, no deploy yet) =="
  ./scripts/build-installer.sh "$VER" --no-deploy
fi
# 自包含证明：staged 安装器必须比 tgz 大（内嵌 payload）。更小的只有一种
# 可能——上一个版本的在线包残留（没打进去 tgz）。WARN 不 fail：紧急发布
# 允许先上 tgz-only manifest，补打安装器后重跑 manifest+deploy 即可。
INST_EXE="$ASSET_DIR/SummriseAgent-Setup-$VER.exe"
if [ -f "$INST_EXE" ]; then
  echo "installer staged: $(basename "$INST_EXE") ($(stat -c %s "$INST_EXE") bytes)"
  if [ "$(stat -c %s "$INST_EXE")" -le "$(stat -c %s "$TGZ")" ]; then
    echo "-- WARN: $INST_EXE not larger than the tgz — stale online-only build? Rebuild with ./scripts/build-installer.sh $VER (manifest will advertise a non-self-contained exe)"
  fi
else
  echo "-- WARN: no $INST_EXE — run ./scripts/build-installer.sh $VER first so fresh installs track this release (manifest will be tgz-only)"
fi
SHA=$(write_version_json "$VER" "$TGZ" "$ASSET_DIR" "$INST_EXE")
echo "sha256: $SHA"

# THE RETIRED INSTALLER MUST NOT LINGER — see retire_installers() in lib/release-lib.sh for
# what was measured and why a warning nobody could clear is not good enough.
if [ "$WITH_INSTALLER" -eq 0 ]; then
  retire_installers "$ASSET_DIR"
fi

echo "== last-5-per-minor prune (round-309) =="
# Keep the newest 5 of EACH major.minor line + the latest alias (the policy
# and its awk grouping live in scripts/lib/release-lib.sh, pinned by
# scripts/test/release-lib.bash).
prune_last5_per_minor "$ASSET_DIR"
prune_installers "$ASSET_DIR"
echo "remaining: $(ls "$ASSET_DIR"/summrise-agent-1.*.*.tgz 2>/dev/null | wc -l) versioned tgz + latest + $(ls "$ASSET_DIR"/SummriseAgent-Setup-1.*.*.exe 2>/dev/null | wc -l) versioned installers + alias"

echo "== commit (NOT here) =="
# The publish step does not commit any more (operator, 2026-09-14: "why does the
# CDN publish need its own commit"). It left a separate `chore(stage-n): release
# ...` commit behind for every version — a commit whose whole diff was a manifest
# and a ledger, sitting next to the round's real one. The two files below are
# written by this run and belong in the commit that ships the change.
echo "   include in THIS round's commit:"
echo "     $PKG"
echo "     $ASSET_DIR/version.json"

echo "== deploy =="
CF_TOKEN="$(cf_token)"
if [ -z "$CF_TOKEN" ]; then
  echo "::error::no Cloudflare token — set CLOUDFLARE_API_TOKEN or write ~/.cloudflare-token (same rule as scripts/build.sh)" >&2
  exit 1
fi
(cd index && CLOUDFLARE_API_TOKEN="$CF_TOKEN" npx wrangler deploy)

echo "== post-publish smoke =="
# Shared with build.sh's index deploy — a bad manifest or mismatched binary
# (versioned OR latest alias) must fail the release, not ship green.
# shellcheck source=smoke-index.sh
source "scripts/smoke-index.sh"
assert_want_sha256 "$SHA" || exit 1
smoke_index_release "$VER" "$SHA" || exit 1

# ── D7: the npm registry, the second channel ────────────────────────────────
# THIS RUNS BEFORE THE AUDIT, ON PURPOSE. The audit cannot pass on a first
# publish -- the GitHub asset is built by release.yml AFTER the tag -- and its
# failure branch exits, so while this block sat BELOW the audit, a first publish
# deployed the CDN, refused the audit, and never reached the registry. Measured
# on 1.2.454: the CDN served the new version, `--npm` was passed, and npm still
# answered latest=1.2.453. Putting a step that MUST happen after a step that is
# EXPECTED to fail is the whole bug; the audit stays last, where its verdict
# belongs.
#
# The design is copied from what dsh actually publishes (measured 2026-09-23):
# a tiny meta-package whose fat parts are DEPENDENCIES, and DIST-TAGS AS
# CHANNELS -- their `latest` was an older rc while `alpha` carried the newer
# builds. The tag defaults to `latest` here, NOT `alpha`: this product's CDN
# `-latest.tgz` alias moves on every release, so npm `latest` must move with it
# or `npm i -g summrise-agent` installs a CLI older than the release the agent is
# being asked to take (the 1.2.453 deadlock). `--npm-tag alpha` is available for a
# deliberate prerelease channel.
echo "== npm registry =="
if [ "$PUBLISH_NPM" = "1" ]; then
  # A TEMP userconfig, not ~/.npmrc: this box's npm points at a read mirror
  # (registry.npmmirror.com), and --registry alone would leave the token nowhere
  # to live. 600 + trap: the credential never outlives the run.
  NPMRC="$(mktemp)"; chmod 600 "$NPMRC"
  trap 'rm -f "$NPMRC"' EXIT
  printf '//registry.npmjs.org/:_authToken=%s\n' "$NPM_TOKEN_VAL" > "$NPMRC"
  npm publish "$TGZ" --registry=https://registry.npmjs.org/ --userconfig "$NPMRC" \
    --tag "$NPM_TAG" --access public
  NPM_NAME=$(node -p "require('./$PKG').name")
  echo "   published $NPM_NAME@$VER under dist-tag '$NPM_TAG'"
  echo "   verify: curl -s https://registry.npmjs.org/$NPM_NAME | grep -o '\"dist-tags\".*'"
else
  echo "::warning::the npm registry was NOT updated — it still serves the 0.0.1 placeholder, so 'npx summrise-agent' installs nothing. Pass --npm (with ~/.npm-token) when npm should carry this release."
fi

echo "== release audit: CDN vs GitHub release asset (P0 dual-builder) =="
# The audit lives in scripts/lib/release-audit.sh. It demands that every
# SOURCE-DERIVED file in the two tarballs be byte-identical and tolerates ONLY
# a differing summrise-agent.exe, because the two builders do not share a
# toolchain: this box builds with rustc `stable` (1.98.0 here) + a hand-built
# llvm18 that cargo-xwin is symlinked to, while release.yml uses
# dtolnay/rust-toolchain@stable (floating) + the distro's llvm. Demanding whole
# -tarball equality — what this block used to do — could therefore NEVER pass,
# which silently turned the gate into a skip. Comparing source-derived files is
# what actually catches a builder that packaged different source.
# See the header of release-audit.sh for the full evidence.
#
# The GitHub asset only exists AFTER the tag push below, so a genuine first
# publish has nothing to audit — that is the only case --skip-reconcile covers,
# and it refuses as soon as an asset exists.
CDN_BASE="${SMOKE_BASE_URL:-https://agent.saisi.online}"
# shellcheck source=lib/release-audit.sh
source "scripts/lib/release-audit.sh"
if [ "$SKIP_RECONCILE" -eq 1 ]; then
  # List to a file FIRST, then grep — never `producer | grep -q` under
  # pipefail (round-288: grep -q exits early, SIGPIPEs the producer, and the
  # pipeline reports failure even on a match).
  SKIP_LIST="/tmp/audit-assets-skip-${VER}.txt"
  SKIP_ERR="/tmp/audit-assets-skip-${VER}.err"
  # rc 3 = the release does not exist (a genuine first publish) and is the ONLY
  # case this flag may cover. rc 1 = the question could not be ASKED (no token,
  # network, API error) — never permission to skip a P0 audit. `|| true` here
  # used to flatten both into "first publish", so an expired token skipped the
  # audit and the run reported success (round 122).
  if audit_asset_names "$VER" >"$SKIP_LIST" 2>"$SKIP_ERR"; then
    echo "::error::--skip-reconcile refused: GitHub release v$VER already EXISTS ($(tr '\n' ' ' <"$SKIP_LIST")) — rerun WITHOUT the flag so the audit executes" >&2
    exit 1
  else
    SKIP_RC=$?
    if [ "$SKIP_RC" -ne 3 ]; then
      echo "::error::--skip-reconcile cannot be honoured: whether GitHub release v$VER exists could not be determined — refusing to skip the audit" >&2
      sed 's/^/  /' "$SKIP_ERR" >&2 || true
      exit 1
    fi
  fi
  echo "-- WARN: --skip-reconcile given and no GitHub release v$VER exists yet (first publish) — audit SKIPPED, verify post-tag via the checklist below"
  # The debt becomes a FILE, not a line of scrollback: the next publish reads it
  # and refuses until it is settled (round 123).
  reconcile_record "$VER" "--skip-reconcile: no GitHub release v$VER at publish time"
else
  audit_release_asset "$VER" "$CDN_BASE" || {
    echo "  the asset is built by release.yml AFTER the tag push below." >&2
    echo "  first publish? rerun with --skip-reconcile, then audit post-tag." >&2
    exit 1
  }
  # The CDN must still serve the exact bytes this run packed (catches a
  # mid-publish drift / a stale deploy), independent of the exe question.
  CDN_SHA=$(curl -fsSL -m 120 "$CDN_BASE/summrise-agent/summrise-agent-$VER.tgz" | sha256sum | cut -d' ' -f1)
  if [ "$CDN_SHA" != "$SHA" ]; then
    echo "::error::audit FAILED: CDN sha $CDN_SHA != just-packed local sha $SHA — the CDN drifted mid-publish" >&2
    exit 1
  fi
  echo "audit OK: CDN serves this run's pack, and its source-derived files match the GitHub asset"
  # (the reconcile ledger this wrote is gone with the rest of the bookkeeping)
fi

# (the npm registry block used to sit HERE, below the audit. It moved above it --
# see the ordering note there. A first publish deployed the CDN, refused the
# audit, and never reached the registry.)

echo "== done. Next: push main, then create the GitHub tag v$VER via the API"
echo "  (release.yml builds the GitHub asset; keep-latest stays manual)."
echo ""
echo "== post-publish checklist (copy-paste; no gh CLI needed — this box has none) =="
echo "  [1] push the release commit:   git push origin main"
echo "  [2] cut the tag via the API (direct tag pushes time out here):"
echo "        curl -s -X POST -H \"Authorization: Bearer \$(cat ~/.github-token)\" -H 'Accept: application/vnd.github+json' \\"
echo "          https://api.github.com/repos/SilasVale/summrise/git/refs \\"
echo "          -d '{\"ref\":\"refs/tags/v$VER\",\"sha\":\"'\$(git rev-parse HEAD)'\"}'"
echo "  [3] wait for release.yml to go green, then audit the two artifacts:"
echo "        ./scripts/publish-release.sh --audit-only $VER     # (or rerun without --skip-reconcile)"
echo "      It demands every SOURCE-derived file be byte-identical and only tolerates"
echo "      a differing summrise-agent.exe (the two builders do not share a build env)."
echo "  [4] OPTIONAL — collapse the two builders so the CDN serves the CI artifact"
echo "      (then CDN == GitHub byte-for-byte):"
echo "        ./scripts/publish-cdn-from-ci.sh $VER"
echo "  [5] keep-latest: delete the PREVIOUS release + tag via the API"
echo "        (DELETE /repos/SilasVale/summrise/releases/<id> and /git/refs/tags/<tag>;"
echo "         the URL needs the full refs path, not just the name)"
echo "  [6] verify what devices see:   curl -s $CDN_BASE/api/version   # want version $VER"
echo "        curl -fsSL $CDN_BASE/summrise-agent/summrise-agent-latest.tgz | sha256sum   # want: $SHA"
echo "        curl -sI $CDN_BASE/summrise-agent/SummriseAgent-Setup.exe | grep -i etag    # must equal the"
echo "        versioned SummriseAgent-Setup-$VER.exe etag (an installer regenerated from a"
echo "        converged tgz legitimately has a NEW sha — re-read the manifest for it)"
