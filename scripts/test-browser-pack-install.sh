#!/usr/bin/env bash
# A pack installer that copies the wrong directory is a browser that never starts. This builds a
# tiny archive with the Unix executable name and checks the layout the device resolves.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

mkdir -p "$WORK/source/chrome-headless-shell-linux64"
printf '#!/bin/sh\nexit 0\n' >"$WORK/source/chrome-headless-shell-linux64/chrome-headless-shell"
chmod +x "$WORK/source/chrome-headless-shell-linux64/chrome-headless-shell"
(cd "$WORK/source" && zip -q -r "$WORK/pack.zip" .)

"$ROOT/scripts/install-browser-pack.sh" --archive "$WORK/pack.zip" --install-dir "$WORK/install"
test -x "$WORK/install/browser/current/chrome-headless-shell"
test -f "$WORK/install/browser/current/icudtl.dat" && exit 1 || true
echo "BROWSER_PACK_INSTALL_TEST_OK"
