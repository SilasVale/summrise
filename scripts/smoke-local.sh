#!/usr/bin/env bash
# scripts/smoke-local.sh — a real end-to-end smoke of the two binaries that ship, on this machine.
#
# It starts `summrise-device` and `summrise-console` on loopback ports with throwaway directories,
# exercises the surfaces a person and an MCP client actually use, proves the device recorded the
# call (audit and journal), then removes everything. It touches no product code.
#
# Usage:
#   bash scripts/smoke-local.sh
#
# Environment:
#   CARGO       the cargo binary (default /home/zss/.cargo/bin/cargo, then PATH)
#   SMOKE_KEEP  1 keeps the temp directories even on success
#
# Exits 0 and prints `SMOKE OK` only when every assertion passed. On the first failure it prints the
# failing request/response (or path), the reason, and keeps the two temp directories as evidence.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEVICE_BIN="$ROOT/device/target/debug/summrise-device"
CONSOLE_BIN="$ROOT/device/target/debug/summrise-console"
CARGO_BIN="${CARGO:-/home/zss/.cargo/bin/cargo}"

DEVICE_DIR=""
CONSOLE_DIR=""
DEVICE_PID=""
CONSOLE_PID=""
HTTP_DIR=""
LAST_URL=""
LAST_STATUS=""
LAST_BODY=""
LAST_HDR=""
DEVICE_LOG=""
CONSOLE_LOG=""

pass() {
    printf 'PASS %s\n' "$*"
}

# A failure body must not become a credential leak: the panel page carries this boot's token, and a
# malformed token file is still a secret-shaped value. Redact both before any body or log is printed.
redact() {
    local text
    text="$(cat)"
    # The panel token is read from the page, so a failure before that read still has to hide it.
    text="$(printf '%s' "$text" | LC_ALL=C sed -E 's/[0-9a-f]{64}/***REDACTED***/g')"
    if [ -n "${TOKEN:-}" ]; then
        text="${text//$TOKEN/***REDACTED***}"
    fi
    if [ -n "${PANEL_TOKEN:-}" ]; then
        text="${text//$PANEL_TOKEN/***REDACTED***}"
    fi
    printf '%s' "$text"
}

fail() {
    printf 'FAIL %s\n' "$*" >&2
    if [ -n "$LAST_URL" ]; then
        printf '  request: %s\n' "$LAST_URL" >&2
        printf '  status:  %s\n' "${LAST_STATUS:-<no response>}" >&2
        if [ -n "$LAST_HDR" ] && [ -f "$LAST_HDR" ]; then
            printf '  headers: %s\n' "$LAST_HDR" >&2
            sed 's/^/    /' "$LAST_HDR" >&2 || true
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
    exit 1
}

cleanup() {
    local code=$?
    trap - EXIT
    set +e

    if [ -n "$CONSOLE_PID" ]; then
        kill "$CONSOLE_PID" 2>/dev/null
    fi
    if [ -n "$DEVICE_PID" ]; then
        kill "$DEVICE_PID" 2>/dev/null
    fi
    local deadline=$((SECONDS + 5))
    while [ "$SECONDS" -lt "$deadline" ]; do
        local alive=0
        if [ -n "$DEVICE_PID" ] && kill -0 "$DEVICE_PID" 2>/dev/null; then
            alive=1
        fi
        if [ -n "$CONSOLE_PID" ] && kill -0 "$CONSOLE_PID" 2>/dev/null; then
            alive=1
        fi
        [ "$alive" -eq 0 ] && break
        sleep 0.1
    done
    if [ -n "$CONSOLE_PID" ]; then
        kill -9 "$CONSOLE_PID" 2>/dev/null
    fi
    if [ -n "$DEVICE_PID" ]; then
        kill -9 "$DEVICE_PID" 2>/dev/null
    fi
    wait 2>/dev/null

    if [ "$code" -ne 0 ] || [ "${SMOKE_KEEP:-0}" = "1" ]; then
        printf 'smoke: kept evidence:\n' >&2
        [ -n "$DEVICE_DIR" ] && printf '  device dir:  %s\n' "$DEVICE_DIR" >&2
        [ -n "$CONSOLE_DIR" ] && printf '  console dir: %s\n' "$CONSOLE_DIR" >&2
    else
        [ -n "$DEVICE_DIR" ] && rm -rf "$DEVICE_DIR"
        [ -n "$CONSOLE_DIR" ] && rm -rf "$CONSOLE_DIR"
    fi
    exit "$code"
}
trap cleanup EXIT

