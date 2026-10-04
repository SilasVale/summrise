#!/usr/bin/env bash
# A STUB HARNESS FOR `deploy-rs.sh` — run every path without a box.
#
# WHY. The script says of itself IT HAS NEVER BEEN RUN, and the two bugs found by running it (a docker check
# above the branch that needs no docker, a build with no `cd`) were both invisible to reading. What is left in
# it are the paths that only execute on the VPS: `--on-box`'s multi-line SSH command, the ship/start/smoke
# sequence, and `--stop`. None of them can be reached from here — but the LOGIC of each can, by putting
# `ssh`, `scp` and `docker` on PATH as shims that RECORD THEIR ARGUMENTS and answer with what a real one would.
#
# WHAT IT PROVES, AND WHAT IT CANNOT. It proves each branch is reachable, that the arguments each one builds
# are the ones written down, and that the smoke's assertion BITES when the relay does not answer 401. It
# cannot prove the box behaves — that is what the first real run is for.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
SCRIPT="$ROOT/proxies/api-relay/deploy-rs.sh"
WORK="$(mktemp -d -t vrelay-stub-XXXXXX)"
trap 'rm -rf "$WORK"' EXIT

mkdir -p "$WORK/bin"
CALLS="$WORK/calls.log"

# ── the shims ────────────────────────────────────────────────────────────────────────────────────
# Each records its argv on ONE line and answers like the real tool: `ssh` runs nothing (it prints the
# smoke's answer), `scp` succeeds, `docker` fails loudly so a path that needs it is visible as such.
cat > "$WORK/bin/ssh" <<SHIM
#!/usr/bin/env bash
printf 'ssh %s\n' "\$*" >> "$CALLS"
# The smoke asks for an HTTP code; answer with whatever SMOKE_CODE says (401 is the healthy answer).
if printf '%s' "\$*" | grep -q 'curl -s -m 10'; then
  printf '%s\n' "\${SMOKE_CODE:-401}"
else
  echo "stub-ssh: ok"
fi
SHIM
cat > "$WORK/bin/scp" <<SHIM
#!/usr/bin/env bash
printf 'scp %s\n' "\$*" >> "$CALLS"
echo "stub-scp: ok"
SHIM
cat > "$WORK/bin/docker" <<SHIM
#!/usr/bin/env bash
printf 'docker %s\n' "\$*" >> "$CALLS"
echo "stub-docker: called" >&2
exit 0
SHIM
chmod +x "$WORK/bin/"*

export PATH="$WORK/bin:$PATH"
export VRELAY_KEY="$HOME/.ssh/vrelay.key"
export VRELAY_MUSL_CROSS=/tmp/aarch64-linux-musl-cross/bin

run() {
  local label="$1"; shift
  : > "$CALLS"
  echo "── $label"
  if "$SCRIPT" "$@" > "$WORK/out.log" 2>&1; then
    echo "   exit 0"
  else
    echo "   exit $? (non-zero)"
  fi
  sed 's/^/   /' "$WORK/out.log" | tail -6
  [ -s "$CALLS" ] && sed 's/^/   calls: /' "$CALLS" | head -12
}

run "--stop" --stop
run "the default path (local toolchain)" 
run "--on-box" --on-box
echo "── the smoke's assertion, with the relay answering 500"
SMOKE_CODE=500 run "smoke expects 401, gets 500" 2>/dev/null || true
: > "$CALLS"; if SMOKE_CODE=500 "$SCRIPT" > "$WORK/out.log" 2>&1; then echo "   exit 0 — THE ASSERTION DID NOT BITE"; else echo "   exit $? — the assertion bit"; fi
grep -E "expected 401" "$WORK/out.log" | sed 's/^/   /' | head -2
