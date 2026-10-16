#!/usr/bin/env bash
# update-model-catalogue.sh — FETCH `models.dev/api.json` AND EXTRACT THE SLICE THIS REPO COMMITS.
#
# WHY A SCRIPT RATHER THAN A HAND-COPIED FILE: the slice is DATA from a third party (`models.dev` is not ours —
# `agent/tests/production_host.rs` would refuse it as a production hostname if it appeared in product code, and this
# tool is not product code). Committing the extracted slice means the worker never fetches at runtime; the script is
# how the slice is refreshed, and the commit message carries the byte count and the date it was taken, which is what
# makes a stale slice visible.
#
# **THE FETCH IS RESUMABLE, AND THAT IS MEASURED RATHER THAN DEFENSIVE.** Measured 2026-10-16 from this box: the
# document is **5,671,659 bytes** (it was 2,006,627 when the plan was written — the catalogue grows) and this link
# delivers ~20 KB/s to that host, so a single `curl` TIMED OUT at 4.76 MB having received most of the document and
# none of a parse. `curl -C -` continues where the last attempt stopped, and the loop below keeps resuming until the
# size stops moving — which is the only reliable proof that a 5.7 MB transfer finished on a link this slow.
#
#     bash scripts/update-model-catalogue.sh --probe     # fetch, then PRINT the schema it found (no writes)
#     bash scripts/update-model-catalogue.sh             # fetch, then write the two slice files
set -uo pipefail
cd "$(dirname "$0")/.." || exit 2

URL="${MODELS_DEV_URL:-https://models.dev/api.json}"
CACHE="${MODELS_DEV_CACHE:-/tmp/models-dev-api.json}"
MAX_ATTEMPTS="${MODELS_DEV_ATTEMPTS:-20}"

fetch() {
  local attempt=0 last=0 now=0
  while [ "$attempt" -lt "$MAX_ATTEMPTS" ]; do
    attempt=$((attempt + 1))
    now=$(stat -c %s "$CACHE" 2>/dev/null || echo 0)
    if [ "$now" = "$last" ] && [ "$now" != 0 ] && [ "$attempt" -gt 1 ]; then
      # The size did not move across a full attempt: either it is complete or the host stopped answering.
      if python3 -c "import json,sys; json.load(open('$CACHE'))" 2>/dev/null; then
        echo "catalogue: complete after $attempt attempt(s) — $now bytes" >&2
        return 0
      fi
    fi
    last="$now"
    curl -sS -C - --retry 2 --retry-connrefused --max-time 100 -o "$CACHE" "$URL" >/dev/null 2>&1
    now=$(stat -c %s "$CACHE" 2>/dev/null || echo 0)
    echo "catalogue: attempt $attempt — $now bytes" >&2
    if [ "$now" = "$last" ] && [ "$attempt" -gt 3 ]; then
      echo "catalogue: the size stopped moving at $now bytes and it does not parse — giving up" >&2
      return 1
    fi
  done
  echo "catalogue: $MAX_ATTEMPTS attempts without a complete document" >&2
  return 1
}

fetch || exit 1

python3 - "$CACHE" "$@" <<'PY'
import json, sys, os
cache, mode = sys.argv[1], (sys.argv[2] if len(sys.argv) > 2 else "")
doc = json.load(open(cache))
providers = {k: v for k, v in doc.items() if k in ("opencode", "opencode-go")}
if mode == "--probe":
    print("providers in the document:", len(doc))
    for name, p in providers.items():
        models = p.get("models", {})
        print(f"  {name}: {len(models)} models; provider keys: {sorted(p.keys())}")
        if models:
            one_id, one = next(iter(models.items()))
            print(f"    sample {one_id}: {sorted(one.keys())}")
            for field in ("cost", "limit", "modalities", "reasoning", "tool_call", "attachment"):
                if field in one:
                    print(f"      {field} = {json.dumps(one[field])[:160]}")
    sys.exit(0)
# ── THE MAPPING, WRITTEN AGAINST THE SCHEMA AS IT WAS READ (2026-10-16) ─────────────────────────────
#
# Read from the live document with `--probe`, not from memory: a model carries `cost{input,output,…}`,
# `limit{context,output,…}`, `modalities{input:[…],output:[…]}`, `status` and a `name`. **THE PLAN'S SIXTH FACT HAS
# NO SOURCE**: it says "`Shape` from `provider.shape`", and a provider in this document carries `api` (a URL), `doc`,
# `env`, `id`, `models`, `name`, `npm` (an SDK package) — **there is no shape field**, and neither the provider nor
# the model says whether a model answers `completions` or `responses`. The slice therefore omits it rather than
# inventing one, because `None` is what "the source does not say" means and a guessed dialect is how a row gets
# adopted onto the wrong arm.
def facts(model):
    cost = model.get("cost") or {}
    limit = model.get("limit") or {}
    modalities = model.get("modalities") or {}
    inputs = modalities.get("input") or []
    return {
        "input_cost": cost.get("input"),
        "output_cost": cost.get("output"),
        "context": limit.get("context"),
        "output": limit.get("output"),
        "status": model.get("status"),
        "vision": "image" in inputs,
    }

out_dir = "gateway/wasm/src/catalogue"
os.makedirs(out_dir, exist_ok=True)
for name, provider in providers.items():
    models = provider.get("models") or {}
    rows = []
    for model_id, model in models.items():
        if not isinstance(model_id, str) or not model_id or "/" in model_id or model_id.startswith("."):
            continue
        row = {"id": model_id}
        row.update(facts(model))
        rows.append(row)
    rows.sort(key=lambda r: r["id"])
    path = os.path.join(out_dir, f"{name}.json")
    with open(path, "w", encoding="utf8") as handle:
        json.dump(rows, handle, indent=2, ensure_ascii=True, sort_keys=False)
        handle.write("\n")
    size = os.path.getsize(path)
    print(f"{path}: {len(rows)} rows, {size} bytes")
PY
