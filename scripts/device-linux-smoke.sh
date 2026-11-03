#!/usr/bin/env bash
# The Phase 2 floor: the device core, not a GUI, serves its health document and its panel on Linux.
#
# This is a run, not another unit test. It starts the same `summrise-device` binary the Windows
# service runs, with a throwaway install/data pair and a loopback port, and asks the two questions an
# operator asks first. The Windows lifecycle suite owns the service, installer and swap; this owns
# "the portable half actually boots".
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PORT=$((20000 + RANDOM % 20000))
WORK="$(mktemp -d)"
PID=""

cleanup() {
    if [ -n "$PID" ] && kill -0 "$PID" 2>/dev/null; then
        kill "$PID" 2>/dev/null || true
        wait "$PID" 2>/dev/null || true
    fi
    rm -rf "$WORK"
}
trap cleanup EXIT

mkdir -p "$WORK/install" "$WORK/data"
cargo build --manifest-path "$ROOT/device/Cargo.toml" -p summrise-device

"$ROOT/device/target/debug/summrise-device" \
    --install "$WORK/install" --data "$WORK/data" --port "$PORT" \
    >"$WORK/device.log" 2>&1 &
PID=$!

HEALTH="$WORK/health.json"
for _ in $(seq 1 100); do
    if curl -fsS -m 1 "http://127.0.0.1:$PORT/api/health" >"$HEALTH" 2>/dev/null; then
        break
    fi
    if ! kill -0 "$PID" 2>/dev/null; then
        echo "device-linux-smoke: the device exited before serving" >&2
        cat "$WORK/device.log" >&2
        exit 1
    fi
    sleep 0.1
done

if ! grep -q '"status":"ok"' "$HEALTH"; then
    echo "device-linux-smoke: no ok health document on port $PORT" >&2
    cat "$HEALTH" >&2 || true
    cat "$WORK/device.log" >&2
    exit 1
fi

curl -fsS -m 5 "http://127.0.0.1:$PORT/" >"$WORK/panel.html"
if [ ! -s "$WORK/panel.html" ]; then
    echo "device-linux-smoke: the device panel was empty" >&2
    exit 1
fi

echo "DEVICE_LINUX_SMOKE_OK port=$PORT panel_bytes=$(wc -c <"$WORK/panel.html") health=$(cat "$HEALTH")"
