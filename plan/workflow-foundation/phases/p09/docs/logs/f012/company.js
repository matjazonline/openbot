const { chromium } = require('/Users/mac02/dev/openbot/node_modules/playwright-core');
(async () => {
  const browser = await chromium.launch({ channel: 'chrome', headless: true });
  const context = await browser.newContext({ storageState: __dirname + '/state.json' });
  const page = await context.newPage();
  await page.goto('http://localhost:3001/ui/companies');
  const status = await page.evaluate(async () => {
    const body = new URLSearchParams({ name: 'Acme Support', slug: 'acme' });
    const r = await fetch('/ui/companies', { method: 'POST', body, headers: { 'HX-Request': 'true' } });
    return r.status;
  });
  console.log('create company', status);
  await context.storageState({ path: __dirname + '/state.json' });
  await browser.close();
})();
