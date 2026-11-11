#!/usr/bin/env python3
"""
How many connections can one forward rule carry AT ONCE, and what does each one cost?

**THE QUESTION THIS ANSWERS IS NOT "HOW FAST IS IT".** The design's §6 leaves one measurement open
(it says "measure the same six-connection starvation under each" TLS front end, and no front end
exists yet). What DOES exist is this build, and its own shape raises a question the design never
asked: `carry()` calls `ssh::open` for EVERY client connection, so one rule is one sshd handshake per
connection carried. An sshd's `MaxStartups` (10:30:100 by default: after ten UNauthenticated
connections, 30% are dropped) is then a wall in front of a page that opens six connections per host.

So this drives the real device binary through the real MCP surface, and measures:
  1. how many connections complete when N are opened AT ONCE, for N = 4, 6, 12, 24;
  2. how long one connection takes when it is alone (the price of the handshake);
  3. what the device's own audit says when a connection is refused.

It prints a table; a human reads it. It asserts nothing, because the numbers are the finding.
"""

import json
import os
import random
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time
import urllib.request

CONTENT = b"CONCURRENCY-MARKER"
TARGET_ANSWERS = (
    b"HTTP/1.1 200 OK\r\nContent-Length: %d\r\nConnection: close\r\n\r\n%s"
    % (len(CONTENT), CONTENT)
)


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
        except Exception as error:  # noqa: BLE001 - the reason is only for the failure message
            last = str(error)
        time.sleep(0.05)
    raise SystemExit(f"the device never became healthy: {last}")


