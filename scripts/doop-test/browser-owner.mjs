// One browser process ledger shared by private test families. Python owns recovery.
import assert from 'node:assert/strict';
import { open, rename, unlink } from 'node:fs/promises';
import { join } from 'node:path';

export function createBrowserOwner(puppeteer) {
  const runtime = process.env.DOOP_TEST_PRIVATE_RUNTIME;
  const owner = process.env.DOOP_TEST_PRIVATE_OWNER;
  assert.ok(runtime && /^[0-9a-f]{32}$/.test(owner ?? ''));
  let phase = 'new';
  let browser;
  let pid;
  let closing;
  const record = async (next, processId) => {
    const temporary = join(runtime, '.browser-process-' + Date.now() + '.json');
    const file = await open(temporary, 'wx', 0o600);
    try {
      await file.writeFile(JSON.stringify({ version: 1, phase: next, pid: processId, owner }));
      await file.sync();
    } finally { await file.close(); }
    try { await rename(temporary, join(runtime, 'browser-process.json')); }
    finally { await unlink(temporary).catch(() => {}); }
    phase = next;
  };
  return {
    async launch() {
      assert.equal(phase, 'new', 'one browser launch per private probe');
      phase = 'pending';
      await record('pending', null);
      browser = await puppeteer.launch({ executablePath: process.env.DOOP_TEST_UI_CHROME, headless: true,
        userDataDir: join(runtime, 'ui-profile'),
        args: [`--ignore-certificate-errors-spki-list=${process.env.DOOP_TEST_TLS_SPKI}`] });
      pid = browser.process()?.pid;
      assert.ok(Number.isInteger(pid) && pid > 1);
      await record('created', pid);
      return browser;
    },
    close() {
      closing ??= (async () => {
        if (browser && phase !== 'removed') {
          await browser.close();
          assert.equal(phase, 'created', 'browser creation identity remains unconfirmed');
          await record('removed', pid);
        }
      })();
      return closing;
    },
  };
}
