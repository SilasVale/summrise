#!/usr/bin/env bash
# scripts/smoke-enroll.sh — the operator loop against the two real binaries.
#
# One console and one device on loopback: the operator signs in, mints a one-time enrollment
# code, leaves it where a deployment leaves it (`<device-data>/console-bootstrap.json`), and the
# device registers itself at first start. The console's `/fleet` must show the machine online; the
# operator mints a permanent token for it; one MCP `tools/call host.facts` goes through the
# console's standard endpoint with that token; and both the console's audit and the device's own
# event log must record the call.
#
# Usage:
#   bash scripts/smoke-enroll.sh
#
# Environment:
#   CARGO       the cargo binary (default ~/.cargo/bin/cargo, then PATH)
#   SMOKE_KEEP  1 keeps the temp directories even on success
#   SMOKE_NO_BUILD 1 skips the (incremental) build and uses the binaries as they are
#
# Exits 0 and prints `ENROLL SMOKE OK` only when every assertion passed. On the first failure it
# prints the failing request/response (or path), the reason, and keeps both temp directories as
# evidence.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEVICE_BIN="$ROOT/device/target/debug/summrise-device"
CONSOLE_BIN="$ROOT/device/target/debug/summrise-console"
export PATH="$HOME/.cargo/bin:$PATH"
CARGO_BIN="${CARGO:-$HOME/.cargo/bin/cargo}"

CONSOLE_DIR=""
DEVICE_DIR=""
CONSOLE_PID=""
DEVICE_PID=""
HTTP_DIR=""
LAST_URL=""
LAST_STATUS=""
LAST_BODY=""
LAST_HDR=""
CONSOLE_LOG=""
DEVICE_LOG=""
OP_TOKEN=""
SESSION=""
ENROLL_CODE=""
SRM_TOKEN=""
TOKEN_ID=""

cleanup_secrets() {
    OP_TOKEN=""
    SESSION=""
    ENROLL_CODE=""
    SRM_TOKEN=""
}

pass() {
    printf 'PASS %s\n' "$*"
}

# Every 64-hex value on these wires is a credential (operator token, session token, one-time code,
# enrollment/channel token), and an `srm_` value is a permanent token. Hide both before any body or
# log is printed: a failure body must not become a credential leak.
redact() {
    local text
    text="$(cat)"
    text="$(printf '%s' "$text" | LC_ALL=C sed -E 's/[0-9a-f]{64}/***REDACTED***/g')"
    text="$(printf '%s' "$text" | LC_ALL=C sed -E 's/srm_[0-9a-f]{64}/***REDACTED***/g')"
    if [ -n "${OP_TOKEN:-}" ]; then text="${text//$OP_TOKEN/***REDACTED***}"; fi
    if [ -n "${SESSION:-}" ]; then text="${text//$SESSION/***REDACTED***}"; fi
    if [ -n "${ENROLL_CODE:-}" ]; then text="${text//$ENROLL_CODE/***REDACTED***}"; fi
    if [ -n "${SRM_TOKEN:-}" ]; then text="${text//$SRM_TOKEN/***REDACTED***}"; fi
    printf '%s' "$text"
}

fail() {
    printf 'FAIL %s\n' "$*" >&2
    if [ -n "$LAST_URL" ]; then
        printf '  request: %s\n' "$LAST_URL" >&2
        printf '  status:  %s\n' "${LAST_STATUS:-<no response>}" >&2
        if [ -n "$LAST_HDR" ] && [ -f "$LAST_HDR" ]; then
            printf '  headers:\n' >&2
            redact <"$LAST_HDR" | sed 's/^/    /' >&2 || true
        fi
        if [ -n "$LAST_BODY" ] && [ -f "$LAST_BODY" ]; then
            printf '  body (%s):\n' "$LAST_BODY" >&2
            head -c 4000 "$LAST_BODY" | redact | sed 's/^/    /' >&2 || true
            printf '\n' >&2
        fi
    fi
    local log
    for log in "$DEVICE_LOG" "$CONSOLE_LOG"; do
        if [ -n "$log" ] && [ -f "$log" ]; then
            printf '  log (%s, tail):\n' "$log" >&2
            tail -n 40 "$log" | redact | sed 's/^/    /' >&2 || true
        fi
    done
    cleanup_secrets
    exit 1
}