process_alive() {
    local pid="$1"
    kill -0 "$pid" 2>/dev/null || return 1
    local state
    state="$(ps -o stat= -p "$pid" 2>/dev/null)" || return 1
    case "$state" in
        Z*) return 1 ;;
    esac
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
    local args=(-sS --max-time 15 -X "$method" -o "$LAST_BODY" -D "$LAST_HDR" -w '%{http_code}')
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
    values = [
        line[5:].strip()
        for line in raw.splitlines()
        if line.startswith("data:")
    ]
    values = [value for value in values if value]
    if not values:
        print("no JSON-RPC data line in the response", file=sys.stderr)
        raise SystemExit(2)
    data = values[-1]
try:
    message = json.loads(data)
except Exception as error:
    print(f"the JSON-RPC message is not JSON: {error}", file=sys.stderr)
    raise SystemExit(2)
print(json.dumps(message, separators=(",", ":")))
PY
}

for tool in curl python3; do
    command -v "$tool" >/dev/null 2>&1 || fail "missing required tool: $tool"
done

# ALWAYS ASK CARGO TO BRING THE BINARIES TO HEAD. An incremental build costs a fraction of a second
# when they are fresh, and a stale binary would smoke the wrong code — measured once: a build from
# before the last four commits passed every assertion because the process it tested was not HEAD.
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

DEVICE_DIR="$(mktemp -d "${TMPDIR:-/tmp}/summrise-smoke-device.XXXXXX")" || fail "mktemp -d failed"
CONSOLE_DIR="$(mktemp -d "${TMPDIR:-/tmp}/summrise-smoke-console.XXXXXX")" || fail "mktemp -d failed"
DEVICE_DATA="$DEVICE_DIR/data"
DEVICE_INSTALL="$DEVICE_DIR/install"
HTTP_DIR="$DEVICE_DIR/http"
CONSOLE_DATA="$CONSOLE_DIR/data"
DEVICE_LOG="$DEVICE_DIR/device.log"
CONSOLE_LOG="$CONSOLE_DIR/console.log"
mkdir -p "$DEVICE_DATA" "$DEVICE_INSTALL" "$HTTP_DIR" "$CONSOLE_DATA" || fail "cannot create temp directories"

echo "smoke: device dir  $DEVICE_DIR"
echo "smoke: console dir $CONSOLE_DIR"
echo "smoke: device binary  $DEVICE_BIN"
echo "smoke: console binary $CONSOLE_BIN"

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
case "$DEVICE_PORT" in
    ''|*[!0-9]*) fail "bad device port: '$DEVICE_PORT'" ;;
esac
case "$CONSOLE_PORT" in
    ''|*[!0-9]*) fail "bad console port: '$CONSOLE_PORT'" ;;
esac
[ "$DEVICE_PORT" != "$CONSOLE_PORT" ] || fail "the two ports collided: $DEVICE_PORT"
echo "smoke: device port $DEVICE_PORT, console port $CONSOLE_PORT"

# The device's install token: 64 hex characters, which its `parse_token_file` decodes to 32 bytes.
# The default path is <data>/token; the file is written before the process starts, because
# `load_token` reads it once at startup.
TOKEN="$(python3 -c 'import secrets; print(secrets.token_hex(32))')" || fail "could not mint a token"
printf '%s\n' "$TOKEN" >"$DEVICE_DATA/token" || fail "cannot write $DEVICE_DATA/token"
chmod 600 "$DEVICE_DATA/token" || fail "cannot chmod $DEVICE_DATA/token"
TOKEN_READ="$(tr -d '\r\n' <"$DEVICE_DATA/token")"
case "$TOKEN_READ" in
    *[!0-9a-f]*) fail "the token file is not lowercase hex" ;;
esac
[ "${#TOKEN_READ}" -eq 64 ] || fail "the token file is ${#TOKEN_READ} characters, not 64"
[ "$TOKEN_READ" = "$TOKEN" ] || fail "the token file did not round-trip"
pass "device install token file is 64 lowercase hex characters (32 bytes)"

# --- the device -------------------------------------------------------------------------------
"$DEVICE_BIN" --install "$DEVICE_INSTALL" --data "$DEVICE_DATA" --port "$DEVICE_PORT" \
    >"$DEVICE_LOG" 2>&1 &
