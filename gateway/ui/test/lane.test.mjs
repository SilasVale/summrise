// A CHANNEL PREFIX'S LANE COLOUR — the console's first migrated module (block ③).
//
// The functions are Rust now (`gateway/ui-logic/src/lib.rs`), so this file awaits the module before it
// asserts anything: under `node --test` the seam reads `gateway/ui/public/ui_logic_bg.wasm` off disk
// and `initSync`s it, which is the SAME artifact the browser fetches. See `src/wasm/consoleLogic.ts`.
//
// WHAT IS PINNED HERE, and each is a way this console has been wrong before:
//   * the table, INCLUDING the fallback — a prefix the gateway's payload does not name is
//     `lane-def`, and that is on purpose rather than a gap;
//   * the WIDER trailing-slash rule (`or//` and `or/` and `or` are one channel) — the two regexes
//     that disagreed about this are why the function exists;
//   * `String(prefix ?? "")`: the JavaScript coerces ANY value, so a number is not a crash and a
//     `null` is the empty prefix.
import test from "node:test";
import assert from "node:assert/strict";
import { barePrefix, laneClass } from "../src/lib/lane.ts";
import { consoleLogic } from "../src/wasm/consoleLogic.ts";

await consoleLogic();

test("laneClass: every prefix the table names, and the fallback", () => {
  for (const p of ["og", "ds", "or", "qw", "nv", "gmi", "cm", "amd"]) {
    assert.equal(laneClass(p), `lane-${p}`);
  }
  // AN UNKNOWN PREFIX IS `lane-def` ON PURPOSE — the gateway's payload decides which prefixes
  // exist, and a console that invented a colour for one it has never heard of would be claiming a
  // lane the gateway never advertised.
  assert.equal(laneClass("nope"), "lane-def");
  assert.equal(laneClass(""), "lane-def");
});

test("barePrefix: ALL trailing slashes, not one", () => {
  assert.equal(barePrefix("or/"), "or");
  assert.equal(barePrefix("or//"), "or");
  assert.equal(barePrefix("or"), "or");
  assert.equal(barePrefix("or///"), "or");
  // NOT a general trim: a slash in the middle is part of the name.
  assert.equal(barePrefix("or/foo"), "or/foo");
});

test("laneClass: the trailing slash is ignored, so `or/` and `or` are one channel", () => {
  assert.equal(laneClass("or/"), "lane-or");
  assert.equal(laneClass("or//"), "lane-or");
});

test("barePrefix: the engine's ToString, for a value that is not a string", () => {
  // The JavaScript was `String(prefix ?? "")`, which accepts ANY value — the signature says
  // `string` and the coercion is what the panel actually relies on.
  assert.equal(barePrefix(5), "5");
  assert.equal(barePrefix(null), "");
  assert.equal(barePrefix(undefined), "");
});
