#!/usr/bin/env python3
"""
Is `RULES_MAX = 4` a RESOURCE limit or a LEGIBILITY limit?

The design's §6 says the cap exists "so a device's forward table stays a list somebody can read and
revoke", and that "four is chosen, not derived". This script settles which of those is true, because
they have different answers:

  * if the cap is RESOURCE-bound, the number is a measurement and can be derived;
  * if it is LEGIBILITY-bound, no measurement can move it and the design's sentence is the whole
    justification — which is a finding, not a defect.

It measures what each rule actually costs the device process, and what happens at the wall:

  1. descriptors and RSS before any rule, and after each one this device accepts;
  2. what each rule adds: one listening socket, and NOTHING ELSE while idle (the SSH connection is
     opened per client connection, not held — so an idle rule costs one fd and one row);
  3. the cap's own sentence, taken from the device rather than from the source;
  4. whether an EDIT at the cap is refused (the design says it is not, and that is what makes a full
     device maintainable).

Run it against a real device binary; it prints a table and asserts nothing.
"""

import json
import os
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time
import urllib.request

TOKEN = "46" * 32


def free_port() -> int:
    sock = socket.socket()
    sock.bind(("127.0.0.1", 0))
    port = sock.getsockname()[1]
    sock.close()
    return port


def wait_http(url: str, timeout: float = 25.0) -> None:
    deadline = time.time() + timeout
    last = ""
    while time.time() < deadline:
        try:
            with urllib.request.urlopen(url, timeout=2) as answer:
                if answer.status == 200:
                    return
        except Exception as error:  # noqa: BLE001 - only for the failure sentence
            last = str(error)
        time.sleep(0.05)
    raise SystemExit(f"the device never became healthy: {last}")


class MCP:
    def __init__(self, base: str):
        self.base = base
        self.session = None

    def _post(self, payload):
        headers = {
            "content-type": "application/json",
            "accept": "application/json, text/event-stream",
            "authorization": f"Bearer {TOKEN}",
        }
        if self.session:
            headers["mcp-session-id"] = self.session
        request = urllib.request.Request(
            self.base + "/mcp", data=json.dumps(payload).encode(), headers=headers, method="POST"
        )
        return urllib.request.urlopen(request, timeout=30)

    def initialize(self):
        answer = self._post(
            {
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": {"name": "cap-measure", "version": "0"},
                },
            }
        )
        self.session = answer.headers.get("mcp-session-id")
        answer.read()
        self._post({"jsonrpc": "2.0", "method": "notifications/initialized"})

    def call(self, tool: str, arguments: dict) -> dict:
        answer = self._post(
            {
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/call",
                "params": {"name": tool, "arguments": arguments},
            }
        )
        raw = answer.read().decode()
        payload = None
        for line in raw.splitlines():
            if not line.startswith("data:"):
                continue
            candidate = line[5:].strip()
            if not candidate:
                continue
            try:
                payload = json.loads(candidate)
            except json.JSONDecodeError:
                continue
            if isinstance(payload, dict) and ("result" in payload or "error" in payload):
                break
        return payload


def text_of(answer: dict) -> str:
    result = answer.get("result", {})
    content = result.get("content") or []
    return content[0]["text"] if content else json.dumps(result)


def refused(answer: dict) -> bool:
    return bool(answer.get("result", {}).get("isError"))


def fds(pid: int) -> int:
    try:
        return len(os.listdir(f"/proc/{pid}/fd"))
    except OSError:
        return -1


def rss_kb(pid: int) -> int:
    try:
        with open(f"/proc/{pid}/status") as handle:
            for line in handle:
                if line.startswith("VmRSS:"):
                    return int(line.split()[1])
    except OSError:
        pass
    return -1


