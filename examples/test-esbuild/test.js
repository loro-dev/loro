import { chromium } from 'playwright';
import { spawn } from 'child_process';
import { setTimeout } from 'timers/promises';

console.log('Starting server...');
const serverProcess = spawn('node', ['server.ts'], {
  stdio: ['ignore', 'inherit', 'inherit'],
});

await setTimeout(3000);

console.log('\nRunning Playwright test...');
const browser = await chromium.launch({
  headless: true,
  args: ['--no-sandbox', '--disable-setuid-sandbox'],
});

const context = await browser.newContext();
const page = await context.newPage();

const consoleLogs = [];
page.on('console', (msg) => {
  const text = msg.text();
  consoleLogs.push(`[${msg.type()}] ${text}`);
  if (text.includes('bug test:')) {
    console.log(`\n>>> CAPTURED OUTPUT: ${text}`);
  }
});

page.on('pageerror', (err) => {
  console.log(`Page error: ${err}`);
});

page.on('requestfailed', (request) => {
  console.log(`Request failed: ${request.url()} - ${request.failure().errorText}`);
});

try {
  console.log('Navigating to http://localhost:8081...');
  await page.goto('http://localhost:8081', { waitUntil: 'domcontentloaded', timeout: 10000 });

  console.log('Waiting for LoroMap to be available...');
  await page.waitForFunction(() => window.LoroMap !== undefined, { timeout: 10000 });

  await setTimeout(2000);

  console.log('Evaluating test in browser...');
  const result = await page.evaluate(() => {
    const map = new window.LoroMap();
    return map.get('k');
  });

  console.log('\n=== TEST RESULTS ===');
  console.log(`Direct evaluation of new LoroMap().get("k"): ${JSON.stringify(result)}`);

  if (consoleLogs.length > 0) {
    console.log('\nAll console logs:');
    consoleLogs.forEach((log) => console.log(log));
  }

  const pageContent = await page.textContent('body');
  console.log('\nPage content:');
  console.log(pageContent);
} catch (error) {
  console.error('\nTest error:', error.message);

  try {
    const hasLoroMap = await page.evaluate(() => typeof window.LoroMap);
    console.log('window.LoroMap type:', hasLoroMap);
  } catch (e) {
    console.log('Could not check window.LoroMap');
  }

  try {
    const pageErrors = await page.evaluate(() => {
      return window.errors || [];
    });
    if (pageErrors.length > 0) {
      console.log('Page errors:', pageErrors);
    }
  } catch (e) {
    // Ignore
  }
} finally {
  // await browser.close();
  // serverProcess.kill();
  // process.exit(0);
}
