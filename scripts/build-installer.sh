#!/usr/bin/env bash
# Summrise Windows self-contained installer build script — produces setup.exe on Linux.
#
#   ./scripts/build-installer.sh <1.2.N> [--no-deploy]
#
# Self-contained: the pinned tgz (summrise-agent-<ver>.tgz) is File'd into the installer and the
# ps1 installs from the bundled copy first (-LocalTgz) — so it installs with no network, and even
# when an old version has been pruned; the tgz is deleted right after the install.
# Artifact: index/public/summrise-agent/SummriseAgent-Setup-<ver>.exe (+ the SummriseAgent-Setup.exe alias)
# By default it is deployed to the CDN together with the index worker; --no-deploy only stages
# it (so publish-release.sh can orchestrate a single deploy).
#
# Prerequisite: index/public/summrise-agent/summrise-agent-<ver>.tgz must already exist (publish
# packs+stages the tgz FIRST and builds the installer after it; the wrong order fails in place
# rather than shipping a half-baked package).
#
# Toolchain: NSIS 3.12 compiled from source (userspace, no sudo needed; apt is only used for
# download + dpkg-deb -x, the same trick as gcc10-root). The first run takes about 10 minutes
# (including the downloads); after that the makensis in $HOME/nsis-dist is reused and it takes
# seconds.
# NOTE: do NOT apt-install nsis (focal only has 3.05, which lacks the 3.11/3.12 privilege fix).
set -euo pipefail
cd "$(dirname "$0")/.."

VER="${1:?usage: ./scripts/build-installer.sh <1.2.N> [--no-deploy]}"
# The manifest verdict (round 130) lives in the lib the CI suite already drives.
# shellcheck source=lib/release-lib.sh
source "scripts/lib/release-lib.sh"
case "$VER" in -*) echo "::error::usage: ./scripts/build-installer.sh <1.2.N> [--no-deploy]" >&2; exit 1;; esac
shift || true
NO_DEPLOY=0
for a in "$@"; do
  case "$a" in
    --no-deploy) NO_DEPLOY=1 ;;
    *) echo "::error::unknown flag: $a (usage: ./scripts/build-installer.sh <1.2.N> [--no-deploy])" >&2; exit 1 ;;
  esac
done
CDN_BASE="${SUMMRISE_CDN_BASE:-https://agent.saisi.online}"

NSIS_ROOT="$HOME/nsis-root"
NSIS_SRC_DIR="$HOME/nsis-src/nsis-3.12-src"
NSIS_DIST="$HOME/nsis-dist"
MAKENSIS="$NSIS_DIST/bin/makensis"
OSSLSIGNCODE="$HOME/osslsigncode-root/usr/bin/osslsigncode"

