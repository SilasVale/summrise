// bindings-stub.mjs — THE R2 BUCKET, THE DURABLE OBJECT NAMESPACE, THE CLOCK AND THE RANDOMNESS.
//
// ── WHY THIS IS SHARED RATHER THAN WRITTEN TWICE ────────────────────────────────────────────────
//
// Two things have to see the SAME world for a differential to mean anything: the SHIPPING JavaScript
// worker (driven by `relay/worker/record-worker.mjs`) and the BUILT Rust worker (driven by
// `run-cases.mjs`). Both are handed the stubs below, so a difference in the answers is a difference in
// the implementations and never in their surroundings — which is the failure mode the satellites'
// harness recorded the hard way ("a difference in the harness that looks exactly like a difference in
// the port").
//
// THREE THINGS A WORKER'S ENVIRONMENT HAS THAT NODE DOES NOT, and all three are decisions here:
//
//   * **R2** — `TEMP_FILES.put/get/delete` over an in-memory map. Every operation is RECORDED, because
//     the sequence is the contract: the upload arm must put exactly once with the right metadata, and
//     the claim arm's one-time semantics are a get followed by a delete. A response-only comparison
//     cannot see any of that.
//   * **THE DURABLE OBJECT** — `TEMP_CLAIM`. The worker only forwards to it, so the stub answers a
//     canned response and records the forwarded request (method, path, headers). **BOTH JS SHAPES ARE
//     PROVIDED** (`idFromName`+`get` for the shipping worker, `getByName` for workers-rs, which calls
//     that method directly) — a stub missing one of them would look like a port that never reaches the
//     DO at all.
//   * **THE CLOCK AND THE RANDOMNESS** — `Date.now()` is in the response body (`expiresAt`) and in the
//     object's metadata, and `genToken(22)` is in the response body AND in the R2 key. Left real, every
//     case would differ on every run; fixed here, the token derivation itself becomes comparable.
//
// Usage:
//   const { installBindings, log } = await import(".../bindings-stub.mjs");
//   const env = installBindings(spec);          // spec.bindings describes the world
//   ...drive the worker...
//   log.r2, log.forwarded                       // what it did to that world

/** The spec's `bindings` section, with the defaults a case may leave out. */
function world(spec) {
  const b = spec.bindings || {};
  return {
    objects: b.r2?.objects || {},
    r2Throws: b.r2?.throws || null,
    doResponse: b.do?.response || { status: 200, headers: {}, body: "" },
    doThrows: b.do?.throws || null,
    nowMs: typeof b.nowMs === "number" ? b.nowMs : 1_700_000_000_000,
    tokenBytes: b.tokenBytes || null,
  };
}

/**
 * Install the world and return the `env` the worker is driven with. **GLOBALS ARE REPLACED, NOT
 * WRAPPED** — `Date.now` and `crypto.getRandomValues` — so this must run before the worker is imported
 * or instantiated, and one process serves one spec.
 */
