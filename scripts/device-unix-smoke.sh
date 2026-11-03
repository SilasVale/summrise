#!/usr/bin/env bash
# The Phase 2 floor: the device core, not a GUI, serves its health document, its panel and its MCP
# registry on Linux and macOS.
#
# This is a run, not another unit test. It starts the same `summrise-device` binary the Windows
# service runs, with a throwaway install/data pair and a loopback port, then asks health, the panel
# and the MCP tool list. The Windows lifecycle suite owns the service, installer and swap; this owns
# "the portable half actually boots" on the Unix targets.
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
# A fixed test token: this process is loopback-only and the data directory dies with the run. The
# token is what lets the smoke ask the protected MCP surface for its tool list, not just health.
TOKEN="0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
printf '%s' "$TOKEN" >"$WORK/data/token"
cargo build --manifest-path "$ROOT/device/Cargo.toml" -p summrise-device

"$ROOT/device/target/debug/summrise-device" \
    --install "$WORK/install" --data "$WORK/data" --port "$PORT" \
    >"$WORK/device.log" 2>&1 &
PID=$!

HEALTH="$WORK/health.json"
i=0
while [ "$i" -lt 100 ]; do
    i=$((i + 1))
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

# The MCP surface is the product. Streamable HTTP requires an initialize handshake and then
# `tools/list`; this proves the registry the console reads is present on Linux, not only that the
# HTTP server accepted a connection.
curl -fsS -m 5 -D "$WORK/init.headers" -o "$WORK/init.body" -X POST "http://127.0.0.1:$PORT/mcp" \
    -H "authorization: Bearer $TOKEN" \
    -H 'content-type: application/json' \
    -H 'accept: application/json, text/event-stream' \
    -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"device-linux-smoke","version":"1"}}}'
SESSION="$(awk 'tolower($1)=="mcp-session-id:" {print $2}' "$WORK/init.headers" | tr -d '\r')"
if [ -z "$SESSION" ]; then
    echo "device-linux-smoke: MCP initialize returned no session id" >&2
    cat "$WORK/init.headers" >&2
    exit 1
fi

curl -fsS -m 5 -X POST "http://127.0.0.1:$PORT/mcp" \
    -H "authorization: Bearer $TOKEN" \
    -H 'content-type: application/json' \
    -H 'accept: application/json, text/event-stream' \
    -H "mcp-session-id: $SESSION" \
    -H 'mcp-protocol-version: 2025-06-18' \
    -d '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}' \
    >"$WORK/tools.json"
if ! grep -q 'terminal.create' "$WORK/tools.json"; then
    echo "device-linux-smoke: the MCP tool list has no terminal.create" >&2
    cat "$WORK/tools.json" >&2 || true
    exit 1
fi

echo "DEVICE_UNIX_SMOKE_OK port=$PORT panel_bytes=$(wc -c <"$WORK/panel.html") health=$(cat "$HEALTH") mcp_tools=terminal.create"
