// THE DEAD-STYLE RATCHET, for the console.
//
// WHY THIS FILE EXISTS. The panel has had this contract for thirty rounds; the console had none, and
// the cost was visible in round 79: eleven rules for a `.lane-port` / `.models-prefix` family that no
// component emits, which I "fixed" (measuring contrast on a rule nobody renders) before discovering
// they were dead. The first run of the tool against the console found **44 classes and 267 lines** —
// an order of magnitude more, because nothing had ever asked.
//
// Dead CSS is not free: it is what a reader has to search through to find the rule that applies, and
// it is where a fix goes to die quietly.
//
// THE TEST CALLS THE TOOL INSTEAD OF REIMPLEMENTING IT. `agent/resources/panel-react/scripts/
// prune-dead-css.py` owns the question "can this be pruned?" — including the part that is easy to get
// wrong: a class name can be ASSEMBLED at runtime (`className={`lane-${prefix}`}`), so "the literal
// never appears" is not evidence of death. The tool was given a `--root` argument for this caller in
// round 80; two copies of that logic would drift, a caller cannot.
import test from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const UI = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const TOOL = path.resolve(UI, "..", "..", "agent", "resources", "panel-react", "scripts", "prune-dead-css.py");

test("the console stylesheet has nothing left that no component can render", () => {
  const out = execFileSync("python3", [TOOL, "--json", "--root", UI], { cwd: UI, encoding: "utf8" });
  const line = out.split("\n").reverse().find((l) => l.trim().startsWith("{"));
  assert.ok(line, `no JSON from the pruner:\n${out}`);
  const report = JSON.parse(line);
  assert.deepEqual(
    report.wouldRemove,
    [],
    `dead CSS — run: python3 agent/resources/panel-react/scripts/prune-dead-css.py --write --root gateway/ui`,
  );
  assert.equal(report.rules, 0);
});

test("the pruner is looking at the console, and finds its stylesheet", () => {
  // A caller that silently pruned the WRONG root would report zero dead classes forever. This is the
  // same failure the spec snapshot's cross-check exists for: prove the instrument is pointed at the
  // thing you think it is.
  const out = execFileSync("python3", [TOOL, "--json", "--root", UI], { cwd: UI, encoding: "utf8" });
  assert.match(out, /declared classes: \d+/);
  const declared = Number(/declared classes: (\d+)/.exec(out)[1]);
  assert.ok(declared > 50, `expected the console's stylesheet to declare many classes, saw ${declared}`);
});
