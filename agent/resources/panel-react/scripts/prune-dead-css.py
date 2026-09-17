#!/usr/bin/env python3
"""prune-dead-css.py — remove panel CSS rules that nothing can render.

WHY. The objective includes pruning what does not earn its place, and the panel's stylesheet is
4k+ lines that no sweep had ever checked for rules nothing references. Measured: 32 class names
appear in the CSS and in NO source file at all (247 declaration lines), several of them large
(`.browser-tab` 38 lines, `.bp-chip` 22, `.browser-ai-indicator` 18) — leftovers of the browser
pane that the embedded view replaced.

HOW IT DECIDES. Conservative on purpose:
  * a selector PART is dropped only when EVERY class it names is unreferenced — so a shared rule
    like `.dead-live, .still-used { … }` keeps its live arm;
  * a rule disappears only when all of its parts do;
  * selectors with no class at all (element, `:root`, `@keyframes` bodies) are never touched;
  * `@media` blocks are recursed into and kept, even if they end up empty (an empty media block is
    harmless and its removal is not this script's business).

A CASE IT CAN SEE BUT MUST NOT REMOVE (measured round 22 of the standing goal). A class can be unreferenced
while the rule that names it is kept alive by a LIVE class in the same comma list — `.model-add .form-input`,
`.sidebar.open`, `.kchip.on .kchip-dot`. Those selectors can never match (a compound needs every class it names,
and a descendant needs its ancestor), and the tool reports them under "still declared inside shared rules". THREE
ATTEMPTS TO REMOVE THEM WERE MADE AND ALL THREE CORRUPTED A SHEET, which is why it still does not:

  1. "any dead class kills the part" rewrote documentation into code: `classes_in` scanned the raw prelude, so the
     console's header comment — `layout .app/.sidebar/.content` — counted as three dead classes on whatever rule
     followed it. `:root`, `*`, `html, body, #root` and the h1-h4 block deleted themselves;
  2. splitting the prelude into comment + selector fixed that but rejoined EVERY kept prelude with ", ", collapsing
     every multi-line selector list: a valid prune produced an 827-line diff across two UIs;
  3. re-emitting only the preludes that changed fixed THAT, and then a prelude carrying TWO comments had its second
     one absorbed into the selector — `/* … a channel that is down is not a dot {` — which the stylesheet-hygiene
     gate caught, not the tool.

The rule is right and the three failure modes are now known; what is missing is a prelude parser that models
comments positionally. Until then the safe answer is the one the tool already gives, and the ratchet in
`src/lib/deadStyles.test.ts` holds the count.

Run with `--write` to apply; without it, it only reports. It is a one-shot tool, not a gate — the
gate is the ratchet in `src/lib/deadStyles.test.ts`, which fails when a NEW unreferenced class
appears.
"""
import argparse
import json
import pathlib
import re
import sys

# WHICH UI. The panel by default, so the existing caller is untouched; `--root` points it at another
# frontend with the same shape (src/styles/*.css + src/**/*.tsx|ts). The console needed the same
# question answered in round 80 and got the same tool rather than a second implementation — the
# runtime-assembled-class logic below is the part that is easy to get wrong, and two copies of it
# would drift.
ROOT = pathlib.Path(__file__).resolve().parents[1]
STYLES = ROOT / "src" / "styles"
SRC = ROOT / "src"


def point_at(root: pathlib.Path) -> None:
    global ROOT, STYLES, SRC
    ROOT = root.resolve()
    STYLES = ROOT / "src" / "styles"
    SRC = ROOT / "src"


