//! `press-anchor-check` — A PRESS MUST BE MEASURED AGAINST THE HOVER, NOT AGAINST REST.
//!
//! WHY THIS EXISTS (round 95 of the standing goal). `pressPass` is the only instrument that can see whether a
//! press reaches the screen: `feedback-check.mjs` proves an `:active` RULE exists, and the judge fails a row
//! whose before and during snapshots are identical. But the pass took its "before" snapshot with the pointer
//! PARKED AWAY from the control, so a sheet that answered `:hover` and had no press rule at all still produced
//! a difference — the hover made it — and the row read `changed: true`.
//!
//! MEASURED ON THE DEVICE, on the landing's theme toggle, which has a `:hover` rule and no `:active` rule:
//!
//! ```text
//! light .theme-toggle 32x32  hoverChanges=[background,color]  pressAddsBeyondHover=[]  PRESS-ADDS-NOTHING
//! dark  .theme-toggle 32x32  hoverChanges=[background,color]  pressAddsBeyondHover=[]  PRESS-ADDS-NOTHING
//! ```
//!
//! ...while `.btn-primary` (transform) and `a` (opacity) both add something beyond their hover on the same
//! page. So the toggle was the one input on that surface that answered a hover and ignored a press, and BOTH
//! gates missed it: the rendered pass for the reason above, and the sheet check because the landing's
//! stylesheet is inline in `index/src/page.js` and `feedback-check.mjs` only read the panel's built sheet and
//! the console's source sheet.
//!
//! WHAT THIS PINS, in two halves — the same split `svgRootPaints` uses in the contrast probe, because the DOM
//! loop around the rule cannot run without a browser and the rule itself can:
//!
//!   1. THE RULE, as a pure function: `pressDelta(hovered, pressed)`.
//!   2. THE WIRING, in the EMITTED artifact: the hovered snapshot is read before `mouse.down()`, and the
//!      verdict comes from the hovered baseline. A rule the device does not run is not a rule.
//!
//! ── MIGRATED FROM `scripts/test/press-anchor-check.mjs`, WHICH IS DELETED ───────────────────────────
//! The `.mjs` drove this from Node and read the same three emitted artifacts. What did NOT change is the
//! part that matters: `pressDelta` is still the SHIPPING function and the three artifacts are still read as
//! TEXT. **THE DECISION IS WHAT MOVED**, and the two things Rust cannot do for itself are asked of the
//! engine that owns them — a small Node program calls `pressDelta` on the same inputs and compiles each
//! emitted artifact, and every expected answer is compared HERE, in Rust. Re-implementing `pressDelta` in
//! Rust would have been the one move this file must not make: the `.mjs` says why — the functions under test
//! are "the ones the browser runs ... not a copy that can drift".
//!
//! ── A RUST GATE THAT SPAWNS A TOOL WHICH RUNS CARGO WOULD DEADLOCK, AND THAT WAS MEASURED ───────────
//! `--emit` embeds the sweep plan, and `agent/scripts/lib/sweep-plan.mjs` reaches it with `cargo run` when
//! cargo is on PATH. Run from inside `cargo test`, that inner cargo blocks on the build lock the outer one
//! holds — measured with a throwaway `#[test]`: the child had not finished after 45 seconds. The same build
//! with a DEDICATED `--target-dir` finished in 32.9 seconds, because the two locks are different files. So
//! this gate builds `summrise-sweep-plan` itself, into `agent/target/sweep-plan-emit`, and hands the result
//! to every emit through `SUMMRISE_SWEEP_PLAN_BIN`. (The second probe is also why the first reading is
//! trustworthy: contention with other cargo processes on this box was the alternative explanation, and the
//! dedicated target dir proves it was not that.)
//!
//! ── THE MUTATION THAT MUST FAIL THIS GATE ──────────────────────────────────────────────────────────
//! Read this when you change this file: the mutation is how you find out whether the gate can still fail
//! at all. A gate that cannot be broken is worse than no gate.
//!
//! MUTATION: read the press baseline BEFORE the hover (put `const hovered = await styleOf(sel);` back above
//!           `page.mouse.move`), or add `width` to `pressDelta`'s key list; **AND, added 2026-09-28: (a)
//!           hand a discovered target back as a bare selector (`out.push(label)` in place of the mark +
//!           handle + label map), or (b) delete the `checkVisibility` predicate from `pressPass`'s `styleOf`**
//! RESULT:   exit 1 every way: "the baseline is read BEFORE the hover (move@…, hovered@…) — that is the
//!           resting anchor this check exists for", "layout properties are not a press response", **"(a) the
//!           discovered control is not MARKED — so the press and the two snapshots re-resolve a bare
//!           selector and can land on different elements, which is how a live button was accused of
//!           rendering nothing"** and **"(b) the baseline reader takes the first match WITHOUT asking
//!           whether the browser renders it, while the box reader does ask — so the press and both snapshots
//!           can describe two different elements"**. Both new cases were run as mutations, not assumed, and
//!           **THE FIRST VERSION OF (a) PASSED ITS OWN MUTATION**: it asked for the string
//!           `data-summrise-press` anywhere in the discovery's first 4000 characters, and the CLEARER
//!           defined three lines above carries that selector in its own body — so reverting the marking left
//!           the gate green. That is this file's recorded trap ("a check that a string exists is not a check
//!           that the RULE is where it has to be") arriving at the assertion written to avoid it, and it is
//!           why the check now pins three STATEMENTS (the mark is assigned, mapped to a readable name, and
//!           handed back) instead of a word. **AND THE 4000-CHARACTER WINDOW WAS ITSELF A DEFECT**: the
//!           discovery's body grew past it while the marking was being added, so the assertion failed
//!           against a CORRECT artifact — a fixed-size window measures how much comment somebody wrote. The
//!           window is now the function (up to the next top-level declaration the pieces module emits). It
//!           pins the rule as a pure function AND the wiring in all three EMITTED artifacts, because a probe
//!           measured against rest calls every hover a press — that is how the landing's theme toggle passed
//!           for as long as it existed. The first version of the wiring assertion checked only "hovered
//!           before down" and PASSED the mutation (round 95). **AND IT REACHES EVERY CONTROL A PAGE
//!           RENDERS (round 15)**: it scrolls an element into view only when it is not already there, clamps
//!           the rect to the viewport, hit-tests the point, and reports a press the pointer never delivered
//!           as a NOTE rather than as an answer.

