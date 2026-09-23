// Summrise Agent — install / download landing page (Cloudflare Worker).
//
// This Worker is the download site for summrise-agent. Device management
// (registry + MCP config + panel proxy) lives in the Summrise console
// (admin-only). This page distributes the npm tgz (the SINGLE install/update
// channel) plus the Windows online installer (SummriseAgent-Setup.exe, NSIS,
// same npm channel underneath — bootstraps Node, installs the pinned tgz,
// runs `summrise setup`) and points users to the
// console. The console URL is set per-deployment via the CONSOLE_URL var
// (no production domain is hardcoded here).
//
// Design aligned with DeepSeek Harness (DSH) web GUI: dark-first design
// system, --dsw-alias-* tokens, 12px radius cards, layered shadows.


// Landing page (FAVICON + PAGE template + its URL-whitelist/escape
// helpers) lives in ./page.js — structure refactor, content verbatim.
import { PAGE } from "./page.js";

/// 413 envelope for uploads over the size cap — used by the claim
/// upload's content-length precheck and its post-parse size check (the
/// two checks used to inline the same Response construction).
function tooLargeResponse(maxBytes) {
  return new Response(
    JSON.stringify({ error: `file too large (max ${maxBytes} bytes)` }),
    {
      status: 413,
      headers: { "content-type": "application/json" },
    },
  );
}

