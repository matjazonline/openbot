// p09 F012 browser pass, stage 1: a local test owner, a company and an agent on the phase
// database (localhost:3001 over cluster 55490). Values are throwaway, for this local run only.
const { chromium } = require('/Users/mac02/dev/openbot/node_modules/playwright-core');
const fs = require('fs');
const crypto = require('crypto');
const BASE = 'http://localhost:3001';
const STATE = __dirname + '/state.json';

(async () => {
  const user = process.env.USERNAME_FOR_RUN || 'owner';
  const password = crypto.randomBytes(18).toString('base64url');
  fs.writeFileSync(__dirname + '/password.' + user, password, { mode: 0o600 });
  const browser = await chromium.launch({ channel: 'chrome', headless: true });
  const context = await browser.newContext({ viewport: { width: 1400, height: 900 } });
  const page = await context.newPage();
  page.on('pageerror', (e) => console.log('PAGEERROR', e.message));
  await page.goto(BASE + '/register');
  await page.fill('[name=username]', user);
  await page.fill('[name=email]', user + '@example.test');
  await page.fill('[name=password]', password);
  if (await page.locator('[name=confirm_password]').count()) {
    await page.fill('[name=confirm_password]', password);
  }
  await Promise.all([page.waitForLoadState('networkidle'), page.locator('form [type=submit]').first().click()]);
  console.log('after register', page.url());
  if (page.url().includes('/login') || page.url().endsWith('/register')) {
    await page.goto(BASE + '/login');
    await page.fill('[name=email_or_username]', user);
    await page.fill('[name=password]', password);
    await Promise.all([page.waitForLoadState('networkidle'), page.locator('form [type=submit]').first().click()]);
    console.log('after login', page.url());
  }
  await page.goto(BASE + '/ui/companies');
  const status = await page.evaluate(async (slug) => {
    const body = new URLSearchParams({ name: 'Beta Support', slug });
    return (await fetch('/ui/companies', { method: 'POST', body, headers: { 'HX-Request': 'true' } })).status;
  }, 'beta-' + user);
  console.log('company', status);
  await context.storageState({ path: STATE });
  await browser.close();
})();
