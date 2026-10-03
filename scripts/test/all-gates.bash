#!/usr/bin/env bash
# Run every gate `ci.yml` invokes, locally, in one command.
#
# WHY THIS EXISTS, and it is a real cost rather than tidiness: AGENTS.md's table lists the
# PER-DIRECTORY test commands (`npm test`, `cargo test`, clippy, fmt) and NOT the forty-odd gate
# scripts the `pack-chain` job runs one step at a time. So "run the command the other end runs" — the
# rule that table exists to enforce — could not be satisfied by reading it, and round 41 paid:
# `contract-vocabulary-check` failed CI over a test fixture while every command in the table was green.
#
# THE LIST IS DERIVED, NOT RESTATED. It is read out of the workflow on every run, so a gate added to
# CI is covered here the moment it is added, and this file can never become a second list that drifts
# from the first.
#
# A gate that exits 2 is declaring "this host cannot run me" (the design sweep needs a browser and says
# so); that is reported as `n/a`, never as a pass and never as a failure.
#
# A gate that exits 3 is declaring the OPPOSITE: "I ran, and I could not measure" — a floor that read
# nothing, patterns that went stale. That is counted as a FAILURE, because an instrument that proves
# nothing is not an exemption (round 172: three gates printed FAIL and exited 2, and this script
# reported them as n/a while exiting 0).
#
# WHAT THIS DOES NOT COVER, learned the hard way on 2026-09-24: this is HALF of "what CI runs". It runs
# the GATE COMMANDS, not the per-directory SUITES in AGENTS.md's table — `gateway`'s
# typecheck/lint/format/test, `gateway/ui`'s build+test, `agent/resources/panel-react`'s, the three
# cargo configurations, and `cd agent/summrise-desktop-electron && npm test`. The stale Source Viewer
# mirror that this omission hid is checked by `gateway`'s suite (`code-viewer-mirror.test.mjs`), which
# is not among the commands below, so run BOTH halves before believing a tree is green:
#
#     bash scripts/test/all-gates.bash && (cd gateway && npm run typecheck && npm run lint \
#       && npm run format:check && npm test)
set -uo pipefail
cd "$(dirname "$0")/../.." || exit 1

WORKFLOW=".github/workflows/ci.yml"
[ -f "$WORKFLOW" ] || { echo "  no $WORKFLOW — run from the repo" >&2; exit 1; }

