// A REPLY MUST NOT DECLARE A LENGTH OR AN ENCODING ITS BODY DOES NOT HAVE.
//
// MEASURED, and this is why the test drives a real gzip upstream rather than constructing a Response by
// hand: Node 24's `fetch` TRANSPARENTLY DECOMPRESSES the body and leaves `content-encoding` and
// `content-length` exactly as the upstream sent them. An upstream answering 10,000 bytes gzipped to 45
// reported BOTH headers while `response.body` yielded all 10,000 decoded bytes — so forwarding them made
// the reply a lie, and a client trusting either one truncates the body or fails to decode it.
//
// It was reachable in production: `forwardHeaders` passes the CALLER's `accept-encoding` upstream, so a
// browser asking for gzip is exactly the case that produces this (round 126 of the standing goal, from the
// eleventh exploration).
import { test } from "node:test";
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { gzipSync } from "node:zlib";
import { collectResponseHeaders } from "../routing.mjs";

const BODY = "x".repeat(10_000);

function gzipUpstream() {
  const gz = gzipSync(BODY);
  return new Promise((resolve) => {
    const srv = createServer((_req, res) => {
      res.writeHead(200, {
        "content-encoding": "gzip",
        "content-length": String(gz.length),
        "content-type": "text/plain",
      });
      res.end(gz);
    });
    srv.listen(0, "127.0.0.1", () =>
      resolve({ url: `http://127.0.0.1:${srv.address().port}/`, close: () => srv.close(), compressed: gz.length }),
    );
  });
}

test("a decompressed body is not labelled with the upstream's length or encoding", async () => {
  const up = await gzipUpstream();
  try {
    const response = await fetch(up.url);
    const text = await response.text();
    assert.equal(text.length, BODY.length, "fetch really does decompress — otherwise this test proves nothing");

    const out = collectResponseHeaders(response);
    assert.equal(out["content-encoding"], undefined, "the body is no longer gzip; claiming it is breaks clients");
    assert.equal(
      out["content-length"],
      undefined,
      `the upstream declared ${up.compressed} compressed bytes and we relay ${text.length} — the header would truncate the reply`,
    );
    // And the headers that are still TRUE must survive, or the fix has thrown away the reply.
    assert.equal(out["content-type"], "text/plain");
  } finally {
    up.close();
  }
});
