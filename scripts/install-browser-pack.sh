#!/usr/bin/env bash
# Install a per-OS Chrome headless-shell pack into <install>/browser/current.
#
# The Windows installer carries the pack; this is the Unix half, so a Linux or macOS device can be
# given the pinned Chrome for Testing archive without a separate package. The archive is unpacked
# and validated in a temp directory first: a truncated download must not delete the working pack.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ARCHIVE=""
INSTALL_DIR=""

usage() {
    cat >&2 <<'USAGE'
usage: install-browser-pack.sh --archive <zip> --install-dir <dir>

The zip is a Chrome for Testing `chrome-headless-shell` archive (or a plain Chrome archive). Its
directory is copied to <dir>/browser/current; the device's browser capability finds either
`chrome-headless-shell` or `chrome` there.
USAGE
}

while [ $# -gt 0 ]; do
    case "$1" in
        --archive) ARCHIVE="${2:-}"; shift 2 ;;
        --install-dir) INSTALL_DIR="${2:-}"; shift 2 ;;
        -h|--help) usage; exit 0 ;;
        *) echo "install-browser-pack: unknown argument $1" >&2; usage; exit 2 ;;
    esac
done

[ -n "$ARCHIVE" ] || { usage; exit 2; }
[ -n "$INSTALL_DIR" ] || { usage; exit 2; }
[ -f "$ARCHIVE" ] || { echo "install-browser-pack: no archive at $ARCHIVE" >&2; exit 1; }
case "$INSTALL_DIR" in
    /*) ;;
    *) echo "install-browser-pack: --install-dir must be absolute" >&2; exit 1 ;;
esac
[ "$INSTALL_DIR" != "/" ] || { echo "install-browser-pack: refusing to install into /" >&2; exit 1; }
command -v unzip >/dev/null 2>&1 || { echo "install-browser-pack: unzip is required" >&2; exit 1; }

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
unzip -q "$ARCHIVE" -d "$WORK/pack"

# Prefer the headless shell when the archive has both; a full Chrome is still a valid pack.
EXE="$(find "$WORK/pack" -type f -name 'chrome-headless-shell' -print -quit)"
if [ -z "$EXE" ]; then
    EXE="$(find "$WORK/pack" -type f -name 'chrome' -print -quit)"
fi
if [ -z "$EXE" ]; then
    echo "install-browser-pack: the archive has no chrome-headless-shell or chrome" >&2
    exit 1
fi
chmod +x "$EXE"

DEST="$INSTALL_DIR/browser/current"
mkdir -p "$INSTALL_DIR/browser"
rm -rf "$DEST"
mkdir -p "$DEST"
cp -a "$(dirname "$EXE")/." "$DEST/"
chmod +x "$DEST/$(basename "$EXE")"

SIZE="$(du -sh "$DEST" | cut -f1)"
echo "BROWSER_PACK_INSTALLED path=$DEST executable=$(basename "$EXE") size=$SIZE"
