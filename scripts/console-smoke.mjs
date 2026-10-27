#!/usr/bin/env node
// A browser check for the console, because the console's own tests can only see strings.
//
// The page broke once in a way no string test could catch: seven functions were deleted while their
// call sites stayed, so the script threw where the login handler was registered and every button
// after it was dead — and the HTML still contained every string the tests looked for. This runs the
// page the way an operator does and fails on a page error, a missing card, or a dead sign-in.
//
// Usage (on a machine with Playwright and the console's operator token):
//   CONSOLE_URL=http://127.0.0.1:19000 \
//   CONSOLE_TOKEN_FILE=C:\SummriseConsole\operator.token \
//   node scripts/console-smoke.mjs [--shot out.png] [--expect 2] [--expect-online 2]
//
// `--terminal` checks the Rust page at /next/terminal/<device> instead of the SPA at `/`: it signs
// in with the page cookie, opens a shell, and waits for a marker it typed to come back through
// xterm. It must run where a browser can start — a signed-in desktop session, not the device
// service (which has no desktop and cannot launch Edge). CONSOLE_BROWSER names a system Chromium
// when the runtime has no bundled one:
//
//   CONSOLE_URL=http://127.0.0.1:19000 CONSOLE_TOKEN_FILE=/path/operator.token \
//   CONSOLE_BROWSER='C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe' \
//   node scripts/console-smoke.mjs --terminal
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
const expectedCardinals = Number(option('--expect', process.env.CONSOLE_EXPECT || '1'));
const expectedOnline = Number(option('--expect-online', process.env.CONSOLE_EXPECT_ONLINE || '-1'));
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
const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });

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

const terminalOnly = args.includes('--terminal');

if (terminalOnly) {
  // The Rust page at /next signs in with a cookie, not the SPA's bearer. The leg proves the page
  // runs a shell: it types a marker and waits to read it back through xterm, which is the one thing
  // a string test cannot see.
  await page.goto(base + '/next', { waitUntil: 'domcontentloaded', timeout: 30000 });
  if (await page.locator('input#token').isVisible().catch(() => false)) {
    await page.fill('input#token', token);
    await Promise.all([page.waitForLoadState('domcontentloaded'), page.click('button[type=submit]')]);
  }
  if (!(await page.locator('a:has-text("Terminal")').count())) {
    await fail('no Terminal link on /next (is the page signed in?)');
  }
  await page.click('a:has-text("Terminal")');
  // The page now asks what kind of session to open; the smoke opens the default (a local shell).
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

await page.goto(base + '/', { waitUntil: 'load', timeout: 30000 });

// A resumed session and a fresh tab are both normal: sign in only if the box is showing.
if (await page.locator('.login').isVisible().catch(() => false)) {
  await page.fill('#token', token);
  await page.click('.login button.primary');
}

try {
  await page.waitForSelector('.card', { timeout: 20000 });
} catch {
  await fail('no device card rendered within 20 s (is the sign-in working?)');
}

const cards = await page.locator('.card').count();
if (cards < expectedCardinals) await fail(`expected at least ${expectedCardinals} card(s), saw ${cards}`);

// A card proves the page rendered; it does not prove a device can be reached. The summary line is
// where the console says how many channels are live, and a fleet that is all offline is a console
// that looks fine and is useless.
if (expectedOnline >= 0) {
  // A device reconnects on its own backoff, which is up to a minute — so a check that runs right after
  // the console restarts must wait for the channels rather than call them broken.
  let summary = '';
  let online = -1;
  for (let attempt = 0; attempt < 18; attempt += 1) {
    await page.click('.panel-head button', { timeout: 5000 }).catch(() => {});
    await page.waitForTimeout(5000);
    summary = await page.locator('.summary').innerText();
    online = Number((summary.match(/(\d+)\s+online/) || [])[1] ?? -1);
    if (online >= expectedOnline) break;
  }
  if (online < expectedOnline) await fail(`expected ${expectedOnline} online, the page says "${summary}"`);
}
if (errors.length > 0) await fail('the page logged errors');

// A dead button is the exact failure this script exists for: click a control that only needs a session
// and check the thing it is supposed to change.
await page.locator('.rail-btn').last().click().catch(() => {});
await page.waitForTimeout(1200);
const auditRows = await page.locator('table tbody tr').count();
if (auditRows === 0) await fail('the audit table stayed empty after opening the audit page');

// The rail and the status bar are the shell: their absence means the page rendered a fragment.
if ((await page.locator('.rail-btn').count()) === 0) await fail('the rail is missing');
if ((await page.locator('.statusbar').count()) === 0) await fail('the status bar is missing');

if (shot) await page.screenshot({ path: shot, fullPage: true });
await browser.close();
console.log(`console-smoke: ok — ${cards} card(s), ${auditRows} audit row(s), no page errors`);
