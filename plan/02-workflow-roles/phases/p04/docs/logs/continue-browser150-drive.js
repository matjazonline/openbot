// Drives the real escalation decision page: reassign, then acknowledge, through actual DOM clicks.
const { chromium } = require(process.env.PLAYWRIGHT_MODULE);
const fs = require('fs');
const [fixtureFile, outDir] = process.argv.slice(2);
(async () => {
  const fixture = JSON.parse(fs.readFileSync(fixtureFile, 'utf8'));
  const browser = await chromium.launch({ channel: 'chrome', headless: true });
  const page = await browser.newPage({ colorScheme: 'light' });
  const steps = [];
  page.on('request', (r) => { if (r.method() === 'POST') steps.push({ post: new URL(r.url()).pathname }); });
  await page.goto(fixture.url, { waitUntil: 'networkidle' });
  await page.screenshot({ path: `${outDir}/before.png`, fullPage: true });
  const reassign = page.locator('form:has(select[name="principal"]) button[type="submit"]');
  steps.push({ reassign_forms: await reassign.count(),
    options: await page.locator('select[name="principal"] option').allInnerTexts() });
  // The fixture session's own principal, whose display label is a hostile markup probe.
  await page.selectOption('select[name="principal"]', { label: 'Review <img src=x onerror=alert(1)> & "team"' });
  await reassign.first().click();
  await page.waitForResponse((r) => r.url().endsWith('/submit') && r.request().method() === 'POST');
  await page.waitForLoadState('networkidle');
  steps.push({ after_reassign: (await page.locator('#workflow-escalation').innerText()).slice(0, 600) });
  await page.getByRole('link', { name: 'View current assignment' }).click();
  await page.waitForLoadState('networkidle');
  await page.screenshot({ path: `${outDir}/current.png`, fullPage: true });
  const ack = page.locator('form:has(input[name="choice"][value="acknowledge"]) button[type="submit"]');
  await ack.first().waitFor({ state: 'visible', timeout: 15000 });
  steps.push({ current: page.url() });
  await ack.first().click();
  await page.waitForResponse((r) => r.url().endsWith('/submit') && r.request().method() === 'POST');
  await page.waitForLoadState('networkidle');
  await page.screenshot({ path: `${outDir}/acknowledged.png`, fullPage: true });
  const final = {
    body: await page.locator('main').innerText().catch(() => page.locator('body').innerText()),
    forms: await page.locator('#workflow-escalation form').count(),
    images: await page.locator('#workflow-escalation img').count(),
    steps,
  };
  fs.writeFileSync(`${outDir}/final.json`, JSON.stringify(final, null, 2));
  await page.request.post(fixture.finish_url).catch(() => {});
  await browser.close();
})().catch((e) => { console.error(e); process.exit(1); });