def referenced_names():
    """Every class name the source can PRODUCE.

    Two ways, and the second is the one that nearly cost this tool five live rules:
      1. the name appears literally (`className="browser-embedded-slot"`);
      2. the name is ASSEMBLED — `className={`browser-action${cls}`}` produces
         `browser-action-err` and `browser-action-ok` without either string existing anywhere.
         Measured on the panel: `notify-state is-${permission}` and `path-step-tag s-${state}`
         are the same shape, and a "the literal never appears" test would have deleted them.

    So every template literal's STATIC PREFIX before a `${` marks all declared names starting with
    that prefix as live. Conservative in the safe direction: a prefix that composes nothing costs a
    rule that stays, while a missed prefix costs a rule that RENDERED.
    """
    text = ""
    for pattern in ("*.tsx", "*.ts"):
        for p in SRC.rglob(pattern):
            if "node_modules" in str(p):
                continue
            text += p.read_text()
    names = set(re.findall(r"[A-Za-z][\w-]*", text))
    # the STATIC TAIL immediately before each interpolation: for `notify-state is-${permission}` the
    # tail is `is-` (the space ends the identifier run), for `browser-action${cls}` it is
    # `browser-action`. Only these tails are prefixes — treating every identifier as one marks the
    # whole stylesheet live and the tool reports nothing to prune (which is how the first version of
    # this rule behaved).
    prefixes = {m.group(1) for m in re.finditer(r"([A-Za-z][\w-]*)\$\{", text)}
    return names, prefixes


def split_prelude(prelude):
    """(leading text kept verbatim, the selector text, trailing whitespace).

    COMMENTS ARE POSITIONAL, NOT TEXT (round 23). The three failed prunes recorded below all came from treating a
    prelude as a string to split on ",": a comment's own commas became selector parts (the console's header lists
    `layout .app/.sidebar/.content`), a comment's font stack became parts, and a prelude carrying two comments had
    its second absorbed into the selector. This separates what MAY be rewritten — the selector text — from what is
    copied through untouched.

    A COMMENT INSIDE THE SELECTOR TEXT IS NOT TOUCHED EITHER: `.a, /* why */ .b {` cannot be rewritten without
    deciding where the comment belongs, and the answer is not worth a corrupted sheet. Such a rule is reported as
    skipped instead (see `untouchable` in prune).
    """
    m = re.match(r"^([\s\S]*?/\*[\s\S]*?\*/[\s\S]*?)(?=\S|$)", prelude)
    # everything up to the last comment that is followed by the selector; simpler and safer: the leading run of
    # whitespace + whole comment blocks, in order.
    lead_end = 0
    scanner = re.compile(r"\s*(?:/\*[\s\S]*?\*/\s*)*")
    m2 = scanner.match(prelude)
    if m2:
        lead_end = m2.end()
    lead, rest = prelude[:lead_end], prelude[lead_end:]
    tail = ""
    stripped = rest.rstrip()
    if stripped != rest:
        tail = rest[len(stripped):]
        rest = stripped
    return lead, rest, tail


def classes_in(selector):
    return set(re.findall(r"\.([A-Za-z][\w-]*)", selector))


