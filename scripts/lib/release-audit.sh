#!/usr/bin/env bash
# release-audit.sh — P0 dual-builder audit, honest about what CAN be identical.
#
# What this replaces: publish-release.sh used to demand that the locally packed
# tgz and the CI-built GitHub asset be BYTE-IDENTICAL (same sha256) or abort.
# That gate could never pass here, because the two builders do not share a
# toolchain:
#   * rustc   — local `stable` 1.98.0 vs CI `dtolnay/rust-toolchain@stable`
#               (whatever stable is that week)
#   * clang   — local cargo-xwin's clang-cl is a hand-built symlink to
#               ~/llvm18/bin/clang (18.1.8); CI installs `apt llvm`
# Pinning all of that on a GitHub runner is not practical, and ubuntu-latest's
# default llvm moves under us anyway. So the exe legitimately differs.
#
# What CAN and MUST be identical is every SOURCE-DERIVED file in the tarball:
# the npm CLI, the electron shell sources, package.json, the icons. If any of
# those drifts, the two builders packaged different source — that IS a real
# integrity failure and this audit fails closed.
#
# Verdicts:
#   0  all source-derived files identical (exe hash difference is expected and
#      reported, with both hashes recorded for the ledger)
#   1  a source-derived file differs, or the audit could not run or could not be
#      completed (fail closed) — including "the bytes differ and I cannot say
#      why", which is NOT an OK verdict (round 122)
#
# Usage: audit_release_asset <version> <cdn_base>
#   env: GITHUB_TOKEN / GH_TOKEN, or ~/.github-token
#        REPO (default SilasVale/summrise)
#        AUDIT_KEEP=1 to keep the temp dir for inspection

AUDIT_REPO="${REPO:-SilasVale/summrise}"

_audit_token() {
  if [[ -n "${GITHUB_TOKEN:-}" ]]; then echo "$GITHUB_TOKEN"
  elif [[ -n "${GH_TOKEN:-}" ]]; then echo "$GH_TOKEN"
  elif [[ -f "$HOME/.github-token" ]]; then tr -d '\r\n' < "$HOME/.github-token"
  else echo ""; fi
}

# audit_asset_names <version>
# Print the release's asset names (one per line) via one cheap API call — no
# artifact download.
#
# THREE VERDICTS, because two of them are not the same thing and the caller used
# to be unable to tell them apart (round 122):
#   0  the release EXISTS and its asset names are printed (possibly none)
#   3  the release does NOT exist yet — the only state that means "first publish"
#   1  the question could not be ASKED (no token, network or API error)
#
# Verdict 1 is why the return codes are distinct. `return 1` for both states let
# `publish-release.sh --skip-reconcile`'s `|| true` turn an expired GitHub token
# into "no release exists" — which skipped the P0 audit and reported success.
audit_asset_names() {
  local ver="$1" token; token="$(_audit_token)"
  if [[ -z "$token" ]]; then
    echo "::error::cannot list assets for v${ver}: no GitHub token (GITHUB_TOKEN/GH_TOKEN/~/.github-token)" >&2
    return 1
  fi
  local api="https://api.github.com/repos/${AUDIT_REPO}/releases/tags/v${ver}"
  # NO -f: the HTTP status is the ANSWER here, not a failure to hide. With -f a
  # 404 and an expired token both came back as "curl exited non-zero".
  local body code
  body="$(curl -sSL -m 60 -w $'\n%{http_code}' -H "Authorization: Bearer ${token}" \
    -H "Accept: application/vnd.github+json" "$api" 2>/dev/null)" || {
    echo "::error::cannot list assets for v${ver}: the GitHub API call failed (network)" >&2
    return 1
  }
  code="${body##*$'\n'}"; body="${body%$'\n'*}"
  case "$code" in
    200) ;;
    404) return 3 ;;
    *) echo "::error::cannot list assets for v${ver}: GitHub answered HTTP ${code}" >&2; return 1 ;;
  esac
  printf '%s\n' "$body" | grep -o '"name": *"[^"]*\.tgz"' | sed 's/.*: *"//; s/"$//'
  return 0
}