export function installBindings(spec) {
  const w = world(spec);
  const log = { r2: [], forwarded: [] };
  const objects = new Map(
    Object.entries(w.objects).map(([k, v]) => [
      k,
      {
        body: v.body ?? "",
        contentType: v.contentType ?? "application/octet-stream",
        contentDisposition: v.contentDisposition ?? null,
        customMetadata: v.customMetadata ?? {},
        size: Buffer.byteLength(v.body ?? "", "utf8"),
      },
    ]),
  );

  // ── the clock ─────────────────────────────────────────────────────────────────────────────────
  const RealDate = Date;
  const fixedNow = w.nowMs;
  class FixedDate extends RealDate {
    constructor(...args) {
      super(...(args.length ? args : [fixedNow]));
    }
    static now() {
      return fixedNow;
    }
  }
  globalThis.Date = FixedDate;

  // ── the randomness ────────────────────────────────────────────────────────────────────────────
  // `genToken` draws 32 bytes at a time and keeps those below 248. A REPEATING BYTE STREAM makes the
  // token a function of the case, so the recorded token is comparable — and a stream chosen to include
  // 248..=255 exercises the rejection sampling through the real code path.
  const stream = w.tokenBytes ? Buffer.from(w.tokenBytes) : null;
  let cursor = 0;
  const realCrypto = globalThis.crypto;
  Object.defineProperty(globalThis, "crypto", {
    configurable: true,
    value: {
      subtle: realCrypto.subtle,
      randomUUID: () => realCrypto.randomUUID(),
      getRandomValues: (buf) => {
        if (!stream) return realCrypto.getRandomValues(buf);
        for (let i = 0; i < buf.length; i++) {
          buf[i] = stream[cursor++ % stream.length];
        }
        return buf;
      },
    },
  });

  // ── R2 ────────────────────────────────────────────────────────────────────────────────────────
  //
  // **THE CLASS NAME IS PART OF THE CONTRACT.** workers-rs does not duck-type a binding: it reads
  // `obj.constructor().name` and compares it to the type it wants (`EnvBinding::get` in the crate), so a
  // plain object literal is refused with *"Binding cannot be cast to the type R2Bucket from Object"* —
  // which is what the first run of the file relay's differential answered for every upload that reached
  // R2. The runtime's own bindings are instances of classes with these names, so the stubs are too.
  class R2Bucket {
    async put(key, value, opts = {}) {
      if (w.r2Throws === "put") throw new Error("stub: R2 put failed");
      // The value is a File (multipart) or a ReadableStream (raw). Both read through Response.
      const body = Buffer.from(await new Response(value).arrayBuffer());
      log.r2.push({
        op: "put",
        key,
        size: body.length,
        contentType: opts.httpMetadata?.contentType ?? null,
        contentDisposition: opts.httpMetadata?.contentDisposition ?? null,
        expiresAt: opts.customMetadata?.expiresAt ?? null,
      });
      objects.set(key, {
        body: body.toString("utf8"),
        contentType: opts.httpMetadata?.contentType ?? "application/octet-stream",
        contentDisposition: opts.httpMetadata?.contentDisposition ?? null,
        customMetadata: opts.customMetadata ?? {},
        size: body.length,
      });
      // R2's put resolves with the object's authoritative size, which the raw arm reports.
      return { size: body.length };
    }
    async get(key) {
      if (w.r2Throws === "get") throw new Error("stub: R2 get failed");
      log.r2.push({ op: "get", key });
      const o = objects.get(key);
      if (!o) return null;
      return {
        body: o.body,
        size: o.size,
        httpMetadata: { contentType: o.contentType, contentDisposition: o.contentDisposition },
        customMetadata: o.customMetadata,
      };
    }
    async delete(key) {
      if (w.r2Throws === "delete") throw new Error("stub: R2 delete failed");
      log.r2.push({ op: "delete", key });
      objects.delete(key);
    }
  }
  const TEMP_FILES = new R2Bucket();

  // ── the Durable Object namespace ──────────────────────────────────────────────────────────────
  const record = (name, request) => {
    log.forwarded.push({
      name,
      method: request.method,
      path: new URL(request.url).pathname,
      headers: [...request.headers.entries()]
        .map(([k, v]) => `${k.toLowerCase()}: ${v}`)
        .sort(),
    });
  };
  const stubFor = (name) => ({
    async fetch(request) {
      record(name, request);
      if (w.doThrows) throw new Error("stub: DO unreachable");
      return new Response(w.doResponse.body, {
        status: w.doResponse.status,
        headers: w.doResponse.headers || {},
      });
    },
  });
  // **AND THE SAME CLASS-NAME CONTRACT FOR THE NAMESPACE** (`DurableObjectNamespace`), with BOTH JS
  // SHAPES: `idFromName`+`get` is what the shipping worker calls, `getByName` is what workers-rs calls
  // directly — a stub missing one of them would look like a port that never reaches the DO at all.
  class DurableObjectNamespace {
    idFromName(name) {
      return { name, toString: () => name };
    }
    get(id) {
      return stubFor(typeof id === "string" ? id : id.name);
    }
    getByName(name) {
      return stubFor(name);
    }
  }
  const TEMP_CLAIM = new DurableObjectNamespace();

  return { env: { TEMP_FILES, TEMP_CLAIM }, log };
}

/**
 * Build the request a case describes — **THE SAME CONSTRUCTION FOR BOTH DRIVERS**, which is the point:
 * a multipart body, a raw stream, an absent `content-length` and an oversized declared one are all part
 * of what the shipping worker decides on, so a case that built them differently on the two sides would
 * be comparing the harnesses.
 *
 * `body.kind`:
 *   "form"   — multipart/form-data with one `file` part (`file.name`, `file.type`, `file.bytes`)
 *   "stream" — a raw body stream (`bytes`), which is the PUT arm
 *   absent   — no body
 * `contentLength` overrides the header, which is how the screening arms are reached: undici sets no
 * `content-length` for a FormData body, and the shipping worker answers 411 without one.
 */
export function buildRequest(host, c) {
  const headers = { ...(c.headers || {}) };
  const init = { method: c.method, headers };
  if (c.body?.kind === "form") {
    const form = new FormData();
    const f = c.body.file || {};
    form.append(
      // The field name is a case knob: the "no file field" arm needs a well-formed multipart body whose
      // part is called something else.
      c.body.field ?? "file",
      new File([Buffer.from(f.bytes ?? "", "utf8")], f.name ?? "file", {
        type: f.type ?? "application/octet-stream",
      }),
    );
    init.body = form;
  } else if (c.body?.kind === "stream") {
    init.body = new Blob([Buffer.from(c.body.bytes ?? "", "utf8")]).stream();
    init.duplex = "half";
  }
  if (c.contentLength !== undefined) {
    headers["content-length"] = String(c.contentLength);
  }
  return new Request(host + c.path, init);
}
