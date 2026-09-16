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
            parts = [p.strip() for p in prelude.split(",") if p.strip()]
            keep = [p for p in parts if not (classes_in(p) and classes_in(p) <= dead)]
            if keep:
                if len(keep) != len(parts):
                    stats["parts"] += len(parts) - len(keep)
                out.append(", ".join(keep) + " {" + body + "}")
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

    stats = {"rules": 0, "parts": 0, "lines": 0, "classes": set()}
    for p in sorted(STYLES.glob("*.css")):
        before = p.read_text()
        after = prune(before, dead, stats)
        if args.write and after != before:
            p.write_text(after)

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
