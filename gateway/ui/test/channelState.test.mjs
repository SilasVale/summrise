// A PROVIDER CHANNEL'S SIGNAL, ITS WORDS, AND A DIAL'S TONE — the console's second migrated module.
//
// All three are Rust now (`gateway/ui-logic/src/lib.rs`), so this file awaits the module before it
// asserts anything — the same door `test/lane.test.mjs` uses. See `src/wasm/consoleLogic.ts`.
//
// WHAT IS PINNED HERE, and each is a way this console has been wrong before:
//   * the signal and the words are ONE derivation, and the provider's `reason` WINS over the generic
//     line — a provider that says why is more useful than a label that says what;
//   * an EMPTY reason is no reason, so the generic line comes back (the `||` is a truthiness test);
//   * `healthTone` has three answers, not two: a count that is SOME-but-not-all healthy is `warn`,
//     and a dial nobody has asked about is `off` even when every counted channel is well.
import test from "node:test";
import assert from "node:assert/strict";
import { channelLabel, channelSignal, healthTone } from "../src/lib/channelState.ts";
import { consoleLogic } from "../src/wasm/consoleLogic.ts";

await consoleLogic();

/** A stand-in dictionary, so the test is about WHICH key the derivation asks for and not about the
 *  console's wording — `en.ts` is 875 lines of strings and none of them is the subject here. */
const t = (key) => `<${key}>`;

test("channelSignal: the provider's own answer, and everything else is a failure", () => {
  assert.equal(channelSignal(true), "ok");
  assert.equal(channelSignal(false), "err");
  // TRUTHINESS, not `=== true` — the JavaScript was `ok ? "ok" : "err"`, so a non-empty string is
  // `ok` there. A port that compared to `true` would paint the wrong dot for one of these.
  assert.equal(channelSignal(/** @type {any} */ (1)), "ok");
  assert.equal(channelSignal(/** @type {any} */ ("")), "err");
  assert.equal(channelSignal(/** @type {any} */ (null)), "err");
});

test("channelLabel: the reason wins, because a provider that says WHY is more useful", () => {
  assert.equal(channelLabel({ ok: true }, t), "<overview.healthOk>");
  assert.equal(channelLabel({ ok: true, reason: "quota" }, t), "<overview.healthOk>");
  assert.equal(channelLabel({ ok: false, reason: "circuit open" }, t), "circuit open");
  assert.equal(channelLabel({ ok: false }, t), "<overview.healthDown>");
  // AN EMPTY REASON IS NO REASON: the `||` is a truthiness test, so `""` falls through to the
  // generic line instead of rendering a blank row.
  assert.equal(channelLabel({ ok: false, reason: "" }, t), "<overview.healthDown>");
});

test("healthTone: three answers, and 'not asked' is not 'none are healthy'", () => {
  assert.equal(healthTone(true, 3, 3), "ok");
  assert.equal(healthTone(true, 2, 3), "warn");
  assert.equal(healthTone(true, 1, 3), "warn");
  assert.equal(healthTone(true, 0, 3), "off");
  // `known` false, and the count says everything is well — the dial has not asked, so it says so.
  assert.equal(healthTone(false, 3, 3), "off");
  // `total === 0`: nothing to be healthy out of.
  assert.equal(healthTone(true, 0, 0), "off");
});