def main() -> int:
    here = os.path.abspath(os.getcwd())
    binary = os.path.join(here, "device", "target", "debug", "summrise-device")
    if not os.path.exists(binary):
        raise SystemExit(f"build it first: cargo build --manifest-path device/Cargo.toml -p summrise-device")

    work = tempfile.mkdtemp(prefix="summrise-cap-")
    install = os.path.join(work, "install")
    data = os.path.join(work, "data")
    os.makedirs(install)
    os.makedirs(data)
    with open(os.path.join(data, "token"), "w") as handle:
        handle.write(TOKEN + "\n")

    port = free_port()
    logs = os.path.join(work, "logs")
    os.makedirs(logs)
    child = subprocess.Popen(
        [binary, "--install", install, "--data", data, "--port", str(port)],
        stdin=subprocess.DEVNULL,
        stdout=open(os.path.join(logs, "out"), "w"),
        stderr=open(os.path.join(logs, "err"), "w"),
    )
    base = f"http://127.0.0.1:{port}"
    try:
        wait_http(base + "/api/health")
        mcp = MCP(base)
        mcp.initialize()

        print()
        print("WHAT EACH IDLE RULE COSTS THE DEVICE PROCESS")
        print(f"  before any rule:  fd={fds(child.pid):3}  rss={rss_kb(child.pid):6} kB")

        # ONE TARGET the rules point at; it is never connected to, so nothing here is a carried
        # connection's cost — this is the price of a rule EXISTING.
        target_port = free_port()
        accepted = []
        for index in range(1, 10):
            listen_port = free_port()
            answer = mcp.call(
                "forward.add",
                {
                    "name": f"rule{index}",
                    "listen": f"127.0.0.1:{listen_port}",
                    "target": f"127.0.0.1:{target_port}",
                    "via": "unused@127.0.0.1:22",
                    "auth": {"kind": "password", "password": "unused"},
                },
            )
            if refused(answer):
                print()
                print(f"  rule {index} REFUSED; the device said:")
                print(f"    {text_of(answer)}")
                break
            # Give the executor a moment to bind before the next fd is read.
            deadline = time.time() + 5
            while time.time() < deadline:
                rows = json.loads(text_of(mcp.call("forward.list", {})))
                row = [r for r in rows if r["name"] == f"rule{index}"]
                if row and row[0]["listening"]:
                    break
                time.sleep(0.05)
            accepted.append(index)
            print(f"  after rule {index}:      fd={fds(child.pid):3}  rss={rss_kb(child.pid):6} kB")

        print()
        print(f"  rules accepted: {len(accepted)}")

        # THE EDIT AT THE CAP: the design says a full device stays maintainable, so a change to an
        # EXISTING name must not be refused by the cap.
        if accepted:
            listen_port = free_port()
            edited = mcp.call(
                "forward.add",
                {
                    "name": accepted and f"rule{accepted[0]}",
                    "listen": f"127.0.0.1:{listen_port}",
                    "target": f"127.0.0.1:{target_port}",
                    "via": "unused@127.0.0.1:22",
                    "auth": {"kind": "password", "password": "unused"},
                },
            )
            print()
            print("AN EDIT AT THE CAP (the same name, a different address)")
            print(f"  refused: {refused(edited)}")
            if refused(edited):
                print(f"  the device said: {text_of(edited)}")

        # WHAT THE ROW COSTS WHILE IDLE, stated rather than inferred: an idle rule holds ONE listening
        # socket and opens nothing else, which is why the cap cannot be a descriptor limit.
        rows = json.loads(text_of(mcp.call("forward.list", {})))
        print()
        print(f"  rows on the device: {len(rows)}; each row's own state: "
              + ", ".join(f"{r['name']}=listening:{r['listening']}/conn:{r['connections']}" for r in rows))



        # ── AND WHAT THE CAP DOES NOT BOUND ──────────────────────────────────────────────────────
        # `RULES_MAX` bounds the number of ROWS. Each carried connection is its own SSH session and
        # its own pair of sockets, so the load per rule is not bounded by this number at all. This
        # holds `via` unreachable on purpose: every connection then sits in `ssh::open` until its
        # 30 s timeout, which is the WORST case for the device's descriptor table and needs no sshd.
        # THE TABLE IS FULL BY HERE, so one rule has to go first — and a skipped section that said
        # nothing would read as a measurement that was not needed rather than one that was not run.
        mcp.call("forward.remove", {"name": f"rule{accepted[-1]}"})
        # A `via` THAT ACCEPTS AND NEVER SPEAKS. Port 1 refuses instantly, which made every
        # connection fail in microseconds and held nothing — measured: descriptors did not move,
        # so that was the wrong fixture and the section was reporting a zero it had caused.
        silent = socket.socket()
        silent.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        silent.bind(("127.0.0.1", 0))
        silent.listen(128)
        silent_port = silent.getsockname()[1]
        held_peers = []

        def accept_silently():
            while True:
                try:
                    peer, _ = silent.accept()
                except OSError:
                    return
                held_peers.append(peer)

        threading.Thread(target=accept_silently, daemon=True).start()

        hold_port = free_port()
        held = mcp.call(
            "forward.add",
            {
                "name": "hold",
                "listen": f"127.0.0.1:{hold_port}",
                "target": f"127.0.0.1:{free_port()}",
                "via": f"nobody@127.0.0.1:{silent_port}",
                "auth": {"kind": "password", "password": "unused"},
            },
        )
        if refused(held):
            print()
            print("WHAT THE CAP DOES NOT BOUND: NOT MEASURED — the device refused the test rule:")
            print(f"  {text_of(held)}")
        else:
            deadline = time.time() + 5
            while time.time() < deadline:
                rows = json.loads(text_of(mcp.call("forward.list", {})))
                if [r for r in rows if r["name"] == "hold" and r["listening"]]:
                    break
                time.sleep(0.05)

            opened = 64
            sockets_held = []
            lock = threading.Lock()

            def hold_one():
                try:
                    sock = socket.create_connection(("127.0.0.1", hold_port), timeout=40)
                except OSError:
                    return
                with lock:
                    sockets_held.append(sock)

            threads = [threading.Thread(target=hold_one) for _ in range(opened)]
            before = fds(child.pid)
            for thread in threads:
                thread.start()
            # Long enough for every accept to happen and each carry to reach `ssh::open`.
            time.sleep(3)
            peak = fds(child.pid)
            print()
            print("WHAT THE CAP DOES NOT BOUND: connections held on ONE rule")
            print(f"  one rule ('hold') accepts {opened} at once; its `via` accepts and never speaks")
            print(f"  device descriptors before: {before}   while they wait: {peak}"
                  f"   (+{peak - before} for {opened} connections = {(peak - before) / opened:.1f} each)")
            print("  each waiting connection is one client socket plus one SSH session attempt,")
            print("  held until OPEN_TIMEOUT_MS = 30 s or the far side answers —")
            print("  **so the number of RULES is capped and the number of CONNECTIONS is not.**")
            with lock:
                for sock in sockets_held:
                    sock.close()
            for peer in held_peers:
                peer.close()
            silent.close()
            mcp.call("forward.remove", {"name": "hold"})

        return 0
    finally:
        child.terminate()
        try:
            child.wait(timeout=10)
        except subprocess.TimeoutExpired:
            child.kill()
        shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main())
