// Run the same fixture checks used interactively, with real Chromium in CI.
const { chromium } = require(process.env.PLAYWRIGHT_MODULE || 'playwright');
const { readFileSync } = require('node:fs');
const { join } = require('node:path');
const { spawn } = require('node:child_process');

(async () => {
  const url = process.env.READER_UX_URL || 'http://127.0.0.1:18763';
  const server = process.env.READER_UX_URL ? null : spawn('python3', [join(__dirname, 'serve.py')], {stdio: 'ignore'});
  let browser;
  try {
    let ready = false;
    for (let attempt = 0; attempt < 100; attempt++) {
      try { ready = (await fetch(url)).ok; } catch (_) { /* Server is starting. */ }
      if (ready) break;
      await new Promise(resolve => setTimeout(resolve, 100));
    }
    if (!ready) throw new Error('Reader fixture server did not start');
    browser = await chromium.launch({headless: true, executablePath: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH || undefined});
    for (const file of ['check.js', 'navigation-check.js', 'settings-update-check.js', 'search-keyboard-check.js', 'search-layout-check.js']) {
      const context = await browser.newContext();
      const page = await context.newPage();
      const errors = [];
      page.on('pageerror', error => errors.push(error.message));
      page.setDefaultTimeout(20000);
      try {
        await page.goto(url);
        const check = new Function(`return (${readFileSync(join(__dirname, file), 'utf8')})`)();
        const result = await check(page);
        if (errors.length) throw new Error(errors.join('\n'));
        console.log(`${file}: ${JSON.stringify(result)}`);
      } catch (error) {
        throw new Error(`${file}: ${error.message}`, {cause: error});
      } finally { await context.close(); }
    }
  } finally {
    if (browser) await browser.close();
    if (server) server.kill();
  }
})().catch(error => { console.error(error); process.exitCode = 1; });
