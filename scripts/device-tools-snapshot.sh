#!/usr/bin/env bash
# Refresh the console's snapshot of what a DEVICE advertises, from the live fleet.
#
#   bash scripts/device-tools-snapshot.sh <device-id-or-name> [release]
#
# The console's pages call tools by name, and the name list they are checked against is
# `device/console/web/src/device-tools.snapshot.json` — a MEASURED artifact, not a hand-typed one. This script re-takes the
# measurement from the console's own `/fleet/<id>/tools` route (the same route the pages read), so refreshing it is one command
# instead of a careful edit.
#
# **WHY A SNAPSHOT AND NOT THE LIVE LIST AT TEST TIME.** A test cannot reach the fleet: CI has no tailnet, and a suite that needed
# one would fail on every run that is not the operator's laptop. A snapshot makes the fleet's answer a REVIEWABLE FILE — the diff
# of a refresh shows exactly which tools, and which fields, the device gained or lost — and the test compares the call sites
# against it.
#
# **THE RED LINE**: this records NAMES AND SCHEMAS, never the answer SHAPES. A tool's response shape is what the wire-shapes file
# exists for (`docs/superpowers/specs/2026-11-12-device-wire-shapes.md`), and the fleet answers older shapes than the source tree —
# generating TYPES from the tree would re-introduce the defects that file records.

set -euo pipefail

DEVICE="${1:?usage: device-tools-snapshot.sh <device-id-or-name> [release]}"
RELEASE="${2:-measured}"
BASE="${CONSOLE_URL:-https://console.saisi.online}"
OUT="$(cd "$(dirname "$0")/.." && pwd)/device/console/web/src/device-tools.snapshot.json"

tmp="$(mktemp)"
trap 'rm -f "$tmp"' EXIT

if ! curl -sf "$BASE/fleet/$DEVICE/tools" -o "$tmp"; then
  echo "device-tools-snapshot: $BASE/fleet/$DEVICE/tools did not answer (a session cookie is required)" >&2
  exit 1
fi

python3 - "$tmp" "$OUT" "$DEVICE" "$RELEASE" <<'PY'
import json, sys

src, out, device, release = sys.argv[1:5]
doc = json.load(open(src))
tools = doc.get("tools") if isinstance(doc, dict) else doc
if not isinstance(tools, list):
    sys.exit("device-tools-snapshot: the answer carries no tool list")

rows = []
for tool in sorted(tools, key=lambda t: t.get("name", "")):
    name = tool.get("name")
    if not isinstance(name, str) or name == "":
        continue
    schema = tool.get("inputSchema") or tool.get("input_schema") or {}
    rows.append(
        {
            "name": name,
            "required": schema.get("required", []),
            "props": sorted((schema.get("properties") or {}).keys()),
        }
    )

snapshot = {
    "source": f"console /fleet/<device>/tools, device {device}",
    "release": release,
    "measured_at": __import__("datetime").date.today().isoformat(),
    "note": "Measured from the FLEET, not from the source tree. Refresh with scripts/device-tools-snapshot.sh.",
    "tools": rows,
}
with open(out, "w", encoding="utf-8") as handle:
    json.dump(snapshot, handle, indent=2)
    handle.write("\n")
print(f"device-tools-snapshot: {len(rows)} tools -> {out}")
PY
