#!/usr/bin/env bash
# Rehearse a canary publish entirely on loopback: build the signer, derive the public keys, publish
# into a throwaway directory, serve it, and make verify-release.sh prove the publish is consistent.
#
#   scripts/rehearse-release.sh [--version 0.3.999] [--keys DIR] [--out DIR] [--port 18099]
#
# Runs on the Linux build host: publish-canary.bash cross-builds the four device .exe files and signs
# the release document, so this needs cargo, cargo-xwin and the MSVC target. The NSIS installer appears
# only when makensis is found (this box: $HOME/tools/nsis/usr/bin/makensis). The console is not
# checked (--console needs a deployed console), and verify-release.sh checks the pointer, artifact
# digest, sidecar and installer presence. It also builds a Linux verifier, boots it once so it has a
# real state document, and has it run the same `--check-update` a device runs; that is the half
# signature acceptance needs and a mirror-only check cannot see.
set -euo pipefail

version="0.3.999"
keys="${SUMMRISE_KEYS_DIR:-$HOME/canary-kit-archive-2026-10-22/keys}"
out=""
device_port="18099"

usage() { sed -n '2,13p' "$0" | sed 's/^# \{0,1\}//'; exit "${1:-0}"; }
while [ $# -gt 0 ]; do
    case "$1" in
        --version) version="$2"; shift 2 ;;
        --keys) keys="$2"; shift 2 ;;
        --out) out="$2"; shift 2 ;;
        --port) device_port="$2"; shift 2 ;;
        --help|-h) usage 0 ;;
        *) echo "rehearse-release: unknown argument '$1'" >&2; usage 2 ;;
    esac
done

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
command -v cargo >/dev/null 2>&1 || export PATH="$HOME/.cargo/bin:$PATH"
command -v cargo >/dev/null 2>&1 || { echo "rehearse-release: cargo is not on PATH" >&2; exit 1; }
for seed in launcher release console; do
    [ -r "$keys/$seed.seed" ] || { echo "rehearse-release: missing seed $keys/$seed.seed" >&2; exit 2; }
done

start="$(date +%s)"
signer="$root/device/target/release/summrise-release"
echo "== building the signer"
cargo build --release --manifest-path device/Cargo.toml -p summrise-release

echo "== deriving the three public keys"
export SUMMRISE_LAUNCHER_PUBKEY="$("$signer" pubkey --key-file "$keys/launcher.seed")"
export SUMMRISE_RELEASE_PUBKEY="$("$signer" pubkey --key-file "$keys/release.seed")"
export SUMMRISE_CONSOLE_GRANT_KEY="$("$signer" pubkey --key-file "$keys/console.seed")"
printf '  launcher %s\n  release  %s\n  console  %s\n' \
    "$SUMMRISE_LAUNCHER_PUBKEY" "$SUMMRISE_RELEASE_PUBKEY" "$SUMMRISE_CONSOLE_GRANT_KEY"

# The verifier is a REAL device binary: the same crate, the same embedded public keys, the same
# `--check-update` mode a deployed device runs. A mirror-only check cannot tell a document it serves
# from one a device refuses, which is why this build is part of the rehearsal.
echo "== building the Linux verifier"
cargo build --release --manifest-path device/Cargo.toml -p summrise-device
device_bin="$root/device/target/release/summrise-device"

if [ -z "$out" ]; then out="$(mktemp -d "${TMPDIR:-/tmp}/canary-rehearsal.XXXXXX")"; fi
mkdir -p "$out"
# A stale installer from an earlier run would be counted by verify without ever being published now.
[ -z "$(ls -A "$out")" ] || { echo "rehearse-release: --out must be empty: $out" >&2; exit 2; }

# The port is chosen BEFORE publishing: the signed document embeds the mirror URL, so the server
# that answers it cannot move afterwards.
mport="$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()')"
mirror="http://127.0.0.1:$mport"

python3 -m http.server "$mport" --bind 127.0.0.1 --directory "$out" >"/tmp/rehearse-http-$mport.log" 2>&1 &
srv=$!
device_pid=""
verifier_root=""
# The `|| true` matters: under `set -e`, the killed server's `wait` returning 143 would abort the
# EXIT trap and make a passing rehearsal exit 143. The verifier device is killed the same way.
cleanup() {
    kill "$srv" 2>/dev/null || true
    wait "$srv" 2>/dev/null || true
    if [ -n "$device_pid" ]; then
        kill "$device_pid" 2>/dev/null || true
        wait "$device_pid" 2>/dev/null || true
    fi
    if [ -n "$verifier_root" ]; then rm -rf "$verifier_root"; fi
}
trap cleanup EXIT
for _ in $(seq 1 100); do
    curl -fsS --max-time 2 -o /dev/null "$mirror/" 2>/dev/null && break
    sleep 0.1
done
curl -fsS --max-time 2 -o /dev/null "$mirror/" || { echo "rehearse-release: server did not start on port $mport" >&2; exit 1; }

echo "== publishing $version to $out (served at $mirror, device port $device_port)"
scripts/publish-canary.bash --version "$version" --channel canary --port "$device_port" \
    --mirror "$mirror" --out "$out" \
    --launcher-key "$keys/launcher.seed" --release-key "$keys/release.seed"

# The verifier boots once so its state document names the release being checked. `--check-update`
# refuses to run without a state document, and a fresh one would name this build's crate version;
# the launcher would have recorded the release version, so this start claims it, as a launcher would.
verifier_root="$(mktemp -d "${TMPDIR:-/tmp}/canary-verifier.XXXXXX")"
device_data="$verifier_root/data"
device_install="$verifier_root/install"
mkdir -p "$device_data" "$device_install"
dport="$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()')"
echo "== booting the verifier device once on 127.0.0.1:$dport as $version"
"$device_bin" --install "$device_install" --data "$device_data" --port "$dport" \
    --release-version "$version" --release-channel canary \
    >"$verifier_root/device.log" 2>&1 &
device_pid=$!
healthy=""
for _ in $(seq 1 100); do
    if curl -fsS --max-time 2 -o /dev/null "http://127.0.0.1:$dport/api/health"; then
        healthy=1
        break
    fi
    kill -0 "$device_pid" 2>/dev/null || break
    sleep 0.1
done
if [ -z "$healthy" ]; then
    echo "rehearse-release: the verifier device did not answer /api/health" >&2
    sed 's/^/  /' "$verifier_root/device.log" >&2 || true
    exit 1
fi
kill "$device_pid" 2>/dev/null || true
wait "$device_pid" 2>/dev/null || true
device_pid=""
[ -s "$device_data/state.json" ] || { echo "rehearse-release: the verifier device wrote no state document" >&2; exit 1; }

echo "== verifying what the mirror actually serves"
rc=0
scripts/verify-release.sh "$version" --mirror "$mirror" --channel canary \
    --device "$device_bin" --device-data "$device_data" --device-install "$device_install" || rc=$?
elapsed=$(( $(date +%s) - start ))
if [ "$rc" -eq 0 ]; then
    echo "PASS: $version published and verified from $mirror in ${elapsed}s"
    echo "  out: $out"
    if [ -f "$out/SummriseSetup-$version.exe" ]; then
        echo "  installer: built ($out/SummriseSetup-$version.exe)"
    else
        echo "  installer: NOT built (makensis was not found)"
    fi
else
    echo "FAIL: verify-release.sh exited $rc after ${elapsed}s; the publish is kept at $out"
fi
exit "$rc"