DEVICE_PID=$!
DEVICE_BASE="http://127.0.0.1:$DEVICE_PORT"

if ! wait_for_200 "$DEVICE_BASE/api/health" device-health "$DEVICE_PID" 15; then
    fail "device GET /api/health did not answer 200 within 15 s"
fi
HEALTH_BODY="$LAST_BODY"
pass "device GET /api/health -> HTTP 200"

if ! DEVICE_HEALTH="$(python3 - "$HEALTH_BODY" <<'PY'
import json
import sys

document = json.load(open(sys.argv[1], encoding="utf-8"))
if document.get("status") != "ok":
    raise SystemExit(f"health status is {document.get('status')!r}: {document}")
if not isinstance(document.get("pid"), int):
    raise SystemExit(f"health has no integer pid: {document}")
print(f"version {document.get('version')}, pid {document['pid']}")
PY
)"; then
    fail "device /api/health did not report status ok"
fi
pass "device /api/health reports status ok ($DEVICE_HEALTH)"

# --- the device panel -------------------------------------------------------------------------
request device-panel GET "$DEVICE_BASE/panel" ""
assert_status 200 "device GET /panel"
PANEL_HTML="$LAST_BODY"
if ! grep -q 'name="summrise-panel"' "$PANEL_HTML"; then
    fail "device /panel does not carry the stable <meta name=\"summrise-panel\"> marker"
fi
pass "device GET /panel carries the summrise-panel meta marker"

if ! PANEL_TOKEN="$(python3 - "$PANEL_HTML" <<'PY'
import re
import sys

html = open(sys.argv[1], encoding="utf-8").read()
for tag in re.findall(r"<meta\b[^>]*>", html, re.IGNORECASE):
    if re.search(r'name="summrise-panel"', tag, re.IGNORECASE):
        match = re.search(r'content="([^"]+)"', tag)
        if match:
            print(match.group(1))
            raise SystemExit(0)
print("no summrise-panel meta tag with a content attribute", file=sys.stderr)
raise SystemExit(2)
PY
)"; then
    fail "device /panel did not serve a readable panel token"
fi
case "$PANEL_TOKEN" in
    *[!0-9a-f]*) fail "the panel token is not lowercase hex" ;;
esac
[ "${#PANEL_TOKEN}" -eq 64 ] || fail "the panel token is ${#PANEL_TOKEN} characters, not 64"
pass "device /panel meta carries a 64-hex panel token"

# `/api/panel` is a loopback-only read in this build; the panel token gates the mutating routes
# (`/api/panel/update`, `/api/enroll`, `/api/terminal/*`). The header is sent here anyway: it is the
# credential the page carries, and a future gate would then be exercised rather than skipped.
request device-api-panel GET "$DEVICE_BASE/api/panel" "" -H "x-summrise-panel: $PANEL_TOKEN"
assert_status 200 "device GET /api/panel with the panel token"
if ! PANEL_SUMMARY="$(python3 - "$LAST_BODY" <<'PY'
import json
import sys

document = json.load(open(sys.argv[1], encoding="utf-8"))
if not isinstance(document, dict):
    raise SystemExit("the snapshot is not a JSON object")
for key in ("version", "pid", "state", "data_dir"):
    if key not in document:
        raise SystemExit(f"the snapshot has no {key!r}: {sorted(document)}")
if not isinstance(document["pid"], int):
    raise SystemExit("the snapshot's pid is not an integer")
print(f"version {document['version']}, pid {document['pid']}, data_dir {document['data_dir']}")
PY
)"; then
    fail "device /api/panel did not answer the snapshot JSON"
fi
pass "device /api/panel is JSON ($PANEL_SUMMARY)"

# --- MCP: streamable HTTP, initialize -> notifications/initialized -> tools/list -> tools/call --
MCP_URL="$DEVICE_BASE/mcp"
AUTH=(-H "Authorization: Bearer $TOKEN")
MCP_HEADERS=(-H 'Content-Type: application/json' -H 'Accept: application/json, text/event-stream')
INIT_REQUEST='{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"smoke-local","version":"0.1.0"}}}'

