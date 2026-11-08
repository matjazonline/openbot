import test from 'node:test';
import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { acquireProfileLock } from '../src/lock.js';
import { fixture } from './support.js';

if (process.env.OPENBOT_LOCK_WORKER) {
  try {
    const release = acquireProfileLock(process.env.OPENBOT_LOCK_WORKER);
    if (process.env.OPENBOT_LOCK_HOLD) {
      process.on('SIGTERM', () => { release(); process.exit(0); });
      process.stdout.write('owned\n'); setInterval(() => {}, 1000);
      await new Promise(() => {});
    } else { release(); process.exit(0); }
  } catch { process.exit(2); }
}
test('real competing connector processes exclude each other and OS releases graceful and killed owners', async () => {
  const f = fixture(); const file = fileURLToPath(import.meta.url); let owner: ReturnType<typeof spawn> | undefined;
  const probe = () => spawnSync(process.execPath, [file], { env: { ...process.env, OPENBOT_LOCK_WORKER: f.file }, timeout: 5000 }).status;
  const hold = async () => {
    const child = spawn(process.execPath, [file], { env: { ...process.env, OPENBOT_LOCK_WORKER: f.file, OPENBOT_LOCK_HOLD: '1' }, stdio: ['ignore', 'pipe', 'pipe'] });
    await new Promise<void>((resolve, reject) => { child.stdout!.once('data', () => resolve()); child.once('exit', code => reject(new Error(`owner exited ${code}`))); });
    return child;
  };
  try {
    for (const signal of ['SIGTERM', 'SIGKILL'] as const) {
      owner = await hold(); assert.equal(probe(), 2);
      const exited = new Promise(resolve => owner!.once('exit', resolve)); owner.kill(signal); await exited;
      assert.equal(probe(), 0); owner = undefined;
    }
  } finally { owner?.kill('SIGKILL'); f.close(); }
});
