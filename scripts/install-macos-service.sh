#!/usr/bin/env bash
# Install the portable device core as a macOS launchd daemon.
#
# The Windows installer has an SCM service; Linux has the systemd unit; this is the launchd half of
# Phase 2. It is deliberately explicit about every path. Run it as root on the Mac that will be the
# device: it creates a hidden system user, installs the binary, and bootstraps the daemon.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${BINARY:-$ROOT/device/target/release/summrise-device}"
INSTALL_DIR="${INSTALL_DIR:-/opt/summrise}"
DATA_DIR="${DATA_DIR:-/var/lib/summrise}"
PORT="${PORT:-18090}"
SERVICE_USER="${SERVICE_USER:-_summrise}"
PLIST="$ROOT/packaging/macos/online.summrise.device.plist"
DEST_PLIST="/Library/LaunchDaemons/online.summrise.device.plist"
LOG="/var/log/summrise-device.log"

if [ "$(id -u)" -ne 0 ]; then
    echo "install-macos-service: run as root (it creates a system user and a LaunchDaemon)" >&2
    exit 1
fi
if [ "$(uname -s)" != "Darwin" ]; then
    echo "install-macos-service: this script is for macOS" >&2
    exit 1
fi
if [ ! -x "$BINARY" ]; then
    echo "install-macos-service: no executable at $BINARY (build summrise-device or set BINARY)" >&2
    exit 1
fi

if ! id "$SERVICE_USER" >/dev/null 2>&1; then
    sysadminctl -addUser "$SERVICE_USER" -fullName "Summrise Device" -shell /usr/bin/false -home "$DATA_DIR"
fi

install -d -m 0755 "$INSTALL_DIR"
install -m 0755 "$BINARY" "$INSTALL_DIR/summrise-device"
install -d -o "$SERVICE_USER" -g "$SERVICE_USER" -m 0750 "$DATA_DIR"
if [ -n "${BROWSER_PACK:-}" ]; then
    if [ ! -x "$BROWSER_PACK/chrome-headless-shell" ] && [ ! -x "$BROWSER_PACK/chrome" ]; then
        echo "install-macos-service: BROWSER_PACK must contain chrome-headless-shell or chrome" >&2
        exit 1
    fi
    install -d -m 0755 "$INSTALL_DIR/browser/current"
    cp -a "$BROWSER_PACK/." "$INSTALL_DIR/browser/current/"
fi
touch "$LOG"
chown "$SERVICE_USER":"$SERVICE_USER" "$LOG"

sed -e "s#/opt/summrise#$INSTALL_DIR#g" \
    -e "s#/var/lib/summrise#$DATA_DIR#g" \
    -e "s#_summrise#$SERVICE_USER#g" \
    -e "s#<string>18090</string>#<string>$PORT</string>#" \
    -e "s#/var/log/summrise-device.log#$LOG#g" \
    "$PLIST" >"$DEST_PLIST"
chown root:wheel "$DEST_PLIST"
chmod 0644 "$DEST_PLIST"

launchctl bootout system/online.summrise.device 2>/dev/null || true
launchctl bootstrap system "$DEST_PLIST"
launchctl enable system/online.summrise.device
launchctl print system/online.summrise.device | head -20 || true
echo "install-macos-service: device should answer on http://127.0.0.1:$PORT/api/health"
