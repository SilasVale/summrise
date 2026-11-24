#!/usr/bin/env bash
# One console, one device, one browser — the Terminal page in the two states that matter.
#
#   bash scripts/console-live-check.sh [--keep]
#
# There is no fixture here. A real `summrise-console` and a real `summrise-device` are built with a
# grant key this run generates, enrolled over loopback, and opened in Chromium. The page is then
# asked to do what the operator does: sign in with the operator token, open the machine's Terminal,
# start a shell, type a command. The device is stopped and the page is asked again, this time for the
# sentence an offline machine deserves.
#
# The assertions live in scripts/console-live-check.mjs; this script owns only the rig. Shots land in
# $CONSOLE_LIVE_OUT (default: the throwaway work directory, printed on success). `--keep` keeps the
# work directory even on success, for reading the console and device logs.
#
# Requires: cargo, openssl (3.x, for Ed25519), node with playwright-core, a Chromium binary in
# $CONSOLE_BROWSER (default /usr/bin/google-chrome).
#
# It builds `summrise-device` with a throwaway grant key, so the next ordinary `cargo build` in
# device/ rebuilds it (the key is a compile-time input, by design). Nothing else in the tree changes.

set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
port="${CONSOLE_LIVE_PORT:-19099}"
device_port=$((port + 1))
keep=""
[[ "${1:-}" == "--keep" ]] && keep="1"

export PATH="$HOME/.cargo/bin:$PATH"
if ! command -v cargo >/dev/null; then
    echo "console-live-check: cargo is not on PATH" >&2
    exit 3
fi

# A console already on the port would answer the health probe and then refuse this run's token —
# the failure reads as an auth bug. Say it up front instead.
for candidate in "$port" "$device_port"; do
    if (exec 3<>"/dev/tcp/127.0.0.1/$candidate") 2>/dev/null; then
        echo "console-live-check: :$candidate is already serving; set CONSOLE_LIVE_PORT to a free pair" >&2
        exit 3
    fi
done

work="$(mktemp -d "${TMPDIR:-/tmp}/console-live.XXXXXX")"
out="${CONSOLE_LIVE_OUT:-$work/shots}"
mkdir -p "$out" "$work/console" "$work/device" "$work/install"

pids=()
cleanup() {
    for pid in "${pids[@]:-}"; do kill "$pid" 2>/dev/null || true; done
    if [[ "$keep" == "1" || "${CONSOLE_LIVE_KEEP:-}" == "1" ]]; then
        echo "console-live-check: kept $work" >&2
    else
        rm -rf "$work"
    fi
}
trap cleanup EXIT

# ── the rig's own secrets ────────────────────────────────────────────────────────────────────────
# The device trusts console grants by a PUBLIC key compiled in at build time; the console signs with
# the seed. Generating both here is what lets a throwaway rig pair without a release build.
openssl genpkey -algorithm ED25519 -out "$work/grant.pem" 2>/dev/null
openssl pkey -in "$work/grant.pem" -outform DER | tail -c 32 | xxd -p -c 64 | tr -d '\n' > "$work/grant.seed"
pub="$(openssl pkey -in "$work/grant.pem" -pubout -outform DER | tail -c 32 | xxd -p -c 64)"
openssl rand -hex 32 | tr -d '\n' > "$work/operator.token"
chmod 600 "$work/grant.seed" "$work/operator.token"

echo "console-live-check: building the console and the device (grant key ${pub:0:12}…)" >&2
(cd "$root/device" && SUMMRISE_CONSOLE_GRANT_KEY="$pub" cargo build -q -p summrise-console -p summrise-device)

console_bin="$root/device/target/debug/summrise-console"
device_bin="$root/device/target/debug/summrise-device"

wait_for() { # wait_for <seconds> <expression> — a shell predicate, polled
    local deadline=$((SECONDS + $1))
    shift
    while (( SECONDS < deadline )); do
        if eval "$@"; then return 0; fi
        sleep 0.2
    done
    return 1
}

online_now() {
    curl -fsS -m 1 "http://127.0.0.1:$port/health" 2>/dev/null |
        python3 -c 'import sys,json;print(json.load(sys.stdin)["online"])' 2>/dev/null || echo 0
}

# ── the console ─────────────────────────────────────────────────────────────────────────────────
"$console_bin" --operator-token-file "$work/operator.token" --grant-key-file "$work/grant.seed" \
    --data "$work/console" --port "$port" >"$work/console.log" 2>&1 &
pids+=($!)

wait_for 10 'curl -fsS -m 1 "http://127.0.0.1:'"$port"'/health" >/dev/null 2>&1' ||
    { echo "console-live-check: the console did not answer on :$port" >&2; cat "$work/console.log" >&2; exit 3; }

session="$(curl -fsS -m 5 -X POST "http://127.0.0.1:$port/auth" -H 'content-type: application/json' \
    -d "{\"token\":\"$(cat "$work/operator.token")\"}" |
    python3 -c 'import sys,json;print(json.load(sys.stdin)["token"])')"
code="$(curl -fsS -m 5 -X POST "http://127.0.0.1:$port/enroll" -H "Authorization: Bearer $session" |
    python3 -c 'import sys,json;print(json.load(sys.stdin)["code"])')"

# ── the device ──────────────────────────────────────────────────────────────────────────────────
"$device_bin" --enroll "http://127.0.0.1:$port" "$code" --data "$work/device" --install "$work/install" \
    --allow-plaintext >/dev/null
openssl rand -hex 32 | tr -d '\n' > "$work/device/token"
chmod 600 "$work/device/token"
"$device_bin" --install "$work/install" --data "$work/device" --port "$device_port" \
    --token-file "$work/device/token" >"$work/device.log" 2>&1 &
pids+=($!)

wait_for 15 '[[ "$(online_now)" == "1" ]]' ||
    { echo "console-live-check: the device never came online" >&2; cat "$work/device.log" >&2; exit 3; }

# ── the two photographs ─────────────────────────────────────────────────────────────────────────
node "$root/scripts/console-live-check.mjs" --phase live \
    --url "http://127.0.0.1:$port/app" --token-file "$work/operator.token" --out "$out"

# The device's other tabs, with the same real device: Files, Runs, Machine and Screen as an
# operator first meets them.
node "$root/scripts/console-live-check.mjs" --phase pages \
    --url "http://127.0.0.1:$port/app" --token-file "$work/operator.token" --out "$out"

kill "${pids[1]}" 2>/dev/null || true
wait_for 20 '[[ "$(online_now)" == "0" ]]' ||
    { echo "console-live-check: the console still reports the device online" >&2; exit 3; }

node "$root/scripts/console-live-check.mjs" --phase offline \
    --url "http://127.0.0.1:$port/app" --token-file "$work/operator.token" --out "$out"

echo "console-live-check: shots in $out" >&2
