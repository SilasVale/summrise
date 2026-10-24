#!/usr/bin/env bash
# One command for a canary release: publish, mirror, verify.
#
# Everything this needs is either an environment variable or a key file, because the alternative is the
# command line I typed twenty times and got wrong twice — once by forgetting `--mirror-dir` (so the mirror
# served the previous version) and once by pointing the verification at a console that had not been
# redeployed yet.
#
#   export SUMMRISE_LAUNCHER_PUBKEY=… SUMMRISE_RELEASE_PUBKEY=… SUMMRISE_CONSOLE_GRANT_KEY=…
#   export SUMMRISE_MIRROR=http://<dev-box>:18100
#   export SUMMRISE_MIRROR_DIR=/tmp/canary-repo
#   export SUMMRISE_CONSOLE=http://<canary>:19000
#   export SUMMRISE_CONSOLE_VERSION=0.3.25        # what you deployed, if you deployed it
#   scripts/release-canary.sh 0.3.26
set -euo pipefail

version="${1:?usage: release-canary.sh <version>}"
here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

: "${SUMMRISE_MIRROR:?set SUMMRISE_MIRROR to the public mirror URL}"
: "${SUMMRISE_MIRROR_DIR:?set SUMMRISE_MIRROR_DIR to the directory that mirror serves}"
keys="$HOME/summrise/canary-kit/keys"

args=(
    --version "$version" --channel canary --port 18090
    --mirror "$SUMMRISE_MIRROR" --mirror-dir "$SUMMRISE_MIRROR_DIR"
    --launcher-key "$keys/launcher.seed" --release-key "$keys/release.seed"
    --out /tmp/canary-mirror
)
# The verification runs once, at the end, and only when there is something to verify against. Passing
# `--console` to the publish as well made it run twice and print the same four lines twice.
"$here/scripts/publish-canary.bash" "${args[@]}"

if [[ -n "${SUMMRISE_CONSOLE:-}" ]]; then
    verify=(--mirror "$SUMMRISE_MIRROR" --console "$SUMMRISE_CONSOLE")
    if [[ -n "${SUMMRISE_CONSOLE_VERSION:-}" ]]; then
        verify+=(--console-version "$SUMMRISE_CONSOLE_VERSION")
    fi
    "$here/scripts/verify-release.sh" "$version" "${verify[@]}"
fi
