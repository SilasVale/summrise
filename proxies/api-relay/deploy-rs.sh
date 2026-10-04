#!/usr/bin/env bash
# deploy-rs.sh — put the RUST relay on the box WITHOUT CUTTING OVER.
#
# WHY THIS EXISTS, AND WHY IT IS NOT THE CUTOVER. The plan names `api-relay` as the highest-risk item in
# the migration and says what to do about it: **it is this repository's own push path, so a binary that
# has never run should RUN before it becomes the remote.** This script is that step and only that step:
#
#   * it cross-builds `vrelay` for the box's architecture and ships it beside the Node bundle;
#   * it installs a SECOND systemd unit on port 8082, so the Rust relay runs while nginx still points at
#     the Node one on 8081 — no traffic moves;
#   * it smokes the new one directly (`curl 127.0.0.1:8082`), which is the only honest way to say it
#     works before anything depends on it;
#   * and it PRINTS the cutover and the rollback, which are each one line of nginx and one reload.
#
# IT HAS NEVER BEEN RUN. There is no `~/.ssh/vrelay.key` on the development box, so the first run is
# somebody's deliberate act, on a machine that can reach the VPS — and the checklist below is what to
# read before doing it.
#
#   ./proxies/api-relay/deploy-rs.sh              # cross-build here (docker), ship, start, smoke
#   ./proxies/api-relay/deploy-rs.sh --on-box     # build ON the VPS instead (needs no cross toolchain)
#   ./proxies/api-relay/deploy-rs.sh --stop       # stop and disable the second unit (leaves the binary)
#
# THE CUTOVER, when you decide (NOT done by this script):
#
#   1. ssh ubuntu@$VRELAY_HOST 'journalctl -u vrelay-rs -n 50'     # it has been answering for a while
#   2. ssh ubuntu@$VRELAY_HOST 'curl -s -o /dev/null -w "%{http_code}\n" \
#        -X POST http://127.0.0.1:8082/api/proxy'                   # expect 401 (the BYOK gate)
#   3. in the `v.saisi.online` vhost, point `/api/` at 127.0.0.1:8082 instead of 8081, `nginx -t`, then
#      `systemctl restart nginx` (the README's reload pitfall: a reload can silently no-op after a failed
#      unit — prefer restart, and probe the behaviour rather than trusting the exit code)
#   4. ROLLBACK IS THE SAME LINE BACKWARDS: point `/api/` at 8081, restart nginx. The Node bundle is
#      still in /opt/vrelay and still enabled, so the old relay never stops being available.
#
# THE ARCHITECTURE IS NOT A DETAIL. The box is `VM.Standard.A1.Flex` — Ampere **aarch64**, not x86_64 —
# so this cross-builds with `docker` and the musl target, which is what makes the binary STATIC (the
# plan's word) and independent of the box's glibc. `ring` (the TLS crypto under rustls) needs a C
# compiler for that target, which is why the build runs in a container rather than on this host: measured
# 2026-10-02, a native `cargo build --target aarch64-unknown-linux-musl` here fails with
# "failed to find tool aarch64-linux-musl-gcc".
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
CRATE="$ROOT/proxies/api-relay/relay"
HOST="${VRELAY_HOST:-132.226.90.175}"
KEY="${VRELAY_KEY:-$HOME/.ssh/vrelay.key}"
PORT="${VRELAY_RS_PORT:-8082}"
# PINNED, like wrangler and the rust toolchain: a bare tag is a moving target, and the first run of this
# script should record the digest it pulled (`docker image inspect --format '{{index .RepoDigests 0}}'`).
IMAGE="${VRELAY_BUILDER_IMAGE:-messense/rust-musl-cross:aarch64-musl}"
TARGET="aarch64-unknown-linux-musl"

say() { printf '%s\n' "$*"; }