cleanup() {
    local code=$?
    trap - EXIT
    set +e

    if [ -n "$DEVICE_PID" ]; then kill "$DEVICE_PID" 2>/dev/null; fi
    if [ -n "$CONSOLE_PID" ]; then kill "$CONSOLE_PID" 2>/dev/null; fi
    local deadline=$((SECONDS + 5))
    while [ "$SECONDS" -lt "$deadline" ]; do
        local alive=0
        if [ -n "$DEVICE_PID" ] && kill -0 "$DEVICE_PID" 2>/dev/null; then alive=1; fi
        if [ -n "$CONSOLE_PID" ] && kill -0 "$CONSOLE_PID" 2>/dev/null; then alive=1; fi
        [ "$alive" -eq 0 ] && break
        sleep 0.1
    done
    if [ -n "$DEVICE_PID" ]; then kill -9 "$DEVICE_PID" 2>/dev/null; fi
    if [ -n "$CONSOLE_PID" ]; then kill -9 "$CONSOLE_PID" 2>/dev/null; fi
    wait 2>/dev/null

    cleanup_secrets
    if [ "$code" -ne 0 ] || [ "${SMOKE_KEEP:-0}" = "1" ]; then
        printf 'smoke: kept evidence:\n' >&2
        [ -n "$CONSOLE_DIR" ] && printf '  console dir: %s\n' "$CONSOLE_DIR" >&2
        [ -n "$DEVICE_DIR" ] && printf '  device dir:  %s\n' "$DEVICE_DIR" >&2
    else
        [ -n "$CONSOLE_DIR" ] && rm -rf "$CONSOLE_DIR"
        [ -n "$DEVICE_DIR" ] && rm -rf "$DEVICE_DIR"
    fi
    exit "$code"
}
trap cleanup EXIT

process_alive() {
    local pid="$1"
    kill -0 "$pid" 2>/dev/null || return 1
    local state
    state="$(ps -o stat= -p "$pid" 2>/dev/null)" || return 1
    case "$state" in Z*) return 1 ;; esac
    return 0
}

# One HTTP exchange. Every body and header set is kept under $HTTP_DIR/<label>.* so a failure can
# point at the exact response instead of a summary.
request() {
    local label="$1" method="$2" url="$3" data="${4:-}"
    shift 4
    LAST_URL="$url"
    LAST_STATUS=""
    LAST_BODY="$HTTP_DIR/$label.body"
    LAST_HDR="$HTTP_DIR/$label.hdr"
    local args=(-sS --max-time 25 -X "$method" -o "$LAST_BODY" -D "$LAST_HDR" -w '%{http_code}')
    if [ -n "$data" ]; then
        args+=(--data-binary "$data")
    fi
    args+=("$@")
    LAST_STATUS="$(curl "${args[@]}" "$url" 2>"$HTTP_DIR/$label.curl.err")" || LAST_STATUS=000
    printf '%s\n' "$LAST_STATUS" >"$HTTP_DIR/$label.status"
}

assert_status() {
    local expected="$1" what="$2"
    if [ "$LAST_STATUS" != "$expected" ]; then
        fail "$what: expected HTTP $expected, got ${LAST_STATUS:-<none>}"
    fi
    pass "$what -> HTTP $expected"
}

wait_for_200() {
    local url="$1" label="$2" pid="$3" timeout="${4:-15}"
    local deadline=$((SECONDS + timeout))
    while [ "$SECONDS" -lt "$deadline" ]; do
        if [ -n "$pid" ] && ! process_alive "$pid"; then
            fail "$label: the process exited before $url answered 200"
        fi
        request "$label" GET "$url" ""
        [ "$LAST_STATUS" = "200" ] && return 0
        sleep 0.2
    done
    return 1
}

# One JSON-RPC message from either a plain JSON body or an SSE body (the last non-empty data line).
jsonrpc_message() {
    python3 - "$1" <<'PY'
import json
import sys

raw = open(sys.argv[1], encoding="utf-8", errors="replace").read()
text = raw.strip()
if text.startswith("{"):
    data = text
else:
    values = [line[5:].strip() for line in raw.splitlines() if line.startswith("data:")]
    values = [value for value in values if value]
    if not values:
        raise SystemExit("no JSON-RPC data line in the response")
    data = values[-1]
try:
    message = json.loads(data)
except Exception as error:
    raise SystemExit(f"the JSON-RPC message is not JSON: {error}")
print(json.dumps(message, separators=(",", ":")))
PY
}

