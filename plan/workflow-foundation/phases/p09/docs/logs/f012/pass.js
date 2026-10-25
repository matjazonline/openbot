// p09 F012 browser pass over B1a + B1b on localhost:3001 (phase database only).
const { chromium } = require('/Users/mac02/dev/openbot/node_modules/playwright-core');
const fs = require('fs');
const BASE = 'http://localhost:3001';
const C = process.env.COMPANY;
const AGENT = process.env.AGENT;
const SHOTS = __dirname + '/shots';
fs.mkdirSync(SHOTS, { recursive: true });
const results = [];
const check = (name, ok, detail = '') => {
  results.push({ name, ok: !!ok, detail });
  console.log(`${ok ? 'PASS' : 'FAIL'} ${name}${detail ? ' — ' + String(detail).replace(/\s+/g, ' ').slice(0, 220) : ''}`);
};

(async () => {
  const browser = await chromium.launch({ channel: 'chrome', headless: true });
  const context = await browser.newContext({
    storageState: __dirname + '/state.json',
    viewport: { width: 1400, height: 950 },
  });
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', (e) => errors.push(e.message));
  page.on('console', (m) => m.type() === 'error' && !m.text().startsWith('Failed to load resource') && errors.push(m.text()));
  let answer = 'dismiss';
  const dialogs = [];
  page.on('dialog', async (d) => {
    dialogs.push(d.message());
    if (answer === 'accept') await d.accept();
    else await d.dismiss();
  });
  const settle = async () => {
    await page.waitForLoadState('domcontentloaded');
    await page.waitForTimeout(200);
    await page.waitForFunction(() => !document.querySelector('.htmx-request, [aria-busy="true"]:not([data-lazy-card])'), null, { timeout: 15000 }).catch(() => {});
    await page.waitForTimeout(250);
  };
  page.on('response', (r) => r.status() === 404 && errors.push('404 ' + r.url()));
  const shot = async (name) => page.screenshot({ path: `${SHOTS}/${name}.png`, fullPage: false });
  // The way a user switches: the rail's theme toggle (daisyUI follows its checked state), which
  // also stores the choice for the next load.
  const theme = async (value) => {
    const state = await page.evaluate((v) => {
      const toggle = document.getElementById('theme-toggle');
      if (!toggle) return 'no toggle';
      toggle.checked = v === 'light';
      toggle.dispatchEvent(new Event('change', { bubbles: true }));
      return `${document.documentElement.getAttribute('data-theme')} / ${getComputedStyle(document.body).backgroundColor}`;
    }, value);
    await page.waitForTimeout(150);
    return state;
  };

  // A channel, through the channel workspace's own form endpoint.
  await page.goto(`${BASE}/ui/channels?company_id=${C}`);
  const channelStatus = await page.evaluate(async (company) => {
    const body = new URLSearchParams({ name: 'Support desk', slug: 'support', command_key: crypto.randomUUID() });
    const r = await fetch(`/ui/channels?company_id=${company}`, { method: 'POST', body, headers: { 'HX-Request': 'true' } });
    return r.status;
  }, C);
  check('channel created', channelStatus === 200, `status ${channelStatus}`);

  // Library → New → copy the reviewed support template.
  await page.goto(`${BASE}/ui/workflows?company_id=${C}`);
  await settle();
  await shot('01-empty-dark');
  await page.getByRole('button', { name: 'New' }).click();
  await settle();
  const copy = page.locator('form', { has: page.locator('input[value="reviewed-support"], [data-template="reviewed-support"]') }).first();
  const copyForms = await page.locator('form[hx-post*="/templates/"]').count();
  let copied = false;
  for (let i = 0; i < copyForms; i++) {
    const form = page.locator('form[hx-post*="/templates/"]').nth(i);
    if ((await form.getAttribute('hx-post')).includes('reviewed-support')) {
      await form.locator('button[type=submit]').click();
      copied = true;
      break;
    }
  }
  await settle();
  check('template copied', copied && (await page.locator('textarea[name=source]').count()) === 1);
  const workflowUrl = page.url();
  const workflow = new URL(workflowUrl).searchParams.get('workflow_id');
  check('address bar names the workflow', !!workflow, workflowUrl);

  // F003: typing marks the editor dirty.
  const form = page.locator('form[data-workflow-editor]');
  check('fresh editor is clean', (await form.getAttribute('data-dirty')) === null);
  await page.locator('input[name=title]').click();
  await page.keyboard.type(' (edited)');
  check('typing marks the editor dirty', (await form.getAttribute('data-dirty')) !== null);

  // F014: cancelling the discard prompt leaves the pressed button usable.
  for (const label of ['Tools', 'New']) {
    dialogs.length = 0;
    answer = 'dismiss';
    const button = page.getByRole('button', { name: label, exact: label === 'Tools' });
    await button.click();
    await page.waitForTimeout(300);
    const state = await button.evaluate((b) => ({ disabled: b.disabled, busy: b.getAttribute('aria-busy'), spinner: !!b.querySelector('.loading') }));
    check(`cancel from ${label} asks first`, dialogs[0] === 'The editor has changes that are not saved. Discard them?', dialogs[0]);
    check(`cancel from ${label} leaves the button usable`, !state.disabled && !state.busy && !state.spinner, JSON.stringify(state));
    check(`cancel from ${label} keeps the editor`, (await page.locator('textarea[name=source]').count()) === 1);
  }

  // F004: the status says what the pending request does.
  await page.route('**/save?**', async (route) => {
    await new Promise((r) => setTimeout(r, 1200));
    await route.continue();
  });
  const saving = page.getByRole('button', { name: 'Save and validate' });
  await saving.click();
  await page.waitForTimeout(400);
  const pendingStatus = await page.locator('#workflow-editor-status').innerText();
  check('status while validating', pendingStatus.includes('Saving and validating'), pendingStatus);
  await settle();
  await page.unroute('**/save?**');
  const afterValidate = await page.locator('#workflow-pane').innerText();
  check('validation preview shown', /is valid|could not be validated|Saved revision/.test(afterValidate), afterValidate.slice(0, 200));
  check('a successful save renders clean', (await page.locator('form[data-workflow-editor]').getAttribute('data-dirty')) === null);
  await shot('02-editor-preview-dark');

  // F004: a failed request keeps the text and says so.
  await page.route('**/save?**', (route) => route.abort());
  await page.locator('input[name=title]').click();
  await page.keyboard.type('!');
  const typedTitle = await page.locator('input[name=title]').inputValue();
  await page.getByRole('button', { name: 'Save draft' }).click();
  await page.waitForTimeout(600);
  const failedStatus = await page.locator('#workflow-editor-status').innerText();
  check('failed save says so', failedStatus === 'Not saved: the request failed. Your text is still here.', failedStatus);
  check('failed save keeps the text', (await page.locator('input[name=title]').inputValue()) === typedTitle);
  check('failed save clears busy', (await page.locator('form[data-workflow-editor]').getAttribute('aria-busy')) === null);
  await page.unroute('**/save?**');

  // The reference panel's failed load, then a retry.
  await page.route('**/ui/workflows/catalogue**', (route) => route.abort());
  const reference = page.locator('#workflow-reference');
  await reference.locator('summary').click();
  await page.waitForTimeout(600);
  const failedReference = await page.locator('#workflow-reference-body').innerText();
  check('reference failure says so', failedReference.includes('could not be loaded'), failedReference);
  check('reference failure clears busy', (await page.locator('#workflow-reference-body').getAttribute('aria-busy')) === null);
  await page.unroute('**/ui/workflows/catalogue**');
  await reference.locator('summary').click();
  await reference.locator('summary').click();
  await settle();
  const loadedReference = await page.locator('#workflow-reference').innerText();
  check('reference loads on retry', loadedReference.includes('agent.run'), loadedReference.slice(0, 120));

  // Save the edited title, validate and publish.
  answer = 'accept';
  await page.getByRole('button', { name: 'Save and validate' }).click();
  await settle();
  const publish = page.getByRole('button', { name: /Publish revision/ });
  check('publish offered', (await publish.count()) === 1, await page.locator('#workflow-pane').innerText().then((t) => t.slice(0, 300)));
  if (await publish.count()) {
    await publish.click();
    await settle();
  }
  const published = await page.locator('#workflow-pane').innerText();
  check('published', published.includes('Published version'), published.slice(0, 200));

  // Bindings tab: create on the channel, then activate.
  await page.getByRole('tab', { name: 'Bindings' }).click();
  await settle();
  check('not bound said plainly', (await page.locator('#workflow-pane').innerText()).includes('Not bound'));
  await shot('03-bindings-dark');
  const channelOption = await page.locator('select[name=association] option', { hasText: 'Support desk' }).getAttribute('value');
  await page.selectOption('select[name=association]', channelOption);
  const owner = await page.locator('select[name=run_as] option').first().getAttribute('value');
  await page.fill('input[name="param:reviewer"]', owner);
  await page.selectOption('select[name=message_trigger]', 'direct');
  await page.getByRole('button', { name: 'Save binding' }).click();
  await settle();
  let bindings = await page.locator('#workflow-pane').innerText();
  check('binding saved paused', bindings.includes('Saved binding') && bindings.includes('paused'), bindings.slice(0, 300));
  await page.getByRole('button', { name: 'Activate' }).click();
  await settle();
  bindings = await page.locator('#workflow-pane').innerText();
  check('binding activated (or refusal shown on its row)', bindings.includes('Activated binding') || bindings.includes('Not activated'), bindings.slice(0, 400));
  await shot('04-bindings-active-dark');

  // Keyboard: tab strip reachable and operable, focus visible.
  await page.getByRole('tab', { name: 'Bindings' }).focus();
  await page.keyboard.press('Shift+Tab');
  const focused = await page.evaluate(() => {
    const el = document.activeElement;
    const style = getComputedStyle(el);
    return { text: el.textContent.trim(), visible: el.matches(':focus-visible'), outline: style.outlineStyle + ' ' + style.outlineWidth, shadow: style.boxShadow };
  });
  check('Shift+Tab reaches the previous tab', focused.text === 'Versions and usage', JSON.stringify(focused));
  check('keyboard focus is visible', focused.visible && (focused.outline !== 'none 0px' || focused.shadow !== 'none'), JSON.stringify(focused));
  await page.keyboard.press('Enter');
  await settle();
  check('Enter on a tab opens it', (await page.locator('#workflow-pane').innerText()).includes('Published versions'));

  // Register the version as a tool from the versions tab; the tools pane lists and revokes it.
  const toolName = 'support_reply_' + Date.now().toString(36);
  await page.fill('#workflow-tool-form input[name=name]', toolName);
  await page.fill('#workflow-tool-form textarea[name=params_json]', JSON.stringify({ reviewer: owner }));
  await page.locator('#workflow-tool-form button[type=submit]').click();
  await settle();
  const registered = await page.locator('#workflow-tool-form').innerText();
  check('tool registered', registered.includes('Registered ' + toolName), registered.slice(0, 300));
  await page.getByRole('button', { name: 'Tools', exact: true }).click();
  await settle();
  const tools = await page.locator('#workflow-pane').innerText();
  check('tools pane lists the tool', tools.includes(toolName), tools.slice(0, 200));
  await shot('05-tools-dark');

  // Agent pane: lazy cards load; the grant saves; "used by" names the binding.
  await page.goto(`${BASE}/ui/agents?company_id=${C}&agent_id=${AGENT}`);
  await settle();
  await page.waitForTimeout(500);
  const toolsCard = page.locator('#agent-workflow-tools');
  const usageCard = page.locator('#agent-workflow-usage');
  check('agent tools card loaded', (await toolsCard.count()) === 1);
  check('agent used-by card loaded', (await usageCard.count()) === 1);
  if (await usageCard.count()) {
    const usage = await usageCard.innerText();
    check('used by names the workflow', usage.includes('Reviewed support') || usage.includes('Channel Support desk'), usage.slice(0, 200));
  }
  if (await toolsCard.count()) {
    await toolsCard.locator('textarea[name=tools]').fill(toolName + '\nmessage.reply');
    await toolsCard.getByRole('button', { name: 'Save tools' }).click();
    await settle();
    const granted = await page.locator('#agent-workflow-tools').innerText();
    check('grant saved', granted.includes('Saved 2 granted tools'), granted.slice(0, 200));
  }
  await shot('06-agent-dark');
  // A card whose load fails says so.
  await page.route('**/ui/workflows/agents/*/usage**', (route) => route.fulfill({ status: 500, body: 'no' }));
  await page.reload();
  await settle();
  await page.waitForTimeout(500);
  const fallback = await page.locator('[data-lazy-card]').allInnerTexts();
  check('failed card says so', fallback.some((t) => t.includes('could not be loaded')), JSON.stringify(fallback));
  await page.unroute('**/ui/workflows/agents/*/usage**');

  // Channel pane card.
  const channel = await page.evaluate(async (company) => {
    const html = await (await fetch(`/ui/channels?company_id=${company}`)).text();
    const m = html.match(/channel_id=([0-9a-f-]{36})/);
    return m && m[1];
  }, C);
  await page.goto(`${BASE}/ui/channels?company_id=${C}&channel_id=${channel}`);
  await settle();
  await page.waitForTimeout(500);
  const channelCard = page.locator('#channel-workflows-card');
  check('channel card loaded', (await channelCard.count()) === 1);
  if (await channelCard.count()) {
    const text = await channelCard.innerText();
    check('channel card states who answers', text.includes('An active workflow answers') || text.includes('No active workflow answers'), text.slice(0, 200));
  }
  await shot('07-channel-dark');

  // Both themes: the same views, and the contrast of their text against what is behind it.
  const contrast = (root = 'main, section') =>
    page.evaluate((root) => {
      const canvas = document.createElement('canvas');
      canvas.width = canvas.height = 1;
      const ctx = canvas.getContext('2d', { willReadFrequently: true });
      // Any CSS colour (oklch included) as sRGB 0–255 plus alpha 0–1, through a painted pixel.
      const rgb = (s) => {
        ctx.clearRect(0, 0, 1, 1);
        ctx.fillStyle = '#000';
        ctx.fillStyle = s;
        ctx.fillRect(0, 0, 1, 1);
        const [r, g, b, a] = ctx.getImageData(0, 0, 1, 1).data;
        return [r, g, b, a / 255];
      };
      const lum = ([r, g, b]) => {
        const f = (c) => ((c /= 255) <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
        return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b);
      };
      const background = (el) => {
        while (el) {
          const c = getComputedStyle(el).backgroundColor;
          const v = rgb(c);
          if (v[3] > 0.5) return v;
          el = el.parentElement;
        }
        return [255, 255, 255];
      };
      const worst = [];
      for (const el of document.querySelectorAll(root.split(',').map((s) => s.trim() + ' *').join(', '))) {
        if (!el.childNodes.length || ![...el.childNodes].some((n) => n.nodeType === 3 && n.textContent.trim())) continue;
        const style = getComputedStyle(el);
        if (style.visibility === 'hidden' || style.display === 'none' || el.getClientRects().length === 0) continue;
        const fg = rgb(style.color);
        let alpha = fg[3];
        for (let node = el; node; node = node.parentElement) alpha *= Number(getComputedStyle(node).opacity);
        if (alpha < 0.05) continue;
        const bg = background(el);
        const mixed = fg.slice(0, 3).map((c, i) => c * alpha + bg[i] * (1 - alpha));
        const [a, b] = [lum(mixed), lum(bg)].sort((x, y) => y - x);
        const ratio = (a + 0.05) / (b + 0.05);
        worst.push({ ratio: Math.round(ratio * 100) / 100, text: el.textContent.trim().slice(0, 40), cls: el.className.toString().slice(0, 60) });
      }
      worst.sort((x, y) => x.ratio - y.ratio);
      return worst.slice(0, 5);
    }, root);
  const views = [
    ['bindings', `${BASE}/ui/workflows?company_id=${C}&workflow_id=${workflow}&tab=bindings`],
    ['editor', `${BASE}/ui/workflows?company_id=${C}&workflow_id=${workflow}&tab=editor`],
    ['versions', `${BASE}/ui/workflows?company_id=${C}&workflow_id=${workflow}&tab=versions`],
    ['tools', `${BASE}/ui/workflows?company_id=${C}&tools=1`],
    ['agent', `${BASE}/ui/agents?company_id=${C}&agent_id=${AGENT}`],
  ];
  for (const mode of ['light', 'dark']) {
    for (const [name, url] of views) {
      await page.goto(url);
      await settle();
      await page.waitForTimeout(300);
      const applied = await theme(mode);
      if (name === 'bindings') check(`${mode} theme applied`, applied.startsWith(mode), applied);
      await shot(`theme-${mode}-${name}`);
      // The agent pane's own form is p08's; p09 owns its two workflow cards there.
      const scope = name === 'agent' ? '#agent-workflow-tools, #agent-workflow-usage' : 'main, section';
      const low = await contrast(scope);
      const bad = low.filter((entry) => entry.ratio < 3);
      check(`${mode} ${name}: no text under 3:1`, bad.length === 0, JSON.stringify(low.slice(0, 3)));
      if (name === 'agent') {
        const whole = (await contrast()).filter((entry) => entry.ratio < 3);
        console.log(`NOTE ${mode} agent pane outside p09 cards under 3:1: ${JSON.stringify(whole.slice(0, 3))}`);
      }
    }
  }
  // The graph is drawn with theme tokens: its node fill changes with the theme.
  // A blank draft validates to a one-step graph drawn with theme tokens.
  await page.goto(`${BASE}/ui/workflows?company_id=${C}&new=1`);
  await settle();
  await page.fill('form[hx-post*="/ui/workflows/new"] input[name=title]', 'Graph check');
  await page.locator('form[hx-post*="/ui/workflows/new"] button[type=submit]').click();
  await settle();
  await page.getByRole('button', { name: 'Save and validate' }).click();
  await settle();
  const fills = {};
  for (const mode of ['light', 'dark']) {
    await theme(mode);
    fills[mode] = await page.evaluate(() => {
      const node = document.querySelector('#workflow-pane svg rect, #workflow-pane svg circle');
      return node ? getComputedStyle(node).fill + ' / ' + getComputedStyle(node).stroke : null;
    });
    await shot(`08-graph-${mode}`);
  }
  check('graph follows the theme', fills.light && fills.dark && fills.light !== fills.dark, JSON.stringify(fills));
  // A refused validation's excerpt is readable in both themes.
  // Back to the valid source with one unknown step type: a located diagnostic.
  await page.goto(page.url());
  await settle();
  const valid = await page.locator('textarea[name=source]').inputValue();
  await page.locator('textarea[name=source]').fill(valid.replace('type: data.map', 'type: data.nope'));
  await page.getByRole('button', { name: 'Save and validate' }).click();
  await settle();
  check('a refused validation shows its excerpt', (await page.locator('#workflow-diagnostic pre').count()) === 1);
  for (const mode of ['light', 'dark']) {
    await theme(mode);
    const low = await contrast();
    check(`${mode} diagnostic: no text under 3:1`, low.filter((e) => e.ratio < 3).length === 0, JSON.stringify(low.slice(0, 3)));
    await shot(`09-diagnostic-${mode}`);
  }

  // htmx logs the requests this script aborted on purpose (save, reference) as errors.
  // …and the 500 it answered the usage card with.
  const unexpected = errors.filter((e) => !/^htmx:(afterRequest|sendError|responseError)$/.test(e) && !/Response Status Error Code 500 from \/ui\/workflows\/agents\/.*\/usage/.test(e));
  check('no unexpected page errors', unexpected.length === 0, unexpected.slice(0, 3).join(' | '));
  fs.writeFileSync(__dirname + '/results.json', JSON.stringify(results, null, 2));
  console.log(`${results.filter((r) => r.ok).length}/${results.length} passed`);
  await browser.close();
})().catch((e) => {
  console.log('SCRIPT ERROR', e.message.split('\n').slice(0, 6).join('\n'));
  fs.writeFileSync(__dirname + '/results.json', JSON.stringify(results, null, 2));
  process.exit(1);
});
