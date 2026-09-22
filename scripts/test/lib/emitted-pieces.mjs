// emitted-pieces.mjs — read the PIECES module out of an emitter's artifact, as data.
//
// WHY IT IS SHARED. An emitter's device-side script is assembled by `agent/scripts/lib/sweep-bundle.mjs` from real
// modules: the program (`<ui>-run.cjs`) and a generated `pieces.cjs` that carries everything varying per run — the
// probes, the page checks, the passes, the diag helper, the paths and the baked stamps. Three gates need to ask what
// those values ARE, and the first two of them did it by grepping for the SPELLING the payload used before the
// migration (`const PROBE = "<json>";`, `EXPECTED_ENTRY = {...}`), which is how a gate ends up failing a payload that
// is byte-identical to what it is checking (rounds 267-268). One extraction, three readers — the same lesson
// `lib/decomment.mjs` records from its own round.
//
// It EVALUATES the factory in a sandbox whose `require` refuses: the pieces module is declarations plus one export, so
// anything it tries to load at load time is a bug, and swallowing it here would hide exactly that.

const MARK = '__factories["pieces.cjs"] = function (module, exports, require) {';

/** @returns {object} the pieces module's exports */
export function piecesOf(artifactText, where = "the artifact") {
  const at = artifactText.indexOf(MARK);
  if (at < 0) {
    throw new Error(`${where} carries no pieces module — this read would prove nothing (is it an emitter artifact?)`);
  }
  const bodyStart = at + MARK.length;
  // THE END IS THE LAST `};` BEFORE THE NEXT REGISTRATION, not the first `\n};` after the start: a factory body
  // contains functions of its own, and the first match cuts the module in half ("Unexpected token ')'").
  const nextReg = [artifactText.indexOf("\n__factories[", bodyStart), artifactText.indexOf("\n__require(", bodyStart)]
    .filter((x) => x > 0)
    .sort((a, b) => a - b)[0] ?? artifactText.length;
  const bodyEnd = artifactText.lastIndexOf("\n};", nextReg);
  if (bodyEnd < 0) throw new Error(`${where}: the pieces module has no end — the artifact was truncated`);
  const body = artifactText.slice(bodyStart, bodyEnd);
  const mod = { exports: {} };
  try {
    // eslint-disable-next-line no-new-func
    new Function("module", "exports", "require", body)(mod, mod.exports, (spec) => {
      throw new Error(`the pieces module required ${spec} at load time`);
    });
  } catch (e) {
    throw new Error(`${where}: the pieces module does not evaluate — ${String(e.message).slice(0, 140)}`);
  }
  return mod.exports;
}

/** The value a probe/pass/check should have, and whether it crosses as JSON (strings) or as code (functions). */
export function classify(value) {
  return typeof value === "function" ? "code" : "json";
}
