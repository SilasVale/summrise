#!/usr/bin/env node
// A browser check for the console, because the console's own tests can only see strings.
//
// The page broke once in a way no string test could catch: seven functions were deleted while their
// call sites stayed, so the script threw where the login handler was registered and every button
// after it was dead — and the HTML still contained every string the tests looked for. This runs the
// page the way an operator does and fails on a page error, a missing table, or a dead sign-in.
//
// The console is the Rust page at /next now (`/` redirects there). Two legs:
//
//   (default)   sign in, read the fleet table, follow one device's Files link, wait for its form.
//   --terminal  open a shell on the first device, type a marker, read it back through xterm, then
//               open a second session and require two tabs.
//
// Usage (on a machine with a browser and the console's operator token):
//   CONSOLE_URL=http://127.0.0.1:19000 \
//   CONSOLE_TOKEN_FILE=C:\SummriseConsole\operator.token \
//   CONSOLE_BROWSER='C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe' \
//   node scripts/console-smoke.mjs [--terminal] [--shot out.png] [--expect 2]
//
// It must run where a browser can start — a signed-in desktop session, not the device service
// (which has no desktop and cannot launch Edge). CONSOLE_BROWSER names a system Chromium when the
// runtime has no bundled one.
//
// Exit codes: 0 the page works · 1 the page is broken · 2 it could not be checked.

import { createRequire } from 'node:module';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import process from 'node:process';

const args = process.argv.slice(2);
const option = (name, fallback) => {
  const index = args.indexOf(name);
  return index === -1 ? fallback : args[index + 1];
};

const base = process.env.CONSOLE_URL || 'http://127.0.0.1:19000';
const tokenFile = process.env.CONSOLE_TOKEN_FILE;
const expectedRows = Number(option('--expect', process.env.CONSOLE_EXPECT || '1'));
const shot = option('--shot', '');

if (!tokenFile) {
  console.error('console-smoke: set CONSOLE_TOKEN_FILE to the operator token file');
  process.exit(2);
}

// CommonJS resolution honours NODE_PATH, which is how a bundled runtime (the one this was first run
// against) is pointed at without installing anything: ESM imports do not.
const require = createRequire(import.meta.url);
let playwright;
for (const candidate of [
  'playwright-core',
  'playwright',
  process.env.PLAYWRIGHT_CORE_DIR && path.join(process.env.PLAYWRIGHT_CORE_DIR, 'node_modules', 'playwright-core'),
].filter(Boolean)) {
  try {
    playwright = require(candidate);
    break;
  } catch {
    /* try the next one */
  }
}
if (!playwright) {
  console.error('console-smoke: no playwright-core; set NODE_PATH or PLAYWRIGHT_CORE_DIR');
  process.exit(2);
}

const token = readFileSync(tokenFile, 'utf8').trim();
const browserPath = process.env.CONSOLE_BROWSER || '';
const browser = await playwright.chromium.launch(browserPath ? { executablePath: browserPath } : {});
const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });

const errors = [];
page.on('pageerror', (error) => errors.push('pageerror: ' + error.message));
page.on('console', (message) => {
  if (message.type() === 'error') errors.push('console: ' + message.text());
});

const fail = async (reason) => {
  if (shot) await page.screenshot({ path: shot, fullPage: true }).catch(() => {});
  await browser.close();
  console.error('console-smoke: FAILED — ' + reason);
  for (const error of errors) console.error('  ' + error);
  process.exit(1);
};

/** Open the page and sign in if the login card is showing. */
async function signIn() {
  await page.goto(base + '/next', { waitUntil: 'domcontentloaded', timeout: 30000 });
  if (await page.locator('input#token').isVisible().catch(() => false)) {
    await page.fill('input#token', token);
    await Promise.all([page.waitForLoadState('domcontentloaded'), page.click('button[type=submit]')]);
  }
}

if (args.includes('--terminal')) {
  // The page signs in with a cookie, not a bearer. The leg proves the page runs a shell: it types a
  // marker and waits to read it back through xterm, which is the one thing a string test cannot see.
  await signIn();
  if (!(await page.locator('a:has-text("Terminal")').count())) {
    await fail('no Terminal link on the page (is it signed in?)');
  }
  await page.click('a:has-text("Terminal")');
  await page.waitForSelector('#terminal-form', { timeout: 20000 });
  await page.click('#terminal-form button[type=submit]');
  await page.waitForSelector('.xterm', { timeout: 20000 });
  try {
    await page.waitForFunction(
      () => {
        const status = document.getElementById('terminal-status');
        return status && /session/.test(status.textContent || '');
      },
      { timeout: 20000 }
    );
  } catch {
    const status = await page.locator('#terminal-status').innerText().catch(() => '(missing)');
    await fail('the terminal did not open a session: ' + status);
  }
  const markerText = 'smoke-terminal-ok';
  await page.click('.xterm');
  await page.keyboard.type('echo ' + markerText);
  await page.keyboard.press('Enter');
  try {
    await page.waitForFunction(
      (text) => {
        const rows = document.querySelector('.xterm-rows');
        return !!rows && rows.innerText.includes(text);
      },
      markerText,
      { timeout: 20000 }
    );
  } catch {
    await fail('the command was not echoed back by xterm');
  }
  // A second session must get its own tab and its own xterm; the first one stays alive behind it.
  await page.click('#terminal-form button[type=submit]');
  await page.waitForFunction(() => document.querySelectorAll('.tab').length === 2, {
    timeout: 20000
  });
  const tabs = await page.locator('.tab').count();
  if (tabs !== 2) await fail(`expected two terminal tabs, saw ${tabs}`);
  const session = await page.locator('#terminal-status').innerText();
  if (shot) await page.screenshot({ path: shot, fullPage: true });
  await browser.close();
  console.log('console-smoke: terminal ok — ' + session.replace(/\s+/g, ' ') + ', 2 tabs');
  process.exit(0);
}

// The default leg: the fleet is a table, every device row carries the pages that act on it, and the
// old root lands here. A redirect that stopped redirecting is the cheapest page break to ship.
const response = await page.goto(base + '/', { waitUntil: 'domcontentloaded', timeout: 30000 });
if (!response || !response.ok()) await fail('GET / did not answer');
if (!page.url().endsWith('/next')) await fail('GET / did not land on /next: ' + page.url());
if (await page.locator('input#token').isVisible().catch(() => false)) {
  await page.fill('input#token', token);
  await Promise.all([page.waitForLoadState('domcontentloaded'), page.click('button[type=submit]')]);
}
try {
  await page.waitForSelector('table tbody tr', { timeout: 20000 });
} catch {
  await fail('no device row rendered within 20 s (is the sign-in working?)');
}
const rows = await page.locator('table tbody tr').count();
if (rows < expectedRows) await fail(`expected at least ${expectedRows} device row(s), saw ${rows}`);
for (const label of [
  'Terminal',
  'Files',
  'Runs',
  'System',
  'State',
  'Memory',
  'Screen',
  'Monitors',
  'Browser',
  'Connect',
  'Vitals',
]) {
  if (!(await page.locator(`a:has-text("${label}")`).count())) {
    await fail(`the device row has no ${label} link`);
  }
}
await page.click('a:has-text("Files")');
await page.waitForSelector('form', { timeout: 20000 });
if (errors.length > 0) await fail('the page logged errors');
if (shot) await page.screenshot({ path: shot, fullPage: true });
await browser.close();
console.log(`console-smoke: ok — ${rows} device row(s), every page link present, / redirected to /next`);
