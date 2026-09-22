// ── the CDN's script assets are hand-synced copies of agent/deploy/ ──
//
// `index/public/summrise-agent/fix-tunnel.ps1` is a TRACKED file: nothing copies it
// there (a grep of scripts/*.sh for `fix-tunnel` returns nothing), and the browser /
// installer fetches it from the CDN rather than from npm — `agent/summrise-agent-npm`'s
// `files` array and its 9-file pack contain no such script.
//
// Round 239 measured the two copies and they DIFFERED: the published one still carried
// the pre-round-237 path derivation (`$installDir = Split-Path -Parent
// $MyInvocation.MyCommand.Path` plus `tools\cloudflared.exe`, which resolves to
// `<InstallDir>\scripts\tools\cloudflared.exe` and exits 1 on d1 — round 238 verified
// that path does not exist while `components\cloudflared.exe` does) and an older header
// describing "user + systemprofile copies" the source had already collapsed to one.
//
// **So the copy a device would fetch was the broken one, and nothing compared them.**
// This is round 199/211's shape — a mirror lagging its source — recurring for the CDN's
// script assets. The assertion reads BOTH files rather than restating either.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync, existsSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { join, dirname } from "node:path";

const REPO = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const SRC = join(REPO, "agent", "deploy");
const CDN = join(REPO, "index", "public", "summrise-agent");

test("every CDN script asset is byte-identical to its agent/deploy/ source", () => {
  const shared = readdirSync(CDN)
    .filter((f) => /\.(ps1|vbs|nsi|bat)$/i.test(f))
    .filter((f) => existsSync(join(SRC, f)));
  assert.ok(
    shared.length > 0,
    "expected at least one script published on the CDN AND authored in agent/deploy/ — " +
      "if that is no longer true, delete this test and say so where the two directories " +
      "are described, rather than leaving an assertion with no subject (ADR 0011's " +
      "deletion criterion)",
  );
  for (const f of shared) {
    assert.equal(
      readFileSync(join(CDN, f), "utf8"),
      readFileSync(join(SRC, f), "utf8"),
      `${f} differs between the CDN asset and its agent/deploy/ source. The CDN copy is ` +
        `what a device fetches, so a fix that lands only in agent/deploy/ ships to nobody. ` +
        `Copy the source over: cp agent/deploy/${f} index/public/summrise-agent/${f}`,
    );
  }
});
