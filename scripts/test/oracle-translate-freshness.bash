#!/usr/bin/env bash
# oracle-translate-freshness — DOES THE SHIPPING TYPESCRIPT STILL ANSWER WHAT THE CORPUS RECORDS?
#
# `gateway/wasm/fixtures/translate-corpus.json` is this repository's largest differential (621 cases) and every
# Rust test in the worker reads it. **UNTIL 2026-10-15 IT COULD NOT BE RE-RECORDED**: its Rust tests name a
# producer (`gateway/wasm/oracle-translate.mjs`) that was not in the tree, so the fixture could only ever be
# compared against a snapshot — and a snapshot cannot tell you that the SHIPPING side moved.
#
# The oracle is back, in REPLAY mode: it feeds each recorded input through the LIVE console code and compares
# against the recorded answer. This gate is that comparison, and it is the one that fires when somebody edits
# `execution/src/routing-logic.ts` (or anything it renders from) in a way the worker does not follow.
#
# **IT COVERS THE FUNCTIONS WHOSE TYPESCRIPT HOME STILL EXISTS** — the five the oracle names. The functions whose
# shipping implementation was deleted at the `/v1` cutover are deliberately NOT replayed: for those the snapshot IS
# the record.
#
# Exit codes follow this repository's convention: 2 means "cannot run here" (no node), 0 clean, 1 red.
set -uo pipefail
cd "$(dirname "$0")/../.." || exit 2

if ! command -v node >/dev/null 2>&1; then
  echo "oracle-translate-freshness: n/a no node on PATH — a host without it cannot make this judgement"
  exit 2
fi

out=$(cd gateway/wasm && node oracle-translate.mjs --check 2>&1)
st=$?
printf '%s\n' "$out" | sed 's/^/  /'
if [ "$st" != 0 ]; then
  echo "  FAIL the live console code no longer answers what fixtures/translate-corpus.json records"
  echo "       re-record ONLY after deciding the change is right:  cd gateway/wasm && node oracle-translate.mjs"
  exit 1
fi
# **A FLOOR, BECAUSE A REPLAY THAT MATCHED NOTHING WOULD ALSO EXIT 0**: the oracle prints how many cases it
# replayed, and the gate refuses a run that covered fewer than the five functions' recorded cases.
if ! printf '%s' "$out" | grep -qE 'replayed [0-9]+ recorded case'; then
  echo "  FAIL the oracle did not report a replay count — a gate that cannot say what it measured is not one"
  exit 1
fi
echo "oracle-translate-freshness: the live cases still answer what the corpus records"
