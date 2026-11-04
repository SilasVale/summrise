#!/usr/bin/env bash
# Watch one device after a canary publish; withdraw the channel if it does not come back.
#
#   canary-watch.sh <new-version> <device-name> [--console URL] [--timeout SECONDS]
#
# The device that took the release must answer `/fleet` with the new version before the release is
# considered delivered. If it does not, the channel pointer is re-signed back to the previous version
# (the artifact is untouched: only the policy signature moves — see docs/RELEASE.md), so a recovered
# machine cannot be broken again by the same build. Exit 0 only when the device reports the version.
set -euo pipefail

VERSION="${1:?usage: canary-watch.sh <new-version> <device-name> [--console URL] [--timeout SECONDS]}"
DEVICE_NAME="${2:?the device to watch, e.g. DESKTOP-CPH4KB2}"
CONSOLE="https://console.saisi.online"
TIMEOUT=600
shift 2
while [ $# -gt 0 ]; do
    case "$1" in
        --console) CONSOLE="$2"; shift 2 ;;
        --timeout) TIMEOUT="$2"; shift 2 ;;
        *) echo "canary-watch: unknown argument '$1'" >&2; exit 2 ;;
    esac
done

MIRROR=http://100.85.163.159:18100
MIRROR_DIR=/home/zss/canary-mirror
KEYS=/home/zss/canary-kit-archive-2026-10-22/keys
SIGNER=/home/zss/summrise-product/device/target/release/summrise-release

version_of() {
    curl -fsS --max-time 10 "$CONSOLE/fleet" |
        python3 -c "import json,sys; d=[x for x in json.load(sys.stdin) if x.get('name','').upper()=='$DEVICE_NAME'.upper()]; print(d[0]['version'] if d and d[0].get('online') else '')"
}

echo "canary-watch: waiting for $DEVICE_NAME to report $VERSION (up to ${TIMEOUT}s)"
deadline=$(( $(date +%s) + TIMEOUT ))
while [ "$(date +%s)" -lt "$deadline" ]; do
    seen=$(version_of || true)
    if [ "$seen" = "$VERSION" ]; then
        echo "canary-watch: $DEVICE_NAME is on $VERSION"
        exit 0
    fi
    sleep 15
done

previous=$(python3 -c "import json; print(json.load(open('$MIRROR_DIR/release-0.3.271.json'))['version'])" 2>/dev/null || echo 0.3.271)
echo "canary-watch: $DEVICE_NAME did not report $VERSION; withdrawing the channel to $previous" >&2
"$SIGNER" sign --key-file "$KEYS/release.seed" \
    --artifact "$MIRROR_DIR/summrise-device-$previous.exe" \
    --version "$previous" --channel canary \
    --url "$MIRROR/summrise-device-$previous.exe" \
    --min-version 0.1.0 --allow-plaintext \
    --out "$MIRROR_DIR/release-$previous.json" >/dev/null
cp "$MIRROR_DIR/release-$previous.json" "$MIRROR_DIR/release-canary.json"
echo "canary-watch: withdrawn; the channel names $previous again" >&2
exit 1
