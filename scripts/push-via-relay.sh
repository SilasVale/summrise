#!/usr/bin/env bash
# Push to GitHub through the oracle-phx tailnet relay when the direct TLS endpoint drops.
#
#   scripts/push-via-relay.sh [--host H] [--user U] [--key K] [--socks-port P] [refspec ...]
#
# Defaults match the measured box: ubuntu@100.79.21.110 with the Oracle Cloud key, a SOCKS proxy on
# 127.0.0.1:1080, and the two refspecs this repository publishes (`next` and `next:main`). The proxy is
# only up for the duration of the push; the tunnel dies with the script.
set -euo pipefail

host="${SUMMRISE_RELAY_HOST:-100.79.21.110}"
user="${SUMMRISE_RELAY_USER:-ubuntu}"
key="${SUMMRISE_RELAY_KEY:-$HOME/.ssh/imported/oci-ssh-key-2026-09-08.key}"
socks="${SUMMRISE_RELAY_SOCKS_PORT:-1080}"
refspecs=()

while [ $# -gt 0 ]; do
    case "$1" in
        --host) host="$2"; shift 2 ;;
        --user) user="$2"; shift 2 ;;
        --key) key="$2"; shift 2 ;;
        --socks-port) socks="$2"; shift 2 ;;
        -h|--help) sed -n '2,8p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        -*) echo "push-via-relay: unknown argument $1" >&2; exit 2 ;;
        *) refspecs+=("$1"); shift ;;
    esac
done
[ ${#refspecs[@]} -gt 0 ] || refspecs=(next next:main)
[ -r "$key" ] || { echo "push-via-relay: no readable key at $key" >&2; exit 2; }

tunnel=""
cleanup() {
    [ -n "$tunnel" ] && kill "$tunnel" 2>/dev/null || true
}
trap cleanup EXIT

echo "== relay $user@$host (socks 127.0.0.1:$socks)"
ssh -i "$key" -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile=/dev/null \
    -o ExitOnForwardFailure=yes -N -D "127.0.0.1:$socks" "$user@$host" &
tunnel=$!

ready=""
for _ in $(seq 1 30); do
    if curl --socks5-hostname "127.0.0.1:$socks" -sS -m 30 -o /dev/null https://github.com; then
        ready=1
        break
    fi
    sleep 1
done
[ -n "$ready" ] || { echo "push-via-relay: the tunnel did not reach GitHub" >&2; exit 1; }

proxy="socks5h://127.0.0.1:$socks"
for refspec in "${refspecs[@]}"; do
    echo "== push origin $refspec"
    git -c "http.proxy=$proxy" push origin "$refspec"
done
echo "== remote"
git -c "http.proxy=$proxy" ls-remote origin refs/heads/main refs/heads/next
