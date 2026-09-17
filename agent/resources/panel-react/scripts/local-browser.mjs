// A browser for the design sweeps on a machine that is not the device.
//
// The sweeps take their browser from `VALE_BROWSER_HELPER`, which on the device points at the agent's
// bundled Playwright. This is the same contract for anywhere else — a CI runner, or a developer's box —
// so the sweeps themselves need no knowledge of where they are running (round 204).
//
// It exports `acquireBrowser()` returning `{ page, close }`, exactly what the emitted scripts destructure.
// Chromium comes from playwright-core's cache; the launch flags are the ones the panel's own screenshot
// script uses, because a container needs both of them.
// MEASURED LIMIT ON THIS BOX (round 204). The cached Chromium here cannot start: `ldd` reports NINE missing
// shared libraries (`libatk-1.0.so.0` first of them), so `chromium.launch()` dies with "Target page, context
// or browser has been closed" and a truncated path. Installing them is `playwright install-deps`, which needs
// root on the operator's server — not something this loop does unasked. A CI runner gets them from
// `npx playwright install --with-deps chromium` in one step, which is why the sweep's paths are overridable
// even though the run itself has to happen there.
import { chromium } from "playwright-core";

export async function acquireBrowser() {
  // AN EXPLICIT BINARY WHEN ONE IS NAMED. playwright-core resolves its OWN build number from its package
  // version, and a machine whose cache holds a different one fails with "Target page, context or browser has
  // been closed" and a path nobody recognises. CI installs the matching build and needs nothing; a box with
  // a pre-existing cache can point at it with VALE_CHROMIUM_PATH.
  const executablePath = process.env.VALE_CHROMIUM_PATH || undefined;
  const browser = await chromium.launch({ headless: true, executablePath, args: ["--no-sandbox", "--disable-dev-shm-usage"] });
  const context = await browser.newContext();
  const page = await context.newPage();
  return {
    page,
    context,
    async close() {
      await context.close();
      await browser.close();
    },
  };
}
