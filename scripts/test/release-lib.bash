#!/usr/bin/env bash
# release-lib.bash — regression tests for the extracted publish-release
# stages (last-5-per-minor prune + version.json writer). Plain bash asserts,
# no framework; exit 0 = all green. Run: bash scripts/test/release-lib.bash
set -euo pipefail
cd "$(dirname "$0")/../.."
source "scripts/lib/release-lib.sh"

PASS=0
check() { # check <desc> <actual> <expected>
  if [ "$2" = "$3" ]; then PASS=$((PASS+1)); else
    echo "FAIL: $1"; echo "  actual:   $2"; echo "  expected: $3"; exit 1
  fi
}
check_match() { # check_match <desc> <string> <regex>
  if [[ "$2" =~ $3 ]]; then PASS=$((PASS+1)); else
    echo "FAIL: $1"; echo "  string: $2"; echo "  wanted to match: $3"; exit 1
  fi
}

# ── last-5-per-minor prune ────────────────────────────────────────────────
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT

mk() { for v in "$@"; do echo "payload-$v" > "$T/summrise-agent-$v.tgz"; done; }
remaining() { ls "$T"/summrise-agent-1.*.*.tgz 2>/dev/null | xargs -r -n1 basename | sort -V | tr '\n' ' '; }

# 1. Seven 1.2.x files + three 1.3.x: keep newest 5 of 1.2 and ALL of 1.3 —
#    the new line must never evict the pinned old line.
mk 1.2.270 1.2.271 1.2.272 1.2.273 1.2.274 1.2.275 1.2.276 1.3.0 1.3.1 1.3.2
echo x > "$T/summrise-agent-latest.tgz"
echo keepme > "$T/unrelated.txt"
prune_last5_per_minor "$T" >/dev/null
check "1.2 keeps newest 5, 1.3 keeps all" "$(remaining)" "summrise-agent-1.2.272.tgz summrise-agent-1.2.273.tgz summrise-agent-1.2.274.tgz summrise-agent-1.2.275.tgz summrise-agent-1.2.276.tgz summrise-agent-1.3.0.tgz summrise-agent-1.3.1.tgz summrise-agent-1.3.2.tgz "
check "1.3 keeps all 3 (under the cap)" \
  "$(ls "$T"/summrise-agent-1.3.*.tgz | xargs -r -n1 basename | sort -V | tr '\n' ' ')" \
  "summrise-agent-1.3.0.tgz summrise-agent-1.3.1.tgz summrise-agent-1.3.2.tgz "
check "latest alias untouched" "$(cat "$T/summrise-agent-latest.tgz")" "x"
check "unrelated files untouched" "$(cat "$T/unrelated.txt")" "keepme"

# 2. Exact-pattern discipline: dot-versions are NOT matched by the hyphen
#    glob analogue, and non-tgz versioned names stay put. 1.2.9 < 1.2.10
#    must sort by VERSION (sort -V), not lexicographically.
rm -rf "$T" && mkdir -p "$T"
mk 1.2.9 1.2.10 1.2.2
echo keep > "$T/summrise-agent-latest.tgz"
echo other > "$T/summrise-agent-2.0.0.tgz"
prune_last5_per_minor "$T" >/dev/null
check "sort -V ordering (9 < 10)" "$(remaining)" "summrise-agent-1.2.2.tgz summrise-agent-1.2.9.tgz summrise-agent-1.2.10.tgz "
check "major 2.x outside the 1.*.* policy" "$(cat "$T/summrise-agent-2.0.0.tgz")" "other"

# 3. Empty dir: nullglob semantics — no literal-pattern rm, no error.
rm -rf "$T" && mkdir -p "$T"
out=$(prune_last5_per_minor "$T")
check "empty asset dir prunes nothing and stays silent" "$out" ""

# 4. Exactly five in one line: nothing pruned (boundary of the -4 arithmetic).
rm -rf "$T" && mkdir -p "$T"
mk 1.2.1 1.2.2 1.2.3 1.2.4 1.2.5
prune_last5_per_minor "$T" >/dev/null
check "exactly 5 keeps all 5" "$(remaining)" "summrise-agent-1.2.1.tgz summrise-agent-1.2.2.tgz summrise-agent-1.2.3.tgz summrise-agent-1.2.4.tgz summrise-agent-1.2.5.tgz "