mod common;

use common::{repo, Spawn};
use std::path::{Path, PathBuf};

/// The three emitters, in the order the `.mjs` walked them.
const SWEEPS: [&str; 3] = [
    "panel-design-sweep",
    "console-design-sweep",
    "landing-design-sweep",
];

/// `src.indexOf(needle)`, as a signed number: JavaScript answers `-1` for "absent", and every ordering
/// assertion in the original is written against that — `move > 0` is "found, and not at index 0".
///
/// **THE INDEX IS A CHARACTER COUNT, NOT A BYTE OFFSET**, and that is not tidiness: the assertions below
/// slice the artifact (`src.slice(discovery, nextDecl)`) and JavaScript counts UTF-16 units, while these
/// artifacts are full of em dashes and arrows — three bytes, one index. A byte index used as a slice start
/// lands in the middle of a comment.
fn at(src: &str, needle: &str) -> i64 {
    match src.find(needle) {
        Some(byte) => src[..byte].chars().count() as i64,
        None => -1,
    }
}

/// `src.indexOf(needle, from)` — the window that starts the discovery's own body, which is the one place the
/// original searches from a position rather than from the top.
fn at_from(src: &str, needle: &str, from: usize) -> i64 {
    let Some(byte) = src.char_indices().nth(from).map(|(b, _)| b) else {
        return -1;
    };
    match src[byte..].find(needle) {
        Some(i) => (from + src[byte..byte + i].chars().count()) as i64,
        None => -1,
    }
}

