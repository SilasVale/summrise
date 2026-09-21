// Device-probe classification pins (SOLID Round-31 — cachedDeviceProbe
// exported additively; plugin wiring untouched). The tunnel/agent split
// decides what the console health card shows, and its two historical bugs
// (round-98: cache never hit; round-101: down devices showed tunnel_up)
// were both silent misclassifications — exactly what direct pins prevent.
// deviceFetch dials through the stubbed global fetch; distinct device
// names isolate the 30s module cache per case (no sleeps).
import test from "node:test";
import assert from "node:assert/strict";
import { cachedDeviceProbe } from "../src/plugins/mcp.ts";
import { withFetch, assertFetchCalls } from "./helpers.mjs";

const dev = (name) => ({ name, hostname: "d1.agent.saisi.online", token: "tok-device-1" });
const statusJson = (obj) =>
  new Response(JSON.stringify(obj), { status: 200, headers: { "content-type": "application/json" } });

test("tunnel down (fetch throws) → tunnel/agent false, no version", async () => {
  const p = await withFetch(async () => {
    throw new TypeError("fetch failed");
  }, () => cachedDeviceProbe({}, dev("r31-down")));
  assert.equal(p.tunnel, false, "round-101: unreachable tunnel must read false");
  assert.equal(p.agent, false);
  assert.equal(p.version, undefined);
});

test("the device's OWN update verdict is forwarded, and its absence is survivable", async () => {
  // ROUND 29 OF THE STANDING GOAL. The console re-derived "is this device behind?" from a KV copy that can be an hour
  // old and ignores the rollback pin, while the device answers the same question at /api/update with newer() plus the
  // pin. The probe now carries the device's own answer; a device that does not give one must leave the field absent
  // so the console can degrade to its old comparison instead of showing a verdict nobody computed.
  const calls = [];
  const withVerdict = await withFetch(
    async (url) => {
      calls.push(String(url));
      if (String(url).includes("/api/update")) {
        return statusJson({ current: "1.2.436", latest: "1.2.437", update_available: true, pinned_to: "1.2.436" });
      }
      return statusJson({ release: "1.2.436" });
    },
    () => cachedDeviceProbe({}, dev("r29-verdict")),
  );
  assert.deepEqual(
    withVerdict.update,
    { current: "1.2.436", latest: "1.2.437", update_available: true, pinned_to: "1.2.436" },
    "the device's verdict travels unchanged, pin included",
  );
  assert.equal(calls.filter((u) => u.includes("/api/update")).length, 1, "one update call per probe");

  const withoutVerdict = await withFetch(
    async (url) => (String(url).includes("/api/update") ? new Response("nope", { status: 404 }) : statusJson({ release: "1.2.436" })),
    () => cachedDeviceProbe({}, dev("r29-noverdict")),
  );
  assert.equal(withoutVerdict.update, undefined, "an agent without /api/update leaves the field absent");
  assert.equal(withoutVerdict.agent, true, "and the probe still reports the device as up");

  const down = await withFetch(
    async () => {
      throw new TypeError("fetch failed");
    },
    () => cachedDeviceProbe({}, dev("r29-down")),
  );
  assert.equal(down.update, undefined, "a down device costs one call and carries no verdict");
});

test("tunnel up, agent down (HTTP error, no unreachable shape) → split verdict", async () => {
  const p = await withFetch(async () => new Response("bad", { status: 500 }), () =>
    cachedDeviceProbe({}, dev("r31-agentdown")),
  );
  assert.equal(p.tunnel, true, "an HTTP answer proves the tunnel");
  assert.equal(p.agent, false);
});

test("healthy device: agent+tunnel true, npm release preferred, version fallback", async () => {
  const rel = await withFetch(async () => statusJson({ release: "1.2.307", version: "1.0.145" }), () =>
    cachedDeviceProbe({}, dev("r31-rel")),
  );
  assert.equal(rel.agent, true);
  assert.equal(rel.tunnel, true);
  assert.equal(rel.version, "1.2.307", "release beats the frozen Cargo version");
  assert.ok(typeof rel.checkedAt === "number" && rel.checkedAt > 0);

  const leg = await withFetch(async () => statusJson({ version: "1.0.140" }), () =>
    cachedDeviceProbe({}, dev("r31-leg")),
  );
  assert.equal(leg.version, "1.0.140", "pre-release agents fall back to version");

  const bare = await withFetch(async () => statusJson({}), () => cachedDeviceProbe({}, dev("r31-bare")));
  assert.equal(bare.agent, true);
  assert.equal(bare.version, undefined, "no version fields → absent, not fabricated");
});

test("30s cache: repeat probe makes no second fetch; fresh=1 bypasses", async () => {
  // TWO CALLS PER PROBE SINCE ROUND 29: /api/status and the device's own /api/update verdict. The property this test
  // is about is unchanged — a REPEAT probe must make no call at all, and `fresh=1` must make a full one — so the
  // numbers moved from 1/2 to 2/4 rather than the assertion being loosened to "at least one".
  await withFetch(async () => statusJson({ release: "1.2.307" }), async () => {
    const a = await cachedDeviceProbe({}, dev("r31-cache"));
    const b = await cachedDeviceProbe({}, dev("r31-cache"));
    assert.equal(a.version, "1.2.307");
    assert.equal(b.version, "1.2.307");
    assertFetchCalls(2, "round-98: second poll served from cache (status + update for the FIRST one only)");
    await cachedDeviceProbe({}, dev("r31-cache"), true);
    assertFetchCalls(4, "fresh bypasses the read (console check-now)");
  });
});

// The boot verdict, forwarded as a FLEET EXCEPTION (round 256). The device can report four
// verdicts; only the fault is carried, and these cases are the whole rule: a crashed device
// must be markable in the console, and a normally-restarted one must not decorate its row.
// Getting it backwards is silent in both directions — a fleet that never marks anything, or
// one where every row shouts and none is read.
test("a crashed last run is carried; a replaced one is dropped", async () => {
  const crashLine =
    "run journal: previous run DID NOT EXIT CLEANLY — CRASHED or was killed; survived 61s";
  const crashed = await withFetch(
    async () => statusJson({ release: "1.2.367", last_boot: crashLine, last_boot_kind: "crashed" }),
    () => cachedDeviceProbe({}, dev("r256-crash")),
  );
  assert.equal(crashed.lastBootKind, "crashed");
  assert.equal(crashed.lastBoot, crashLine, "the device's own sentence rides along for the hover");

  for (const kind of ["replaced", "clean-exit", "first-run", "machine-restart"]) {
    const p = await withFetch(
      async () => statusJson({ last_boot: "run journal: something", last_boot_kind: kind }),
      () => cachedDeviceProbe({}, dev(`r256-${kind}`)),
    );
    assert.equal(p.lastBootKind, undefined, `${kind} must not decorate a fleet row`);
    assert.equal(p.lastBoot, undefined);
  }

  // A crash with no sentence: no kind either. The console keys its mark on the kind and
  // hangs the sentence on it, so a kind alone would render a mark nobody can interrogate.
  const mute = await withFetch(async () => statusJson({ last_boot_kind: "crashed" }), () =>
    cachedDeviceProbe({}, dev("r256-mute")),
  );
  assert.equal(mute.lastBootKind, undefined, "no sentence → nothing to show");

  // And a device too old to send either field: absent, never fabricated.
  const legacy = await withFetch(async () => statusJson({ release: "1.2.366" }), () =>
    cachedDeviceProbe({}, dev("r256-legacy")),
  );
  assert.equal(legacy.lastBootKind, undefined);
});
