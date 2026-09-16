// DID THE HARNESS ACTUALLY BOOT? Run this BEFORE believing any measurement of it.
//
// WHY THIS EXISTS. Rounds 110-112 lost three rounds to a harness that never ran. A fixture was inserted
// outside `window.fetch`, where `u` does not exist, so it threw while the stub was being DEFINED; the
// route was never served; the update card rendered "The device did not answer" — which was LITERALLY
// TRUE — and I spent two of those rounds reading that message as a bug report about the card. Nothing
// looked at whether the page had mounted, so every number I collected in that time described a fallback.
//
// `node --check` on the emitter proves the FILE parses. It says nothing about the inlined script, which
// is where the stub lives. This is the check that closes that gap, and it is deliberately made of
// SENTINELS rather than of "the page loaded":
//
//   * the app must MOUNT — #root populated, and its node count in a plausible range;
//   * nothing may THROW — every page error is fatal here;
//   * the stub must ANSWER — the update card must show the version the fixture serves. That sentinel is
//     not arbitrary: it is exactly the card that stayed blank for three rounds, so this check fails on
//     the failure it was written for.
//
// USAGE (device d1): run through the browser tool, or as a Playwright script with the bundled runtime.
// It takes no arguments; it emits the harness first so the thing under test is always the current one.
//
// WHAT IT IS NOT: an offline gate. It needs a browser, so it belongs to the measurement step, not to
// `scripts/test/`. I tried an offline version — parse the inlined <script> from the emitted HTML — and
// it reported the stub as broken while the very same harness rendered the update card correctly in a
// real browser. The extraction was the broken part, and a gate that cries wolf about a working harness
// is worse than no gate. (That is the second reason this file is a measurement script: the browser is
// the only authority on whether the harness works.)
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";

const HARNESS = process.env.VALE_HARNESS || "C:\\ProgramData\\Vale\\pwout\\panel-harness.html";
const EMITTER = "agent/scripts/panel-render-audit.mjs";

export function bootHarness() {
  // 1. the emitter must succeed (exit 2 = it wrote the harness; exit 1 = the template literal broke)
  let code = 0;
  try {
    execFileSync("node", [EMITTER], { stdio: "ignore" });
  } catch (e) {
    code = e.status ?? -1;
  }
  if (code !== 2) {
    return { ok: false, why: `the emitter exited ${code} — it did not write a harness (a stray backtick in the emitted template is the usual cause)` };
  }
  const html = readFileSync(HARNESS, "utf8");
  if (html.length < 200_000) {
    return { ok: false, why: `the harness on the device is only ${html.length} bytes — the download did not land` };
  }
  return { ok: true, bytes: html.length };
}

/** The DOM-side assertions, to run inside the page after load. */
export const BOOT_PROBE = `(() => {
  const root = document.getElementById('root');
  return {
    nodes: root ? root.querySelectorAll('*').length : 0,
    updateLine: (() => {
      const sec = [...document.querySelectorAll('.settings-section')]
        .find((x) => /update/i.test((x.querySelector('h3') || {}).textContent || ''));
      const lat = sec ? sec.querySelector('.update-latest') : null;
      return lat ? lat.textContent.trim() : null;
    })(),
  };
})()`;

/** Judge the probe: mounted, and the sentinel card showing the fixture's version. */
export function judgeBoot(probe, pageErrors) {
  const problems = [];
  if (pageErrors && pageErrors.length) problems.push(`page threw: ${pageErrors.join('; ')}`);
  if (!probe || probe.nodes < 40) problems.push(`the app did not mount (#root has ${probe ? probe.nodes : 0} nodes)`);
  if (probe && probe.updateLine !== '1.2.433 available') {
    problems.push(`the stub did not answer: the update card reads ${JSON.stringify(probe.updateLine)} instead of "1.2.433 available"`);
  }
  return { ok: problems.length === 0, problems };
}
