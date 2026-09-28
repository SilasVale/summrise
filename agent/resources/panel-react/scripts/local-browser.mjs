// A browser for the design sweeps on a machine that is not the device.
//
// The sweeps take their browser from `SUMMRISE_BROWSER_HELPER`, which on the device points at the agent's
// bundled Playwright. This is the same contract for anywhere else — a CI runner, or a developer's box —
// so the sweeps themselves need no knowledge of where they are running (round 204).
//
// It exports `acquireBrowser()` returning `{ page, close }`, exactly what the emitted scripts destructure.
// Chromium comes from playwright-core's cache; the launch flags are the ones the panel's own screenshot
// script uses, because a container needs both of them.
// ── THE CACHED CHROMIUM NEEDS TEN `.so`s, AND `apt-get download` SUPPLIES THEM WITHOUT ROOT ──────────
//
// MEASURED 2026-09-28. THE FAILURE IS REAL, AND SO IS EVERY WORD OF IT: `ldd
// ~/.cache/ms-playwright/chromium-1228/chrome-linux64/chrome` reports TEN missing shared libraries
// (`libatk-1.0.so.0` first of them), and `chromium.launch()` dies with "Target page, context or browser has
// been closed" and a truncated path. WHAT USED TO FOLLOW WAS FALSE — that installing them means
// `playwright install-deps`, which needs root on the operator's server, so the run "has to happen" in CI.
// It does not: `apt-get download` fetches a package's `.deb` with no root and no install, and a private
// prefix on `LD_LIBRARY_PATH` is enough. SIXTEEN packages, 1,061,792 bytes — the ten above plus six
// transitive (`libharfbuzz0b`, `libthai0`, `libwayland-server0`, `libdatrie1`, `libgraphite2-3`,
// `libwayland-client0`):
//
//     mkdir -p /tmp/chromium-libs/{debs,prefix} && cd /tmp/chromium-libs/debs
//     apt-get download libatk1.0-0 libatk-bridge2.0-0 libxkbcommon0 libgbm1 libpango-1.0-0 \
//       libxcomposite1 libxdamage1 libxfixes3 libxrandr2 libatspi2.0-0 \
//       libharfbuzz0b libthai0 libwayland-server0 libdatrie1 libgraphite2-3 libwayland-client0
//     for d in *.deb; do dpkg-deb -x "$d" ../prefix; done
//
//     export LD_LIBRARY_PATH=/tmp/chromium-libs/prefix/usr/lib/x86_64-linux-gnu
//     export SUMMRISE_CHROMIUM_PATH=~/.cache/ms-playwright/chromium-1228/chrome-linux64/chrome
//
// AFTER THAT `ldd` REPORTS ZERO MISSING, `chrome --version` answers, and the REAL panel sweep runs HERE,
// against what this checkout builds — no relay, no device, no listener. `scripts/design-sweep-ci.bash`'s own
// recipe, with those two exports set:
//
//     node agent/scripts/panel-render-audit.mjs            # writes /tmp/panel-render-audit/panel-harness.html
//     node agent/scripts/panel-design-sweep.mjs --emit --passes=all > /tmp/panel.js
//     SUMMRISE_PANEL_HARNESS=/tmp/panel-render-audit/panel-harness.html \
//     SUMMRISE_SWEEP_REPORT=/tmp/panel-report.json \
//     SUMMRISE_BROWSER_HELPER=$PWD/agent/resources/panel-react/scripts/local-browser.mjs \
//       node /tmp/panel.js
//     node agent/scripts/panel-design-sweep.mjs --judge /tmp/panel-report.json
//
// MEASURED, THAT RUN: `--passes=all`, 142 surfaces and 7058 rows over both densities, the harness build
// `282231-598d41e94ef2` matching the one the emit expected, and the judge's own verdict — "panel design sweep
// OK: nothing above found a defect", rc=0 — about 228 s after the emitted program was written. So a design
// question can be ANSWERED HERE in under four minutes instead of being routed to the device, and "this box
// has no browser" is not a reason to skip it. What the DEVICE is still for: the LIVE console and landing,
// the delivered artifacts, and anything that needs the operator's own screen.
//
// A CI runner still needs none of this (`npx playwright install --with-deps chromium` does it in one step),
// which is why the sweep's paths stay overridable.
import { chromium } from "playwright-core";

export async function acquireBrowser() {
  // AN EXPLICIT BINARY WHEN ONE IS NAMED. playwright-core resolves its OWN build number from its package
  // version, and a machine whose cache holds a different one fails with "Target page, context or browser has
  // been closed" and a path nobody recognises. CI installs the matching build and needs nothing; a box with
  // a pre-existing cache can point at it with SUMMRISE_CHROMIUM_PATH.
  const executablePath = process.env.SUMMRISE_CHROMIUM_PATH || undefined;
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