mapfile -t gates < <(
  # THE PATH PREFIX IS STRIPPED, NOT MATCHED AROUND (round 170). ci.yml invokes one gate as
  # `node ${{ github.workspace }}/scripts/test/console-assets-check.mjs`, and the old pattern —
  # which required the script path to start immediately after the interpreter — never matched it.
  # So that gate was named in the workflow and INVISIBLE to this runner, which is precisely the
  # drift the header promises cannot happen ("THE LIST IS DERIVED, NOT RESTATED ... can never
  # become a second list that drifts from the first"). MEASURED by the fifteenth exploration:
  # 1 mention in ci.yml, 0 derived here.
  #
  # The strip runs FIRST, and that ordering is the whole fix: the prefix contains a SPACE, so a
  # character class in the regex cannot span it — my first attempt widened the pattern to
  # `[^ "]*`, which still excluded the prefix and still derived zero.
  sed 's|\${{ github.workspace }}/||g' "$WORKFLOW" |
    grep -ohE '(node|bash|python3) +scripts/test/[A-Za-z0-9._-]+' |
    sort -u |
    # ITSELF, EXCLUDED — load-bearing rather than tidy: this file is invoked from ci.yml (build-pins
    # enforces that EVERY scripts/test file is), so the extraction finds it, and running it would run
    # this script again, forever. It is the one command here that is a superset of the others rather
    # than a peer. Filtered out rather than blanked in the array, so the counts below stay exact.
    grep -v '^bash scripts/test/all-gates\.bash$'
)
# THE FLOOR IS A FLOOR, NOT A FORMALITY (round 170). It was 20 against a list of 50, so HALVING the
# workflow still passed it — a check that cannot notice half its subject missing is the vacuity this
# file exists to catch in others. 40 leaves room for a deliberate removal and none for a collapse.
#
# AND THE FLOOR ITSELF HAS TO FOLLOW THE MIGRATION DOWN, which is why it is 30 as of batch 6 of the P1
# gate migration. MEASURED: 43 commands were derived before that batch and 34 after it, because EIGHT
# gates moved into `agent/tests/*.rs` and each one's `run:` line went with its `.mjs`. The `cargo test`
# steps that run them now are added AFTER this check, so they cannot hold this count up — and batch 5's
# three removals had already taken 43 to 40, exactly ON the floor, before batch 6 took it to 34, where
# this file refused to run at all. A floor that a deliberate, verified migration crosses silently stops
# being a floor and becomes an outage; a floor lowered without a measurement stops being one too. 30
# leaves room for four more moves and none for a collapse.
#
# AND IT IS 26 AS OF THE NEXT TWO MOVES, because 30 no longer left room for four — it left room for
# NONE. MEASURED, with the derivation above run against each tree in turn: `main` derives 32, and each
# of the two gates that landed together takes it down by one — `docs-budget-check.mjs` (31) and
# `chrome-stillness-check.mjs` (30). Thirty is not a number this file tolerates quietly: the comparison
# is `-lt 30`, so the tree passes ON the floor and the NEXT single removal takes it to 29, where this
# file refuses to run at all — the exact outage batch 6 created and this paragraph exists to prevent.
# 26 is two below the count the two removals leave and four below `main`, which is the same margin the
# paragraph above describes. The number moved because the tree moved, and the measurement is the reason.
#
# AND THE RULE IS NOW WRITTEN AS A RULE, BECAUSE THE NUMBER KEPT NEEDING A PARAGRAPH. Three migrations
# have each lowered it by hand and each wrote its own justification, which is how a floor becomes a
# number somebody edits rather than a margin somebody checks. THE RULE: THE FLOOR IS THE DERIVED COUNT
# MINUS FOUR. Four is the margin the round-170 paragraph chose ("leaves room for a deliberate removal
# and none for a collapse"), and it is a margin in MOVES, not in gates — so it is re-measured the same
# way each time: run the derivation above against the tree, subtract four.
#
# MEASURED FOR THIS MOVE: `workflow-yaml-check.mjs` moves to `agent/tests/workflow_yaml.rs` and the
# derivation reads 29, so the floor is 25. (The two moves before it: 32 → 31 → 30, floor 26.)
#
# AND AGAIN FOR THE NEXT ONE, BY THE SAME RULE: `motion-check.mjs` moves to
# `agent/tests/motion_check.rs` and the derivation reads 28, so the floor is 24. THE RULE IS APPLIED
# RATHER THAN REMEMBERED — the derivation above is run against the tree and four is subtracted — which
# is what this paragraph is for; the numbers are the record of it being done.
#
# AND AGAIN: `particles-check.mjs` moves to `agent/tests/particles_check.rs` and the derivation reads
# 27, so the floor is 23.
#
# AND AGAIN: `console-marks-check.mjs` moves to `agent/tests/console_marks.rs` and the derivation
# reads 26, so the floor is 22.
#
# AND AGAIN: `workflow-shell-check.mjs` moves to `agent/tests/workflow_shell.rs` and the derivation
# reads 25, so the floor is 21.
#
# AND AGAIN: `state-colour-check.mjs` moves to `agent/tests/state_colour.rs` and the derivation
# reads 24, so the floor is 20.
#
# AND AGAIN: `feedback-check.mjs` moves to `agent/tests/feedback.rs` and the derivation reads 23,
# so the floor is 19.
if [ "${#gates[@]}" -lt 19 ]; then
  echo "  read only ${#gates[@]} gate command(s) from $WORKFLOW — the workflow moved, so this proves nothing" >&2
  exit 1
fi

# THE RUST GATES ARE A CI JOB, NOT A `scripts/test/` SCRIPT — AND THE DERIVATION ABOVE CANNOT SEE
# THEM. `cargo test -p summrise-agent` runs in ci.yml's `agent` job under `working-directory: agent`,
# and the extraction above matches `node|bash|python3 scripts/test/…` only. So the gates that have
# MOVED into `agent/tests/*.rs` would be invisible here — and "invisible to the local run" reads
# exactly like "passed". This is that reach, restored as a derived command rather than a promise:
# the same invocation CI runs, from the same directory. `cargo:` marks the entries that need it,
# because a relative path resolved from the repo root is the one mistake a migrated gate can make
# and this is the file that would hide it.
mapfile -t cargo_gates < <(
  # THE PATTERN IS ANCHORED ON THE WHOLE INVOCATION, not on its prefix. A looser one
  # (`cargo test -p summrise-agent([^ ]*)`) cannot span the space before `--features`, so it
  # silently derives the BARE command and drops the flag — a pattern that truncates its subject is
  # the defect this suite exists to catch, and it would have been committed here.
  #
  # AND IT NOW COVERS `clippy` AND `fmt`, WHICH IT DID NOT UNTIL 2026-09-29 — MEASURED, AND IT COST A
  # RED `main`. The pattern was `cargo test …` alone, so this runner answered `25 ok, 0 failed` on a
  # tree whose `cargo fmt --all -- --check` FAILED, and CI's `agent` job found it: a targeted edit to
  # `agent/tests/wire_fields.rs` was made after the last `cargo fmt --all`, rustfmt reformats that
  # closure, and the format check was the one command the local runner did not run. **The runner's
  # blind spot was exactly the command the change could break** — this repository's own rule, "run the
  # command the other end runs", with this file as the other end.
  #
  # THE ANCHOR IS `^` AND NOT A BARE `cargo …` SEARCH, and the reason is the job's own `name:` line:
  # `name: agent (cargo test + clippy + fmt)` contains all three verbs, so an unanchored pattern
  # derives the sentence `cargo test + clippy + fmt)` and tries to run it. Anchoring on the line start
  # with an optional `run: ` is what separates an INVOCATION from prose about one.
  grep -oE '^[[:space:]]*(run: )?cargo (test|clippy|fmt) [^"]*$' "$WORKFLOW" |
    sed -E 's/^[[:space:]]*(run: )?//; s/[[:space:]]+$//' |
    sort -u |
    sed 's/^/cargo:/'
)
gates+=("${cargo_gates[@]}")

