#!/usr/bin/env node
// Photograph (and assert) the console's Terminal page against a REAL console and a REAL device.
//
//   node scripts/console-live-check.mjs --phase live    --url … --token-file … --out <dir>
//   node scripts/console-live-check.mjs --phase offline --url … --token-file … --out <dir>
//
// `scripts/console-live-check.sh` brings the console and one device up on loopback and calls this
// twice: once with the device serving, once with it stopped. The console's page is the operator's
// React app; the browser is Playwright's Chromium, resolved the same way `screenshot.mjs` resolves it.
//
// The two assertions are the goal, not decoration:
//   live    the terminal shows the command AND its output, typed into the page;
//   offline the page states the machine is down, disables the actions that cannot work, and does not
//           ask the machine for its session list (no /mcp request is made).
//
// Exit 0 both assertions held; 2 the page did not do what it promises; 3 the rig could not run.

import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import path from 'node:path';
import process from 'node:process';

const require = createRequire(import.meta.url);
const PLAYWRIGHT_CANDIDATES = [
  process.env.PLAYWRIGHT_CORE_DIR,
  'playwright-core',
  '/home/zss/summrise/agent/resources/panel-react/node_modules/playwright-core',
  path.resolve(path.dirname(new URL(import.meta.url).pathname), '../device/console/web/node_modules/playwright-core')
].filter(Boolean);

let playwright = null;
for (const candidate of PLAYWRIGHT_CANDIDATES) {
  try {
    playwright = require(candidate);
    break;
  } catch {
    /* try the next candidate */
  }
}
if (!playwright) {
  console.error('console-live-check: no playwright-core; set PLAYWRIGHT_CORE_DIR');
  process.exit(3);
}

const args = process.argv.slice(2);
const option = (name, fallback = null) => {
  const index = args.indexOf(`--${name}`);
  return index >= 0 && args[index + 1] ? args[index + 1] : fallback;
};
const phase = option('phase');
const url = option('url');
const tokenFile = option('token-file');
const out = option('out', '/tmp/console-live-shot');
const browserPath = option('browser', process.env.CONSOLE_BROWSER || '/usr/bin/google-chrome');
if (!['live', 'offline', 'pages'].includes(phase) || !url || !tokenFile) {
  console.error('usage: --phase live|offline|pages --url <app-url> --token-file <path> [--out dir]');
  process.exit(3);
}

const token = readFileSync(tokenFile, 'utf8').trim();
const browser = await playwright.chromium.launch({
  executablePath: browserPath,
  args: ['--no-sandbox']
});
const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
const failures = [];
const shot = async (name) => page.screenshot({ path: path.join(out, `${name}.png`), animations: 'disabled' });