if [ "${1:-}" = "--stop" ]; then
  [ -f "$KEY" ] || { say "  !! relay ssh key not found at $KEY (VRELAY_KEY to override)" >&2; exit 1; }
  say "=== [stop] the second unit (the Node relay is untouched) ==="
  ssh -i "$KEY" -o StrictHostKeyChecking=no "ubuntu@$HOST" \
    'sudo systemctl disable --now vrelay-rs 2>/dev/null; systemctl is-active vrelay-rs || true'
  say "  ok: vrelay-rs stopped; /opt/vrelay-rs/vrelay and the unit are still there for the next run"
  exit 0
fi

[ -f "$KEY" ] || { say "  !! relay ssh key not found at $KEY (VRELAY_KEY to override)" >&2; exit 1; }

# **THE DOCKER CHECK USED TO SIT HERE, ABOVE THE `--on-box` BRANCH, AND THAT WAS A BUG**: `--on-box`
# builds ON the VPS and its own comment says it "needs no cross toolchain at all" — but this line refused
# before the branch could run, so the path that always works required a docker daemon it never used.
# Measured 2026-10-03: this development box has no reachable docker socket, so `--on-box` died on a check
# about a tool it does not touch. The check lives in the branch that uses docker now.

if [ "${1:-}" = "--on-box" ]; then
  # **BUILD ON THE BOX**, which needs no cross toolchain at all: the VPS is aarch64, so a native
  # `cargo build --release` there produces the binary the machine will run. This is the path that always
  # works — the cross-build needs docker AND a C compiler for the musl target (`ring` is C), and this
  # development box has neither the daemon socket nor `aarch64-linux-musl-gcc` (both measured
  # 2026-10-02). It costs a source tarball and a few minutes of the box's CPU, once per deploy.
  say "=== [build] vrelay ON the box (native aarch64; rustup installed on first use) ==="
  TARBALL="$(mktemp -t vrelay-src-XXXXXX.tar.gz)"
  tar -czf "$TARBALL" -C "$ROOT/proxies/api-relay" relay
  scp -i "$KEY" -o StrictHostKeyChecking=no "$TARBALL" "ubuntu@$HOST:/tmp/vrelay-src.tar.gz"
  rm -f "$TARBALL"
  ssh -i "$KEY" -o StrictHostKeyChecking=no "ubuntu@$HOST" '
    set -e
    command -v cargo >/dev/null 2>&1 || {
      curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
    }
    rm -rf /tmp/vrelay-src && mkdir -p /tmp/vrelay-src
    tar -xzf /tmp/vrelay-src.tar.gz -C /tmp/vrelay-src
    cd /tmp/vrelay-src/relay
    "$HOME/.cargo/bin/cargo" build --release --bin vrelay
  '
  ssh -i "$KEY" -o StrictHostKeyChecking=no "ubuntu@$HOST" \
    'install -m 755 /tmp/vrelay-src/relay/target/release/vrelay /tmp/vrelay'
  BIN=""
elif [ -n "${VRELAY_MUSL_CROSS:-}" ] || command -v aarch64-linux-musl-gcc >/dev/null 2>&1 \
   || [ -x "${VRELAY_MUSL_CROSS:-/tmp}/aarch64-linux-musl-cross/bin/aarch64-linux-musl-gcc" ]; then
  # **A THIRD PATH, MEASURED 2026-10-03: A LOCAL MUSL CROSS TOOLCHAIN.** The container path below needs a
  # docker daemon; this one needs a tarball. It was proved end to end here — `musl.cc`'s
  # `aarch64-linux-musl-cross.tgz` (108 MB, GCC 11.2.1), unpacked, with the two variables below — and it
  # produced `ELF 64-bit LSB executable, ARM aarch64, statically linked, stripped` at 3,831,200 bytes.
  # `ring` is the C in the dependency tree, which is why the compiler and the LINKER both have to be set.
  CROSS="${VRELAY_MUSL_CROSS:-$(dirname "$(command -v aarch64-linux-musl-gcc 2>/dev/null || echo /tmp/aarch64-linux-musl-cross/bin/aarch64-linux-musl-gcc)")}"
  say "=== [build] vrelay for $TARGET (local toolchain: $CROSS) ==="
  # **`cd "$CRATE"`, WHICH THE CONTAINER PATH GETS FROM `-w /src` AND THIS ONE DID NOT.** Running the
  # script for the first time found it: the build ran from the repository root and answered
  # `error: could not find Cargo.toml in /home/zss/summrise`. Nobody had run this file before, which is
  # exactly the class of defect that running it is for.
  ( cd "$CRATE" && \
    CC_aarch64_unknown_linux_musl="$CROSS/aarch64-linux-musl-gcc" \
    CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER="$CROSS/aarch64-linux-musl-gcc" \
      cargo build --release --target "$TARGET" --bin vrelay )
  BIN="$CRATE/target/$TARGET/release/vrelay"
  # The container image strips by default; match it, or the artifact differs by 1.8 MB for no reason.
  [ -x "$CROSS/aarch64-linux-musl-strip" ] && "$CROSS/aarch64-linux-musl-strip" "$BIN"
