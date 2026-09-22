#!/bin/bash
# Sync the vale-gate sources into public/code/files and generate the manifest.
# Usage: run `bash scripts/sync-code-viewer.sh` after editing code, then `wrangler deploy`.
#
# MIRROR DISCIPLINE: scripts/build.sh deploy_worker (gateway) calls THIS script
# as its single sync path — there is no second implementation to keep in
# step. Both use rm -rf + re-copy: plain cp never deletes, so files removed
# from gateway/src (e.g. plugin-hub.ts, round-341) kept being served by the
# Source Viewer until the rm -rf discipline landed.
set -e
cd "$(dirname "$0")/.."
DEST=public/code/files

rm -rf "$DEST"
mkdir -p "$DEST/vale-gate/src" "$DEST/vale-gate/public"

# vale-gate sources: the TS migration (round-83) moved the real source to
# .ts files (the .js re-export shims are long gone). Copy the live tree
# wholesale (incl. plugins/) so the published snapshot shows the
# implementation. rm -rf above guarantees deleted sources vanish here too.
cp "$PWD"/src/*.ts "$DEST/vale-gate/src/"
# The TS migration removed the re-export .js shims; tolerate their absence.
cp "$PWD"/src/*.js "$DEST/vale-gate/src/" 2>/dev/null || true
mkdir -p "$DEST/vale-gate/src/plugins"
cp "$PWD"/src/plugins/*.ts "$DEST/vale-gate/src/plugins/"
cp "$PWD"/src/plugins/*.js "$DEST/vale-gate/src/plugins/" 2>/dev/null || true
# Subdirectory domains (structure refactors): src/store/ (split from store.ts)
# and src/lib/ (ratelimit factory). The old top-level-only copy silently
# dropped them from the published snapshot — copy each live subdir so the
# mirror stays a byte-identical tree (redactions below still apply per file).
mkdir -p "$DEST/vale-gate/src/store" "$DEST/vale-gate/src/lib"
cp "$PWD"/src/store/*.ts "$DEST/vale-gate/src/store/"
cp "$PWD"/src/lib/*.ts "$DEST/vale-gate/src/lib/"
# Live public/ is a Vite build shell (index.html + hashed assets/ + static
# files). The dead single-file public/app.js was removed round-341 — do NOT
# re-add it here; sync only what live serves.
cp public/index.html public/style.css "$DEST/vale-gate/public/"
cp wrangler.jsonc "$DEST/vale-gate/"

# ── THE INSTRUMENTS (round 225) ───────────────────────────────────────────────────────────────────────────────────────
# The viewer showed only the worker, so the scripts that MEASURE a device were the one thing an operator could not fetch from
# it — and getting a 116 KB library onto a device turned out to be the hard part of running the live probe there (raw GitHub
# times out on it, the device has no git, and the relay inbox wants an admin token). They are small, self-contained node files
# and they are exactly what someone debugging a device reaches for, so the mirror carries them too:
#
#     https://<dist-host>/code/files/instruments/live-panel-probe.mjs
#     https://<dist-host>/code/files/instruments/lib/design-sweep.mjs
#
# The tree under agent/scripts is mirrored SHALLOW (the top level plus lib/), because that is where the emitters, the probes and
# the shared helpers live; the per-suite directories are not part of any device-side run.
# THE SCRIPT'S OWN CONVENTION IS $PWD (= gateway/), so the repo root is ONE level up; it defines no $ROOT, and my first
# version used a `$ROOT` that does not exist AND a `../..` that overshoots the repo, so the guard was false and the block
# never ran (44 files, no instruments, exit 0). The second wrong guess is why the guard now says WHICH path it looked in.
REPO_ROOT="$PWD/.."
if [ -d "$REPO_ROOT/agent/scripts" ]; then
  mkdir -p "$DEST/instruments/lib"
  cp "$REPO_ROOT"/agent/scripts/*.mjs "$DEST/instruments/" 2>/dev/null || true
  cp "$REPO_ROOT"/agent/scripts/lib/*.mjs "$DEST/instruments/lib/" 2>/dev/null || true
  echo "  instruments mirrored: $(find "$DEST/instruments" -type f | wc -l | tr -d ' ') file(s)"
else
  echo "  !! agent/scripts not found from $PWD — instruments NOT mirrored" >&2
fi

# (openrouter-proxy mirror removed with the worker's 2026-09-07 retirement —
# the sibling-path block never fired inside the monorepo anyway.)

# Generate the manifest from WHAT WAS ACTUALLY COPIED (no hardcoded file
# list — the old static src/*.js spec rotted when the tree moved to .ts and
# the viewer 404d on every entry). vercel-proxy is deprecated and contains
# a hardcoded key — excluded.
python3 - "$DEST" <<'EOF'
import json, os, sys
dest = sys.argv[1]
files = []
vg = os.path.join(dest, "vale-gate")
def walk(base, group, prefix):
    for root, _dirs, names in os.walk(base):
        for n in sorted(names):
            full = os.path.join(root, n)
            rel = os.path.relpath(full, base).replace(os.sep, "/")
            files.append({"name": rel, "path": f"{prefix}/{rel}", "group": group})

walk(vg, "vale-gate", "files/vale-gate")
# The instruments are listed too (round 225): a tree the viewer does not enumerate is a tree nobody finds, and the whole point
# of mirroring them is that an operator can reach the probe that measures their own device.
instruments = os.path.join(dest, "instruments")
if os.path.isdir(instruments):
    walk(instruments, "instruments", "files/instruments")
with open(os.path.join(dest, "..", "manifest.json"), "w") as f:
    json.dump({"files": files}, f, indent=2, ensure_ascii=False)
print(f"generated manifest: {len(files)} files → public/code/")
EOF

# Redact the production download host in the PUBLISHED snapshot (comments
# only — code strings, error copy and defaults keep the real host). The
# mirror is otherwise byte-identical to live; without this step every
# re-sync wipes the redaction. Fail LOUD if a pattern stops matching
# (source line moved) instead of silently publishing the raw host.
redact() { # $1=file $2=live-text ERE $3=sed-expr $4=expected-count
  local n m
  n="$(grep -c -E -e "$2" "$DEST/vale-gate/$1" || true)"
  [ "$n" = "$4" ] || { echo "  !! redaction pattern gone in $1 (want $4, got $n) — update sync-code-viewer.sh" >&2; exit 1; }
  sed -i -e "$3" "$DEST/vale-gate/$1"
  m="$(grep -c -F '<dist-host>' "$DEST/vale-gate/$1" || true)"
  [ "$m" = "$4" ] || { echo "  !! redaction did not apply in $1 (want $4, got $m)" >&2; exit 1; }
}
redact "src/auth.ts" '\*\.agent\.saisi\.online' 's/\*\.agent\.saisi\.online/*.<dist-host>/g' 1
redact "src/store/devices.ts" 'd1\.agent\.saisi\.online' 's/d1\.agent\.saisi\.online/d1.<dist-host>/g' 1
redact "src/plugins/devices.ts" '/api/version on agent\.saisi\.online' 's|/api/version on agent\.saisi\.online|/api/version on https://<dist-host>|g' 1
echo "redacted production host in 3 mirror comments"