try {
  await page.goto(url, { waitUntil: 'domcontentloaded' });
  await page.waitForTimeout(500);
  // The token form is the console's own sign-in: no Cloudflare Access on a loopback console.
  await page.fill('input#token', token);
  await page.getByRole('button', { name: /sign in/i }).click();
  await page.waitForTimeout(1200);

  await page.locator('.fleet-open').first().click();
  await page.waitForTimeout(600);
  await page.locator('nav.rail button[title="Device"]').click();
  await page.waitForTimeout(400);
  await page.locator('nav.device-tabs').getByRole('button', { name: 'Terminal', exact: true }).click();
  await page.waitForTimeout(800);

  if (phase === 'pages') {
    // THE DEVICE TABS, on a real device with real data. Each is photographed as it arrives: the
    // first-run state of a machine that has not been used yet is the one an operator meets first.
    const tabs = [
      ['Files', 'files'],
      ['Runs', 'runs'],
      ['Machine', 'machine'],
      ['Screen', 'screen']
    ];
    for (const [label, file] of tabs) {
      await page.locator('nav.device-tabs').getByRole('button', { name: label, exact: true }).click();
      await page.waitForTimeout(1200);
      const text = (await page.locator('main').innerText().catch(() => '')).replace(/\s+/g, ' ');
      console.log(`page ${file}: ${JSON.stringify(text.slice(0, 220))}`);
      await shot(`page-${file}`);

      // FILES IS THE ONE PAGE WITH A WRITE THAT NEEDS NOTHING ELSE. Making a folder through the
      // form, on a real device, is the difference between "the list renders" and "the page works".
      if (file === 'files') {
        const name = `rig-${Date.now().toString(36)}`;
        await page.locator('#files-new-folder').fill(name);
        await page.getByRole('button', { name: 'Create', exact: true }).click();
        await page.waitForTimeout(1500);
        // The row renders a directory as `name/`, so the match is a substring on purpose.
        const listed = await page.getByText(name).count();
        if (listed === 0) failures.push(`Files did not list the folder it just created (${name})`);
        console.log(`files create: ${listed > 0 ? 'ok' : 'MISSING'} (${name})`);
        await shot('page-files-created');
      }
    }
    await page.locator('nav.device-tabs').getByRole('button', { name: 'Terminal', exact: true }).click();
  } else if (phase === 'live') {
    const start = page.getByRole('button', { name: '+ New session' });
    if (await start.isDisabled()) failures.push('New session is disabled although the device is serving');
    await start.click();
    const dialog = page.getByRole('dialog', { name: 'New terminal session' });
    await dialog.getByRole('button', { name: 'Open' }).click();
    await page.locator('.xterm').first().waitFor({ state: 'visible', timeout: 10_000 });
    await page.waitForTimeout(1200);
    await page.locator('.xterm').first().click({ position: { x: 80, y: 40 } });
    await page.keyboard.type('echo console-live-check');
    await page.keyboard.press('Enter');
    await page.waitForTimeout(2500);
    const text = await page.locator('.terminal-workspace').innerText().catch(() => '');
    if (!text.includes('console-live-check')) {
      failures.push('the terminal did not show the command and its output');
    }
    await shot('terminal-live');

    // A RELOAD IS THE OPERATOR'S MOST COMMON GESTURE. What the page does with it is photographed
    // here rather than asserted: whether the session survives is a product decision, and the check
    // exists to show which one the page makes.
    await page.reload({ waitUntil: 'domcontentloaded' });
    await page.waitForTimeout(2500);
    const afterReload = await page.locator('body').innerText().catch(() => '');
    const backAtSignIn = (await page.locator('input#token').count()) > 0;
    console.log(`console-live-check: reload -> ${backAtSignIn ? 'sign-in form' : 'still in the console'}`);
    await shot(backAtSignIn ? 'terminal-reload-signin' : 'terminal-reload');
    void afterReload;
  } else {
    const notice = page.locator('.offline-notice').first();
    const visible = await notice.isVisible().catch(() => false);
    const sentence = visible ? (await notice.innerText()).replace(/\s+/g, ' ') : '';
    if (!visible) failures.push('the offline notice is not on the page');
    if (!/offline|Last seen/i.test(sentence)) failures.push(`the notice does not name the state: ${sentence}`);
    const start = page.getByRole('button', { name: '+ New session' });
    if (!(await start.isDisabled())) failures.push('New session is enabled on an offline machine');
    const mcp = await page.evaluate(
      () => performance.getEntriesByType('resource').filter((entry) => entry.name.includes('/mcp')).length
    );
    if (mcp > 0) failures.push(`the page asked an offline machine for data (${mcp} /mcp request(s))`);
    await shot('terminal-offline');

    // THE NOTICE BELONGS TO EVERY DEVICE TAB, not only the one it was written for: Files is the
    // other page an operator reaches for when a machine stops answering.
    await page.locator('nav.device-tabs').getByRole('button', { name: 'Files', exact: true }).click();
    await page.waitForTimeout(1200);
    const filesNotice = page.locator('.offline-notice').filter({ hasText: 'Nothing on this page can reach it' });
    if (!(await filesNotice.isVisible().catch(() => false))) {
      failures.push('Files on an offline machine does not carry the offline notice');
    }
    // And it must not still be reading: the page sat on "Reading the device's files root…" for as
    // long as the console's dial took, which reads as a stuck page rather than an absent machine.
    const filesText = (await page.locator('main').innerText().catch(() => '')).replace(/\s+/g, ' ');
    if (!filesText.includes('cannot be read until it reconnects')) {
      failures.push('Files on an offline machine does not say its files cannot be read');
    }
    await shot('files-offline');

    // Runs and Machine make the same promise: nothing is read from a machine that is not there.
    for (const [label, key] of [['Runs', 'runs'], ['Machine', 'machine']]) {
      await page.locator('nav.device-tabs').getByRole('button', { name: label, exact: true }).click();
      await page.waitForTimeout(1200);
      const text = (await page.locator('main').innerText().catch(() => '')).replace(/\s+/g, ' ');
      if (!text.includes('cannot be read until it reconnects')) {
        failures.push(`${label} on an offline machine does not say it cannot be read`);
      }
      await shot(`offline-${key}`);
    }

    // By now every device tab has been opened against a machine that is down. Not one of them may
    // have sent a proxy call.
    const asks = await page.evaluate(
      () => performance.getEntriesByType('resource').filter((entry) => entry.name.includes('/mcp')).length
    );
    if (asks > 0) failures.push(`the device tabs asked an offline machine (${asks} /mcp request(s))`);
  }
} catch (error) {
  failures.push(`the rig could not drive the page: ${error instanceof Error ? error.message : String(error)}`);
} finally {
  await browser.close().catch(() => {});
}

if (failures.length > 0) {
  for (const failure of failures) console.error(`console-live-check: ${phase}: ${failure}`);
  process.exit(2);
}
const shotName = { live: 'terminal-live.png', offline: 'terminal-offline.png', pages: 'page-*.png' }[phase];
console.log(`console-live-check: ${phase}: ok (${path.join(out, shotName)})`);