/// `src.slice(start, start + n)` measured in CHARACTERS, which is what JavaScript's UTF-16 index counts for
/// every character in these artifacts. A byte slice would be a SMALLER window (an em dash costs three bytes
/// and one index) and would silently stop reaching the code the assertion is about.
fn window(src: &str, start: usize, n: usize) -> String {
    src.chars().skip(start).take(n).collect()
}

/// **THE BUILD THE OUTER `cargo test` WOULD DEADLOCK ON**, done into a target directory of its own.
///
/// `cargo run` is what `sweep-plan.mjs` reaches for when cargo is on PATH, and a nested cargo blocks on the
/// build lock the outer one holds — so the emits below are never allowed to make that call. The binary is
/// built HERE, once, with `CARGO_TARGET_DIR` pointed somewhere the outer cargo does not own, and every emit
/// gets it through `SUMMRISE_SWEEP_PLAN_BIN` (which `sweep-plan.mjs` honours before it looks for cargo).
fn sweep_plan_bin() -> PathBuf {
    let target = repo().join("agent/target/sweep-plan-emit");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let build = Spawn::new(&cargo)
        .args([
            "build",
            "--quiet",
            "-p",
            "summrise-sweep-plan",
            "--target-dir",
            &target.to_string_lossy(),
        ])
        .cwd(repo().join("agent"))
        .run();
    assert!(
        build.ok(),
        "press-anchor: `summrise-sweep-plan` did not build (rc={}), so the sweeps cannot emit a plan — and \
         letting them reach for `cargo run` instead would deadlock on the build lock this test already \
         holds:\n{}",
        build.code(),
        build.both()
    );
    let binary = target.join("debug/summrise-sweep-plan");
    assert!(
        binary.exists(),
        "press-anchor: cargo reported success but {} is not there",
        binary.display()
    );
    binary
}

/// The emitted artifact, as text. A failed emit is a panic rather than a skipped sweep: the `.mjs` called
/// `execFileSync` without a try, so a broken emitter failed the gate loudly, and that is the behaviour kept.
fn emit(sweep: &str, plan: &Path, landing_out: &Path) -> String {
    let script = repo().join("agent/scripts").join(format!("{sweep}.mjs"));
    let out = Spawn::new("node")
        .arg_path(&script)
        .args(["--emit"])
        .env("SUMMRISE_SWEEP_PLAN_BIN", &plan.to_string_lossy())
        .env("SUMMRISE_LANDING_OUT", &landing_out.to_string_lossy())
        .run();
    assert!(
        out.ok(),
        "press-anchor: `node {sweep}.mjs --emit` failed (rc={}) — there is no artifact to check:\n{}",
        out.code(),
        out.both()
    );
    out.stdout
}

