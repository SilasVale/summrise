#!/usr/bin/env bash
# The LaunchDaemon plist is a product artifact. Validate its XML and the keys launchd requires
# without macOS: plistlib is in Python's standard library and runs on the Ubuntu CI job.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
python3 - "$ROOT/packaging/macos/online.summrise.device.plist" <<'PY'
import plistlib
import sys

path = sys.argv[1]
with open(path, "rb") as handle:
    plist = plistlib.load(handle)
assert plist["Label"] == "online.summrise.device", plist
assert plist["ProgramArguments"][0] == "/opt/summrise/summrise-device", plist
assert "--install" in plist["ProgramArguments"], plist
assert plist["RunAtLoad"] is True, plist
assert plist["KeepAlive"] is True, plist
assert plist["UserName"] == "_summrise", plist
print("MACOS_PLIST_VERIFY_OK")
PY