# **EVERY CARGO STEP MUST SAY WHERE IT RUNS, AND THIS IS THE CHECK FOR THE LINE THAT WENT MISSING.**
# The runner below hardcodes `cd agent` for these commands, because every one of them means the agent's own
# crate. The WORKFLOW does not hardcode it — each step carries its own `working-directory:` — so the two only
# agree while every such step has one. On 2026-10-02 a text-range patch that ended at the next `- name:` ate
# `working-directory: agent` from the Tests step: CI began running `cargo test` from the repo root (no
# `Cargo.toml` there) and every local run stayed green, because this script's `cd agent` was still in place.
# The failure read `error: could not find Cargo.toml in /home/runner/work/summrise/summrise`, and the loop
# called the local red "the expected three" for two rounds.
#
# THE SHAPE CHECKED IS THE ONE THAT BROKE: for each extracted command, the lines between it and the NEXT
# `- name:` step must declare a `working-directory:`.
missing=0
while IFS=: read -r lineno rest; do
  [ -n "$lineno" ] || continue
  # THE SAME EXEMPTION THE RUNNER BELOW MAKES, for the same reason: a command that NAMES ITS MANIFEST runs
  # from anywhere, and `proxies/*/worker/Cargo.toml` only resolves from the root.
  case $rest in *--manifest-path*) continue ;; esac
  found=$(awk -v start="$lineno" '
    NR > start {
      if ($0 ~ /^[[:space:]]*- name:/) exit
      if ($0 ~ /^[[:space:]]*working-directory:/) { print "yes"; exit }
    }' "$WORKFLOW")
  if [ "$found" != "yes" ]; then
    echo "  FAIL  the cargo step at $WORKFLOW:$lineno declares no working-directory"
    missing=$((missing + 1))
  fi
done < <(grep -nE '^[[:space:]]*(run: )?cargo (test|clippy|fmt) [^"]*$' "$WORKFLOW")
if [ "$missing" -gt 0 ]; then
  echo "  a cargo step without a working-directory runs from the REPO ROOT in CI and from agent/ here —"
  echo "  the two ends disagree, which is the one thing this script exists to prevent."
  exit 1
fi

fail=0
notrun=0
ok=0
for cmd in "${gates[@]}"; do
  case $cmd in
    # A CARGO COMMAND THAT NAMES ITS OWN MANIFEST RUNS FROM THE REPO ROOT (2026-09-29). Every cargo
    # line here meant the agent's own crate, so the runner hardcoded `cd agent` — and a crate OUTSIDE
    # that workspace (`index/worker/`, block ④ of the migration) has a path that only resolves from
    # the root. Run from `agent/` it failed with "could not read Cargo.toml", which reads exactly like
    # a red crate and is nothing of the kind: it is the right command in the wrong place, which is the
    # shape `docs/superpowers/plans/2026-09-28-migration-error-prevention.md` names.
    # **`bash -c`, BECAUSE A REDIRECT IS NOT A WORD.** The command is extracted from the workflow as TEXT
    # and expanded here, and expansion happens AFTER parsing — so `cargo test -p x > /tmp/y 2>&1`, run as
    # `${cmd#cargo:}`, hands `>` and `2>&1` to cargo as ARGUMENTS. Measured 2026-10-02: the three agent test
    # commands in the workflow gained redirects (to keep the first run's output for the CI diagnostic) and
    # this script answered `Usage: cargo test [OPTIONS] [TESTNAME]`. `bash -c` re-parses the text as a
    # command, which is what it is.
    cargo:*--manifest-path*) out=$( (bash -c "${cmd#cargo:}") 2>&1) ;;
    cargo:*) out=$( (cd agent && bash -c "${cmd#cargo:}") 2>&1) ;;
    *)       out=$($cmd 2>&1) ;;
  esac
  code=$?
  case $code in
    # A GATE THAT SAID NOTHING HAS PROVED NOTHING (round 175). Exit 0 with EMPTY output is
    # indistinguishable from a gate that measured, and every gate here prints a summary line — that is
    # the convention this suite reads. So empty output is a FAILURE, not a pass, and it is labelled so
    # the reader knows it was not the gate s verdict that failed but the gate s silence.
    0)
      # ONE NAMED EXCEPTION, AND IT IS NAMED RATHER THAN PATTERN-MATCHED SO A SECOND ONE CANNOT RIDE
      # ALONG UNNOTICED. `cargo fmt --all -- --check` is a CHECKER whose silence IS its pass: rustfmt
      # prints nothing when everything is formatted, and it exits 0. Every other command here reports,
      # which is the convention the rule below reads — so this one is written out, with the reason, and
      # the rule is left intact for everything else. Found by widening this runner to the clippy/fmt
      # steps (2026-09-29): the first run called a perfectly formatted tree a vacuous gate.
      case "$cmd" in
        "cargo:cargo fmt --all -- --check")
          ok=$((ok + 1))
          printf '  ok    %s (silent by design: rustfmt prints nothing when the tree is formatted)\n' "$cmd"
          continue
          ;;
      esac
      if [ -z "$out" ]; then
        fail=$((fail + 1))
        printf '  FAIL  %s (exited 0 having printed NOTHING — a gate that says nothing has proved nothing)\n' "$cmd"
      else
        ok=$((ok + 1)); printf '  ok    %s\n' "$cmd"
      fi
      ;;
    2) notrun=$((notrun + 1)); printf '  n/a   %s (declared it cannot run here)\n' "$cmd" ;;
    # 3 IS A FAILURE, NOT AN EXEMPTION (round 172). A gate that ran and could not measure — its patterns
    # went stale, a floor read nothing — has proved nothing, and "proved nothing" must not read as "not
    # applicable". Three gates printed FAIL and exited 2, which this case used to swallow.
    # A FAILING GATE MUST NAME WHAT FAILED, AND `tail -4` IS WHERE THAT NAME WAS THROWN AWAY.
    # MEASURED on `3024bc5e`: this runner reported `cargo test -p summrise-agent --features
    # terminal,keyring` as "756 passed; 1 failed" and NOTHING ELSE — because the name of the failing
    # test sits in cargo's `failures:` block, a dozen lines ABOVE the summary that `tail -4` keeps.
    # The next round was left holding a red it could not act on, and the cost of finding that out was
    # a CI cycle. **A GATE WHOSE OUTPUT DOES NOT NAME THE FAILURE IS A GATE SOMEBODY RE-RUNS HOPING.**
    # So the naming lines come FIRST and the tail is kept after them: nothing that used to be printed
    # is removed, and what is added is the one line a reader needs in order to act.
    #   * `^failures:` and the test paths under it — cargo's own list, which is the measured case. The
    #     indented arm is anchored on a Rust test PATH (`    module::path::name`), so it does not also
    #     collect cargo's `    Finished …` line, which the first version of this did.
    #   * `panicked at` — a unit test's panic line, which cargo prints above that list.
    #   * `FAILED` — every reporter in this repository ends with one (the suite has three).
    # `awk '!seen[$0]++'` rather than two prints: the summary line is in BOTH blocks, and a report that
    # repeats itself is a report a reader learns to skim. `pipefail` is on and awk exits 0, so the
    # pipeline's status stays what it was.
    3) fail=$((fail + 1)); printf '  FAIL  %s (ran but could not measure)\n' "$cmd"
       { printf '%s\n' "$out" | grep -E '^(failures:|    [a-z_][A-Za-z0-9_:]*$)|panicked at|FAILED' | head -12
         printf '%s\n' "$out" | tail -4; } | awk '!seen[$0]++' | sed 's/^/          /' ;;
    *) fail=$((fail + 1)); printf '  FAIL  %s\n' "$cmd"
       { printf '%s\n' "$out" | grep -E '^(failures:|    [a-z_][A-Za-z0-9_:]*$)|panicked at|FAILED' | head -12
         printf '%s\n' "$out" | tail -4; } | awk '!seen[$0]++' | sed 's/^/          /' ;;
  esac
done

printf '\n  %d ok, %d failed, %d not runnable here (of %d gate command(s) in %s)\n' \
  "$ok" "$fail" "$notrun" "${#gates[@]}" "$WORKFLOW"
[ "$fail" -eq 0 ]
