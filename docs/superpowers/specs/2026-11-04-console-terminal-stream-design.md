# The terminal stream: output pushed, not polled

**Status:** slice 1 shipped (console route + page client + fallback). Slice 2 (device-side push) is
designed here and not built. Measured 2026-11-04 against `hub/v1` in `/home/zss/summrise-product`.

## Why this exists

The operator's report was a comparison, not a bug: the legacy Electron desktop app's terminal felt
fluid, the console's does not. The cause is not the renderer.

| | legacy Electron desktop | the console page |
|---|---|---|
| distance | window loads `http://127.0.0.1:18080/desktop`; agent, ConPTY and xterm on one machine | browser → Cloudflare edge → cloudflared → console host → device channel (WebSocket) → agent → ConPTY |
| direction | output pushed by SSE `/api/events/term` | output **pulled**: one `terminal.read` long-poll per answer, a fresh request after each one |
| input | every keystroke sent at once | 25 ms batch, then one `terminal.write` |
| measured echo | ~1 ms + one frame | 104.5–106.4 ms at a simulated 25 ms round trip |

The same xterm in both. In a local harness (`/tmp/console-probe.mjs`, a mock console with a 25 ms
per-call delay) frame pacing stayed at 16.7 ms p50 under a 400 KB burst, so the browser is not the
bottleneck: the gap is the distance and the pull.

**The ceiling is honest.** A public console cannot have loopback. What it can have is one round trip
per keystroke (write up, bytes pushed back down) and continuous output while the device produces it,
instead of a request per chunk with a gap between chunks.

## What slice 1 is

`GET /terminal/:device/:session/stream?from=N`, an SSE route on the console, plus a page client.

- **Auth** is the operator session (`OperatorSession`: cookie, bearer, or Access identity) — the same
  credential `/mcp` takes. A stream is an operator reading a device, nothing more.
- **Events.** `output` carries `{data, cursor}` — exactly the text `terminal.read` would have
  answered. `end` carries `{cursor, exit_code, signal}`, the device's own end facts. `fault` carries
  `{sentence, status}`, the refusal a failed read produced.
- **The cursor belongs to the page.** It sends where its terminal ends, so a reconnect replays only
  what was missed and never duplicates output. The page therefore never lets `EventSource`
  auto-reconnect: its retry reuses the original URL, which would ask from a stale cursor.
- **It is a read, not a new capability.** Each stream iteration calls the same `terminal.read` with
  the same operator scope, so the device's authorization and audit are unchanged.
- **`wait_ms` fallback.** A device from before `wait_ms` refuses the field; the stream drops it once
  and paces its reads at 120 ms instead of spinning on an instant empty answer.
- **Bounds.** `MAX_TERMINAL_STREAMS` (64) slots, held by a `Drop` guard; each stream has at most one
  device read in flight, bounded by the channel's own 30 s reply timeout; an idle stream sends an SSE
  keep-alive every 15 s.
- **Proxy buffering.** The route sends `x-accel-buffering: no`; an nginx in front of the console
  buffers proxied responses by default, which would hold every event until the stream ended.

**The page keeps the poll as a fallback**, and that is a requirement, not a courtesy: a console older
than this route answers 404 and an `EventSource` cannot see the status, jsdom has no `EventSource`,
and a stream that keeps failing must still be a terminal. Two failed attempts is where the page
starts polling again.

**Input got the other half of the round trip back.** The write batcher's 25 ms timer never merged a
normal typist's keys (they are 100–200 ms apart) — it only added 25 ms to every echo. Input now
leaves at once, and anything typed while a write is in flight is accumulated and sent as one call
when it lands, which preserves order and still merges a burst. The output pump writes a small chunk
in the same turn; only a large one waits for a frame.

Measured with the page build changed and nothing else: echo 104.5–106.4 ms → 86.4–88.0 ms at the
same simulated 25 ms round trip.

## What slice 1 does not fix, and why slice 2 exists

The console still **polls the device**: the stream parks one `terminal.read` at a time, so between two
answers the device has no reader, and the gap is the console→device round trip. On a device on the
same network that is a few milliseconds; over a relay in another city it is the whole remaining
stutter.

Slice 2 makes the device push, over the channel it already holds:

- The device's host frame grows a capability, e.g. `"caps": ["terminal.stream"]`, so the console can
  tell an old device from a new one without guessing from a version string.
- The console sends `{"id": N, "subscribe": {"session_id": "...", "from": M}, <grant|token>}`. The
  device admits it as the `terminal.read` it is, then runs a follow loop on the session's existing
  notifier and sends `{"term": {"sub": N, "data_base64": "...", "cursor": C, "alive": true}}` frames
  unprompted — the same shape the host report already uses for device-initiated frames.
- The console prefers the subscription when the device advertises it, and keeps the parked-read pump
  otherwise. **The page does not change**: it subscribes to the same route and gets the same events.

## Alternatives considered

- **Keep the pull and tune it.** Cheaper, and it does not move the gap: the browser still asks for
  every chunk, and the device still has no reader between two of them.
- **A WebSocket per session instead of SSE.** More machinery (a second upgrade path, its own auth,
  its own reconnect) for the same one-way stream. SSE is what the device's own panel already uses.
- **Device push from the start, no console stream.** Correct end state, but it would have needed a
  device update before the operator saw any improvement at all. The console stream ships today and
  makes slice 2 a change behind the same route.

## Verification

- `cargo test -p summrise-console` — the six stream tests drive a real listener and a fake device:
  output down one connection, the `wait_ms` fallback, the session's end facts, a refused read as a
  fault, and 401 without a session.
- `npm test` in `device/console/web` — the stream client's path, events, close semantics and the
  no-`EventSource` fallback; the write batcher's ordering and burst merge.
- `npm run gate:browser` — a real browser signs in, opens a pty and reads a typed marker back **down
  the stream** (the gate fails if the page never subscribed).
- `/tmp/console-probe.mjs` — the echo numbers above, before and after, with the same mock.
