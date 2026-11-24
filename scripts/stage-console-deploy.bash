#!/usr/bin/env bash
# Build summrise-console for Windows, stage it on the mirror, and write the deploy script.
#
#   scripts/stage-console-deploy.bash 0.3.153
#   scripts/stage-console-deploy.bash 0.3.153 --mirror-dir /tmp/canary-mirror \
#       --base-url http://100.85.163.159:18100 --console-origin http://127.0.0.1:7736
#
# --console-origin (or SUMMRISE_CONSOLE_ORIGIN) must match the existing Console's --bind/--port.
# The legacy default is http://127.0.0.1:19000 and warns; staging never changes run-console.cmd.
#
# After this, ONE command runs on the console host (it is printed at the end):
#
#   powershell -NoProfile -Command "Invoke-WebRequest -UseBasicParsing <base>/deploy-console-<version>.cmd
#       -OutFile C:\Windows\Temp\deploy.cmd; cmd /c C:\Windows\Temp\deploy.cmd"
#
# The host has no Rust toolchain (measured 2026-10-28: cargo, rustc and git are all absent), so the
# binary is built here and fetched by the host over the tailnet. The bytes never pass through an AI
# context, and the host opens nothing inbound.
#
# The address the host fetches from is the canary mirror the operator already runs for device
# artifacts; `--base-url` is the address that host can reach, not necessarily the one this host uses.
set -euo pipefail
cd "$(dirname "$0")/.."

version=""
mirror_dir="/tmp/canary-mirror"
base_url="http://100.85.163.159:18100"
console_origin="${SUMMRISE_CONSOLE_ORIGIN-http://127.0.0.1:19000}"
console_origin_explicit="${SUMMRISE_CONSOLE_ORIGIN+x}"
target="x86_64-pc-windows-msvc"
previous=""

while [ $# -gt 0 ]; do
    case "$1" in
        --mirror-dir) mirror_dir="$2"; shift 2 ;;
        --base-url) base_url="${2%/}"; shift 2 ;;
        --console-origin)
            [ $# -ge 2 ] || { echo "stage-console-deploy: --console-origin needs a value" >&2; exit 2; }
            console_origin="${2%/}"; console_origin_explicit=x; shift 2 ;;
        --target) target="$2"; shift 2 ;;
        --previous) previous="$2"; shift 2 ;;
        -h | --help) sed -n '2,18p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        -*) echo "stage-console-deploy: unknown argument $1" >&2; exit 2 ;;
        *) version="$1"; shift ;;
    esac
done

[ -n "$version" ] || { echo "usage: scripts/stage-console-deploy.bash <version> [--mirror-dir DIR] [--base-url URL] [--console-origin http://HOST:PORT]" >&2; exit 2; }
case "$version" in
    *[!0-9.]* | "") echo "stage-console-deploy: version must be digits and dots: $version" >&2; exit 2 ;;
esac
console_origin="${console_origin%/}"
# These URLs are rendered into CMD and PowerShell. Reject shell-sensitive characters rather than
# escaping them differently in sed, CMD and PowerShell; the health origin is the existing listener.
python3 - "$base_url" "$console_origin" <<'PY'
import ipaddress
import re
import sys
from urllib.parse import urlsplit

for name, value in zip(("base URL", "Console origin"), sys.argv[1:]):
    try:
        parsed = urlsplit(value)
        host, port = parsed.hostname, parsed.port
        if not re.fullmatch(r"[A-Za-z0-9:/._~\[\]-]+", value):
            raise ValueError("URL contains unsupported or command-sensitive characters")
        if parsed.scheme not in ("http", "https") or not host or parsed.username or parsed.password:
            raise ValueError("expected an http(s) address without credentials")
        if parsed.query or parsed.fragment or (name == "Console origin" and parsed.path):
            raise ValueError("Console origin cannot have a path; queries and fragments are unsupported")
        if (name == "Console origin" and port is None) or (port is not None and not 1 <= port <= 65535):
            raise ValueError("Console origin needs an explicit port in 1..65535")
        try:
            ipaddress.ip_address(host)
        except ValueError:
            labels = host.split(".")
            if len(host) > 253 or re.fullmatch(r"[0-9.]+", host) or any(
                not re.fullmatch(r"[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?", label)
                for label in labels
            ):
                raise ValueError("invalid IP address or host name")
    except ValueError as error:
        raise SystemExit(f"stage-console-deploy: invalid {name}: {error}")
