import test from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, rmSync, chmodSync, symlinkSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { once } from 'node:events';
import { acquireProfile } from './profile.mjs';
import { startOwned, stopOwned } from './processes.mjs';

test('profile ownership excludes competing processes and refuses unsafe directory/symlink', () => {
  const directory = mkdtempSync(join(tmpdir(), 'openbot-service-lock-')); const profile = join(directory, 'profile');
  const module = new URL('./profile.mjs', import.meta.url).href;
  const probe = () => spawnSync(process.execPath, ['--input-type=module', '-e',
    `import {acquireProfile} from ${JSON.stringify(module)}; acquireProfile(process.argv[1])();`, profile], { timeout: 5000, stdio: 'ignore' }).status;
  try {
    const release = acquireProfile(profile);
    try { assert.notEqual(probe(), 0); } finally { release(); }
    assert.equal(probe(), 0);
    chmodSync(profile, 0o755); assert.throws(() => acquireProfile(profile)); chmodSync(profile, 0o700);
    symlinkSync(profile, join(directory, 'alias')); assert.throws(() => acquireProfile(join(directory, 'alias')));
  } finally { rmSync(directory, { recursive: true, force: true }); }
});
test('supervisor observes failure, stops siblings and joins real children', async () => {
  const children = []; const stop = new AbortController();
  const sibling = startOwned(children, process.execPath, ['-e', 'setInterval(()=>{},1000)'], {}, stop);
  const failed = startOwned(children, process.execPath, ['-e', 'process.exit(3)'], {}, stop);
  try {
    await failed.done; assert(stop.signal.aborted); assert.equal(failed.child.exitCode, 3);
  } finally { stop.abort(); await stopOwned(children); }
  assert(sibling.exited); assert.equal(sibling.child.signalCode, 'SIGTERM');
});
test('shutdown escalates a child which ignores TERM and joins it within the fixed drain', async () => {
  const children = []; const stop = new AbortController();
  const owned = startOwned(children, process.execPath, ['-e', "process.on('SIGTERM',()=>{}); process.send('ready'); setInterval(()=>{},1000)"],
    { stdio: ['ignore', 'ignore', 'ignore', 'ipc'] }, stop);
  await once(owned.child, 'message');
  const began = performance.now(); stop.abort(); await stopOwned(children);
  assert(owned.exited); assert.equal(owned.child.signalCode, 'SIGKILL'); assert(performance.now() - began < 7500);
});
