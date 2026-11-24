#!/usr/bin/env bash
# Install the portable device core as a Linux systemd service.
#
# The Windows installer owns a launcher, a baseline and a swap; Linux has none of those yet. This
# script is the Phase 2 adapter's first half: one binary, one system user, one data directory and the
# unit in packaging/linux. It is deliberately explicit about every path so an operator can audit it
# before running it as root.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${BINARY:-$ROOT/device/target/release/summrise-device}"
INSTALL_DIR="${INSTALL_DIR:-/opt/summrise}"
DATA_DIR="${DATA_DIR:-/var/lib/summrise}"
PORT="${PORT:-18090}"
SERVICE_USER="${SERVICE_USER:-summrise}"
UNIT="$ROOT/packaging/linux/summrise-device.service"
DEST_UNIT="/etc/systemd/system/summrise-device.service"

if [ "$(id -u)" -ne 0 ]; then
    echo "install-linux-service: run as root (it creates a system user and a systemd unit)" >&2
    exit 1
fi
if [ ! -x "$BINARY" ]; then
    echo "install-linux-service: no executable at $BINARY (build summrise-device or set BINARY)" >&2
    exit 1
fi

if ! id "$SERVICE_USER" >/dev/null 2>&1; then
    useradd --system --home-dir "$DATA_DIR" --shell /usr/sbin/nologin "$SERVICE_USER"
fi

install -d -m 0755 "$INSTALL_DIR"
install -m 0755 "$BINARY" "$INSTALL_DIR/summrise-device"
install -d -o "$SERVICE_USER" -g "$SERVICE_USER" -m 0750 "$DATA_DIR"
if [ -n "${BROWSER_PACK:-}" ]; then
    if [ ! -x "$BROWSER_PACK/chrome-headless-shell" ] && [ ! -x "$BROWSER_PACK/chrome" ]; then
        echo "install-linux-service: BROWSER_PACK must contain chrome-headless-shell or chrome" >&2
        exit 1
    fi
    install -d -m 0755 "$INSTALL_DIR/browser/current"
    cp -a "$BROWSER_PACK/." "$INSTALL_DIR/browser/current/"
fi

# The unit is installed from the repository so the checked-in file and the running service cannot
# drift. Rewrite the three paths the operator can override.
sed -e "s#/opt/summrise#$INSTALL_DIR#g" \
    -e "s#/var/lib/summrise#$DATA_DIR#g" \
    -e "s#--port 18090#--port $PORT#" \
    -e "s#^User=summrise#User=$SERVICE_USER#" \
    -e "s#^Group=summrise#Group=$SERVICE_USER#" \
    "$UNIT" >"$DEST_UNIT"
chmod 0644 "$DEST_UNIT"

systemctl daemon-reload
systemctl enable --now summrise-device.service
systemctl --no-pager --full status summrise-device.service || true
echo "install-linux-service: device should answer on http://127.0.0.1:$PORT/api/health"