else
  command -v docker >/dev/null 2>&1 || {
    say "  !! no build path: no --on-box, no local musl cross toolchain, and no docker" >&2
    say "     (VRELAY_MUSL_CROSS=<dir> points at an unpacked aarch64-linux-musl-cross)" >&2
    exit 1
  }
  say "=== [build] vrelay for $TARGET (in $IMAGE) ==="
  docker run --rm -v "$CRATE":/src -w /src "$IMAGE" \
    cargo build --release --target "$TARGET" --bin vrelay
  BIN="$CRATE/target/$TARGET/release/vrelay"
fi
if [ -n "$BIN" ]; then
  [ -x "$BIN" ] || { say "  !! the build produced no binary at $BIN" >&2; exit 1; }
  say "  built: $(stat -c%s "$BIN") bytes — $(file -b "$BIN" | cut -c1-80)"
else
  say "  built on the box: $(ssh -i "$KEY" -o StrictHostKeyChecking=no "ubuntu@$HOST" 'stat -c%s /tmp/vrelay') bytes"
fi

say "=== [ship] to /opt/vrelay-rs (BESIDE the Node bundle, not over it) ==="
if [ -n "$BIN" ]; then
  scp -i "$KEY" -o StrictHostKeyChecking=no "$BIN" "ubuntu@$HOST:/tmp/vrelay"
fi
ssh -i "$KEY" -o StrictHostKeyChecking=no "ubuntu@$HOST" "
  set -e
  sudo install -d -m 755 /opt/vrelay-rs
  sudo install -m 755 /tmp/vrelay /opt/vrelay-rs/vrelay
  rm -f /tmp/vrelay
  sudo tee /etc/systemd/system/vrelay-rs.service >/dev/null <<'UNIT'
[Unit]
Description=vrelay (Rust) — the api-relay migration's candidate, on a port of its own
After=network-online.target
Wants=network-online.target

[Service]
ExecStart=/opt/vrelay-rs/vrelay 127.0.0.1:$PORT
Restart=always
RestartSec=2
User=ubuntu
# THE ENVIRONMENT THE JAVASCRIPT READS AT MODULE LOAD, read once at start-up here:
# Environment=SUMMRISE_RELAY_HEADER_TIMEOUT_MS=30000

[Install]
WantedBy=multi-user.target
UNIT
  sudo systemctl daemon-reload
  sudo systemctl enable --now vrelay-rs
  sleep 1
  systemctl is-active vrelay-rs
"

say "=== [smoke] the NEW relay, on its own port, before anything depends on it ==="
code=$(ssh -i "$KEY" -o StrictHostKeyChecking=no "ubuntu@$HOST" \
  "curl -s -m 10 -o /dev/null -w '%{http_code}' -X POST http://127.0.0.1:$PORT/api/proxy" || echo 000)
[ "$code" = 401 ] || { say "  !! expected 401 from the BYOK gate, got $code — check: ssh ubuntu@$HOST 'journalctl -u vrelay-rs -n'" >&2; exit 1; }
say "  ok: the Rust relay answers the BYOK gate with 401 on 127.0.0.1:$PORT"

say ""
say "NOTHING HAS BEEN CUT OVER. nginx still points /api/ at the Node relay on 8081."
say "The switch, and the rollback, are each one line of nginx — see this file's header."