// P2-9 helper: build a hardened Content-Disposition for an uploaded
// filename. Strips quotes/backslashes/controls (header-split defence),
// returns null when nothing survives (caller answers 400 — an illegal name
// must never reach the R2 put as a forged header); keeps the quoted
// filename parameter pure-ASCII and carries non-ASCII names via filename*
// (RFC 5987) with an ASCII fallback.
// Exported for direct pins (SOLID Round-30; additive — call sites untouched).
export function buildContentDisposition(rawName) {
  const cleaned = String(rawName || "")
    .replace(/["\\\u0000-\u001f\u007f]/g, "")
    .trim();
  if (!cleaned) return null;
  const ascii = cleaned.replace(/[^\x20-\x7e]/g, "").trim() || "download.bin";
  if (ascii === cleaned) return `attachment; filename="${ascii}"`;
  return `attachment; filename="${ascii}"; filename*=UTF-8''${encodeURIComponent(cleaned)}`;
}

// round-554: RAW-STREAM upload (PUT /api/upload). Same credential (verified
// by the caller), same stored shape + one-time-claim semantics as the
// multipart path — the only difference is that the body is handed to R2 as a
// stream instead of being materialized by formData() inside the 128 MB
// isolate. Filename arrives as ?name=<percent-encoded> or X-Filename and is
// reduced to its basename: a client-supplied path must never shape the stored
// Content-Disposition.
async function rawUpload(request, env, url) {
  const MAX_BYTES = 100 * 1024 * 1024;
  const declaredRaw = request.headers.get("content-length");
  if (declaredRaw === null || declaredRaw === "") {
    return new Response(JSON.stringify({ error: "content-length required" }), {
      status: 411,
      headers: { "content-type": "application/json" },
    });
  }
  const declared = Number(declaredRaw);
  if (!Number.isFinite(declared) || declared < 0) {
    return new Response(JSON.stringify({ error: "invalid content-length" }), {
      status: 400,
      headers: { "content-type": "application/json" },
    });
  }
  if (declared > MAX_BYTES) {
    return tooLargeResponse(MAX_BYTES);
  }
  const rawName =
    url.searchParams.get("name") || request.headers.get("x-filename") || "file";
  const base = String(rawName).split(/[/\\]/).pop() || "file";
  const disposition = buildContentDisposition(base);
  if (!disposition) {
    return new Response(JSON.stringify({ error: "invalid filename" }), {
      status: 400,
      headers: { "content-type": "application/json" },
    });
  }
  const token = genToken(22);
  const key = `files/${token}`;
  const expiresAt = Date.now() + 24 * 3600 * 1000;
  let stored;
  try {
    stored = await env.TEMP_FILES.put(key, request.body, {
      httpMetadata: {
        contentType:
          request.headers.get("x-content-type") || "application/octet-stream",
        contentDisposition: disposition,
      },
      customMetadata: { expiresAt: String(expiresAt) },
    });
  } catch (err) {
    // A mid-stream abort or an R2 outage must answer as JSON, never as the
    // catch-all 500 with an un-`String(err)`-formatted envelope.
    return new Response(
      JSON.stringify({ error: `r2 put failed: ${String(err)}` }),
      {
        status: 502,
        headers: { "content-type": "application/json" },
      },
    );
  }
  // R2 reports the stored object's authoritative size; fall back to the
  // declared length only if the put result lacks it.
  const size = typeof stored?.size === "number" ? stored.size : declared;
  return new Response(
    JSON.stringify({
      token,
      url: `${url.origin}/files/${token}`,
      size,
      filename: base,
      expiresAt: new Date(expiresAt).toISOString(),
      note: "one-time download: file is deleted after first access or 24h",
    }),
    { headers: { "content-type": "application/json" } },
  );
}

// P2-5: agent_update refuses unverifiable installs (round-119) — a
// truncated/placeholder sha in version.json must never be served as if it
// were a real manifest. Same shape as assert_want_sha256 in
// scripts/smoke-index.sh (the shared pre-publish guard): 64 hex chars.
// Exported for direct pins (SOLID Round-30; additive).
export const SHA256_RE = /^[0-9a-f]{64}$/i;

// The one-time claim no longer lives here: ADR 0010 D6 moved the whole file relay to
// its own worker (`summrise-relay`), which serves /files/<token> through a ROUTE and is
// called for uploads over a SERVICE BINDING. What remains true of this side: there is NO
// token store, the R2 key + its 24h customMetadata deadline IS the state, and the tokens
// the relay mints are 22 chars URL-safe.
const TOKEN_CHARS =
  "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

// Exported for direct pins (SOLID Round-30; additive — call sites untouched).
export function genToken(len = 22) {
  // P2-11: rejection sampling — TOKEN_CHARS.length (62) does not divide
  // 256, so buf%62 would overweight the first 256%62 = 8 symbols (A-H).
  // Accept only bytes in [0, 248) (largest multiple of 62 below 256) and
  // discard the rest: uniform over the alphabet at ~3% redraw cost.
  const RANGE = 256 - (256 % TOKEN_CHARS.length); // 248
  let s = "";
  while (s.length < len) {
    const buf = new Uint8Array(32);
    crypto.getRandomValues(buf);
    for (let i = 0; i < buf.length && s.length < len; i++) {
      if (buf[i] < RANGE) s += TOKEN_CHARS[buf[i] % TOKEN_CHARS.length];
    }
  }
  return s;
}

// Constant-time string equality for credential-shaped values (same intent
// as gateway/src/auth.ts safeEq/timingSafeEqual, but hardened: SHA-256 both
// sides to fixed 32-byte digests first, so there is no length early-exit
// to leak on, then fold XOR across every byte without short-circuiting).
async function safeEq(a, b) {
  const enc = new TextEncoder();
  const [da, db] = await Promise.all([
    crypto.subtle.digest("SHA-256", enc.encode(String(a))),
    crypto.subtle.digest("SHA-256", enc.encode(String(b))),
  ]);
  const a8 = new Uint8Array(da);
  const b8 = new Uint8Array(db);
  let diff = 0;
  for (let i = 0; i < a8.length; i++) diff |= a8[i] ^ b8[i];
  return diff === 0;
}

// The cloudflared release this worker proxies. MUST equal
// `agent/src/tunnel.rs`'s `CLOUDFLARED_VERSION`, whose `CLOUDFLARED_SHA256` pins the bytes
// of THIS asset — `cloudflared_pin_matches_the_agent` reads that file and fails if the two
// drift, because a drift is exactly the "installer stages unverified bytes" bug this
// constant exists to prevent.
const CLOUDFLARED_VERSION = "2026.8.3";

// A proxy that cannot deliver answers with THIS FILE'S protocol: 502 and the JSON
// envelope the upload handler established ("must answer as JSON, never as the catch-all
// 500"), and it must never be cached — a cached failure outlives the outage.
//
// Round-99 F3 found all three binary proxies returning bare text, and a REJECTED fetch
// (DNS, TLS, a GitHub 5xx storm) propagating as an unhandled rejection — i.e. the
// platform's 500 HTML page, which no device-side reader parses. electron-route.test.mjs
// had recorded that gap in its own header ("flagged to the stage-n owner, deliberately
// not cemented here"); it is cemented now.
function proxyFailure(what, detail) {
  return new Response(JSON.stringify({ error: `${what}: ${detail}` }), {
    status: 502,
    headers: {
      "content-type": "application/json",
      "cache-control": "no-store",
    },
  });
}

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    // Console URL is per-deployment (CONSOLE_URL var, see the header
    // contract above); fall back to this worker's own origin — no
    // production domain is hardcoded.
    const consoleUrl = (env && env.CONSOLE_URL) || url.origin;

    // Version endpoint for the agent_update MCP tool (and legacy tray
    // check). ROUND-297: this was hard-coded to v1.2.141/1.0.145 and rotted
    // (the 141 tgz was deleted from assets long ago — an update check that
    // ever fired would 404). The manifest is now derived from the version
    // discovery asset (/summrise-agent/version.json, written by the release
    // flow) so it tracks every release automatically. The sha256 field is
    // REQUIRED by agent_update (round-119: unverifiable installs refused)
    // and is published into version.json by the release flow.
    if (new URL(request.url).pathname === "/api/version") {
      try {
        const vresp = await env.ASSETS.fetch(
          new Request("https://worker.local/summrise-agent/version.json"),
        );
        if (vresp.ok) {
          const vj = await vresp.json();
          const ver = vj && vj.version;
          const sha = vj && vj.sha256;
          // P2-5: assert the sha shape (64 hex), not just presence — a
          // truncated/placeholder sha would otherwise ship a manifest that
          // agent_update refuses anyway; fail to the honest 503 instead.
          // P2-1: the tarball filename is LIVE data, not decoration —
          // version.json names the exact file (publish writes the
          // versionless latest alias today, a versioned name tomorrow).
          // Serve exactly that basename after a flat-name validation (no
          // slashes, must end .tgz — a hostile manifest must not escape
          // /summrise-agent/). Absent/invalid falls back to the derived
          // versioned name so older manifests keep working; smoke pins
          // the consistent case (tarball field == download basename).
          const tbRaw = vj && vj.tarball;
          const tb =
            typeof tbRaw === "string" &&
            /^summrise-agent-[A-Za-z0-9][A-Za-z0-9._-]*\.tgz$/.test(tbRaw)
              ? tbRaw
              : `summrise-agent-${ver}.tgz`;
          if (ver && typeof sha === "string" && SHA256_RE.test(sha)) {
            const base = new URL(request.url).origin;
            // Installer fields are ADDITIVE (older manifests lack them —
            // fresh-install clients treat absence as "installer unknown").
            // Same flat-name discipline as the tarball: the versioned
            // Setup-<ver>.exe shape only, never a path.
            const instRaw = vj && vj.installer;
            const instShaRaw = vj && vj.installer_sha256;
            const inst =
              typeof instRaw === "string" &&
              /^SummriseAgent-Setup-[0-9]+\.[0-9]+\.[0-9]+\.exe$/.test(instRaw)
                ? instRaw
                : null;
            const instSha =
              typeof instShaRaw === "string" && SHA256_RE.test(instShaRaw)
                ? instShaRaw
                : null;
            const body = {
              version: ver,
              download: `${base}/summrise-agent/${tb}`,
              sha256: sha,
            };
            // THE BOXED COMPONENTS, PINNED (grilling Q4). `summrise setup` fetches
            // cloudflared, the playwright bundle and the electron runtime from this
            // host and verifies each against a sha256 in THIS manifest; without the
            // pins it can only warn that it verified nothing. Version.json is DATA, so
            // it gets the same discipline as every other field here: a flat basename
            // (never a path), a 64-hex digest, and the URL REBUILT against this
            // request's origin rather than echoed — a manifest must not be able to
            // point a device at another host.
            const compsIn = vj && vj.components;
            if (compsIn && typeof compsIn === "object") {
              const comps = {};
              for (const [key, val] of Object.entries(compsIn)) {
                if (!val || typeof val !== "object") continue;
                const name = val.url ? String(val.url).split("/").pop() : "";
                if (!/^[A-Za-z0-9][A-Za-z0-9._-]*$/.test(name)) continue;
                if (typeof val.sha256 !== "string" || !SHA256_RE.test(val.sha256)) {
                  continue;
                }
                comps[key] = { url: `${base}/summrise-agent/${name}`, sha256: val.sha256 };
              }
              if (Object.keys(comps).length) body.components = comps;
            }
            if (inst && instSha) {
              body.installer = `${base}/summrise-agent/${inst}`;
              body.installer_sha256 = instSha;
            }
            return new Response(JSON.stringify(body), {
              headers: {
                "content-type": "application/json",
                "cache-control": "no-store",
              },
            });
          }
        }
      } catch (e) {
        // fall through to the static fallback below
      }
      // Static fallback (assets unavailable): never serve a fabricated
      // manifest — agent_update refuses invalid sha256 anyway (round-119),
      // so an explicit error is the honest answer.
      return new Response("release manifest unavailable", { status: 503 });
    }
    const pathname = new URL(request.url).pathname;
    // Windows online installer (NSIS, same npm channel underneath): the
    // versionless alias + versioned names are served straight from ASSETS
    // (staged by scripts/build-installer.sh on every release). Exact-pattern
    // discipline like the tgz route below — a missing exe must 404 (never
    // the landing page as 200 HTML; devices once downloaded HTML as the
    // installer and the agent never started).
    const setupMatch =
      /^\/summrise-agent\/SummriseAgent-Setup-[0-9]+\.[0-9]+\.[0-9]+\.exe$/.exec(
        pathname,
      );
    if (setupMatch || pathname === "/summrise-agent/SummriseAgent-Setup.exe") {
      return env.ASSETS.fetch(request);
    }
    // npm tgz download path (the documented `npm i -g
    // https://agent.saisi.online/summrise-agent/summrise-agent-<v>.tgz` command).
    // The versionless latest alias (the landing page's install command)
    // is matched EXACTLY here — the versioned regex is intentionally NOT
    // loosened to cover it (exact-pattern discipline on download paths).
    const tgzMatch =
      /^\/summrise-agent\/summrise-agent-[0-9]+\.[0-9]+\.[0-9]+\.tgz$/.exec(pathname);
    if (tgzMatch || pathname === "/summrise-agent/summrise-agent-latest.tgz") {
      // The tgz (~12MB) fits Workers Assets and is served fast from here.
      return env.ASSETS.fetch(request);
    }
    // cloudflared.exe proxy: the boxed tunnel binary (~54MB) is NOT bundled
    // in the npm package (kept small); devices download it on demand from
    // the official GitHub release. GitHub is often unreachable from devices
    // (GFW etc.), so proxy it through this worker — Cloudflare's network
    // reaches GitHub fast, and the device only talks to agent.saisi.online.
    if (pathname === "/summrise-agent/cloudflared.exe") {
      // PINNED, NOT `latest`. This used to proxy
      // `.../releases/latest/download/cloudflared-windows-amd64.exe`, which means the bytes
      // of a binary the service SPAWNS were chosen by whatever GitHub marked latest at that
      // moment — while the agent's own `CLOUDFLARED_SHA256` pin (agent/src/tunnel.rs) only
      // holds `while latest stays` the pinned version, as that file's comment admits. So
      // upstream moving a release silently made this proxy serve bytes the pin would reject:
      // the ON-DEMAND path failed closed (good) and the INSTALLER staged the new bytes
      // unverified (bad), because nothing in that chain hashes them.
      //
      // A VERSIONED GitHub asset path is immutable, which is exactly why the agent uses it
      // for the direct download. This mirrors it, so the proxy and the pin agree BY
      // CONSTRUCTION.
      const upstream = `https://github.com/cloudflare/cloudflared/releases/download/${CLOUDFLARED_VERSION}/cloudflared-windows-amd64.exe`;
      let resp;
      try {
        resp = await fetch(upstream, { redirect: "follow" });
      } catch (err) {
        return proxyFailure("cloudflared upstream fetch failed", String(err));
      }
      if (!resp.ok) {
        return proxyFailure("cloudflared upstream fetch failed", resp.status);
      }
      // Stream the body through (no buffering — 54MB fits the response path).
      return new Response(resp.body, {
        status: 200,
        headers: {
          "content-type": "application/octet-stream",
          "content-disposition": 'attachment; filename="cloudflared.exe"',
          "cache-control": "public, max-age=3600",
        },
      });
    }
    // Electron desktop-shell binary proxy: the ~115MB win32-x64 dist is NOT
    // bundled in the npm package (kept small) and the installer must NOT rely
    // on npmjs/npmmirror — those are unreachable from many device boxes (GFW /
    // corporate firewalls), which left the desktop shell dead on fresh installs.
    // Cloudflare's edge reaches GitHub fine, so the device pulls Electron from
    // THIS worker (same origin it already reaches for the tgz + cloudflared).
    // Pinned to the version the installer's $ElectronVersion expects.
    // electron runtime: STAGED IN R2, NOT PROXIED (2026-09-23). It used to be a
    // fetch() through this worker to a PINNED upstream GitHub asset ("Cloudflare's
    // edge reaches GitHub fine"), which was the right answer while the alternative
    // was the device reaching GitHub at all. Two things changed that:
    //
    //   1. IT COULD NOT BE HASHED. The release flow writes a sha256 per component
    //      into version.json so `summrise setup` can verify what it fetched, and a
    //      proxy has no bytes to hash without downloading 234MB at build time.
    //   2. R2 already holds the playwright bundle, so this is one mechanism
    //      instead of two -- and the device's dependency on GitHub becomes ZERO
    //      rather than "the edge proxies it".
    //
    // The object was staged FROM the pinned v33.4.11 upstream asset (the version
    // `summrise setup` expects), and version.json's component digest is what pins
    // the content now -- the same pair of ideas as cloudflared's CLOUDFLARED_SHA256.
    if (pathname === "/summrise-agent/electron-win32-x64.zip") {
      let obj;
      try {
        obj = await env.TEMP_FILES.get("electron-win32-x64.zip");
      } catch (err) {
        return proxyFailure("electron runtime read failed", String(err));
      }
      if (!obj || obj.size === 0) {
        // A ZERO-BYTE OBJECT IS NOT A BUNDLE. Measured the hard way: an absent
        // object answers 502 and `summrise setup` warns; an EMPTY object answers
        // 200 and stages nothing, which is the same false-success shape this
        // suite keeps finding. Absent and empty are the same verdict here.
        return proxyFailure("electron runtime unavailable", "not in R2");
      }
      // Same validator rule as the playwright bundle below: an EXECUTED artifact
      // at a mutable key must revalidate, or a device can spend a day being handed
      // a stale archive after the object is replaced.
      const etag = obj.httpEtag;
      const validators = { etag, "cache-control": "public, no-cache" };
      if (etag && request.headers.get("if-none-match") === etag) {
        return new Response(null, { status: 304, headers: validators });
      }
      return new Response(obj.body, {
        status: 200,
        headers: {
          "content-type": "application/zip",
          "content-disposition":
            'attachment; filename="electron-win32-x64.zip"',
          "cache-control": "public, no-cache",
          ...(etag ? { etag } : {}),
        },
      });
    }
    // Playwright browser-tools bundle proxy: the ~30MB boxed node_modules
    // (@playwright/mcp + playwright-core + node.exe) is NOT bundled in the npm
    // package (kept small) and exceeds the 25MiB Workers-Assets per-file cap,
    // so it lives in R2 and streams from here. The installer downloads it
    // best-effort so `summrise setup` stages components\playwright and the browser_*
    // tools come up on fresh installs (npmjs is unreachable from many boxes).
    if (pathname === "/summrise-agent/summrise-playwright.zip") {
      // IT IS AN EXECUTED ARTIFACT AT A MUTABLE KEY (round-99 F2). This served
      // `public, max-age=86400` with NO validator, so a device could spend a day being
      // handed a stale archive after the bundle was replaced — and `summrise setup` stages
      // it into components\playwright without hashing it (the same chain round 96 found
      // for cloudflared, where the fix was a versioned immutable upstream).
      //
      // R2 gives this object's digest for free, and `no-cache` means REVALIDATE, not
      // "do not store": a matching If-None-Match costs a 304, never a second 30MB body.
      // A re-upload changes the digest, which is exactly the staleness this pins.
      let obj;
      try {
        obj = await env.TEMP_FILES.get("summrise-playwright.zip");
      } catch (err) {
        return proxyFailure("playwright bundle read failed", String(err));
      }
      if (!obj || obj.size === 0) {
        // Same rule as the electron runtime above: an EMPTY object answers 200 and
        // stages nothing, where an absent one answers 502 and warns. Absent and
        // empty are the same verdict.
        return proxyFailure("playwright bundle unavailable", "not in R2");
      }
      const etag = obj.httpEtag;
      const validators = { etag, "cache-control": "public, no-cache" };
      if (etag && request.headers.get("if-none-match") === etag) {
        return new Response(null, { status: 304, headers: validators });
      }
      return new Response(obj.body, {
        status: 200,
        headers: {
          "content-type": "application/zip",
          "content-disposition": 'attachment; filename="summrise-playwright.zip"',
          "content-length": String(obj.size),
          ...validators,
        },
      });
    }
    // A missing binary must 404, not return the download PAGE as 200 HTML —
    // devices silently downloaded HTML as SummriseAgent-Setup.exe and the agent
    // never started. Only "/" and "/index.html" render the page.
    if (pathname !== "/" && pathname !== "/index.html") {
      return new Response("Not Found", { status: 404 });
    }
    // round-319: the download page's install command pointed at the DELETED
    // 1.2.141 tgz on the Vercel mirror (v.saisi.online/dl/) — every copy-
    // paste install failed. Use the versionless latest alias served by this
    // worker itself (mirrored on every release) so the command always
    // installs the current build. The base is the request's own origin —
    // npm must hit the host that actually serves the tgz, and no production
    // domain is hardcoded.
    const installerUrl = `${url.origin}/summrise-agent/summrise-agent-latest.tgz`;
    // Windows setup.exe 别名（build-installer.sh 每次发版同步），同源、无硬编码。
    //
    // ...BUT ONLY WHEN THIS RELEASE ACTUALLY PUBLISHED ONE (round 125). A tgz-only
    // publish (the documented emergency path) leaves the versionless alias serving
    // the PREVIOUS release while /api/version advertises the new one — so linking
    // it unconditionally hands a fresh install the old build and says nothing. The
    // manifest is the only thing that knows: it carries `installer` +
    // `installer_sha256` exactly when build-installer.sh produced one for THIS
    // version. Unreadable manifest = no promise, not a guess.
    let setupUrl = null;
    try {
      const mresp = await env.ASSETS.fetch(
        new Request("https://worker.local/summrise-agent/version.json"),
      );
      if (mresp.ok) {
        const mj = await mresp.json();
        if (
          mj &&
          typeof mj.installer === "string" &&
          typeof mj.installer_sha256 === "string"
        ) {
          setupUrl = `${url.origin}/summrise-agent/SummriseAgent-Setup.exe`;
        }
      }
    } catch {
      /* no installer advertised — the npm channel below is the honest path */
    }

    return new Response(PAGE(consoleUrl, installerUrl, setupUrl), {
      headers: { "content-type": "text/html; charset=utf-8" },
    });
  },
};