# ── version.json writer ──────────────────────────────────────────────────
echo "payload for sha" > "$T/payload.tgz"
WANT_SHA=$(sha256sum "$T/payload.tgz" | cut -d' ' -f1)
GOT_SHA=$(write_version_json "1.2.297" "$T/payload.tgz" "$T")
check "writer echoes the sha" "$GOT_SHA" "$WANT_SHA"
check_match "manifest is valid JSON with version+tarball" \
  "$(cat "$T/version.json")" \
  '^\{"version":"1\.2\.297","tarball":"summrise-agent-latest\.tgz","updated":"[0-9TZ:+-]+","sha256":"[0-9a-f]{64}"(,"components":\{.*\})?\}$'
# The written sha must equal an independent hash of the tgz (agent_update
# REFUSES installs without a correct sha — round-119).
WRITTEN_SHA=$(node -p "JSON.parse(require('fs').readFileSync('$T/version.json','utf8')).sha256")
check "manifest sha256 matches the packed tgz" "$WRITTEN_SHA" "$WANT_SHA"
# The components block is ADDITIVE and CWD-DEPENDENT BY DESIGN (grilling Q4): run
# where index/components.json exists — the repo — and the manifest pins the three
# boxed components `summrise setup` fetches and verifies; run where it does not and
# the shape is exactly what it always was, so an older consumer sees nothing new.
check "manifest written from the repo pins the three components" \
  "$(node -p "JSON.stringify(Object.keys(JSON.parse(require('fs').readFileSync('$T/version.json','utf8')).components).sort())")" \
  '["cloudflared","electron","playwright"]'
( cd "$T" && write_version_json "1.2.297" "$T/payload.tgz" "$T" >/dev/null )
check "with no index/components.json in sight the manifest has NO components key" \
  "$(node -p "JSON.stringify(Object.keys(JSON.parse(require('fs').readFileSync('$T/version.json','utf8'))).sort())")" \
  '["sha256","tarball","updated","version"]'
# No installer staged: the manifest keeps the tgz + components shape (old consumers
# ignore nothing, new consumers treat missing installer fields as absent).
write_version_json "1.2.297" "$T/payload.tgz" "$T" >/dev/null
check "no-installer manifest carries no installer fields" \
  "$(node -p "Object.keys(JSON.parse(require('fs').readFileSync('$T/version.json','utf8'))).filter(k=>k.startsWith('installer')).join(',')")" \
  ''

# With a staged installer: additive installer + installer_sha256 fields.
echo "fake-exe-payload" > "$T/SummriseAgent-Setup-1.2.297.exe"
WANT_ISH=$(sha256sum "$T/SummriseAgent-Setup-1.2.297.exe" | cut -d' ' -f1)
GOT_SHA2=$(write_version_json "1.2.297" "$T/payload.tgz" "$T" "$T/SummriseAgent-Setup-1.2.297.exe")
check "writer with installer still echoes the tgz sha" "$GOT_SHA2" "$WANT_SHA"
check "manifest installer basename" \
  "$(node -p "JSON.parse(require('fs').readFileSync('$T/version.json','utf8')).installer")" \
  "SummriseAgent-Setup-1.2.297.exe"
check "manifest installer_sha256 matches the staged exe" \
  "$(node -p "JSON.parse(require('fs').readFileSync('$T/version.json','utf8')).installer_sha256")" \
  "$WANT_ISH"
# Missing installer path: falls back to the tgz-only shape (never writes a
# dangling installer name). Asserted on the INSTALLER keys rather than the whole
# key list: the components block is additive, and a test that pins every key turns
# every future addition into a red suite for no reason.
write_version_json "1.2.297" "$T/payload.tgz" "$T" "$T/SummriseAgent-Setup-9.9.9.exe" >/dev/null
check "dangling installer path writes NO installer fields" \
  "$(node -p "Object.keys(JSON.parse(require('fs').readFileSync('$T/version.json','utf8'))).filter(k=>k.startsWith('installer')).join(',')")" \
  ''

# ── installer prune ────────────────────────────────────────────────────
rm -rf "$T" && mkdir -p "$T"
for v in 1.2.300 1.2.301 1.2.302 1.2.303 1.2.304 1.2.305 1.2.306; do echo "exe-$v" > "$T/SummriseAgent-Setup-$v.exe"; done
echo "alias" > "$T/SummriseAgent-Setup.exe"
echo keep > "$T/unrelated.txt"
prune_installers "$T" >/dev/null
check "installer prune keeps newest 5 versioned" \
  "$(ls "$T"/SummriseAgent-Setup-1.*.*.exe 2>/dev/null | xargs -r -n1 basename | sort -V | tr '\n' ' ')" \
  "SummriseAgent-Setup-1.2.302.exe SummriseAgent-Setup-1.2.303.exe SummriseAgent-Setup-1.2.304.exe SummriseAgent-Setup-1.2.305.exe SummriseAgent-Setup-1.2.306.exe "
