// Exercise the real sandboxed host browser before the expensive service/integration gates.
import assert from 'node:assert/strict';
import { randomBytes } from 'node:crypto';
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { join, resolve } from 'node:path';
import { createBrowserOwner } from './browser-owner.mjs';

const [source, output] = process.argv.slice(2);
const require = createRequire(join(resolve(source), 'package.json'));
const runtime = await mkdtemp(join(resolve(output), 'browser-ipc-'));
const longTemporary = join(runtime, 'long-browser-temporary-directory-'.repeat(5));
await mkdir(longTemporary);
assert.ok(Buffer.byteLength(longTemporary) > 108);
Object.assign(process.env, {
  DOOP_TEST_PRIVATE_RUNTIME: runtime,
  DOOP_TEST_PRIVATE_OWNER: randomBytes(16).toString('hex'),
  DOOP_TEST_UI_CHROME: process.env.CHROME_PATH,
  DOOP_TEST_TLS_SPKI: 'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=',
  TMPDIR: longTemporary,
});
const owner = createBrowserOwner(require('puppeteer-core'));
const browser = await owner.launch();
try {
  const page = await browser.newPage();
  await page.goto('about:blank');
  const receipt = JSON.parse(await readFile(join(runtime, 'browser-process.json'), 'utf8'));
  assert.equal(receipt.phase, 'created');
  assert.equal(receipt.pid, browser.process().pid);
  assert.equal(process.env.TMPDIR, longTemporary);
} finally {
  await owner.close();
}
const receipt = JSON.parse(await readFile(join(runtime, 'browser-process.json'), 'utf8'));
assert.equal(receipt.phase, 'removed');
await rm(runtime, { recursive: true });
await writeFile(join(resolve(output), 'browser-ipc-check.json'), JSON.stringify({
  version: 1, status: 'PASS', parentTemporaryBytes: Buffer.byteLength(longTemporary),
  profileRunOwned: true, processRemovalConfirmed: true,
}) + '\n', { mode: 0o600 });
console.log('DOOP_BROWSER_IPC_PASS long parent temporary directory, run-owned profile, confirmed close');