# Print the id of the one device `/fleet` shows online; exit 1 for any other shape.
fleet_online_id() {
    python3 - "$1" <<'PY'
import json
import sys

try:
    rows = json.load(open(sys.argv[1], encoding="utf-8"))
except Exception:
    raise SystemExit(1)
if not isinstance(rows, list) or len(rows) != 1:
    raise SystemExit(1)
row = rows[0]
if row.get("online") is True and isinstance(row.get("id"), str) and row["id"]:
    print(row["id"])
    raise SystemExit(0)
raise SystemExit(1)
PY
}

for tool in curl python3; do
    command -v "$tool" >/dev/null 2>&1 || fail "missing required tool: $tool"
done

if [ -z "${SMOKE_NO_BUILD:-}" ]; then
    if [ ! -x "$CARGO_BIN" ]; then
        CARGO_BIN="$(command -v cargo 2>/dev/null || true)"
    fi
    [ -x "$CARGO_BIN" ] || fail "no cargo was found (set CARGO, or SMOKE_NO_BUILD=1 for a binary-only run)"
    echo "smoke: building (incremental) with $CARGO_BIN"
    (cd "$ROOT" && "$CARGO_BIN" build --manifest-path device/Cargo.toml -p summrise-device -p summrise-console) \
        || fail "cargo build failed"
fi
[ -x "$DEVICE_BIN" ] || fail "missing binary: $DEVICE_BIN"
[ -x "$CONSOLE_BIN" ] || fail "missing binary: $CONSOLE_BIN"

CONSOLE_DIR="$(mktemp -d "${TMPDIR:-/tmp}/summrise-smoke-enroll-console.XXXXXX")" || fail "mktemp -d failed"
DEVICE_DIR="$(mktemp -d "${TMPDIR:-/tmp}/summrise-smoke-enroll-device.XXXXXX")" || fail "mktemp -d failed"
CONSOLE_DATA="$CONSOLE_DIR/data"
DEVICE_DATA="$DEVICE_DIR/data"
DEVICE_INSTALL="$DEVICE_DIR/install"
HTTP_DIR="$DEVICE_DIR/http"
CONSOLE_LOG="$CONSOLE_DIR/console.log"
DEVICE_LOG="$DEVICE_DIR/device.log"
mkdir -p "$CONSOLE_DATA" "$DEVICE_DATA" "$DEVICE_INSTALL" "$HTTP_DIR" || fail "cannot create temp directories"

echo "smoke: console dir $CONSOLE_DIR"
echo "smoke: device dir  $DEVICE_DIR"
echo "smoke: console binary $CONSOLE_BIN"
echo "smoke: device binary  $DEVICE_BIN"

# Two ports, held open together so they cannot collide, then released for the two listeners.
if ! read -r DEVICE_PORT CONSOLE_PORT < <(python3 - <<'PY'
import socket

sockets = []
ports = []
for _ in range(2):
    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    sock.bind(("127.0.0.1", 0))
    sockets.append(sock)
    ports.append(sock.getsockname()[1])
for sock in sockets:
    sock.close()
print(*ports)
PY
); then
    fail "could not find two free loopback ports"
fi
case "$DEVICE_PORT" in ''|*[!0-9]*) fail "bad device port: '$DEVICE_PORT'" ;; esac
case "$CONSOLE_PORT" in ''|*[!0-9]*) fail "bad console port: '$CONSOLE_PORT'" ;; esac
[ "$DEVICE_PORT" != "$CONSOLE_PORT" ] || fail "the two ports collided: $DEVICE_PORT"
echo "smoke: device port $DEVICE_PORT, console port $CONSOLE_PORT"

# --- the console -------------------------------------------------------------------------------
# `--setup` writes the two secrets the running flags name: the operator token (64 hex) and the
# Ed25519 grant seed (32 bytes as hex, mode 0600). Neither is overwritten on a second run.
if ! "$CONSOLE_BIN" --setup "$CONSOLE_DATA" >"$CONSOLE_DIR/setup.out" 2>"$CONSOLE_DIR/setup.err"; then
    cat "$CONSOLE_DIR/setup.err" >&2 || true
    fail "console --setup failed"
fi
[ -s "$CONSOLE_DATA/operator.token" ] || fail "console --setup did not write operator.token"
[ -s "$CONSOLE_DATA/grant.seed" ] || fail "console --setup did not write grant.seed"
for secret in "$CONSOLE_DATA/operator.token" "$CONSOLE_DATA/grant.seed"; do
    mode="$(stat -c '%a' "$secret" 2>/dev/null)" || fail "cannot stat $secret"
    [ "$mode" = "600" ] || fail "$secret is mode $mode, not 600"