check "installer alias untouched" "$(cat "$T/SummriseAgent-Setup.exe")" "alias"
check "installer prune leaves unrelated files" "$(cat "$T/unrelated.txt")" "keep"
# 4b. TWO minor lines: the installer prune must be PER MINOR, like the tgz policy
#     its own comment calls it a companion to. A flat last-5 evicts 1.2.x the
#     moment 1.3.x ships five — and a versioned installer pins the tgz it was
#     built against, so the rollback path the tgz policy protects goes with it.
#     THE ONE-MINOR FIXTURE ABOVE CANNOT TELL THE TWO POLICIES APART (round 127).
rm -rf "$T" && mkdir -p "$T"
mkexe() { for v in "$@"; do echo "payload-$v" > "$T/SummriseAgent-Setup-$v.exe"; done; }
mkexe 1.2.301 1.2.302 1.2.303 1.2.304 1.2.305 1.3.0 1.3.1 1.3.2 1.3.3 1.3.4 1.3.5
echo alias > "$T/SummriseAgent-Setup.exe"
prune_installers "$T" >/dev/null
check "installer prune is PER MINOR: every 1.2 survives a 1.3 line at the cap" \
  "$(ls "$T"/SummriseAgent-Setup-1.*.*.exe 2>/dev/null | xargs -r -n1 basename | sort -V | tr '\n' ' ')" \
  "SummriseAgent-Setup-1.2.301.exe SummriseAgent-Setup-1.2.302.exe SummriseAgent-Setup-1.2.303.exe SummriseAgent-Setup-1.2.304.exe SummriseAgent-Setup-1.2.305.exe SummriseAgent-Setup-1.3.1.exe SummriseAgent-Setup-1.3.2.exe SummriseAgent-Setup-1.3.3.exe SummriseAgent-Setup-1.3.4.exe SummriseAgent-Setup-1.3.5.exe "
check "and the new line is still capped at 5" \
  "$(ls "$T"/SummriseAgent-Setup-1.3.*.exe 2>/dev/null | wc -l)" "5"
check "installer alias untouched by the two-minor prune" "$(cat "$T/SummriseAgent-Setup.exe")" "alias"

rm -rf "$T" && mkdir -p "$T"
out=$(prune_installers "$T")
check "empty asset dir installer-prunes nothing and stays silent" "$out" ""

# ── the reconcile ledger was deleted 2026-09-14 ─────────────────────────────
# The publish step stopped writing it: it produced a markdown file per release and a
# gate that had to be satisfied by hand. What it recorded — "this version's CDN bytes
# were never compared against the GitHub asset" — is now the audit's own output.


# ── a SETTLED debt must leave the ledger (round 265) ────────────────────────
# The audit-only path printed "audit: v1.2.450 settled" and left the entry in place, so the debt outlived the
# audit that discharged it and the next publish refused. Two checks, because the defect was a CALL that was
# missing rather than a function that was wrong: the pair behaves, and the call site is where it has to happen.
L="$T/reconcile.txt"
RECONCILE_LEDGER="$L" reconcile_record "1.2.450" "test" >/dev/null
RECONCILE_LEDGER="$L" reconcile_record "1.2.451" "test" >/dev/null
check "two debts are owed" "$(RECONCILE_LEDGER="$L" reconcile_pending | tr '\n' ' ')" "1.2.450 1.2.451 "
RECONCILE_LEDGER="$L" reconcile_clear "1.2.450"
check "a settled version leaves the ledger" "$(RECONCILE_LEDGER="$L" reconcile_pending | tr '\n' ' ')" "1.2.451 "
RECONCILE_LEDGER="$L" reconcile_record "1.2.450" "test" >/dev/null
RECONCILE_LEDGER="$L" reconcile_clear "1.2.450"
check "...and clearing it twice is not an error" "$(RECONCILE_LEDGER="$L" reconcile_pending | tr '\n' ' ')" "1.2.451 "
# THE CALL SITE, pinned as source: publish-release.sh has no harness (this file's own note above), and the
# shape that shipped was an echo with no clear next to it.
check_match "the audit-only branch CLEARS the ledger it declares settled" \
  "$(sed -n '/^if \[ "${1:-}" = "--audit-only" \]/,/^fi$/p' scripts/publish-release.sh)" "reconcile_clear"
case "$(sed -n '/^if \[ "${1:-}" = "--audit-only" \]/,/^fi$/p' scripts/publish-release.sh)" in
  *'echo "audit: v$VER settled"'*'reconcile_clear'*|*'reconcile_clear'*'echo "audit: v$VER settled"'*) PASS=$((PASS+1));;
  *) echo "FAIL: the audit-only branch says settled without clearing — the debt would outlive its audit"; exit 1;;