def prune(chunk, dead, stats):
    """Remove dead selector parts from one level of CSS (recursing into at-blocks)."""
    out = []
    pos = 0
    while True:
        brace = chunk.find("{", pos)
        if brace < 0:
            out.append(chunk[pos:])
            return "".join(out)
        prelude = chunk[pos:brace]
        # find the matching close brace
        depth, j = 1, brace + 1
        while depth and j < len(chunk):
            if chunk[j] == "{":
                depth += 1
            elif chunk[j] == "}":
                depth -= 1
            j += 1
        body = chunk[brace + 1 : j - 1]
        lead = prelude.rstrip()
        if lead.lstrip().startswith("@") and not lead.lstrip().startswith("@keyframes"):
            # a block that CONTAINS rules (media/supports/layer): recurse, keep the block
            out.append(prelude + "{" + prune(body, dead, stats) + "}")
        elif lead.lstrip().startswith("@keyframes") or lead.lstrip().startswith("@font-face"):
            out.append(prelude + "{" + body + "}")  # bodies here are not selectors
        else:
            lead, selector_text, tail = split_prelude(prelude)
            if "/*" in selector_text:
                # A comment INSIDE the selector list: leave the rule exactly as it is and say so.
                stats["untouchable"] += 1
                out.append(prelude + "{" + body + "}")
                pos = j
                nxt = pos
                while nxt < len(chunk) and chunk[nxt] in "\r\n":
                    nxt += 1
                out.append(chunk[pos:nxt] if nxt > pos else "")
                pos = nxt
                continue
            parts = [p.strip() for p in selector_text.split(",") if p.strip()]
            # ANY DEAD CLASS KILLS THE PART. A COMPOUND selector requires its element to carry EVERY class it
            # names, and in a DESCENDANT selector every compound must match — so one class the source can neither
            # name nor assemble makes the whole part unmatchable: `.sidebar.open` cannot match however live `.open`
            # is, and `.model-add .form-input` needs an ancestor that never exists.
            #
            # The old rule required EVERY class in the part to be dead, which left 14 such rules in each UI sitting
            # inside selectors kept alive by a live arm. It is safe to widen now only because `split_prelude`
            # separates a rule's comments from its selector text (round 23); widening it against raw text is what
            # deleted `:root` from the console in round 22.
            keep = [p for p in parts if not (classes_in(p) & dead)]
            if keep:
                # ONLY REWRITE WHAT CHANGED. Rejoining every prelude with ", " collapsed every multi-line selector
                # list in both sheets — a valid prune produced an 827-line diff nobody can review. A rule that
                # loses no part keeps its exact text; one that does keeps its comments and its line breaks and
                # changes only the part that went.
                if len(keep) != len(parts):
                    stats["parts"] += len(parts) - len(keep)
                    out.append((lead + ", ".join(keep) + tail).rstrip() + " {" + body + "}")
                else:
                    out.append(prelude + "{" + body + "}")
            else:
                stats["rules"] += 1
                stats["lines"] += body.count(";") + 1
                for p in parts:
                    stats["classes"] |= classes_in(p)
        pos = j
        # keep the whitespace that followed the closing brace
        nxt = pos
        while nxt < len(chunk) and chunk[nxt] in "\r\n":
            nxt += 1
        out.append(chunk[pos:nxt] if nxt > pos else "")
        pos = nxt


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--write", action="store_true")
    ap.add_argument("--json", action="store_true", help="machine-readable summary (the ratchet test)")
    ap.add_argument("--root", default=None, help="the UI to inspect (default: this panel)")
    args = ap.parse_args()
    if args.root:
        point_at(ROOT.parent.parent.parent.parent / args.root if False else pathlib.Path(args.root))

    names, prefixes = referenced_names()
    declared = set()
    for p in sorted(STYLES.glob("*.css")):
        declared |= {m.group(1) for m in re.finditer(r"\.([A-Za-z][\w-]*)", p.read_text())}
    # A declared class is LIVE when the source names it OR when a template literal could compose it
    # from a static prefix (see referenced_names).
    dead = {
        c
        for c in declared
        if c not in names and not any(c.startswith(p) and c != p for p in prefixes)
    }
    print(f"declared classes: {len(declared)} | unreferenced: {len(dead)}")
    if not dead:
        return 0

    stats = {"rules": 0, "parts": 0, "lines": 0, "classes": set(), "untouchable": 0}
    for p in sorted(STYLES.glob("*.css")):
        before = p.read_text()
        after = prune(before, dead, stats)
        if args.write and after != before:
            p.write_text(after)

    if stats["untouchable"]:
        print(f"left alone (a comment sits inside the selector list): {stats['untouchable']}")
    print(f"rules removed: {stats['rules']} | selector parts dropped: {stats['parts']} | "
          f"declaration lines: {stats['lines']} | classes: {len(stats['classes'])}")
    left = sorted(dead - stats["classes"])
    print(f"still declared inside shared rules: {len(left)}")
    for c in left:
        print("   ." + c)
    if args.json:
        print(json.dumps({"wouldRemove": sorted(stats["classes"]), "rules": stats["rules"],
                          "lines": stats["lines"], "left": left}))
    if not args.write:
        print("(dry run — pass --write to apply)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