request mcp-initialize POST "$MCP_URL" "$INIT_REQUEST" "${AUTH[@]}" "${MCP_HEADERS[@]}"
assert_status 200 "mcp initialize"
SESSION="$(sed -n 's/^[Mm][Cc][Pp]-[Ss]ession-[Ii][Dd]:[[:space:]]*//p' "$HTTP_DIR/mcp-initialize.hdr" | tr -d '\r' | tail -n 1)"
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
if [ -n "$SESSION" ]; then
    pass "mcp initialize negotiated $PROTOCOL with session id $SESSION"
else
    pass "mcp initialize negotiated $PROTOCOL (no session id issued)"
fi

SESSION_HEADERS=()
if [ -n "$SESSION" ]; then
    SESSION_HEADERS+=(-H "Mcp-Session-Id: $SESSION")
fi
SESSION_HEADERS+=(-H "Mcp-Protocol-Version: $PROTOCOL")

request mcp-initialized POST "$MCP_URL" '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
    "${AUTH[@]}" "${MCP_HEADERS[@]}" "${SESSION_HEADERS[@]}"
case "$LAST_STATUS" in
    200|202) pass "mcp notifications/initialized accepted (HTTP $LAST_STATUS)" ;;
    *) fail "mcp notifications/initialized was refused: HTTP $LAST_STATUS" ;;
esac

request mcp-tools-list POST "$MCP_URL" '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}' \
    "${AUTH[@]}" "${MCP_HEADERS[@]}" "${SESSION_HEADERS[@]}"
assert_status 200 "mcp tools/list"
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
if not isinstance(tools, list) or not tools:
    raise SystemExit(f"tools/list returned no tools: {message}")
names = [tool.get("name") for tool in tools if isinstance(tool, dict)]
if "host.facts" in names:
    chosen = "host.facts"
elif "state.get" in names:
    chosen = "state.get"
else:
    raise SystemExit(f"neither host.facts nor state.get is advertised: {names}")
print(f"{len(tools)} {chosen}")
PY
)"; then
    fail "mcp tools/list did not advertise a usable tool"
fi
read -r TOOL_COUNT MCP_TOOL <<<"$TOOLS_INFO"
pass "mcp tools/list advertised $TOOL_COUNT tools; using $MCP_TOOL"

CALL_REQUEST="$(printf '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"%s","arguments":{}}}' "$MCP_TOOL")"
request mcp-tools-call POST "$MCP_URL" "$CALL_REQUEST" \
    "${AUTH[@]}" "${MCP_HEADERS[@]}" "${SESSION_HEADERS[@]}"
assert_status 200 "mcp tools/call $MCP_TOOL"
if ! jsonrpc_message "$HTTP_DIR/mcp-tools-call.body" >"$HTTP_DIR/mcp-tools-call.json"; then
    fail "mcp tools/call $MCP_TOOL did not answer a JSON-RPC message"
fi
if ! CALL_SUMMARY="$(python3 - "$HTTP_DIR/mcp-tools-call.json" "$MCP_TOOL" <<'PY'
import json
import sys

message = json.load(open(sys.argv[1], encoding="utf-8"))
tool = sys.argv[2]
if "error" in message:
    raise SystemExit(f"{tool} returned a JSON-RPC error: {message['error']}")
result = message.get("result")
if not isinstance(result, dict):
    raise SystemExit(f"{tool} has no result object: {message}")
if result.get("isError") is True:
    raise SystemExit(f"{tool} returned isError: {json.dumps(result)[:2000]}")
content = result.get("content")
if not isinstance(content, list) or not content:
    raise SystemExit(f"{tool} returned no content: {message}")
text = content[0].get("text") if isinstance(content[0], dict) else None
if not isinstance(text, str):
    raise SystemExit(f"{tool} returned no text content: {message}")
try:
    parsed = json.loads(text)
except Exception as error:
    raise SystemExit(f"{tool} text content is not JSON: {error}: {text[:500]}")
if not isinstance(parsed, dict):
    raise SystemExit(f"{tool} text content is not a JSON object: {text[:500]}")
print(f"{len(content)} content item(s), keys {', '.join(sorted(parsed)[:8])}")
PY
)"; then
    fail "mcp tools/call $MCP_TOOL did not return a successful JSON-RPC result"
fi
pass "mcp tools/call $MCP_TOOL returned a JSON-RPC result with no error ($CALL_SUMMARY)"

# --- the record: audit and journal -------------------------------------------------------------
AUDIT_FILE="$DEVICE_DATA/events.jsonl"
deadline=$((SECONDS + 5))
while [ "$SECONDS" -lt "$deadline" ]; do
    if [ -f "$AUDIT_FILE" ] && grep -qF "\"tool\":\"$MCP_TOOL\"" "$AUDIT_FILE"; then
        break
    fi
    sleep 0.1