# Optional Authenticode signing (kills the SmartScreen blue warning).
# Provide SUMMRISE_SIGN_CRT (PEM cert file) + SUMMRISE_SIGN_KEY (PEM key file),
# optionally SUMMRISE_SIGN_PASS (key password) and SUMMRISE_SIGN_TSA (RFC3161
# timestamp URL — without it the signature dies with the cert).
# NEVER commit these, never echo them. No cert = skip with a note (the
# build stays shippable; SmartScreen will warn on first run).
# A self-signed cert exercises the whole pipeline (sign+verify) but buys
# ZERO trust — SmartScreen only respects public CA certs (OV: cheaper,
# warns until reputation accrues; EV: instant trust, hardware token).
sign_exe() { # $1 = exe path to sign in place
  local exe="$1"
  if [[ -z "${SUMMRISE_SIGN_CRT:-}" || -z "${SUMMRISE_SIGN_KEY:-}" ]]; then
    echo "-- code signing skipped (SUMMRISE_SIGN_CRT/SUMMRISE_SIGN_KEY unset; SmartScreen will warn)"
    return 0
  fi
  [[ -f "$SUMMRISE_SIGN_CRT" ]] || { echo "::error::SUMMRISE_SIGN_CRT not found: $SUMMRISE_SIGN_CRT" >&2; return 1; }
  [[ -f "$SUMMRISE_SIGN_KEY" ]] || { echo "::error::SUMMRISE_SIGN_KEY not found: $SUMMRISE_SIGN_KEY" >&2; return 1; }
  if [[ ! -x "$OSSLSIGNCODE" ]]; then
    echo "::error::osslsigncode missing at $OSSLSIGNCODE (cert given but no tool)" >&2
    return 1
  fi
  local args=(sign -certs "$SUMMRISE_SIGN_CRT" -key "$SUMMRISE_SIGN_KEY" -h sha256
    -in "$exe" -out "$exe.signed")
  [[ -n "${SUMMRISE_SIGN_PASS:-}" ]] && args+=( -pass "${SUMMRISE_SIGN_PASS}" )
  if [[ -n "${SUMMRISE_SIGN_TSA:-}" ]]; then args+=( -ts "${SUMMRISE_SIGN_TSA}" );
  else echo "-- WARN: no SUMMRISE_SIGN_TSA — signature expires with the cert"; fi
  LD_LIBRARY_PATH="$HOME/osslsigncode-root/usr/lib/x86_64-linux-gnu:${LD_LIBRARY_PATH:-}" \
    "$OSSLSIGNCODE" "${args[@]}" \
    || { echo "::error::osslsigncode sign failed" >&2; return 1; }
  LD_LIBRARY_PATH="$HOME/osslsigncode-root/usr/lib/x86_64-linux-gnu:${LD_LIBRARY_PATH:-}" \
    "$OSSLSIGNCODE" verify "$exe.signed" >/dev/null \
    || { echo "::error::signed exe failed verify" >&2; return 1; }
  mv "$exe.signed" "$exe"
  echo "signed: $exe ($(stat -c %s "$exe") bytes)"
}



need_toolchain() {
  [[ -x "$MAKENSIS" ]] || return 0
  "$MAKENSIS" -VERSION 2>/dev/null | grep -q "v3.12" || return 0
  return 1
}

