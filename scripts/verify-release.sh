#!/usr/bin/env bash
# Check that a published release is the release that is actually running.
#
# The failures this exists for are the quiet ones: a channel pointer left on the previous version, an
# artifact on the mirror that does not match the digest its signed document names, a console that was
# never redeployed after its own build. Each is invisible from the repository and obvious from here.
#
#   scripts/verify-release.sh 0.3.18 \
#       --mirror http://<dev-box>:18100 \
#       --console http://<canary>:19000 \
#       --console-version-file /path/to/expected   # optional, defaults to the released version
set -euo pipefail

version="${1:?usage: verify-release.sh <version> [--mirror URL] [--console URL]}"
shift
mirror=""
console=""
console_version=""
channel="canary"
while [[ $# -gt 0 ]]; do
    case "$1" in
        --mirror) mirror="$2"; shift 2 ;;
        --console) console="$2"; shift 2 ;;
        --console-version) console_version="$2"; shift 2 ;;
        --channel) channel="$2"; shift 2 ;;
        *) echo "verify-release: unknown argument $1" >&2; exit 2 ;;
    esac
done

failures=0
note() { printf '  %-8s %s\n' "$1" "$2"; }
pass() { note "ok" "$1"; }
fail() { note "FAIL" "$1"; failures=$((failures + 1)); }

if [[ -n "$mirror" ]]; then
    echo "mirror $mirror"
    pointer="$(curl -fsS --max-time 20 "$mirror/release-$channel.json" || true)"
    if [[ -z "$pointer" ]]; then
        fail "release-$channel.json is not served"
    else
        pointed="$(printf '%s' "$pointer" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("version",""))')"
        if [[ "$pointed" == "$version" ]]; then
            pass "the $channel pointer names $version"
        else
            fail "the $channel pointer names '$pointed', not $version"
        fi
        artifact_url="$(printf '%s' "$pointer" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("artifactUrl",""))')"
        artifact_sha="$(printf '%s' "$pointer" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("sha256",""))')"
        if [[ -n "$artifact_url" ]]; then
            served="$(curl -fsS --max-time 120 "$artifact_url" | sha256sum | cut -d' ' -f1)"
            if [[ "$served" == "$artifact_sha" ]]; then
                pass "the artifact matches its signed digest"
            else
                fail "the artifact hashes to $served, the document says $artifact_sha"
            fi
        fi
        # The installer is the ONLY channel that carries `summrise-shell.exe`: the canary update
        # swaps the app and nothing else. A publish without it (no makensis on the host) can still
        # update the app, but a shell fix reaches a device only through this file.
        installer="SummriseSetup-$version.exe"
        if curl -fsS --max-time 20 -o /dev/null "$mirror/$installer"; then
            digest="$(curl -fsS --max-time 300 "$mirror/$installer" | sha256sum | cut -d' ' -f1)"
            pass "$installer is served (sha256 $digest) — the tray's only update channel"
        else
            note "warn" "$installer is not served: this publish cannot deliver a tray change"
        fi
    fi
fi

if [[ -n "$console" ]]; then
    echo "console $console"
    health="$(curl -fsS --max-time 20 "$console/health" || true)"
    if [[ -z "$health" ]]; then
        fail "the console does not answer /health"
    else
        reported="$(printf '%s' "$health" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("version",""))')"
        # The console has its own build stream: a device release does not redeploy it, so comparing it
        # to the DEVICE version is only right when the operator says which console build they deployed.
        if [[ -z "$console_version" ]]; then
            pass "the console answers, running $reported"
        elif [[ "$reported" == "$console_version" ]]; then
            pass "the console runs $console_version"
        else
            fail "the console runs '$reported', not $console_version"
        fi
        devices="$(printf '%s' "$health" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("devices",0))')"
        pass "$devices device(s) registered"
    fi
fi

if [[ "$failures" -gt 0 ]]; then
    echo "verify-release: $failures check(s) failed" >&2
    exit 1
fi
echo "verify-release: ok"