# audit_release_asset <version> <cdn_base>
# Returns 0 = source-identical (exe may differ), 1 = real drift / cannot audit.
audit_release_asset() {
  local ver="$1" cdn="$2"
  local tgz="summrise-agent-${ver}.tgz"
  local token; token="$(_audit_token)"
  if [[ -z "$token" ]]; then
    echo "::error::release audit: no GitHub token (GITHUB_TOKEN/GH_TOKEN/~/.github-token) — cannot audit" >&2
    return 1
  fi

  # 1. The release asset must exist (CI builds it after the tag push).
  local api="https://api.github.com/repos/${AUDIT_REPO}/releases/tags/v${ver}"
  local listing; listing="$(curl -fsSL -m 60 -H "Authorization: Bearer ${token}" \
    -H "Accept: application/vnd.github+json" "$api" 2>/dev/null)" || {
    echo "::error::release audit: no GitHub release v${ver} (or no access) — CI may still be building" >&2
    return 1
  }
  # Here-string, NOT `printf | grep -q`: under `set -o pipefail` an early
  # grep -q exit SIGPIPEs the producer and the pipeline reports failure even on
  # a match (the round-288 lesson).
  if ! grep -q "\"name\": *\"${tgz}\"" <<< "$listing"; then
    echo "::error::release audit: release v${ver} has no asset ${tgz}" >&2
    return 1
  fi

  local work; work="$(mktemp -d)"
  # shellcheck disable=SC2064
  trap "[[ \"${AUDIT_KEEP:-0}\" == 1 ]] || rm -rf '$work'" RETURN

  # 2. Fetch both artifacts. Retries matter: this box's route to GitHub's
  # release-asset host is intermittent (a 0-byte timeout has been observed),
  # and a single flaky attempt used to read as "audit failed".
  curl -fsSL -m 300 --retry 5 --retry-delay 3 --retry-connrefused \
    "https://github.com/${AUDIT_REPO}/releases/download/v${ver}/${tgz}" \
    -o "$work/gh.tgz" || { echo "::error::release audit: cannot download the GitHub asset" >&2; return 1; }
  curl -fsSL -m 300 --retry 5 --retry-delay 3 --retry-connrefused \
    "${cdn}/summrise-agent/${tgz}" -o "$work/cdn.tgz" \
    || { echo "::error::release audit: cannot download the CDN tgz" >&2; return 1; }

  local cdn_sha gh_sha
  cdn_sha="$(sha256sum "$work/cdn.tgz" | cut -d' ' -f1)"
  gh_sha="$(sha256sum "$work/gh.tgz" | cut -d' ' -f1)"

  if [[ "$cdn_sha" == "$gh_sha" ]]; then
    echo "release audit OK: CDN == GitHub asset byte-for-byte (${cdn_sha:0:16}…)"
    return 0
  fi

  # 3. Whole-tarball hashes differ. That is EXPECTED (exe toolchain) — but only
  #    if every source-derived file still matches. Compare file by file.
  mkdir -p "$work/cdn" "$work/gh"
  # `-p`, AND IT IS THE WHOLE POINT OF THIS AUDIT. tar applies the UMASK to extracted files unless
  # told to preserve their permissions, so on a machine with umask 077 both trees come out 0600 — the
  # modes then "match" perfectly, the comparison passes, and mode drift is invisible. That is precisely
  # the defect this audit was written for (a 600-vs-644 difference that survived twenty consecutive
  # releases under the words "packaging metadata"). Found by running the gates under a hostile umask.
  tar xzpf "$work/cdn.tgz" -C "$work/cdn" || { echo "::error::release audit: CDN tgz is not a readable tarball" >&2; return 1; }
  tar xzpf "$work/gh.tgz"  -C "$work/gh"  || { echo "::error::release audit: GitHub asset is not a readable tarball" >&2; return 1; }

  # Same file LIST first — a missing/extra file is drift regardless of content.
  #
  # A LISTING THAT COULD NOT BE TAKEN MUST NOT COMPARE EQUAL. These were bare
  # command substitutions: if the top directory is not `package`, both `cd`s fail
  # and BOTH sides become the empty string — equal — so the loop below compares
  # NOTHING, `exe_cdn` stays empty, and the function reports WARN and returns 0.
  # A release audit that passes while comparing zero files is the exact failure
  # this whole log is about. (Reproduced against trees with a different top dir.)
  if [[ ! -d "$work/cdn/package" || ! -d "$work/gh/package" ]]; then
    echo "::error::release audit FAILED: a tarball has no top-level 'package/' directory — nothing could be compared" >&2
    return 1
  fi
  local lc lg
  lc="$( (cd "$work/cdn/package" && find . -type f | sort) )" || { echo "::error::release audit: cannot list the CDN tree" >&2; return 1; }
  lg="$( (cd "$work/gh/package"  && find . -type f | sort) )" || { echo "::error::release audit: cannot list the GitHub tree" >&2; return 1; }
  if [[ -z "$lc" && -z "$lg" ]]; then
    echo "::error::release audit FAILED: both trees listed ZERO files — the audit compared nothing" >&2
    return 1
  fi
  if [[ "$lc" != "$lg" ]]; then
    echo "::error::release audit FAILED: tarball contents differ" >&2
    diff <(printf '%s\n' "$lc") <(printf '%s\n' "$lg") | sed 's/^/  /' >&2
    return 1
  fi

  # MODES COME FROM THE ARCHIVE LISTING, NOT FROM THE EXTRACTED FILES. `stat` on an extracted tree is
  # what the umask can reach — even with `tar -p`, and even for DIRECTORIES — which is how this case
  # failed on CI while passing on the machine that wrote it (twice: rounds 102 and 124). `tar tzvf`
  # prints the mode STORED in the archive, so the environment cannot influence the reading at all.
  # The extraction above is still used, for CONTENT.
  # MODES COME FROM THE ARCHIVE LISTING, NOT FROM THE EXTRACTED FILES. `stat` on an extracted tree is
  # what the umask can reach — even with `tar -p`, and even for DIRECTORIES — which is how this case
  # failed on CI while passing on the machine that wrote it (twice: rounds 102 and 124). `tar tzvf`
  # prints the mode STORED in the archive, so the environment cannot influence the reading at all.
  # Built as a TEXT listing and compared with diff, the same shape as the content comparison above: an
  # associative array fought the input (empty and dotted names gave "bad array subscript") and this is
  # simpler besides. A listing that cannot be taken yields an empty side, and the diff below then FAILS
  # rather than quietly matching — which is what case 3 of the suite exists to check.
  local modes_cdn modes_gh
  # The path is printed the way the CONTENT listing prints it ("./a.txt"), because that is what an
  # operator diffs against — the suite checks the message names it.
  mode_list() { tar tzvf "$1" 2>/dev/null | awk '{ m=$1; $1=$2=$3=$4=$5=""; sub(/^ +/, ""); sub(/^\.\//,""); sub(/^[^/]*\//,""); print m, "./" $0 }' | sort; }
  modes_cdn="$(mode_list "$work/cdn.tgz")" || { echo "::error::release audit: cannot list the CDN tarball's modes" >&2; return 1; }
  modes_gh="$(mode_list "$work/gh.tgz")"  || { echo "::error::release audit: cannot list the GitHub tarball's modes" >&2; return 1; }
  if [[ -z "$modes_cdn" || "$modes_cdn" != "$modes_gh" ]]; then
    echo "::error::release audit FAILED: the two tarballs' FILE MODES differ (or a listing could not be taken)" >&2
    diff <(printf '%s\n' "$modes_cdn") <(printf '%s\n' "$modes_gh") | sed 's/^/  /' >&2
    return 1
  fi

  local f a b drifted=0 exe_cdn="" exe_gh="" mode_drift=()
  while IFS= read -r f; do
    a="$(sha256sum "$work/cdn/package/$f" | cut -d' ' -f1)"
    b="$(sha256sum "$work/gh/package/$f"  | cut -d' ' -f1)"
    # MODES ARE PART OF THE ARTIFACT, and comparing only CONTENT is how a
    # one-file mode difference survived TWENTY consecutive releases under a
    # message blaming "packaging metadata (mtimes/order)". npm pack preserves the
    # SOURCE file's mode, so a worktree whose permissions differ from what CI
    # checks out packs a different tarball from identical bytes. Measured on the
    # live pair: 3 differing bytes out of 17,774,080, all of them README.md's mode
    # field plus its header checksum; every sha256 identical, the 17.5 MB exe
    # included.
    # Modes were compared in full, above, from the archive listings.
    if [[ "$a" != "$b" ]]; then
      if [[ "$f" == "./summrise-agent.exe" ]]; then
        # The one file a different toolchain legitimately changes.
        exe_cdn="$a"; exe_gh="$b"
      else
        echo "::error::release audit FAILED: source-derived file drifted: $f" >&2
        echo "  CDN: ${a:0:24}…" >&2
        echo "  GH : ${b:0:24}…" >&2
        drifted=1
      fi
    fi
  done <<< "$lc"

  if [[ "$drifted" == 1 ]]; then
    echo "::error::release audit: the two builders packaged DIFFERENT SOURCE — do not ship" >&2
    return 1
  fi

  if [[ "${#mode_drift[@]}" -gt 0 ]]; then
    # NAME THE CAUSE. "packaging metadata" told a reader nothing and was wrong:
    # the difference is a FILE MODE, and it is fixable rather than a property of
    # the universe. It is reported as a failure of ARTIFACT IDENTITY, not as a
    # curiosity — the tgz IS the release, and two different tgz files are two
    # different releases however identical their contents.
    echo "::error::release audit FAILED: the two tarballs differ in FILE MODES — the same bytes, a different artifact" >&2
    printf '  %s\n' "${mode_drift[@]}" >&2
    echo "  Cause: npm pack preserves each SOURCE file's mode, so a worktree whose" >&2
    echo "  permissions differ from a fresh CI checkout packs a different tarball." >&2
    echo "  Fix: make the tracked files' modes match the index (git ls-files -s)" >&2
    echo "       e.g. \`chmod 644 agent/summrise-agent-npm/README.md\`." >&2
    return 1
  fi

  if [[ -z "$exe_cdn" ]]; then
    # No mode drift and no content drift, yet the tarball BYTES differ: that
    # leaves ordering/mtime-level metadata, which this comparison cannot name.
    #
    # IT USED TO RETURN 0 (round 122). The caller then printed "audit OK: …
    # its source-derived files match the GitHub asset" — a verdict this function
    # had explicitly failed to reach, on the one artifact that IS the release.
    # Twenty-five lines above, this same file states the rule: "the tgz IS the
    # release, and two different tgz files are two different releases however
    # identical their contents." An audit that cannot explain a difference has
    # not cleared it.
    echo "::error::release audit FAILED: every file's content AND mode match, yet the two tarballs' BYTES differ — this audit cannot name the difference, and the tgz IS the release" >&2
    echo "  CDN: ${cdn_sha:0:24}…" >&2
    echo "  GH : ${gh_sha:0:24}…" >&2
    echo "  Rebuild one side so both come from one artifact (\`./scripts/publish-cdn-from-ci.sh <ver>\`)," >&2
    echo "  or name the metadata difference before shipping." >&2
    return 1
  fi

  echo "release audit OK (source-identical): every source-derived file matches byte-for-byte."
  echo "  summrise-agent.exe differs by TOOLCHAIN (expected — not a source difference):"
  echo "    CDN (local build): ${exe_cdn:0:24}…"
  echo "    GH  (CI build)   : ${exe_gh:0:24}…"
  echo "  CDN remains authoritative (devices update from the CDN manifest sha256)."
  return 0
}