done
pass "console --setup created 0600 operator.token and grant.seed"

OP_TOKEN="$(tr -d '\r\n' <"$CONSOLE_DATA/operator.token")" || fail "cannot read the operator token"
case "$OP_TOKEN" in
    ''|*[!0-9a-f]*) fail "the operator token is not lowercase hex" ;;
esac
[ "${#OP_TOKEN}" -eq 64 ] || fail "the operator token is ${#OP_TOKEN} characters, not 64"

"$CONSOLE_BIN" \
    --operator-token-file "$CONSOLE_DATA/operator.token" \
    --grant-key-file "$CONSOLE_DATA/grant.seed" \
    --data "$CONSOLE_DATA" \
    --port "$CONSOLE_PORT" \
    --bind 127.0.0.1 \
    >"$CONSOLE_LOG" 2>&1 &
CONSOLE_PID=$!
CONSOLE_BASE="http://127.0.0.1:$CONSOLE_PORT"

if ! wait_for_200 "$CONSOLE_BASE/health" console-health "$CONSOLE_PID" 20; then
    fail "console GET /health did not answer 200 within 20 s"
fi
pass "console GET /health -> HTTP 200"

# --- the operator session and the enrollment code ----------------------------------------------
AUTH_BODY="$(python3 -c 'import json,sys; print(json.dumps({"token": sys.argv[1]}))' "$OP_TOKEN")" \
    || fail "cannot encode the auth body"
request console-auth POST "$CONSOLE_BASE/auth" "$AUTH_BODY" -H 'Content-Type: application/json'
assert_status 200 "console POST /auth with the operator token"
if ! SESSION="$(python3 - "$LAST_BODY" <<'PY'
import json
import sys

document = json.load(open(sys.argv[1], encoding="utf-8"))
token = document.get("token")
if not isinstance(token, str) or len(token) != 64:
    raise SystemExit("the auth answer has no 64-hex session token")
print(token)
PY
)"; then
    fail "console POST /auth did not return a session token"
fi
pass "console POST /auth issued an operator session"

request console-enroll POST "$CONSOLE_BASE/enroll" '{}' \
    -H "Authorization: Bearer $SESSION" -H 'Content-Type: application/json'
assert_status 200 "console POST /enroll (operator session)"
if ! ENROLL_CODE="$(python3 - "$LAST_BODY" <<'PY'
import json
import sys

document = json.load(open(sys.argv[1], encoding="utf-8"))
code = document.get("code")
if not isinstance(code, str) or not code:
    raise SystemExit("the enroll answer has no code")
print(code)
PY
)"; then
    fail "console POST /enroll did not return a one-time code"
fi
pass "console POST /enroll minted a one-time code"

# --- the device registers itself ---------------------------------------------------------------
# A deployment that cannot pass a code on a command line leaves `<data>/console-bootstrap.json`;
# the device enrols at first start (before the console link is loaded), writes `<data>/console-token.json`,
# and deletes the bootstrap file as the receipt. That is the real self-enrollment path.
mkdir -p "$DEVICE_DATA" "$DEVICE_INSTALL" || fail "cannot create the device directories"
if ! python3 - "$DEVICE_DATA/console-bootstrap.json" "$CONSOLE_BASE" "$ENROLL_CODE" <<'PY'
import json
import os
import sys

path, url, code = sys.argv[1], sys.argv[2], sys.argv[3]
with open(path, "w", encoding="utf-8") as handle:
    json.dump({"url": url, "code": code, "allow_plaintext": True}, handle)
os.chmod(path, 0o600)
PY
then
    fail "cannot write the device bootstrap file"
fi
pass "wrote the one-time code to the device's console-bootstrap.json"

# The device's loopback API is not used by this smoke, but its start reads this file once: writing
# it avoids a spurious "protected endpoints will answer 401" warning in the evidence log.
INSTALL_TOKEN="$(python3 -c 'import secrets; print(secrets.token_hex(32))')" || fail "cannot mint a device token"
printf '%s\n' "$INSTALL_TOKEN" >"$DEVICE_DATA/token" || fail "cannot write the device token file"
chmod 600 "$DEVICE_DATA/token" || fail "cannot chmod the device token file"

