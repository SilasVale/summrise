// ── one boolean keeps six `slice(prefix.length + 1)` sites from silently
//    emptying a bare model name ────────────────────────────────────────────────
//
// `translate.ts` and `translate-vision.ts` both derive a prefix as
// `model.split("/")[0]` and then strip it as `model.slice(prefix.length + 1)`.
// For a model WITH a prefix that is right. For a BARE model name — the documented
// default (`no prefix -> Command Code`) — the "prefix" is the whole name, and the
// slice runs one past the end and yields "".
//
// What makes that harmless is not a guard at any of those sites: it is
// `defaultRoute.stripPrefix === false`. A bare name falls through to the default
// route, stripPrefix is false, and the `: model` arm is taken instead. Flip that
// one boolean and EVERY bare-model request asks its upstream for the empty model
// name — silently, with no error at any layer.
//
// Nothing stated or pinned that. This does. (Round 220 found it by chasing a
// suspected bug that turned out not to exist: the premise "the default route is
// one of the stripPrefix:true routes" was never checked, and the test refuted it.)
import { test } from "node:test";
import assert from "node:assert/strict";
import { resolveRoute } from "../src/upstream.ts";

test("the default route does NOT strip a prefix, whatever the caller passed", async () => {
  // A string that is no built-in prefix and no custom provider: the fall-through.
  const bare = "a-model-name-that-is-not-a-prefix";
  const route = await resolveRoute({}, bare, null);
  assert.notEqual(
    route.type,
    "error",
    "an unknown prefix must still resolve (the documented fall-through), otherwise this " +
      "test is no longer probing the default route",
  );
  assert.equal(
    route.stripPrefix,
    false,
    "defaultRoute.stripPrefix must stay FALSE. translate.ts and translate-vision.ts strip a " +
      "model's prefix with `model.slice(prefix.length + 1)`; for a BARE model name that is one " +
      "past the end and yields \"\". The only thing preventing every bare-model request from " +
      "asking its upstream for the empty model name is this flag being false — the slice sites " +
      "have no guard of their own.",
  );
});

test("a bare model name survives the prefix-strip expression unchanged", async () => {
  for (const bare of ["minimax-m3", "gpt-5", "claude-sonnet-4"]) {
    const prefix = bare.split("/")[0] || "";
    const route = await resolveRoute({}, prefix, null);
    const upstreamModel = route.stripPrefix ? bare.slice(prefix.length + 1) : bare;
    assert.equal(
      upstreamModel,
      bare,
      `a bare model name must reach the upstream unchanged, but ${JSON.stringify(bare)} became ` +
        `${JSON.stringify(upstreamModel)} — the prefix-strip expression emptied it`,
    );
  }
});

test("a prefixed model name IS stripped to its wire name", async () => {
  const model = "og/minimax-m3";
  const prefix = model.split("/")[0] || "";
  const route = await resolveRoute({}, prefix, null);
  const upstreamModel = route.stripPrefix ? model.slice(prefix.length + 1) : model;
  assert.equal(
    upstreamModel,
    "minimax-m3",
    "a prefixed model must still be stripped — this is the arm that makes the bare-name " +
      "exception safe rather than a reason to remove the strip",
  );
});