PY
if [ -z "$console_origin_explicit" ]; then
    echo "stage-console-deploy: using legacy Console origin $console_origin; pass --console-origin matching the existing --bind/--port" >&2
fi
command -v cargo >/dev/null 2>&1 || export PATH="$HOME/.cargo/bin:$PATH"
command -v cargo-xwin >/dev/null 2>&1 || { echo "stage-console-deploy: cargo-xwin is not on PATH" >&2; exit 2; }
[ -d "$mirror_dir" ] || { echo "stage-console-deploy: mirror dir does not exist: $mirror_dir" >&2; exit 2; }

# THE PAGE IS EMBEDDED IN THE BINARY, SO A STALE TREE SHIPS YESTERDAY'S UI. This happened: a local
# branch built a page that no longer carried main's Activity export and time window, and the only
# symptom was a page-size drop at deploy time. Refuse a dirty tree, a HEAD that does not contain the
# fetched main, and a page that does not match the committed source.
if [ -n "$(git status --porcelain)" ]; then
    echo "stage-console-deploy: the worktree is dirty; commit the page and source before staging" >&2
    git status --short >&2
    exit 2
fi
if git rev-parse --verify -q origin/main >/dev/null; then
    if ! git merge-base --is-ancestor origin/main HEAD; then
        echo "stage-console-deploy: HEAD does not contain origin/main; fetch and merge/rebase before staging" >&2
        exit 2
    fi
fi
echo "stage-console-deploy: checking the page against its source..."
scripts/build-web.sh --check >/dev/null

echo "stage-console-deploy: building $version for $target"
SUMMRISE_BUILD_VERSION="$version" cargo xwin build --manifest-path device/Cargo.toml \
    --release --target "$target" -p summrise-console

exe="device/target/$target/release/summrise-console.exe"
[ -f "$exe" ] || { echo "stage-console-deploy: the build produced no $exe" >&2; exit 2; }

# The version must be in the bytes, not only in the variable: a build that ignored
# SUMMRISE_BUILD_VERSION would deploy a binary that reports yesterday's release.
# `grep -c`, not `grep -q`: with `set -o pipefail`, an early-exiting `grep -q` gives `strings` a
# SIGPIPE and the pipeline reports failure on a match — the check would fail exactly when it should pass.
if [ "$(strings -n 4 "$exe" | grep -cF "$version" || true)" -eq 0 ]; then
    echo "stage-console-deploy: $exe does not contain the string $version — did the build ignore SUMMRISE_BUILD_VERSION?" >&2
    exit 2
fi

sha="$(sha256sum "$exe" | cut -d' ' -f1)"
staged="$mirror_dir/summrise-console-$version.exe"
deploy="$mirror_dir/deploy-console-$version.cmd"
cp "$exe" "$staged"
# The post-deploy real-device check travels with the deploy, so the runbook's command fetches the
# same script that was tested with this tree instead of relying on a copy from an earlier session.
cp scripts/real-device-check.ps1 "$mirror_dir/real-device-check.ps1"

sed -e "s|__VERSION__|$version|g" \
    -e "s|__SHA256__|$sha|g" \
    -e "s|__BASE_URL__|$base_url|g" \
    -e "s|__CONSOLE_ORIGIN__|$console_origin|g" \
    scripts/deploy-console.cmd.template > "$deploy"

echo
echo "staged:"
echo "  $staged  ($(stat -c%s "$staged") bytes, sha256 $sha)"
echo "  $deploy"
echo
echo "run on the console host (DESKTOP-9VH92BJ, task SummriseConsole):"
echo "  powershell -NoProfile -Command \"Invoke-WebRequest -UseBasicParsing $base_url/deploy-console-$version.cmd -OutFile C:\\Windows\\Temp\\deploy.cmd; cmd /c C:\\Windows\\Temp\\deploy.cmd\""
echo
echo "rollback, if the health line does not say $version:"
echo "  schtasks /end /tn SummriseConsole && taskkill /F /IM summrise-console.exe"
echo "  copy /Y C:\\SummriseConsole\\summrise-console.exe.bak-<previous> C:\\SummriseConsole\\summrise-console.exe"
echo "  schtasks /run /tn SummriseConsole"
