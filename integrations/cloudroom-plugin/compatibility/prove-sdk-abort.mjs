import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { prepareSource } from './check-source.mjs';

const checkout = process.argv[2];
assert(checkout, 'usage: node compatibility/prove-sdk-abort.mjs <pinned-source-checkout>');
const artifact = prepareSource(checkout);
const { createNodeBbSdk } = await import(pathToFileURL(resolve(checkout, 'gui/packages/sdk/dist/node.js')).href);
const server = createServer(); const requests = []; let arrived;
server.on('request', (request, response) => {
  let body = '';
  request.setEncoding('utf8'); request.on('data', part => { body += part; });
  request.on('end', () => { requests.push({ path: request.url, body: body ? JSON.parse(body) : null });
    arrived?.(response); });
});
await new Promise(done => server.listen(0, '127.0.0.1', done));
const address = server.address(); assert(address && typeof address !== 'string');
const sdk = createNodeBbSdk({ baseUrl: `http://127.0.0.1:${address.port}`, callerToken: 'disposable-sdk-proof' });
const operations = {
  spawn: signal => sdk.threads.spawn({ projectId: 'proj_disposable', providerId: 'codex', model: 'gpt-test', prompt: 'disposable cancellation proof', signal }),
  send: signal => sdk.threads.send({ threadId: 'thr_disposable', mode: 'queue-if-active', model: 'gpt-test', input: [{ type: 'text', text: 'disposable', mentions: [] }], signal }),
  stop: signal => sdk.threads.stop({ threadId: 'thr_disposable', signal }),
  archive: signal => sdk.threads.archive({ threadId: 'thr_disposable', signal }),
  archiveAll: signal => sdk.threads.archiveAll({ threadId: 'thr_disposable', signal }),
};
const results = [];
try {
  for (const [method, call] of Object.entries(operations)) {
    const before = requests.length;
    await assert.rejects(call(AbortSignal.abort()));
    assert.equal(requests.length, before, 'already-aborted operation must not reach HTTP');
    for (const cut of ['headers', 'body']) {
      const controller = new AbortController();
      const received = new Promise(done => { arrived = response => { if (cut === 'body') { response.writeHead(200, { 'content-type': 'application/json' }); response.write('{'); } done(); }; });
      const pending = call(controller.signal); const rejected = assert.rejects(pending);
      await received; const began = performance.now(); controller.abort(); await rejected;
      assert(performance.now() - began < 1_000, 'real HTTP wait must abort inside host drain');
      const request = requests.at(-1); assert(!JSON.stringify(request.body).includes('signal'));
      if (method === 'send') assert.equal(request.body.mode, 'queue-if-active');
      server.closeAllConnections(); results.push({ method, cut, cancelled: true });
    }
  }
  console.log(JSON.stringify({ artifact, results, nativeEffects: 'NOT EXERCISED' }, null, 2));
} finally { server.closeAllConnections(); await new Promise(done => server.close(done)); }