"$DEVICE_BIN" \
    --console-url "$CONSOLE_BASE" \
    --console-token-file "$DEVICE_DATA/console-token.json" \
    --data "$DEVICE_DATA" \
    --install "$DEVICE_INSTALL" \
    --port "$DEVICE_PORT" \
    --token-file "$DEVICE_DATA/token" \
    >"$DEVICE_LOG" 2>&1 &
DEVICE_PID=$!
DEVICE_BASE="http://127.0.0.1:$DEVICE_PORT"

if ! wait_for_200 "$DEVICE_BASE/api/health" device-health "$DEVICE_PID" 25; then
    fail "device GET /api/health did not answer 200 within 25 s"
fi
pass "device GET /api/health -> HTTP 200"

if ! DEVICE_HEALTH="$(python3 - "$LAST_BODY" <<'PY'
import json
import sys

document = json.load(open(sys.argv[1], encoding="utf-8"))
if document.get("status") != "ok":
    raise SystemExit(f"health status is {document.get('status')!r}")
print(f"version {document.get('version')}, pid {document.get('pid')}")
PY
)"; then
    fail "device /api/health did not report status ok"
fi
pass "device /api/health reports status ok ($DEVICE_HEALTH)"

# The bootstrap enrollment must have written the registration the channel uses. Reading its id is
# also the cross-check that the /fleet row below is this device and not a leftover.
[ -s "$DEVICE_DATA/console-token.json" ] || fail "the device did not write console-token.json (self-enrollment failed)"
if ! DEVICE_ID_FILE="$(python3 - "$DEVICE_DATA/console-token.json" <<'PY'
import json
import sys

document = json.load(open(sys.argv[1], encoding="utf-8"))
device_id = document.get("id")
token = document.get("token")
if not isinstance(device_id, str) or len(device_id) != 64:
    raise SystemExit("console-token.json has no 64-hex id")
if not isinstance(token, str) or len(token) != 64:
    raise SystemExit("console-token.json has no 64-hex token")
print(device_id)
PY
)"; then
    fail "the device's console-token.json is not a registration response"
fi
[ ! -e "$DEVICE_DATA/console-bootstrap.json" ] || fail "the bootstrap file survived a successful enrollment"
mode="$(stat -c '%a' "$DEVICE_DATA/console-token.json" 2>/dev/null)" || fail "cannot stat console-token.json"
[ "$mode" = "600" ] || fail "console-token.json is mode $mode, not 600"
pass "device self-enrolled, wrote 0600 console-token.json, and consumed the bootstrap file"

DEVICE_ID=""
FLEET_DEADLINE=$((SECONDS + 20))
while [ "$SECONDS" -lt "$FLEET_DEADLINE" ]; do
    if ! process_alive "$DEVICE_PID"; then
        fail "the device exited before it appeared online"
    fi
    request fleet GET "$CONSOLE_BASE/fleet" "" -H "Authorization: Bearer $SESSION"
    if [ "$LAST_STATUS" = "200" ] && DEVICE_ID="$(fleet_online_id "$LAST_BODY" 2>/dev/null)"; then
        [ -n "$DEVICE_ID" ] && break
    fi
    DEVICE_ID=""
    sleep 0.2
done
[ -n "$DEVICE_ID" ] || fail "the device did not appear online in console GET /fleet within 20 s"
[ "$DEVICE_ID" = "$DEVICE_ID_FILE" ] || fail "console /fleet named $DEVICE_ID, console-token.json named $DEVICE_ID_FILE"
pass "the device enrolled and is online"
echo "smoke: device id $DEVICE_ID"

# --- the operator mints a token through the console --------------------------------------------
TOKEN_BODY="$(python3 - "$DEVICE_ID" <<'PY'
import json
import sys

print(json.dumps({
    "name": "enroll-smoke",
    "device_id": sys.argv[1],
    "tools": ["host.facts"],
    # The console's shortest accepted lifetime is 3 600 s (MIN_TOKEN_EXPIRY_S); anything shorter
    # is a 400 `expiry_too_short`, which is why the smoke asks for the floor, not a few minutes.
    "expires_in_s": 3600,
    "allow_destructive": False,
}))
PY
)" || fail "cannot encode the token body"
request console-token POST "$CONSOLE_BASE/tokens" "$TOKEN_BODY" \
    -H "Authorization: Bearer $SESSION" -H 'Content-Type: application/json'
assert_status 201 "console POST /tokens for the enrolled device"
if ! TOKEN_INFO="$(python3 - "$LAST_BODY" "$DEVICE_ID" <<'PY'
import json
import sys