esac

# ── the release scripts' own silent self-disabling checks (round 127) ────────
# These are SOURCE pins, the instrument this file already uses for the reconcile
# gate: publish-release.sh has no harness of its own, and the two shapes below
# are both "a check that quietly stops checking".
check_match "an undateable exe input REFUSES instead of defaulting to zero" \
  "$(sed -n '/^SRC_TS=/,/^fi$/p' scripts/publish-release.sh)" "cannot date the exe inputs"
check_match "...and never falls back to the \${VAR:-0} default that disabled it" \
  "$(sed -n '/^SRC_TS=/,/^fi$/p' scripts/publish-release.sh)" "Refusing to disable the exe-staleness gate"
case "$(sed -n '/^SRC_TS=/,/^fi$/p' scripts/publish-release.sh)" in
  *'SRC_TS=${SRC_TS:-0}'*) echo "FAIL: the silent-zero default is back in publish-release.sh"; exit 1;;
  *) PASS=$((PASS+1));;
esac

# ── the installer manifest verdict (round 130) ──────────────────────────────
SHA_A="$(printf 'exe-a' | sha256sum | cut -d' ' -f1)"
SHA_B="$(printf 'exe-b' | sha256sum | cut -d' ' -f1)"
check "the manifest advertises this build" \
  "$(installer_manifest_verdict "{\"installer_sha256\":\"$SHA_A\"}" "$SHA_A")" "ok"
check "no installer advertised at all" \
  "$(installer_manifest_verdict '{"sha256":"x"}' "$SHA_A")" "absent"
check "...and an EMPTY answer reads as absent, not as a match" \
  "$(installer_manifest_verdict '' "$SHA_A")" "absent"
check "a different installer is a mismatch, and named" \
  "$(installer_manifest_verdict "{\"installer_sha256\":\"$SHA_B\"}" "$SHA_A")" "mismatch:$SHA_B"
check "garbage in the manifest is absent, not a crash" \
  "$(installer_manifest_verdict 'not json' "$SHA_A")" "absent"

# The WIRING: a deploy path that can drop the verdict silently is the defect this
# round fixed, so the call site is pinned too (build-installer.sh has no harness of
# its own — it builds NSIS installers).
BI="$(sed -n '/wrangler deploy/,$p' scripts/build-installer.sh)"
check_match "the deploy path consults the manifest verdict" "$BI" "installer_manifest_verdict"
case "$BI" in
  *'"== done =="'*) ;;
  *) echo "FAIL: build-installer.sh no longer prints its completion line (did the pin's anchor move?)"; exit 1;;
esac
# ...and the verdict must come BEFORE the success line, or it guards nothing.
# Pure shell, so a failure PRINTS (my first version used a python heredoc whose
# sys.exit(1) died under `set -e` before the message — a caught mutation with no
# diagnostic is half a test).
V_LINE="$(grep -n 'installer_manifest_verdict' scripts/build-installer.sh | tail -1 | cut -d: -f1)"
D_LINE="$(grep -n '^echo "== done =="' scripts/build-installer.sh | cut -d: -f1)"
check "the verdict runs BEFORE the success line" \
  "$([ "${V_LINE:-0}" -lt "${D_LINE:-0}" ] && echo before || echo after)" "before"

echo "release-lib: $PASS checks passed"


# ── retiring the NSIS installer (round 27) ───────────────────────────────────
# Measured state that motivated it: six staged exes (1.2.358-1.2.365, 40 MB) were uploaded by
# every deploy, and the versionless alias served 1.2.365 from the CDN while the release was
# 1.2.406. The publish flow retires them when it is not building one.
R=$(mktemp -d)
touch "$R/SummriseAgent-Setup.exe" "$R/SummriseAgent-Setup-1.2.365.exe" "$R/SummriseAgent-Setup-1.2.361.exe"
echo tgz > "$R/summrise-agent-latest.tgz"
echo '{}' > "$R/version.json"
out="$(retire_installers "$R")"
check_match "retire_installers counts what it removed" "$out" "retired 3 staged installer"
check "the alias is gone" "$(ls "$R"/SummriseAgent-Setup*.exe 2>/dev/null | wc -l)" "0"
# …and it touches NOTHING else: the npm channel is the one that must survive.
check "the tgz survives" "$(cat "$R/summrise-agent-latest.tgz")" "tgz"
check "the manifest survives" "$(cat "$R/version.json")" "{}"
# A second call has nothing to do, and says so rather than claiming a removal.
out="$(retire_installers "$R")"
check_match "an empty asset dir is reported honestly" "$out" "no staged installer to retire"
rm -rf "$R"