/// `new Function(src)` for every artifact, in ONE Node process, because a source that does not parse is a
/// fact about the JS engine and this is the engine that runs it. The parse itself proves nothing about the
/// RULES — that is what the assertions below are for — but the `.mjs` asserted it, and an emitter that
/// printed 30 KB of broken text is exactly the failure this repository has paid for twice.
fn assert_they_parse(sources: &[(&str, &String)]) {
    let mut program = String::from(
        r#"const fs = require("fs");
for (const f of process.argv.slice(1)) {
  const src = fs.readFileSync(f, "utf8");
  try { new Function(src); } catch (e) { console.error(`${f}: ${e.message}`); process.exit(1); }
}
"#,
    );
    program.push_str("process.stdout.write(\"parsed ok\\n\");\n");

    let dir = std::env::temp_dir().join(format!("press-anchor-parse-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a writable temp directory");
    let mut args: Vec<String> = vec!["-e".to_string(), program];
    let mut written = Vec::new();
    for (i, (name, src)) in sources.iter().enumerate() {
        let p = dir.join(format!("{i}-{name}.js"));
        std::fs::write(&p, src).expect("a writable temp artifact");
        args.push(p.to_string_lossy().into_owned());
        written.push(p);
    }
    let out = Spawn::new("node").args(args).run();
    for p in written {
        let _ = std::fs::remove_file(p);
    }
    let _ = std::fs::remove_dir(&dir);
    assert!(
        out.ok(),
        "press-anchor: an emitted artifact does not PARSE — the bundle printed for the device is not a \
         program:\n{}",
        out.both()
    );
}

/// **HALF ONE — THE RULE, IN THE FUNCTION THE DEVICE RUNS.** The inputs and the expected answers are the
/// `.mjs`'s own three cases, carried across one at a time; what moved is where the comparison happens.
fn check_the_rule(mut n: usize) -> usize {
    let program = r#"import { pressDelta } from "./agent/scripts/lib/design-sweep.mjs";
const RESTING = { transform: "none", opacity: "1", background: "rgb(255,255,255)", filter: "none" };
const hovered = { ...RESTING, background: "rgb(240,240,241)" };
const pressed = { ...hovered };
const out = {
  hoverOnly: pressDelta(hovered, pressed),
  oldAnchor: ["transform", "opacity", "background", "filter"].filter((k) => RESTING[k] !== pressed[k]),
  transform: pressDelta(hovered, { ...hovered, transform: "matrix(1, 0, 0, 1, 0, 1)" }),
  opacity: pressDelta(hovered, { ...hovered, opacity: "0.72" }),
  both: pressDelta(hovered, { ...hovered, transform: "matrix(1, 0, 0, 1, 0, 1)", opacity: "0.9" }),
  layout: pressDelta(RESTING, { ...RESTING, width: "12px", height: "14px" }),
  nullHovered: pressDelta(null, { ...hovered, transform: "x" }),
  nullPressed: pressDelta(hovered, null),
};
process.stdout.write(JSON.stringify(out));
"#;
    let out = Spawn::new("node")
        .args(["--input-type=module", "-e", program])
        .run();
    assert!(
        out.ok(),
        "press-anchor: could not ask the shipping `pressDelta` for its answers (rc={}) — the rule half \
         cannot be checked against a copy, so this is a failure and not a skip:\n{}",
        out.code(),
        out.both()
    );
    let json: serde_json::Value = serde_json::from_str(out.stdout.trim()).unwrap_or_else(|e| {
        panic!(
            "press-anchor: pressDelta's answers were not JSON ({e}): {}",
            out.stdout
        )
    });

    n += 1;
    assert_eq!(
        json["hoverOnly"],
        serde_json::json!([]),
        "a hover-only control must NOT report a press that renders (the landing's theme toggle)"
    );
    assert_eq!(
        json["oldAnchor"],
        serde_json::json!(["background"]),
        "the resting anchor saw the hover and called it a press — that is the false pass this check exists for"
    );

    n += 1;
    assert_eq!(
        json["transform"],
        serde_json::json!(["transform"]),
        "a control that answers the press itself reports the property it moved"
    );
    assert_eq!(json["opacity"], serde_json::json!(["opacity"]));
    assert_eq!(
        json["both"],
        serde_json::json!(["transform", "opacity"]),
        "BOTH, when a control does both — the row then says so instead of reporting the first one"
    );

    n += 1;
    assert_eq!(
        json["layout"],
        serde_json::json!([]),
        "layout properties are not a press response — a width that moves on press re-lays-out the page every \
         frame, and counting it would let exactly the press `feedback.rs` bans pass here"
    );
    assert_eq!(
        json["nullHovered"],
        serde_json::json!([]),
        "a snapshot the probe could not read is UNKNOWN, never \"it moved\""
    );
    assert_eq!(json["nullPressed"], serde_json::json!([]));

    n
}

#[test]
fn a_press_is_measured_against_the_hover_in_every_emitted_artifact() {
    let mut n = check_the_rule(0);

    let plan = sweep_plan_bin();
    let landing_out = std::env::temp_dir().join(format!("press-anchor-{}", std::process::id()));
    std::fs::create_dir_all(&landing_out).expect("a writable landing output directory");

    let mut sources: Vec<(&str, String)> = Vec::new();
    for name in SWEEPS {
        sources.push((name, emit(name, &plan, &landing_out)));
    }
    assert_they_parse(
        &sources
            .iter()
            .map(|(name, src)| (*name, src))
            .collect::<Vec<_>>(),
    );

    for (name, src) in &sources {
        // THE FIRST OF THE ORIGINAL'S TWO PER-ARTIFACT CHECKS: the artifact is a program and not a stub. The
        // PARSE half of it was run above, in one Node process for all three, because a parse is a fact about
        // the engine rather than about a sweep; the count below still follows the `.mjs`'s grouping, so the
        // ok line reports the same number of checks it always did.
        n += 1;
        let len = src.chars().count();
        assert!(
            len > 5000,
            "{name}: the emit produced {len} bytes, so it proves nothing"
        );

        // THE ORDER IS THE MEASUREMENT: hover the control, let the hover transition SETTLE, read the
        // baseline, then press. Asserting only "hovered before down" does not catch the defect — the first
        // version of this check did exactly that and passed a mutated probe that read the baseline with the
        // pointer still parked away, which IS the bug. (Found by running the mutation, which is the only way
        // this kind of assertion is ever found.)
        let turn = at(src, "await page.mouse.move(box.x, box.y)");
        let hovered = at(src, "const hovered = await styleOf(sel)");
        let down = at(src, "await page.mouse.down()");
        let call = at(src, "const props = pressDelta(hovered, pressed)");
        assert!(
            turn > 0,
            "{name}: no hover move — the baseline cannot be the hover"
        );
        assert!(
            down > 0,
            "{name}: no mouse.down() — there is no press to measure"
        );
        assert!(
            turn < hovered,
            "{name}: the baseline is read BEFORE the hover (move@{turn}, hovered@{hovered}) — that is the \
             resting anchor this check exists for"
        );
        assert!(
            hovered < down,
            "{name}: the baseline is read AFTER the press (hovered@{hovered}, down@{down})"
        );
        assert!(
            call > down,
            "{name}: the verdict is not pressDelta(hovered, pressed) — the baseline is not the hover"
        );
        assert!(
            src.contains("function pressDelta"),
            "{name}: pressDelta is not embedded, so the browser runs a different rule"
        );

        // AND IT MAY NOT ACCUSE FROM A BLIND SPOT (round 15). The pass takes the element's rect as it finds
        // it: for `.device-logs-toggle` that was y=1582 in an 860px viewport, so the pointer moved to a
        // coordinate outside the page, nothing hovered, nothing pressed, and the row read "press adds
        // nothing" — a finding against a button that answers. Four rules, pinned here because each is a way
        // this instrument lied:
        assert!(
            src.contains(r#"el.scrollIntoView({ block: "nearest", inline: "nearest", behavior: "instant" })"#),
            "{name}: the element is not scrolled into view — a control below the fold will be \"pressed\" at \
             a coordinate outside the page"
        );
        // The condition is wrapped across lines in the source, so the assertion names its parts rather than
        // one formatted string: a check that goes stale on a reflow is a check that gets deleted instead of
        // fixed.
        assert!(
            src.contains("const movedPage =") && src.contains("before.bottom > innerHeight"),
            "{name}: every element is scrolled unconditionally — an instrument may move the page to REACH a \
             control, it may not rearrange the page it is measuring"
        );
        assert!(
            src.contains("Math.min(r.right, innerWidth)"),
            "{name}: the rect is not clamped to the viewport, so a half-visible control is refused or pressed \
             off-page"
        );
        assert!(
            src.contains("document.elementFromPoint"),
            "{name}: the hit test is gone — a covered element and a still element would read the same"
        );
        assert!(
            src.contains("the pointer never reached this control"),
            "{name}: a press the pointer never delivered is reported as an answer (or as its absence) instead \
             of as a note"
        );
        // (The judge half — `r.reached !== false` gating the dead-control verdict — lives in `judgeReport`,
        // which is NOT part of the emitted sweep, so it is planted at the judge:
        // `panel-design-sweep.bash`.)
        // AND "ARRIVED" IS DECIDED AT THE POINT PRESSED (round 265). This pinned
        // `!(box.movedPage && hovered && hovered.hit === false)` — which counted the hit test ONLY when the
        // element had to be scrolled first. The connect tabs inside a closed `<details>` were already "in
        // the viewport" by their rect while the section behind them took every hit, so a control that
        // presses perfectly was reported twelve times as one that ignores a press. `box.reaches` is
        // `elementFromPoint` AT THE CLAMPED CENTRE — the coordinate the press uses — so it is the honest
        // test, and `checkVisibility` keeps the passes from offering a control the browser does not render
        // in the first place.
        assert!(
            src.contains("const reached = box.reaches !== false"),
            "{name}: the row no longer carries whether the pointer arrived"
        );

        // SCOPED TO THE DISCOVERY, because the first version of this assertion matched the guard's TEXT
        // ANYWHERE and the same three lines also live in the two press probes — so deleting the discovery's
        // copy still passed. A check that a string exists is not a check that the RULE is where it has to be
        // (round 265's own mutation caught it: the guard was removed from `discoverPressTargets` and the
        // gate stayed green).
        let discovery = at(src, "function discoverPressTargets");
        assert!(discovery >= 0, "{name}: the DOM discovery is not embedded");
        // THE WINDOW IS THE FUNCTION, NOT A CHARACTER COUNT. It was `discovery + 4000` and that number is a
        // trap: the discovery's own body grew past it while this round was adding the marking, so the
        // assertion written to pin the new rule failed against a CORRECT artifact — a fixed-size window
        // measures how much comment somebody wrote, not where the code is. The pieces module declares each
        // function as `const <name> = <fn.toString()>;` joined by newlines, so the next top-level
        // declaration is the honest end of this one.
        let discovery = discovery as usize;
        let next_decl = at_from(src, "\nconst ", discovery + 1);
        let marks: String = if next_decl > 0 {
            src.chars()
                .skip(discovery)
                .take(next_decl as usize - discovery)
                .collect()
        } else {
            src.chars().skip(discovery).collect()
        };
        assert!(
            marks.contains("checkVisibilityCSS: true"),
            "{name}: the DOM discovery offers controls the browser does not render — a closed <details> \
             keeps layout boxes for its content, which is how the connect tabs were pressed through the \
             section drawn over them"
        );
        // ── AND A DISCOVERED TARGET IS A HANDLE, NOT A SELECTOR (measured 2026-09-28 on `main` at
        // 067efc52) ─────────────────────────────────────────────────────────────────────────────────────
        // The pass pressed `Save & connect` and read BOTH of its snapshots from `Update to 1.2.433` — a
        // button inside a CLOSED `<details>`, a layout box with no hit-testable surface, which is exactly the
        // control the discovery rejects three lines above. `styleOf` and `box` both begin
        // `document.querySelectorAll(sel)` and take match #1, and the row's whole handle was the STRING
        // `button.btn`, so the two disagreed about which element that was: the hidden one. Two identical
        // snapshots, one false finding — `renders NOTHING when pressed` — and the same collapse made
        // `found: 2` a fiction, because `box` re-resolved the string too and pressed element #1 twice while
        // the rows claimed both controls had been measured.
        //
        // THE RULE, asserted at the FUNCTION rather than as a string somewhere in the artifact: what the
        // discovery hands back must RESOLVE TO THE ELEMENT IT FOUND, and the pass must unmark what it marked.
        //
        // **THE FIRST VERSION OF THIS ASSERTION PASSED THE MUTATION, WHICH IS HOW IT WAS FOUND.** It asked
        // for the string `data-summrise-press` anywhere in the discovery's first 4000 characters — and the
        // CLEARER, defined three lines above, carries that selector in its own body, so reverting the marking
        // to a bare selector left the gate green. What is pinned now is the SHAPE the rule needs: the
        // discovery ASSIGNS the mark, MAPS it to the name a finding prints, and HANDS BACK THE MARK — three
        // statements, and the mutation deletes all three.
        assert!(
            marks.contains(r#"el.setAttribute("data-summrise-press""#),
            "{name}: the discovered control is not MARKED — so the press and the two snapshots re-resolve a \
             bare selector and can land on different elements, which is how a live button was accused of \
             rendering nothing"
        );
        assert!(
            marks.contains("out.push(handle)"),
            "{name}: the discovery hands back something other than the marked handle — a bare selector is not \
             a handle, and a page with two controls sharing one class had both rows describe the first one"
        );
        assert!(
            marks.contains("labels[handle] = label"),
            "{name}: the marked handle has no readable name, so a finding would print an attribute selector \
             instead of the control it is about"
        );
        assert!(
            src.contains("window.__summriseUnmark"),
            "{name}: the pass marks the page and never takes the marks off — the next probe would measure a \
             page this one wrote to"
        );
        // AND THE TWO READERS OF ONE TARGET ASK THE SAME VISIBILITY QUESTION. `box` has asked
        // `checkVisibility` since round 265; `styleOf` did not, so for any selector whose first match is
        // unhittable they read different elements. A marked handle cannot be ambiguous, but a CURATED
        // selector (`.tab`, `.btn`, `a`) still can, so the predicate is pinned in BOTH readers rather than in
        // one. The window is 1400 CHARACTERS because that is what the `.mjs` measured with
        // `src.slice(styleOf, styleOf + 1400)`.
        let style_of = at(src, "const styleOf = (sel) => page.evaluate");
        assert!(style_of >= 0, "{name}: the baseline reader is not embedded");
        assert!(
            window(src, style_of as usize, 1400).contains("checkVisibilityCSS: true"),
            "{name}: the baseline reader takes the first match WITHOUT asking whether the browser renders \
             it, while the box reader does ask — so the press and both snapshots can describe two different \
             elements"
        );
        // AND IT ASKS THE DOM FOR THE CONTROLS, with the count that lets the judge size its floor to the
        // page.
        assert!(
            src.contains("function discoverPressTargets"),
            "{name}: the DOM discovery is not embedded — a control on a page no list names is never pressed \
             (that is how the log toggle was missed)"
        );
        assert!(
            src.contains("found,"),
            "{name}: the row does not carry what the page HAD, so the judge cannot tell a one-control page \
             from a vacuous pass"
        );
        assert!(
            src.contains("label.skip"),
            "{name}: the discovery has no skip list, so its cap is spent on chrome another pass already \
             presses (that is how the log toggle stayed unpressed)"
        );
        // AND A PASS THAT MOVES THE POINTER PUTS IT BACK. Leaving it where the last press ended meant the
        // next surface's probes ran with whatever sat under that position still hovered — a session row kept
        // its actions revealed and the target probe measured a state nobody had asked for.
        assert!(
            src.contains("A pass that moves the pointer owns putting it back"),
            "{name}: the pass leaves the pointer where it stopped, so the next surface measures whatever \
             that position hovers"
        );

        // THE PANEL IS WHERE A HOVER-REVEALED TARGET EXISTS, so the reveal pass is pinned there and only
        // there: the console's stylesheets have no `:hover` rule that reveals a child (`display`/`visibility`/
        // `opacity` of a descendant — checked when this was written) and the landing's action buttons are
        // always in flow. If either grows one, this assertion is where the next reader should widen the pass
        // rather than guess.
        //
        // AND THE EMPTY FLEET RENDERS THE OVERVIEW, WHERE THE OFF TONE LIVES (round 25). The console declares
        // `stat-off` unstyled-by-design — a class with no matching rule, painted by the base card — and the
        // unstyled pass reported it UNSEEN ("1 of 1 declared unstyled-by-design class(es) were not seen in
        // this run (0 unstyled name(s) over 6 page(s))"). The tone goes off when there is nothing to report
        // (no device online, no channels, no keys); the empty-fleet fixture existed and visited only #/devices
        // and #/keys, so the one state that needs that declaration had never been rendered by anything. A
        // state with no surface cannot be measured.
        //
        // THE NEEDLE IS THE PLAN'S JSON NOW, AND THE REQUIREMENT IS UNCHANGED (landing 2b, 2026-10-08). The
        // fixture list was `[['overview-empty', '#/'], …]` in the payload; it is a surface of the Rust plan,
        // embedded in the emitted bundle as `{"page":"overview-empty","hash":"#/",…}`. The assertion still
        // asks the same question — does the artifact that runs carry the empty-fleet Overview — and it names
        // the pair rather than the page alone, because a surface with the right name and the wrong hash
        // renders a different page.
        if name.starts_with("console") {
            assert!(
                src.contains(r##""page":"overview-empty","hash":"#/""##),
                "{name}: the empty-fleet fixture does not render the Overview — the stat-off declaration then \
                 waives a state no surface shows"
            );
        }
        if !name.starts_with("console") && !name.starts_with("landing") {
            assert!(
                src.contains("function revealPass"),
                "{name}: the revealed state is not measured deliberately — a hover-revealed target is then \
                 measured only when the pointer happens to rest on its row"
            );
            // AND THE ACKNOWLEDGEMENT'S LATENCY IS A PASS OF ITS OWN (round 19): the emitted script must
            // carry it, or the "fires on the event, not on the network" clause is asserted nowhere but in a
            // comment.
            assert!(
                src.contains("function ackPass"),
                "{name}: the acknowledgement latency is not measured — the panel's promise that feedback \
                 fires on the EVENT is then only a claim in a doc comment"
            );
            assert!(
                src.contains("msToAck"),
                "{name}: the acknowledgement pass does not report how long it took"
            );
            // AND IT REACHES ITS CONTROL THE SAME WAY THE PRESS PASS DOES (round 19): the first run clicked
            // two buttons at y=1200-1430 in an 860px viewport, measured nothing, and reported a finding. The
            // same three rules — minimal scroll, clamp to the visible part, hit-test the point — are asserted
            // here for the same reason.
            assert!(
                src.contains(r#"el.scrollIntoView({ block: "nearest", inline: "nearest", behavior: "instant" })"#),
                "{name}: the acknowledgement pass does not scroll its control into view — a click outside the \
                 page reads as \"no acknowledgement\""
            );
            assert!(
                src.contains("box.reaches === false"),
                "{name}: the acknowledgement pass presses controls the pointer cannot reach"
            );
            // AND ITS BASELINE IS THE HOVER, not rest: read with the pointer away, a control's own hover rule
            // looks like an acknowledgement and every control passes for free.
            // DECLARATION-AGNOSTIC, and that is not fussiness: this assertion matched the literal
            // `const before` and went red in CI the moment the baseline became `let` so the retry could
            // re-read it. A gate that pins a keyword instead of an ORDER reports the edit rather than the
            // rule.
            assert!(
                at(src, "before = await read(sel)")
                    > at(
                        src,
                        "await page.mouse.move(box.x, box.y)\n    await page.waitForTimeout(260)"
                    ),
                "{name}: the acknowledgement baseline is read before the hover — that is the resting anchor \
                 this rule exists for"
            );
            assert!(
                src.contains("Math.min(r.right, innerWidth)"),
                "{name}: the acknowledgement pass does not clamp the rect to the viewport"
            );
        }
        // (The emitter's own guard — every borrowed helper must be DEFINED in the text it prints — is
        // enforced where it can bite: each `--emit` branch calls `assertEmbedded`, the pre-commit hook runs
        // all of them, and `panel-design-sweep.bash` asserts the three emitters still call it.)

        // THE SECOND: the wiring, which is everything between here and the top of this block.
        n += 1;
    }

    let _ = std::fs::remove_dir_all(&landing_out);
    assert_eq!(
        n, 9,
        "the check count is the `.mjs`'s own: three rule cases and two per artifact"
    );
    println!("press-anchor: all {n} checks passed");
}
