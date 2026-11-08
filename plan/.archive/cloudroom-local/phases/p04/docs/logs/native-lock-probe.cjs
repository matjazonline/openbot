const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { fork, spawnSync } = require('node:child_process');
const { once } = require('node:events');
const fixture = process.argv[2];
const { flockSync } = require(require.resolve('fs-ext', { paths: [fixture] }));
const lock = path.join(fixture, 'connector.lock');
const open = () => fs.openSync(lock, fs.constants.O_RDWR | fs.constants.O_CREAT | fs.constants.O_NOFOLLOW, 0o600);
if (process.argv[3] === 'try') {
  const fd = open();
  try { flockSync(fd, 'exnb'); process.stdout.write('acquired\n'); }
  catch (error) { assert(['EAGAIN', 'EWOULDBLOCK'].includes(error.code)); process.stdout.write('blocked\n'); process.exitCode = 73; }
  finally { fs.closeSync(fd); }
} else if (process.argv[3] === 'hold') {
  const fd = open();
  flockSync(fd, 'exnb');
  process.send('held');
  process.on('message', () => {});
} else {
  const compete = () => spawnSync(process.execPath, [__filename, fixture, 'try'], { timeout: 5000, encoding: 'utf8' });
  (async () => {
    const fd = open();
    flockSync(fd, 'exnb');
    assert.equal(compete().status, 73, 'second process cannot take live lock');
    fs.closeSync(fd);
    assert.equal(compete().status, 0, 'graceful close releases ownership');
    const child = fork(__filename, [fixture, 'hold'], { stdio: ['ignore', 'ignore', 'pipe', 'ipc'] });
    try {
      const [ready] = await once(child, 'message', { signal: AbortSignal.timeout(5000) });
      assert.equal(ready, 'held');
      assert.equal(compete().status, 73, 'a child process owns the same lock');
      const exited = once(child, 'exit', { signal: AbortSignal.timeout(5000) });
      child.kill('SIGKILL');
      await exited;
      assert.equal(compete().status, 0, 'OS releases lock after abrupt process death');
      process.stdout.write('PASS fs-ext2.1.1/Node24.21.0 process lock: competing process refused, graceful release, SIGKILL release; no stale-file deletion\n');
    } finally { if (child.exitCode === null && child.signalCode === null) child.kill('SIGKILL'); }
  })().catch(error => { console.error(error); process.exitCode = 1; });
}
