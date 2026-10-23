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
//   node scripts/console-smoke.mjs [--shot out.png] [--expect 2]
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
const browser = await playwright.chromium.launch();
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

await page.goto(base + '/', { waitUntil: 'load', timeout: 30000 });

// A resumed session and a fresh tab are both normal: sign in only if the box is showing.
if (await page.locator('#login').isVisible().catch(() => false)) {
  await page.fill('#operator-token', token);
  await page.click('#login-form button[type=submit]');
}

try {
  await page.waitForSelector('#device-cards .card', { timeout: 20000 });
} catch {
  await fail('no device card rendered within 20 s (is the sign-in working?)');
}

const cards = await page.locator('#device-cards .card').count();
if (cards < expectedCardinals) await fail(`expected at least ${expectedCardinals} card(s), saw ${cards}`);
if (errors.length > 0) await fail('the page logged errors');

// A dead button is the exact failure this script exists for: click the one that only needs a session.
await page.click('#refresh-audit').catch(() => {});
await page.waitForTimeout(1500);
const auditRows = await page.locator('#audit-list tr').count();
if (auditRows === 0) await fail('the audit table stayed empty after Refresh');

if (shot) await page.screenshot({ path: shot, fullPage: true });
await browser.close();
console.log(`console-smoke: ok — ${cards} card(s), ${auditRows} audit row(s), no page errors`);
