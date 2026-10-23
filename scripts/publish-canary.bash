#!/usr/bin/env bash
# Publish one canary build: the artifact the update path downloads, the document that names it, the
# first-generation sidecar, and the Windows installer — all in one directory a mirror can serve.
#
# The three files an update needs are the artifact, its document and (only for a seed) the sidecar.
# They are produced together here because a version signed into one and not the other is a release
# that half-exists: the runbook's 0.2.1 attempt shipped a 0.2.1 installer carrying a 0.1.9 sidecar,
# and the machine reported the old version while every log said the install succeeded.
#
#   scripts/publish-canary.bash --version 0.2.7 \
#       --channel canary --port 18090 --mirror http://100.85.163.159:18100 \
#       --launcher-key ~/.keys/launcher.seed --release-key ~/.keys/release.seed \
#       --out /srv/canary
#
# `--mirror` is the base the DOCUMENT advertises: the device fetches the artifact from it, so it
# must be an address that device can reach, not necessarily the one this host uses. `--allow-plaintext`
# is added automatically when that address is http, because the signer and the device both refuse
# plaintext to a non-loopback host without the operator's explicit opt-in.
#
# The build keys are NOT passed here: the binaries must already have been built with the matching
# public keys, and a script that also chose them would be a second place for the key policy to live.
set -euo pipefail

# A non-login shell does not read ~/.profile; rustup's default location is the one place cargo is
# guaranteed to be, and a publish that fails because PATH was small is a publish that lied.
command -v cargo >/dev/null 2>&1 || export PATH="$HOME/.cargo/bin:$PATH"
command -v cargo >/dev/null 2>&1 || { echo "publish-canary: cargo is not on PATH" >&2; exit 1; }

version=""
channel="canary"
port="18080"
mirror=""
launcher_key=""
release_key=""
out=""
target="x86_64-pc-windows-msvc"

usage() {
    sed -n '2,26p' "$0" | sed 's/^# \{0,1\}//'
    exit "${1:-0}"
}

while [ $# -gt 0 ]; do
    case "$1" in
        --version) version="$2"; shift 2 ;;
        --channel) channel="$2"; shift 2 ;;
        --port) port="$2"; shift 2 ;;
        --mirror) mirror="${2%/}"; shift 2 ;;
        --launcher-key) launcher_key="$2"; shift 2 ;;
        --release-key) release_key="$2"; shift 2 ;;
        --out) out="$2"; shift 2 ;;
        --target) target="$2"; shift 2 ;;
        --help|-h) usage 0 ;;
        *) echo "publish-canary: unknown argument '$1'" >&2; usage 2 ;;
    esac
done

for required in version mirror launcher_key release_key out; do
    [ -n "${!required}" ] || { echo "publish-canary: --${required//_/-} is required" >&2; exit 2; }
done
case "$version" in
    [0-9]*.[0-9]*.[0-9]*) ;;
    *) echo "publish-canary: --version must be X.Y.Z, not '$version'" >&2; exit 2 ;;
esac

root="$(cd "$(dirname "$0")/.." && pwd)"
# The cargo workspace is `device/`; the repository root is not a crate.
cd "$root/device"

# The signer is the same binary for both keys; the scheme of the advertised address decides whether
# the opt-in is needed, and it is passed in exactly the form the device will see.
plaintext=""
case "$mirror" in
    http://localhost*|http://127.*|http://\[::1\]*) ;;
    http://*) plaintext="--allow-plaintext" ;;
    https://*) ;;
    *) echo "publish-canary: --mirror must be http(s)://, not '$mirror'" >&2; exit 2 ;;
esac

# **THE BUILD KEYS ARE NOT OPTIONAL.** A binary built without them embeds the phase-1 placeholder
# and refuses every seed and every update — the first CI-built installer did exactly that on a real
# machine. They are public keys, so they are environment variables that must be present, not flags
# that can be forgotten: a publish without them is a publish of a device that cannot be trusted.
: "${SUMMRISE_LAUNCHER_PUBKEY:?SUMMRISE_LAUNCHER_PUBKEY is not set}"
: "${SUMMRISE_RELEASE_PUBKEY:?SUMMRISE_RELEASE_PUBKEY is not set}"
: "${SUMMRISE_CONSOLE_GRANT_KEY:?SUMMRISE_CONSOLE_GRANT_KEY is not set}"
export SUMMRISE_LAUNCHER_PUBKEY SUMMRISE_RELEASE_PUBKEY SUMMRISE_CONSOLE_GRANT_KEY

echo "== building the signer for this host (it is what signs; it never ships to the device)"
cargo build --release -p summrise-release
signer="target/release/summrise-release"

echo "== building the runtime for $target"
cargo xwin build --release --target "$target" -p summrise-launch -p summrise-device -p summrise-console
bin="target/$target/release"
mkdir -p "$out"

# The console page is a build artifact inside the binary. A publish that embeds yesterday's JavaScript
# is the one failure this check exists for, and it costs a second.
"$PWD/scripts/build-web.sh" --check

echo "== signing the release document for $version ($channel)"
signer_bin="$signer"
artifact="$bin/summrise-device.exe"
cp "$artifact" "$out/summrise-device-$version.exe"
"$signer_bin" sign --key-file "$release_key" \
    --artifact "$out/summrise-device-$version.exe" \
    --version "$version" --channel "$channel" \
    --url "$mirror/summrise-device-$version.exe" \
    --min-version 0.1.0 $plaintext \
    --out "$out/release-$version.json"

# The channel pointer is what a device's `update.json` names: one file per channel that always holds
# the newest document. Written here so publishing cannot forget it — the alternative was a manual copy
# that worked exactly until the first publish someone did from another shell.
cp "$out/release-$version.json" "$out/release-$channel.json"
echo "== channel pointer: release-$channel.json -> $version"

echo "== signing the first-generation sidecar with the launcher key"
"$signer_bin" sidecar --key-file "$launcher_key" \
    --artifact "$artifact" --version "$version" --channel "$channel" \
    --out "$bin/summrise-device.sidecar.json"

echo "== building the installer on this host (NSIS)"
# NSIS is not a build dependency of the runtime, so it is looked for where it actually lands: PATH,
# the Chocolatey install on a Windows host, and the extracted release tree this repository uses when
# the host has none.
makensis=""
for candidate in "$(command -v makensis 2>/dev/null || true)" \
                 "/c/Program Files (x86)/NSIS/makensis.exe" \
                 "$HOME/tools/nsis/usr/bin/makensis" \
                 "/tmp/nsis/usr/bin/makensis"; do
    if [ -n "$candidate" ] && [ -x "$candidate" ]; then makensis="$candidate"; break; fi
done
if [ -n "$makensis" ]; then
    if [ ! -e /usr/share/nsis/Stubs ] && [ -d /tmp/nsis/usr/share/nsis/Stubs ]; then
        export NSISDIR="/tmp/nsis/usr/share/nsis"
    fi
    source_dir="$PWD/$bin"
    (cd installer && "$makensis" -DVERSION="$version" -DPORT="$port" \
        -DCHANNEL="$channel" -DSOURCE="$source_dir" summrise.nsi)
    cp installer/SummriseSetup-"$version".exe "$out/"
else
    echo "publish-canary: makensis was not found; the installer is not in this publish" >&2
fi

echo "== what a mirror must serve"
ls -l "$out"
cat <<EOF

The device updates with:

  summrise-device --check-update $mirror/release-$version.json \\
      --channel $channel --allow-plaintext   # only when the mirror is plain http

The first generation is seeded by the installer, which carries the sidecar signed above.
EOF
