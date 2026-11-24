#!/usr/bin/env bash
# The systemd unit is a product artifact; a typo in ExecStart is a service that never starts.
# `systemd-analyze verify` checks the syntax and that ExecStart names an executable. It runs here
# with the freshly built device binary and this host's user, so CI needs no root and no running
# systemd.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:-$ROOT/device/target/debug/summrise-device}"
if [ ! -x "$BINARY" ]; then
    echo "verify-linux-unit: no executable at $BINARY" >&2
    exit 1
fi

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
install -d "$WORK/install" "$WORK/data"
cp "$BINARY" "$WORK/install/summrise-device"
install -d "$WORK/install/browser/current"

sed -e "s#/opt/summrise#$WORK/install#g" \
    -e "s#/var/lib/summrise#$WORK/data#g" \
    -e "s#^User=summrise#User=$(id -un)#" \
    -e "s#^Group=summrise#Group=$(id -gn)#" \
    -e "s#--port 18090#--port 18097#" \
    "$ROOT/packaging/linux/summrise-device.service" >"$WORK/summrise-device.service"

if ! command -v systemd-analyze >/dev/null 2>&1; then
    echo "verify-linux-unit: systemd-analyze is not installed" >&2
    exit 1
fi
systemd-analyze verify "$WORK/summrise-device.service"
echo "LINUX_UNIT_VERIFY_OK"