build_toolchain() {
  echo "== NSIS 3.12 toolchain bootstrap =="
  mkdir -p "$NSIS_ROOT" "$HOME/nsis-src"
  local tmp; tmp="$(mktemp -d)"
  ( cd "$tmp" && timeout 280 apt-get download mingw-w64 binutils-mingw-w64-x86-64 binutils-mingw-w64-i686 gcc-mingw-w64-x86-64 g++-mingw-w64-x86-64 gcc-mingw-w64-i686 g++-mingw-w64-i686 mingw-w64-x86-64-dev mingw-w64-i686-dev mingw-w64-common libz-mingw-w64-dev scons )
  for d in "$tmp"/*.deb; do dpkg-deb -x "$d" "$NSIS_ROOT"; done
  # The unsuffixed driver names Debian alternatives generate do not exist in this userspace
  # tree, so add symlinks for them.
  mkdir -p "$NSIS_ROOT/bin"
  ( cd "$NSIS_ROOT/bin" && for a in x86_64-w64-mingw32 i686-w64-mingw32; do
      for t in gcc g++ cpp windres ar ranlib dlltool strip objcopy objdump; do
        if [[ ! -e "$a-$t" ]]; then
          src="$(ls "$NSIS_ROOT/usr/bin/$a-$t"-* 2>/dev/null | head -1)"
          # -f: the dangling links alternatives leaves behind MUST be overwritten
          [[ -n "$src" ]] && ln -sfn "$src" "$a-$t"
        fi
      done
    done )
  if [[ ! -d "$NSIS_SRC_DIR" ]]; then
    curl -sSL -o "$tmp/nsis-src.tar.bz2" \
      "https://sourceforge.net/projects/nsis/files/NSIS%203/3.12/nsis-3.12-src.tar.bz2/download" --max-time 180
    tar xjf "$tmp/nsis-src.tar.bz2" -C "$HOME/nsis-src"
  fi
  # The scons build (the apport hook is broken on this machine, so a runner works around it;
  # NSIS_SCONS_GNU_ENVPATHHACK makes the cross env inherit PATH, or windres is not found)
  cat > "$tmp/runscons.py" <<'EOF'
import sys
sys.excepthook = sys.__excepthook__
import runpy
sys.argv = ['scons'] + sys.argv[1:]
runpy.run_path('/home/REPLACEME/nsis-root/usr/bin/scons', run_name='__main__')
EOF
  sed -i "s|/home/REPLACEME|$HOME|" "$tmp/runscons.py"
  ( cd "$NSIS_SRC_DIR" \
    && export PATH="$NSIS_ROOT/bin:$NSIS_ROOT/usr/bin:$PATH" \
    && export ZLIB_W32="$NSIS_ROOT/usr/i686-w64-mingw32" \
    && export NSIS_SCONS_GNU_ENVPATHHACK=1 \
    && python3 "$tmp/runscons.py" -j"$(nproc)" VERSION=3.12 \
    && python3 "$tmp/runscons.py" install PREFIX="$NSIS_DIST" VERSION=3.12 )
  rm -rf "$tmp"
  "$MAKENSIS" -VERSION
}

if need_toolchain; then build_toolchain; else echo "toolchain OK: $("$MAKENSIS" -VERSION)"; fi

echo "== stage =="
# PS 5.1 parses a BOM-less .ps1 as the ANSI codepage — the Chinese strings
# in the bootstrap break GBK decoding and the script dies with PARSE errors
# before any statement runs (installer.log never written; round-552 field
# failure). UTF-8 BOM is mandatory for non-ASCII ps1 shipped to Windows.
if ! head -c 3 agent/deploy/summrise-online-setup.ps1 | grep -q $'\xef\xbb\xbf'; then
  echo "::error::summrise-online-setup.ps1 lacks a UTF-8 BOM (PS5.1 would parse it as GBK and die) — fix: python3 -c \"d=open(p,'rb').read(); open(p,'wb').write(b'\\xef\\xbb\\xbf'+d.lstrip(b'\\xef\\xbb\\xbf'))\"" >&2
  exit 1
fi
STAGE="$(mktemp -d)/installer"
mkdir -p "$STAGE/res"
cp agent/deploy/summrise-setup.nsi agent/deploy/summrise-online-setup.ps1 agent/deploy/summrise-agent.ico "$STAGE/"
# round-124: the ps1 dot-sources lib/SummriseIntegrity.ps1 — stage it too, or the
# installer builds fine and fails on the user's machine.
mkdir -p "$STAGE/lib"
cp agent/deploy/lib/SummriseIntegrity.ps1 "$STAGE/lib/"
cp agent/deploy/res/header.bmp agent/deploy/res/welcome.bmp "$STAGE/res/"
# Self-contained prerequisite: the pinned tgz must already be staged (publish packs it first; the
# wrong order fails in place).
TGZ_SRC="index/public/summrise-agent/summrise-agent-$VER.tgz"
[[ -f "$TGZ_SRC" ]] || { echo "::error::missing $TGZ_SRC — pack+stage the tgz first (publish-release.sh does this before the installer)" >&2; exit 1; }
cp "$TGZ_SRC" "$STAGE/summrise-agent-$VER.tgz"
TGZ_SIZE=$(stat -c %s "$STAGE/summrise-agent-$VER.tgz")
# THE DIGEST OF THE PAYLOAD WE EMBED, computed here because this is the only place it
# is known: NSIS carries it into the installer and summrise-online-setup.ps1 checks the
# file against it before npm installs it. The bundled arm was EXEMPT because the payload
# was called "code signed" — a property this build does not provide: SUMMRISE_SIGN_CRT
# and SUMMRISE_SIGN_KEY are set by nothing (10 references, all in this file, none in
# .github/workflows and none at the two callers), so sign_exe prints "code signing
# skipped" and returns 0 in every automated build. That arm runs with NO network, so it
# has no /api/version manifest to consult either; without this number it installs a
# tarball nothing has hashed.
TGZ_SHA256=$(sha256sum "$STAGE/summrise-agent-$VER.tgz" | cut -d' ' -f1)
echo "bundled tgz: summrise-agent-$VER.tgz ($TGZ_SIZE bytes, sha256 $TGZ_SHA256)"

echo "== compile =="
( cd "$STAGE" && "$MAKENSIS" "-DSUMMRISE_VERSION=$VER" "-DSUMMRISE_CDN=$CDN_BASE" "-DSUMMRISE_TGZ_SHA256=$TGZ_SHA256" summrise-setup.nsi )
EXE="$STAGE/SummriseAgent-Setup-$VER.exe"
[[ -f "$EXE" ]] || { echo "::error::makensis produced no exe" >&2; exit 1; }
# Sign BEFORE the size proof + staging so every downstream hash
# (manifest installer_sha256, smoke) covers the final shipped bytes.
sign_exe "$EXE"
SIZE=$(stat -c %s "$EXE")
# The self-contained proof: the finished exe must be LARGER than the tgz embedded in it (stub +
# lzma overhead). The 150KB gate from the online-package era is void — a 200KB "self-contained"
# package always means the tgz never made it in.
[[ "$SIZE" -gt "$TGZ_SIZE" ]] || { echo "::error::exe ($SIZE bytes) not larger than bundled tgz ($TGZ_SIZE bytes) — payload missing?" >&2; exit 1; }
# The version number is already compiled into the file name (OutFile SummriseAgent-Setup-$VER.exe
# is the check); the payload is lzma-compressed, so no plaintext can be grepped out of the exe and
# there is no content-grep gate.
echo "built $EXE ($SIZE bytes, self-contained with $TGZ_SIZE-byte tgz)"

echo "== publish to CDN staging =="
ASSET_DIR="index/public/summrise-agent"
cp "$EXE" "$ASSET_DIR/SummriseAgent-Setup-$VER.exe"
cp "$EXE" "$ASSET_DIR/SummriseAgent-Setup.exe"
if [[ "$NO_DEPLOY" -eq 1 ]]; then
  echo "-- --no-deploy: staged only, skipping wrangler deploy (caller deploys)"
  echo "== done (staged, not deployed) =="
  echo "  versioned: $ASSET_DIR/SummriseAgent-Setup-$VER.exe"
  echo "  alias:     $ASSET_DIR/SummriseAgent-Setup.exe"
  exit 0
fi
CF_TOKEN="$(cf_token)"
if [[ -z "$CF_TOKEN" ]]; then
  echo "::error::no Cloudflare token — set CLOUDFLARE_API_TOKEN or write ~/.cloudflare-token" >&2
  exit 1
fi
(cd index && CLOUDFLARE_API_TOKEN="$CF_TOKEN" npx wrangler deploy)

# A DEPLOY THAT LEAVES THE MANIFEST BEHIND IS NOT A SUCCESS (round 130).
# This path staged the exe, deployed the worker and printed "== done ==" with the
# new URLs while `/api/version` still described the PREVIOUS installer (or none),
# because nothing rewrote version.json — and publish-release.sh tells operators
# to run this script standalone. Since round 125 the landing page asks the manifest
# BEFORE offering the button, so such a build is invisible at best; at worst the
# door keeps serving the previous installer with the manifest's blessing.
EXE_SHA="$(sha256sum "$EXE" | cut -d' ' -f1)"
MANIFEST_JSON="$(curl -sS -m 30 "$CDN_BASE/api/version" 2>/dev/null)" || MANIFEST_JSON=""
VERDICT="$(installer_manifest_verdict "$MANIFEST_JSON" "$EXE_SHA")"
case "$VERDICT" in
  ok) echo "  manifest: /api/version advertises THIS installer (installer_sha256 matches)" ;;
  *)
    echo "::error::deployed, but /api/version does not advertise this installer ($VERDICT)" >&2
    echo "  the door links SummriseAgent-Setup.exe only when the manifest carries installer +" >&2
    echo "  installer_sha256, so this build is invisible — or the previous one is still the" >&2
    echo "  one being offered. Use ./scripts/publish-release.sh $VER (it writes the manifest" >&2
    echo "  and smokes it), or rewrite version.json's installer fields for $VER by hand." >&2
    exit 1 ;;
esac

echo "== done =="
echo "  versioned: $CDN_BASE/summrise-agent/SummriseAgent-Setup-$VER.exe"
echo "  latest:    $CDN_BASE/summrise-agent/SummriseAgent-Setup.exe"
echo "  next: run one install verification on the Windows sandbox machine (see agent/deploy/README-installer.md), then update the index download page."
