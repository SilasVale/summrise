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
#
# With `--device PATH` the same mirror is also checked the way a device checks it: the binary is run
# as `--check-update <mirror>/release-<version>.json --channel <channel> --data DIR --install DIR`,
# so a document the mirror serves but no device will accept cannot pass. `--device` needs both
# directories, because `--check-update` reads the installed release from `<data>/state.json` and
# derives its staging paths from `<install>`.
set -euo pipefail

# Installer trust additionally needs --installer-pubkey HEX (or SUMMRISE_LAUNCHER_PUBKEY) and
# --release-tool PATH (defaults to device/target/release/summrise-release). --require-installer makes
# missing setup/configuration an error. Installer downloads need curl 8.4.0+ to bound unknown-length
# bodies. This verifies publisher-signed bytes/version/channel only.
version="${1:?usage: verify-release.sh <version> [--mirror URL] [--console URL] [--channel CHANNEL] [--device PATH --device-data DIR --device-install DIR] [--installer-pubkey HEX] [--release-tool PATH] [--require-installer]}"
shift
mirror=""
console=""
console_version=""
channel="canary"
device=""
device_data=""
device_install=""
installer_pubkey="${SUMMRISE_LAUNCHER_PUBKEY:-}"
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
release_tool="$repo_root/device/target/release/summrise-release"
require_installer=false
while [[ $# -gt 0 ]]; do
    case "$1" in
        --mirror) mirror="$2"; shift 2 ;;
        --console) console="$2"; shift 2 ;;
        --console-version) console_version="$2"; shift 2 ;;
        --channel) channel="$2"; shift 2 ;;
        --device) device="$2"; shift 2 ;;
        --device-data) device_data="$2"; shift 2 ;;
        --device-install) device_install="$2"; shift 2 ;;
        --installer-pubkey|--release-tool)
            if [[ $# -lt 2 || -z "$2" || "$2" == --* ]]; then
                echo "verify-release: $1 needs a value" >&2
                exit 2
            fi
            if [[ "$1" == --installer-pubkey ]]; then installer_pubkey="$2"; else release_tool="$2"; fi
            shift 2
            ;;
        --require-installer) require_installer=true; shift ;;
        *) echo "verify-release: unknown argument $1" >&2; exit 2 ;;
    esac
done
mirror="${mirror%/}"

if $require_installer; then
    if [[ -z "$installer_pubkey" ]]; then
        echo "verify-release: --require-installer needs --installer-pubkey or SUMMRISE_LAUNCHER_PUBKEY" >&2
        exit 2
    fi
    if [[ ! -f "$release_tool" || ! -x "$release_tool" ]]; then
        echo "verify-release: --require-installer needs an executable --release-tool: $release_tool" >&2
        exit 2
    fi
    if [[ -z "$mirror" ]]; then
        echo "verify-release: --require-installer needs --mirror" >&2
        exit 2
    fi
fi

if [[ -n "$device" ]]; then
    if [[ -z "$device_data" ]]; then
        echo "verify-release: --device needs --device-data <dir> naming the directory that holds state.json" >&2
        exit 2
    fi
    if [[ -z "$device_install" ]]; then
        echo "verify-release: --device needs --device-install <dir> naming the device's install root" >&2
        exit 2
    fi
    if [[ -z "$mirror" ]]; then
        echo "verify-release: --device needs --mirror, because the device is pointed at <mirror>/release-$version.json" >&2
        exit 2
    fi
elif [[ -n "$device_data" || -n "$device_install" ]]; then
    echo "verify-release: --device-data and --device-install only apply with --device <path>" >&2
    exit 2
fi

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/verify-release.XXXXXX")"
trap 'rm -rf "$tmpdir"' EXIT

failures=0
note() { printf '  %-8s %s\n' "$1" "$2"; }
pass() { note "ok" "$1"; }
fail() { note "FAIL" "$1"; failures=$((failures + 1)); }

# Download once into a temporary file. Ignore curlrc so it cannot enable redirects/retries. Both
# transport failure and non-2xx status remain distinct from a successfully received HTTP404.
fetch_installer_file() {
    local url="$1" destination="$2" cap="$3" seconds="$4"
    download_status=""
    download_status="$(curl -q -sS --proto '=http,https' --max-redirs 0 \
        --max-time "$seconds" --max-filesize "$cap" \
        -o "$destination" -w '%{http_code}' -- "$url")"
}