class MCP:
    """A minimal streamable-HTTP MCP client: one POST per call, which is all this measurement needs."""

    def __init__(self, base: str, token: str):
        self.base = base
        self.token = token
        self.session = None

    def _post(self, payload, extra=None):
        body = json.dumps(payload).encode()
        headers = {
            "content-type": "application/json",
            "accept": "application/json, text/event-stream",
            "authorization": f"Bearer {self.token}",
        }
        if self.session:
            headers["mcp-session-id"] = self.session
        if extra:
            headers.update(extra)
        request = urllib.request.Request(
            self.base + "/mcp", data=body, headers=headers, method="POST"
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
                    "clientInfo": {"name": "measure", "version": "0"},
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
        # THE BODY IS AN EVENT STREAM, EVEN FOR ONE ANSWER. The first frames carry no `data:` payload
        # (`id:`, `retry:`), so the FIRST `data:` line with JSON is the answer — and the frame after
        # it is not a second answer. A client that took the first `data:` line unconditionally would
        # parse an empty string on some frames; one that took the whole body would parse a stream.
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
        if payload is None:
            raise RuntimeError(f"no JSON-RPC answer in {raw[:400]!r}")
        return payload


def one_connection(port: int) -> tuple[bool, float]:
    """Open a connection to the rule's listen address, read the marker, and time the whole thing."""
    started = time.time()
    try:
        sock = socket.create_connection(("127.0.0.1", port), timeout=30)
    except OSError:
        return False, time.time() - started
    try:
        sock.sendall(b"GET / HTTP/1.1\r\nHost: measure\r\nConnection: close\r\n\r\n")
        body = b""
        while True:
            chunk = sock.recv(4096)
            if not chunk:
                break
            body += chunk
        ok = CONTENT in body
        return ok, time.time() - started
    except OSError:
        return False, time.time() - started
    finally:
        sock.close()


def burst(port: int, count: int) -> tuple[int, float]:
    """Open `count` connections at once; return how many carried and the span the burst took."""
    results: list[bool] = []
    lock = threading.Lock()

    def run():
        ok, _ = one_connection(port)
        with lock:
            results.append(ok)

    threads = [threading.Thread(target=run) for _ in range(count)]
    started = time.time()
    for thread in threads:
        thread.start()
    for thread in threads:
        thread.join()
    return sum(1 for ok in results if ok), time.time() - started


def control() -> float:
    """
    THE SAME REQUEST WITH NO FORWARD IN IT.

    Without this, the median above could be the HTTP request's own cost and the handshake would be
    invisible. Measured on this box: a direct loopback request is ~0 ms, so the difference IS the
    forward's price — one SSH handshake per connection, which is the finding.
    """
    port = free_port()
    stop = threading.Event()

    def serve():
        listener = socket.socket()
        listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        listener.bind(("127.0.0.1", port))
        listener.listen(64)
        listener.settimeout(0.5)
        while not stop.is_set():
            try:
                client, _ = listener.accept()
            except socket.timeout:
                continue
            except OSError:
                break

            def answer(peer=client):
                try:
                    peer.recv(4096)
                    peer.sendall(TARGET_ANSWERS)
                finally:
                    peer.close()

            threading.Thread(target=answer, daemon=True).start()
        listener.close()

    threading.Thread(target=serve, daemon=True).start()
    samples = []
    for _ in range(5):
        started = time.time()
        sock = socket.create_connection(("127.0.0.1", port), timeout=10)
        sock.sendall(b"GET / HTTP/1.1\r\nHost: control\r\nConnection: close\r\n\r\n")
        while sock.recv(4096):
            pass
        sock.close()
        samples.append((time.time() - started) * 1000)
    stop.set()
    return sorted(samples)[len(samples) // 2]


def main() -> int:
    here = os.path.abspath(os.getcwd())
    binary = os.path.join(here, "device", "target", "debug", "summrise-device")
    if not os.path.exists(binary):
        raise SystemExit(f"build it first: cargo build --manifest-path device/Cargo.toml -p summrise-device ({binary})")

    via = os.environ.get("SUMMRISE_FORWARD_VIA")
    key = os.environ.get("SUMMRISE_FORWARD_KEY")
    if not via or not key:
        raise SystemExit("SUMMRISE_FORWARD_VIA=user@host:port and SUMMRISE_FORWARD_KEY=/abs/key are required")

    # THE TARGET: a plain HTTP service on loopback, so a carried connection is a real request/answer.
    target_port = free_port()
    stop = threading.Event()

    def serve():
        listener = socket.socket()
        listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        listener.bind(("127.0.0.1", target_port))
        listener.listen(64)
        listener.settimeout(0.5)
        while not stop.is_set():
            try:
                client, _ = listener.accept()
            except socket.timeout:
                continue
            except OSError:
                break

            def answer(peer=client):
                try:
                    peer.recv(4096)
                    peer.sendall(TARGET_ANSWERS)
                except OSError:
                    pass
                finally:
                    peer.close()

            threading.Thread(target=answer, daemon=True).start()
        listener.close()

    threading.Thread(target=serve, daemon=True).start()

    work = tempfile.mkdtemp(prefix="summrise-measure-")
    install = os.path.join(work, "install")
    data = os.path.join(work, "data")
    os.makedirs(install)
    os.makedirs(data)
    token = "46" * 32
    with open(os.path.join(data, "token"), "w") as handle:
        handle.write(token + "\n")

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
        mcp = MCP(base, token)
        mcp.initialize()

        listen_port = free_port()
        added = mcp.call(
            "forward.add",
            {
                "name": "measure",
                "listen": f"127.0.0.1:{listen_port}",
                "target": f"127.0.0.1:{target_port}",
                "via": via,
                "auth": {"kind": "key", "path": key},
            },
        )
        if added.get("result", {}).get("isError"):
            print("forward.add was REFUSED:", added["result"]["content"][0]["text"])
            return 1

        deadline = time.time() + 20
        while time.time() < deadline:
            rows = mcp.call("forward.list", {})
            row = json.loads(rows["result"]["content"][0]["text"])[0]
            if row.get("listening"):
                break
            time.sleep(0.1)
        else:
            print("the rule never listened:", row)
            return 1

        # 1. THE PRICE OF ONE CONNECTION, measured alone and repeated so one slow one does not set it.
        singles = []
        for _ in range(5):
            ok, seconds = one_connection(listen_port)
            singles.append((ok, seconds))
            time.sleep(0.2)
        carried = sum(1 for ok, _ in singles if ok)
        typical = sorted(s for _, s in singles)[len(singles) // 2]

        # 2. THE WALL: how many of N AT ONCE come back carrying the marker.
        table = []
        for count in (4, 6, 12, 24):
            ok, span = burst(listen_port, count)
            table.append((count, ok, span))
            time.sleep(1.0)

        # 3. THE DEVICE'S OWN COUNT, so the numbers above are checkable against what it counted.
        rows = mcp.call("forward.list", {})
        row = json.loads(rows["result"]["content"][0]["text"])[0]

        print()
        print("ONE CONNECTION, ALONE")
        print(f"  carried {carried} of {len(singles)} in a row, never two at once")
        for ok, seconds in singles:
            print(f"    {'ok  ' if ok else 'FAIL'} {seconds * 1000:7.0f} ms")

        direct = control()

        print()
        print("WHAT THE FORWARD COSTS, AGAINST THE SAME REQUEST WITH NO FORWARD IN IT")
        print(f"  direct to the target, no forward: {direct:.0f} ms")
        print(f"  through the rule:                {typical * 1000:.0f} ms")
        print(f"  the forward's own price:         {typical * 1000 - direct:.0f} ms per connection")

        print()
        print("THE sshd THIS MEASURED AGAINST")
        try:
            ceiling = subprocess.run(
                ["sshd", "-T", "-f", os.environ.get("SUMMRISE_FORWARD_SSHD_CONFIG", "/etc/ssh/sshd_config")],
                capture_output=True, text=True, timeout=20
            ).stdout
            for line in ceiling.splitlines():
                if line.startswith(("maxstartups", "maxsessions", "maxauthtries")):
                    print("  " + line)
        except Exception as error:  # noqa: BLE001 - a missing sshd -T is not a failed measurement
            print(f"  (could not read the sshd's limits: {error})")

        print()
        print("N CONNECTIONS AT ONCE")
        print("  opened  carried  span")
        for count, ok, span in table:
            print(f"  {count:6}  {ok:7}  {span * 1000:6.0f} ms")

        print()
        print("THE DEVICE'S OWN ROW")
        print(f"  listening={row.get('listening')} connections={row.get('connections')} "
              f"attempts={row.get('attempts')} last_error={row.get('last_error')!r}")

        # 4. WHAT THE AUDIT SAYS, because a refused connection is a log line and not a count (D6).
        audit = os.path.join(data, "events.jsonl")
        if os.path.exists(audit):
            with open(audit) as handle:
                lines = [line for line in handle if "forward" in line]
            print()
            print(f"THE AUDIT: {len(lines)} line(s) mention a forward")
            for line in lines[-6:]:
                print("  " + line.strip()[:220])

        mcp.call("forward.remove", {"name": "measure"})
        return 0
    finally:
        child.terminate()
        try:
            child.wait(timeout=10)
        except subprocess.TimeoutExpired:
            child.kill()
        stop.set()
        shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main())