done
if [ ! -f "$AUDIT_FILE" ] || ! grep -qF "\"tool\":\"$MCP_TOOL\"" "$AUDIT_FILE"; then
    fail "device audit $AUDIT_FILE does not contain a row for $MCP_TOOL"
fi
if ! AUDIT_SUMMARY="$(python3 - "$AUDIT_FILE" "$MCP_TOOL" <<'PY'
import json
import sys

path, tool = sys.argv[1], sys.argv[2]
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
    if row.get("event") == "begin" and row.get("tool") == tool:
        begins[row.get("id")] = (number, row)
    elif row.get("event") == "end":
        ends[row.get("id")] = (number, row)
if not begins:
    raise SystemExit(f"no begin row for {tool}")
matched = []
for call_id, (begin_line, begin) in begins.items():
    if begin.get("decision") != "allow":
        continue
    end = ends.get(call_id)
    if end and end[1].get("ok") is True:
        matched.append((call_id, begin_line, end[0]))
if not matched:
    raise SystemExit(f"no allowed begin/end pair for {tool}: begins={begins}, ends={ends}")
call_id, begin_line, end_line = matched[-1]
print(f"id {call_id}, begin line {begin_line}, end line {end_line}, ok")
PY
)"; then
    fail "device audit $AUDIT_FILE has no allowed begin/end pair for $MCP_TOOL"
fi
pass "device audit events.jsonl records $MCP_TOOL ($AUDIT_SUMMARY)"

JOURNAL_DIR="$DEVICE_DATA/journal"
shopt -s nullglob
SEGMENTS=("$JOURNAL_DIR"/*.seg)
shopt -u nullglob
if [ "${#SEGMENTS[@]}" -eq 0 ]; then
    fail "device journal directory $JOURNAL_DIR has no *.seg segment"
fi
JOURNAL_MATCH=""
for segment in "${SEGMENTS[@]}"; do
    if grep -qF '"type":"device.started"' "$segment"; then
        JOURNAL_MATCH="$segment"
        break
    fi
done
if [ -z "$JOURNAL_MATCH" ]; then
    fail "no journal segment under $JOURNAL_DIR contains device.started (segments: ${SEGMENTS[*]})"
fi
pass "device journal $(basename "$JOURNAL_MATCH") contains device.started"

# --- the console ------------------------------------------------------------------------------
# The console's required flags: --operator-token-file (64 hex), --grant-key-file (the 32-byte
# Ed25519 seed in hex) and --data. `--setup` writes both secrets at mode 0600, which `load` checks.
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

"$CONSOLE_BIN" \
    --operator-token-file "$CONSOLE_DATA/operator.token" \
    --grant-key-file "$CONSOLE_DATA/grant.seed" \
    --data "$CONSOLE_DATA" \
    --port "$CONSOLE_PORT" \
    --bind 127.0.0.1 \
    >"$CONSOLE_LOG" 2>&1 &
CONSOLE_PID=$!
CONSOLE_BASE="http://127.0.0.1:$CONSOLE_PORT"

if ! wait_for_200 "$CONSOLE_BASE/health" console-health "$CONSOLE_PID" 15; then
    fail "console GET /health did not answer 200 within 15 s"
fi
CONSOLE_HEALTH_BODY="$LAST_BODY"
pass "console GET /health -> HTTP 200"

if ! CONSOLE_HEALTH="$(python3 - "$CONSOLE_HEALTH_BODY" <<'PY'
import json
import sys

document = json.load(open(sys.argv[1], encoding="utf-8"))
if document.get("status") != "ok":
    raise SystemExit(f"health status is {document.get('status')!r}: {document}")
print(f"version {document.get('version')}")
PY
)"; then
    fail "console /health did not report status ok"
fi
pass "console /health reports status ok ($CONSOLE_HEALTH)"

request console-root GET "$CONSOLE_BASE/" ""
assert_status 200 "console GET /"
if ! grep -qF '<div id="root"></div>' "$LAST_BODY"; then
    fail "console GET / is not the embedded React page (no <div id=\"root\"></div>)"
fi
pass 'console GET / serves the embedded React page (<div id="root"></div>)'

printf 'SMOKE OK\n'
