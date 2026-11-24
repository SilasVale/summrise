#!/usr/bin/env bash
# The console deploy goes through the host device's shell: the fetch and the swap are `runs.exec`
# calls on DESKTOP-9VH92BJ. With that device offline the console still serves its page, and the only
# symptom used to be a bare `the console did not answer: HTTP Error 503` after a full build. This
# checks the door BEFORE the build, and in the three states that matter:
#
#   reconnecting     /mcp answers 503 device reconnecting -> refuse by name, before building
#   open             the door answers -> preflight succeeds, nothing is built
#   console-down     /health does not answer -> say the console is down, do not ask the door
#
# The console is a loopback stub; no real console, host device or Rust build is touched.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORK="$(mktemp -d)"
SERVER=""
PORT=""

cleanup() {
    [[ -n "$SERVER" ]] && kill "$SERVER" 2>/dev/null || true
    rm -rf "$WORK"
}
trap cleanup EXIT

fail() {
    echo "console-deploy-preflight: $*" >&2
    exit 1
}

bash -n "$ROOT/scripts/console-deploy.sh" || fail "bash -n scripts/console-deploy.sh"

cat >"$WORK/fake-console.py" <<'PY'
import http.server
import json
import os
import sys

MODE = os.environ["FAKE_MODE"]
LOG = os.environ["FAKE_LOG"]


class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *args):
        pass

    def record(self, line):
        with open(LOG, "a") as handle:
            handle.write(line + "\n")

    def send_json(self, status, payload):
        body = json.dumps(payload).encode()
        self.send_response(status)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        self.record("GET " + self.path)
        if self.path != "/health":
            self.send_json(404, {"error": "not found"})
        elif MODE == "console-down":
            self.send_json(503, {"error": "down"})
        else:
            self.send_json(
                200,
                {
                    "status": "ok",
                    "devices": 4,
                    "online": 1 if MODE == "open" else 0,
                    "uptime_s": 1,
                    "access_configured": True,
                    "max_queued_frames": 1,
                    "version": "9.9.9-fake",
                },
            )

    def do_POST(self):
        length = int(self.headers.get("content-length") or 0)
        try:
            tool = json.loads(self.rfile.read(length))["request"]["params"]["name"]
        except Exception:
            tool = "?"
        self.record("POST %s %s" % (self.path, tool))
        if self.path != "/mcp":
            self.send_json(404, {"error": "not found"})
        elif MODE == "reconnecting":
            self.send_json(503, {"error": "device reconnecting", "retry_after_ms": 1000})
        else:
            self.send_json(
                200,
                {
                    "jsonrpc": "2.0",
                    "id": 1,
                    "result": {"content": [{"type": "text", "text": "{\"status\":\"ok\"}"}]},
                },
            )


class Server(http.server.HTTPServer):
    def handle_error(self, request, client_address):
        # The probe connect and the teardown kill can close a socket mid-handler. That is the rig
        # hanging up, not a wrong answer; anything else is still printed.
        import sys

        if isinstance(sys.exc_info()[1], (BrokenPipeError, ConnectionResetError)):
            return
        super().handle_error(request, client_address)


Server(("127.0.0.1", int(sys.argv[1])), Handler).serve_forever()
PY

free_port() {
    python3 - <<'PY'
import socket

sock = socket.socket()
sock.bind(("127.0.0.1", 0))
print(sock.getsockname()[1])
sock.close()
PY
}

start_console() { # start_console <mode>
    PORT="$(free_port)"
    : >"$WORK/requests"
    FAKE_MODE="$1" FAKE_LOG="$WORK/requests" python3 "$WORK/fake-console.py" "$PORT" &
    SERVER=$!
    for _ in $(seq 1 100); do
        if (exec 3<>"/dev/tcp/127.0.0.1/$PORT") 2>/dev/null; then
            return 0
        fi
        sleep 0.05
    done
    fail "the fake console did not start on :$PORT"
}

stop_console() {
    [[ -n "$SERVER" ]] && kill "$SERVER" 2>/dev/null || true
    [[ -n "$SERVER" ]] && wait "$SERVER" 2>/dev/null || true
    SERVER=""
}

OUTPUT=""
STATUS=0

run_preflight() { # run_preflight <fake console root>
    set +e
    # `--preflight-only` is the mode under test: it must stop before cargo, so a slow run is a bug.
    OUTPUT="$(SUMMRISE_CONSOLE_URL="$1" timeout 20 bash "$ROOT/scripts/console-deploy.sh" \
        0.0.0-preflight --preflight-only 2>&1)"
    STATUS=$?
    set -e
}

expect() { # expect <description> <text>
    case "$OUTPUT" in
        *"$2"*) ;;
        *) fail "$1: the output lacks '$2': $OUTPUT" ;;
    esac
}

refuse() { # refuse <description> <text>
    case "$OUTPUT" in
        *"$2"*) fail "$1: the output should not contain '$2': $OUTPUT" ;;
    esac
}

# ── 1. THE MEASURED FAILURE: the host device's channel is down ──────────────────────────────────
start_console reconnecting
run_preflight "http://127.0.0.1:$PORT"
[[ "$STATUS" -eq 1 ]] || fail "the reconnecting preflight exited $STATUS, not 1: $OUTPUT"
expect "the closed door is named" "door is closed"
expect "the host device is named" "DESKTOP-9VH92BJ"
expect "the console's own sentence is kept" "device reconnecting"
expect "the dependency is named" "runs.exec"
expect "the console's facts are kept" "9.9.9-fake"
expect "the fleet state is kept" "0 of 4 devices online"
refuse "the build did not run" "Compiling"
refuse "the build did not run" "built 0.0.0-preflight"
grep -q '^GET /health' "$WORK/requests" || fail "the preflight did not read /health"
grep -q '^POST /mcp state.get' "$WORK/requests" || fail "the preflight did not ask the host's door"
stop_console

# ── 2. THE DOOR IS OPEN: the preflight passes and still does not build ──────────────────────────
start_console open
run_preflight "http://127.0.0.1:$PORT"
[[ "$STATUS" -eq 0 ]] || fail "the open preflight exited $STATUS: $OUTPUT"
expect "the open door is stated" "door is open"
expect "the host device is named" "DESKTOP-9VH92BJ"
refuse "the preflight did not build" "Compiling"
[[ "$(grep -c '^POST /mcp' "$WORK/requests")" -eq 1 ]] ||
    fail "the preflight asked the door more than once: $(cat "$WORK/requests")"
stop_console

# ── 3. THE CONSOLE ITSELF IS DOWN: name it, and do not ask the host's door ───────────────────────
start_console console-down
run_preflight "http://127.0.0.1:$PORT"
[[ "$STATUS" -eq 1 ]] || fail "the console-down preflight exited $STATUS, not 1: $OUTPUT"
expect "the console is named" "the console"
expect "the health path is named" "/health"
expect "the consequence is stated" "cannot begin"
[[ "$(grep -c '^POST ' "$WORK/requests")" -eq 0 ]] ||
    fail "the preflight asked the door while the console was down: $(cat "$WORK/requests")"
stop_console

echo "CONSOLE_DEPLOY_PREFLIGHT_TEST_OK"