document = json.load(open(sys.argv[1], encoding="utf-8"))
requested_device = sys.argv[2]
token = document.get("token")
token_id = document.get("id")
if not isinstance(token, str) or not token.startswith("srm_") or len(token) != 68:
    raise SystemExit("the mint answer has no srm_ token")
if not isinstance(token_id, str) or not token_id:
    raise SystemExit("the mint answer has no token record id")
if document.get("device_id") != requested_device:
    raise SystemExit("the mint answer names another device")
print(token)
print(token_id)
PY
)"; then
    fail "console POST /tokens did not return a usable token"
fi
SRM_TOKEN="$(printf '%s\n' "$TOKEN_INFO" | sed -n '1p')"
TOKEN_ID="$(printf '%s\n' "$TOKEN_INFO" | sed -n '2p')"
[ -n "$SRM_TOKEN" ] || fail "the minted token is empty"
[ -n "$TOKEN_ID" ] || fail "the minted token record id is empty"
pass "the console minted a token"
echo "smoke: token record $TOKEN_ID permits host.facts"

# --- one MCP call through the console ----------------------------------------------------------
MCP_URL="$CONSOLE_BASE/mcp/$DEVICE_ID"
AUTH=(-H "Authorization: Bearer $SRM_TOKEN")
MCP_HEADERS=(-H 'Content-Type: application/json' -H 'Accept: application/json, text/event-stream')
INIT_REQUEST='{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"smoke-enroll","version":"0.1.0"}}}'

request mcp-initialize POST "$MCP_URL" "$INIT_REQUEST" "${AUTH[@]}" "${MCP_HEADERS[@]}"
assert_status 200 "mcp initialize through the console"
if ! jsonrpc_message "$HTTP_DIR/mcp-initialize.body" >"$HTTP_DIR/mcp-initialize.json"; then
    fail "mcp initialize did not answer a JSON-RPC message"
fi
if ! PROTOCOL="$(python3 - "$HTTP_DIR/mcp-initialize.json" <<'PY'
import json
import sys

message = json.load(open(sys.argv[1], encoding="utf-8"))
if "error" in message:
    raise SystemExit(f"initialize returned a JSON-RPC error: {message['error']}")
result = message.get("result")
if not isinstance(result, dict):
    raise SystemExit(f"initialize has no result object: {message}")
version = result.get("protocolVersion")
if not isinstance(version, str) or not version:
    raise SystemExit(f"initialize has no protocolVersion: {message}")
print(version)
PY
)"; then
    fail "mcp initialize did not negotiate a protocol version"
fi
SESSION_ID="$(sed -n 's/^[Mm][Cc][Pp]-[Ss]ession-[Ii][Dd]:[[:space:]]*//p' "$HTTP_DIR/mcp-initialize.hdr" | tr -d '\r' | tail -n 1)"
pass "mcp initialize negotiated $PROTOCOL (session ${SESSION_ID:-none})"

SESSION_HEADERS=()
if [ -n "$SESSION_ID" ]; then SESSION_HEADERS+=(-H "Mcp-Session-Id: $SESSION_ID"); fi
SESSION_HEADERS+=(-H "Mcp-Protocol-Version: $PROTOCOL")

request mcp-initialized POST "$MCP_URL" '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
    "${AUTH[@]}" "${MCP_HEADERS[@]}" "${SESSION_HEADERS[@]}"
case "$LAST_STATUS" in
    200|202) pass "mcp notifications/initialized accepted (HTTP $LAST_STATUS)" ;;
    *) fail "mcp notifications/initialized was refused: HTTP $LAST_STATUS" ;;
esac

request mcp-tools-list POST "$MCP_URL" '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}' \
    "${AUTH[@]}" "${MCP_HEADERS[@]}" "${SESSION_HEADERS[@]}"
assert_status 200 "mcp tools/list through the console"
if ! jsonrpc_message "$HTTP_DIR/mcp-tools-list.body" >"$HTTP_DIR/mcp-tools-list.json"; then
    fail "mcp tools/list did not answer a JSON-RPC message"
fi
if ! TOOLS_INFO="$(python3 - "$HTTP_DIR/mcp-tools-list.json" <<'PY'
import json
import sys

message = json.load(open(sys.argv[1], encoding="utf-8"))
if "error" in message:
    raise SystemExit(f"tools/list returned a JSON-RPC error: {message['error']}")
result = message.get("result")
if not isinstance(result, dict):
    raise SystemExit(f"tools/list has no result object: {message}")