check_installer() {
    local installer="SummriseSetup-$version-$channel.exe" sidecar detail curl_version
    # Before curl 8.4, --max-filesize did not cap responses with unknown Content-Length. Refuse
    # unsupported/unparseable versions before any installer body, including the no-key partial mode.
    if curl_version="$(curl -q --version 2>/dev/null)" &&
        [[ "$curl_version" =~ ^curl[[:space:]]+([0-9]{1,3})\.([0-9]{1,3})\.([0-9]{1,3})([[:space:]]|$) ]] &&
        (( 10#${BASH_REMATCH[1]} > 8 || (10#${BASH_REMATCH[1]} == 8 && 10#${BASH_REMATCH[2]} >= 4) )); then
        :
    else
        fail "installer bounded downloads require curl 8.4.0 or newer; unsupported or unknown curl version"
        return
    fi
    if ! fetch_installer_file "$mirror/$installer" "$tmpdir/installer.exe" 134217728 300; then
        fail "cannot download $installer (transport or size limit failure)"
        return
    fi
    if [[ "$download_status" == 404 ]]; then
        installer="SummriseSetup-$version.exe"
        if ! fetch_installer_file "$mirror/$installer" "$tmpdir/installer.exe" 134217728 300; then
            fail "cannot download $installer (transport or size limit failure)"
            return
        fi
    fi
    case "$download_status" in
        2[0-9][0-9])
            if [[ -z "$installer_pubkey" ]]; then
                note "warn" "$installer is served; installer trust was not checked (no caller public key)"
                return
            fi
            if [[ ! -f "$release_tool" || ! -x "$release_tool" ]]; then
                fail "installer trust needs an executable --release-tool: $release_tool"
                return
            fi
            sidecar="${installer%.exe}.sidecar.json"
            if ! fetch_installer_file "$mirror/$sidecar" "$tmpdir/installer.sidecar.json" 65536 20; then
                fail "cannot download $sidecar (transport or size limit failure)"
                return
            fi
            case "$download_status" in
                2[0-9][0-9]) ;;
                *) fail "cannot fetch $sidecar (HTTP ${download_status:-none})"; return ;;
            esac
            if detail="$("$release_tool" verify-artifact --artifact "$tmpdir/installer.exe" \
                --sidecar "$tmpdir/installer.sidecar.json" --public-key "$installer_pubkey" \
                --version "$version" --channel "$channel" 2>&1)"; then
                pass "$installer: trusted publisher-signed bytes for $version ($channel)"
            else
                fail "$installer verification failed: $detail"
            fi
            ;;
        404)
            if $require_installer; then
                fail "installer is not served under either setup name"
            else
                note "warn" "installer is not served: this publish cannot deliver a tray change"
            fi
            ;;
        *) fail "cannot fetch $installer (HTTP ${download_status:-none})" ;;
    esac
}

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
        # The sidecar is the first-generation signed statement about the same artifact. The publish
        # writes it beside the exe; a device-only publish may omit it, so absence is a warning, not
        # a failure. When it is served it must name the same release and digest as the document.
        printf '%s' "$pointer" >"$tmpdir/release-document.json"
        # The sidecar's channel is part of its signed facts, so the channel-qualified file is the
        # one a verifier must read; the generic name is the last publish's and only a fallback.
        sidecar_url="$mirror/summrise-device-$version-$channel.sidecar.json"
        if [ "$(curl -sS --max-time 20 -o /dev/null -w '%{http_code}' "$sidecar_url" || true)" = "404" ]; then
            sidecar_url="$mirror/summrise-device-$version.sidecar.json"
        fi
        sidecar_status="$(curl -sS --max-time 20 -o "$tmpdir/sidecar.json" -w '%{http_code}' "$sidecar_url" || true)"
        case "$sidecar_status" in
            2*)
                if detail="$(
                    python3 - "$tmpdir/release-document.json" "$tmpdir/sidecar.json" 2>&1 <<'PY'
import json
import sys

try:
    with open(sys.argv[1]) as handle:
        document = json.load(handle)
    with open(sys.argv[2]) as handle:
        sidecar = json.load(handle)
except (OSError, ValueError) as error:
    print(f"cannot parse the document or sidecar as JSON: {error}")
    raise SystemExit(1)

if not isinstance(document, dict) or not isinstance(sidecar, dict):
    print("the document or sidecar is not a JSON object")
    raise SystemExit(1)

problems = []
for field in ("version", "channel", "sha256"):
    expected = document.get(field)
    found = sidecar.get(field)
    if field == "sha256" and isinstance(expected, str) and isinstance(found, str):
        agreed = expected.lower() == found.lower()
    else:
        agreed = expected == found
    if not agreed:
        problems.append(f"{field} is {found!r}, the document says {expected!r}")
print("; ".join(problems))
raise SystemExit(1 if problems else 0)
PY
                )"; then
                    pass "the sidecar agrees with the document"
                else
                    fail "the sidecar disagrees with the document: $detail"
                fi
                ;;
            404)
                note "warn" "summrise-device-$version.sidecar.json is not served: the sidecar check was skipped"
                ;;
            *)
                fail "cannot fetch summrise-device-$version.sidecar.json (HTTP ${sidecar_status:-none})"
                ;;
        esac
        # A setup proves publisher-signed bytes/version/channel only. Its sidecar always belongs to
        # the selected filename; a qualified setup with bad/missing metadata cannot select legacy.
        check_installer
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

if [[ -n "$device" ]]; then
    # The checks above prove the mirror serves what its document says. This one proves a device
    # believes the document: the verifier is run in the same hidden mode a device uses, against the
    # same state document, so a signature the mirror cannot detect a problem with is still caught.
    if "$device" --check-update "$mirror/release-$version.json" --channel "$channel" \
        --data "$device_data" --install "$device_install" \
        >"$tmpdir/device.out" 2>"$tmpdir/device.err"; then
        pass "the device accepts the signed document (version $version)"
    else
        rc=$?
        stderr="$(cat "$tmpdir/device.err")"
        stderr="${stderr//$'\n'/ }"
        if [[ -z "$stderr" ]]; then stderr="(no stderr)"; fi
        fail "the device refused the signed document (exit $rc): $stderr"
    fi
fi

if [[ "$failures" -gt 0 ]]; then
    echo "verify-release: $failures check(s) failed" >&2
    exit 1
fi
echo "verify-release: ok"
