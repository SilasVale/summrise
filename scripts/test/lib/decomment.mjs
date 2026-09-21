// decomment.mjs — A COMMENT IS NOT A PRODUCER, NOR A DERIVATION (rounds 135-137 of the standing goal).
//
// WHY IT IS SHARED: rounds 135 and 136 each wrote this function into the gate that needed it, which made three copies of one
// rule — the exact defect this objective spends its rounds removing, committed by the gates that enforce it. The four gates
// that match TEXT now import one definition.
//
// THE DIRECTION OF ERROR MATTERS AND IS NOT THE SAME EVERYWHERE: for `wire-field-check` and its siblings a comment that
// satisfied them was a FALSE NEGATIVE (a requirement met by prose, so a deleted producer could be kept alive by a comment);
// for `one-derivation-check` a comment that quoted the pattern was a FALSE POSITIVE (a gate flagging its own documentation).
// `production-host-check` does NOT use this, on purpose: a comment spelling the deployment's host is exactly what it should
// flag, because the rule applies to the record about the rule.
//
// THE STRIP IS CONSERVATIVE BECAUSE `//` ALSO OPENS A URL: block comments always go; a whole-line `//` comment goes; a
// trailing `// …` goes only when the character before it is not a colon.
export function decomment(text) {
  return text
    .replace(/\/\*[\s\S]*?\*\//g, "")
    .split("\n")
    .map((line) => (line.trimStart().startsWith("//") ? "" : line.replace(/(^|[^:])\/\/.*$/, "$1")))
    .join("\n");
}