tools = result.get("tools")
if not isinstance(tools, list):
    raise SystemExit(f"tools/list has no tools array: {message}")
names = [tool.get("name") for tool in tools if isinstance(tool, dict)]
if "host.facts" not in names:
    raise SystemExit(f"host.facts is not advertised; tools: {names}")
print(f"{len(tools)} tool(s), host.facts advertised")
PY
)"; then
    fail "mcp tools/list did not advertise host.facts"
fi
pass "mcp tools/list advertised host.facts ($TOOLS_INFO)"

CALL_REQUEST='{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"host.facts","arguments":{}}}'
request mcp-tools-call POST "$MCP_URL" "$CALL_REQUEST" \
    "${AUTH[@]}" "${MCP_HEADERS[@]}" "${SESSION_HEADERS[@]}"
assert_status 200 "mcp tools/call host.facts through the console"
if ! jsonrpc_message "$HTTP_DIR/mcp-tools-call.body" >"$HTTP_DIR/mcp-tools-call.json"; then
    fail "mcp tools/call host.facts did not answer a JSON-RPC message"
fi
if ! CALL_SUMMARY="$(python3 - "$HTTP_DIR/mcp-tools-call.json" <<'PY'
import json
import sys

message = json.load(open(sys.argv[1], encoding="utf-8"))
if "error" in message:
    raise SystemExit(f"host.facts returned a JSON-RPC error: {message['error']}")
result = message.get("result")
if not isinstance(result, dict):
    raise SystemExit(f"host.facts has no result object: {message}")
if result.get("isError") is True:
    raise SystemExit(f"host.facts returned isError: {json.dumps(result)[:2000]}")
content = result.get("content")
if not isinstance(content, list) or not content:
    raise SystemExit(f"host.facts returned no content: {message}")
text = content[0].get("text") if isinstance(content[0], dict) else None
if not isinstance(text, str):
    raise SystemExit(f"host.facts returned no text content: {message}")
try:
    parsed = json.loads(text)
except Exception as error:
    raise SystemExit(f"host.facts text content is not JSON: {error}: {text[:500]}")
if not isinstance(parsed, dict):
    raise SystemExit(f"host.facts text content is not a JSON object: {text[:500]}")
print(f"{len(content)} content item(s), keys {', '.join(sorted(parsed)[:8])}")
PY
)"; then
    fail "mcp tools/call host.facts did not return a successful JSON-RPC result"
fi
pass "the call through the console returned host.facts"

# --- the record on both sides ------------------------------------------------------------------
DEVICE_AUDIT="$DEVICE_DATA/events.jsonl"
CONSOLE_AUDIT="$CONSOLE_DATA/audit.jsonl"

CONSOLE_AUDIT_INFO=""
AUDIT_DEADLINE=$((SECONDS + 10))
while [ "$SECONDS" -lt "$AUDIT_DEADLINE" ]; do
    if [ -f "$CONSOLE_AUDIT" ]; then
        if CONSOLE_AUDIT_INFO="$(python3 - "$CONSOLE_AUDIT" "$DEVICE_ID" "$TOKEN_ID" 2>/dev/null <<'PY'
import json
import sys

path, device_id, token_id = sys.argv[1], sys.argv[2], sys.argv[3]
for number, line in enumerate(open(path, encoding="utf-8"), start=1):
    line = line.strip()
    if not line:
        continue
    try:
        row = json.loads(line)
    except Exception:
        continue
    if (
        row.get("kind") == "mcp"
        and row.get("device_id") == device_id
        and row.get("tool") == "host.facts"
        and row.get("status") == 200
        and row.get("caller") == "client"
        and row.get("token_id") == token_id
    ):
        print(f"line {number}, status 200, caller client, token {token_id[:8]}…")
        raise SystemExit(0)
raise SystemExit(1)
PY
        )"; then
            [ -n "$CONSOLE_AUDIT_INFO" ] && break
        fi
    fi
    CONSOLE_AUDIT_INFO=""
    sleep 0.1
done
[ -n "$CONSOLE_AUDIT_INFO" ] || fail "console audit $CONSOLE_AUDIT has no finished mcp row for host.facts (device $DEVICE_ID, token $TOKEN_ID)"
pass "the console audit records the call ($CONSOLE_AUDIT_INFO)"

