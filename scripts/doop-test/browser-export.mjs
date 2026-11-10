// Normal UI PDF controls and one owned browser download session, shared by private families.
import assert from 'node:assert/strict';
import { mkdir, readFile } from 'node:fs/promises';
import { join } from 'node:path';

export async function downloadCanvasPdf({ browser, page, origin, canvas, frames, directory }) {
  assert.ok(frames.length >= 1 && frames.length <= 2);
  const runtime = process.env.DOOP_TEST_PRIVATE_RUNTIME;
    const downloads = join(runtime, directory); await mkdir(downloads, { mode: 0o700 });
    const session = await browser.target().createCDPSession();
    assert.equal(typeof page.browserContext().id, 'string');
    await session.send('Browser.setDownloadBehavior', { behavior: 'allowAndName', downloadPath: downloads,
      browserContextId: page.browserContext().id, eventsEnabled: true });
    let started;
    let timer;
    const saved = new Promise((resolve, reject) => {
      timer = setTimeout(() => reject(new Error('Normal UI PDF download did not complete')), 20000);
      session.on('Browser.downloadWillBegin', (event) => { started = event; });
      session.on('Browser.downloadProgress', (event) => {
        if (event.state === 'canceled') reject(new Error('Normal UI PDF download was canceled'));
        if (event.state === 'completed') {
          try {
            assert.equal(event.guid, started?.guid);
            assert.equal(new URL(started.url).origin, origin);
            assert.ok(started.suggestedFilename.endsWith('.pdf'));
            resolve(event.guid);
          } catch (error) { reject(error); }
        }
      });
    });
    saved.catch(() => {});
    try {
    await page.click('button[title="Export frames as a PDF"]');
    await page.waitForSelector('[role="dialog"]', { timeout: 10000 });
    const labels = await page.$$('[role="dialog"] label');
    assert.ok(labels.length >= frames.length);
    for (const label of labels) {
      const state = await label.evaluate((node) => ({ text: node.textContent,
        checked: node.querySelector('input[type="checkbox"]')?.checked }));
      assert.equal(typeof state.checked, 'boolean');
      const selected = frames.some((frame) => state.text.includes(frame.name));
      if (state.checked !== selected) await label.click();
    }
    const expectedPath = `/api/canvases/${canvas.id}/export.pdf`;
    const expectedFrames = frames.map((frame) => frame.id).join(',');
    const download = page.waitForResponse((response) => {
      const url = new URL(response.url());
      return url.origin === origin && url.pathname === expectedPath && url.searchParams.get('frames') === expectedFrames;
    }, { timeout: 20000 });
    download.catch(() => {});
    const buttons = await page.$$('[role="dialog"] button');
    let clicked = false;
    for (const button of buttons) if ((await button.evaluate((node) => node.textContent)).startsWith('Download PDF')) {
      await button.click(); clicked = true; break;
    }
    assert.ok(clicked, 'normal PDF download control is missing');
    const response = await download; assert.equal(response.status(), 200);
    const guid = await saved;
    assert.ok(/^[0-9a-f-]{36}$/.test(guid));
    const bytes = await readFile(join(downloads, guid));
    assert.deepEqual(bytes, Buffer.from(await response.buffer()));
    assert.ok(bytes.length <= 1024 * 1024);
    return bytes;
    } finally { clearTimeout(timer); await session.detach(); }
}