DEVICE_AUDIT_INFO=""
AUDIT_DEADLINE=$((SECONDS + 10))
while [ "$SECONDS" -lt "$AUDIT_DEADLINE" ]; do
    if [ -f "$DEVICE_AUDIT" ]; then
        if DEVICE_AUDIT_INFO="$(python3 - "$DEVICE_AUDIT" 2>/dev/null <<'PY'
import json
import sys

path = sys.argv[1]
begins = {}
ends = {}
for number, line in enumerate(open(path, encoding="utf-8"), start=1):
    line = line.strip()
    if not line:
        continue
    try:
        row = json.loads(line)
    except Exception:
        continue
    if row.get("event") == "begin" and row.get("tool") == "host.facts":
        begins[row.get("id")] = (number, row)
    elif row.get("event") == "end":
        ends[row.get("id")] = (number, row)
if not begins:
    raise SystemExit(1)
matched = []
for call_id, (begin_line, begin) in begins.items():
    if begin.get("decision") != "allow":
        continue
    end = ends.get(call_id)
    if end and end[1].get("ok") is True:
        matched.append((call_id, begin_line, end[0], begin.get("transport"), begin.get("token_id")))
if not matched:
    raise SystemExit(1)
call_id, begin_line, end_line, transport, token_id = matched[-1]
extra = f", transport {transport}" if transport else ""
extra += f", token {token_id[:8]}…" if token_id else ""
print(f"id {call_id}, begin line {begin_line}, end line {end_line}, ok{extra}")
PY
        )"; then
            [ -n "$DEVICE_AUDIT_INFO" ] && break
        fi
    fi
    DEVICE_AUDIT_INFO=""
    sleep 0.1
done
[ -n "$DEVICE_AUDIT_INFO" ] || fail "device audit $DEVICE_AUDIT has no allowed begin/end pair for host.facts"
pass "the device audit records the call ($DEVICE_AUDIT_INFO)"

# §4.2's join, proven on two real processes: the console's audit line and the device journal's
# call.started/call.finished pair name one `trace.call`, so the two records pair without clocks.
JOIN_INFO=""
JOIN_DEADLINE=$((SECONDS + 10))
while [ "$SECONDS" -lt "$JOIN_DEADLINE" ]; do
    if JOIN_INFO="$(python3 - "$CONSOLE_AUDIT" "$DEVICE_DATA/journal" "$DEVICE_ID" 2>/dev/null <<'PY'
import glob
import json
import os
import sys

audit_path, journal_dir, device_id = sys.argv[1], sys.argv[2], sys.argv[3]
traces = set()
for line in open(audit_path, encoding="utf-8"):
    line = line.strip()
    if not line:
        continue
    try:
        row = json.loads(line)
    except Exception:
        continue
    if (
        row.get("kind") == "mcp"
        and row.get("device_id") == device_id
        and row.get("tool") == "host.facts"
        and row.get("status") == 200
    ):
        trace = row.get("trace")
        if trace:
            traces.add(trace)
if not traces:
    raise SystemExit("no audit row for host.facts carries a trace")
started, finished = {}, {}
for path in sorted(glob.glob(os.path.join(journal_dir, "*.seg"))):
    for line in open(path, encoding="utf-8", errors="replace"):
        line = line.strip()
        if not line:
            continue
        try:
            row = json.loads(line)
        except Exception:
            continue
        call = (row.get("trace") or {}).get("call")
        if not call:
            continue
        if row.get("type") == "call.started":
            started[call] = row
        elif row.get("type") in ("call.finished", "call.denied"):
            finished[call] = row
joined = sorted(traces & set(started) & set(finished))
if not joined:
    raise SystemExit(
        f"audit traces {sorted(traces)} do not pair with journal calls "
        f"(started {len(started)}, finished {len(finished)})"
    )
trace = joined[-1]
print(
    f"trace {trace}: audit line joins journal seq {started[trace]['seq']} -> {finished[trace]['seq']}"
)
PY
)"; then
        [ -n "$JOIN_INFO" ] && break
    fi
    JOIN_INFO=""
    sleep 0.1
done
[ -n "$JOIN_INFO" ] || fail "the console audit and the device journal do not share a trace.call for host.facts"
pass "the two records join on trace.call ($JOIN_INFO)"

# The credential must never be in either record: both logs name the token record, never the secret.
if grep -qF "$SRM_TOKEN" "$CONSOLE_AUDIT" "$DEVICE_AUDIT" 2>/dev/null; then
    fail "a permanent token secret was written to an audit file"
fi
pass "neither audit contains the permanent token secret"

printf 'ENROLL SMOKE OK\n'
