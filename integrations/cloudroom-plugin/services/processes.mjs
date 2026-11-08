import { spawn } from 'node:child_process';
import { setTimeout as delay } from 'node:timers/promises';

export function startOwned(children, command, args, options, stop) {
  stop.signal.throwIfAborted();
  const child = spawn(command, args, { detached: true, stdio: ['ignore', 'ignore', 'ignore'], ...options });
  const owned = { child, exited: false, done: undefined };
  owned.done = new Promise(resolve => {
    const end = () => { owned.exited = true; if (!stop.signal.aborted) stop.abort(new Error('owned component exited')); resolve(); };
    child.once('error', end); child.once('exit', end);
  });
  children.push(owned); return owned;
}
function signalGroup(owned, signal) {
  if (!owned.child.pid) return;
  try { process.kill(-owned.child.pid, signal); }
  catch (error) { if (error.code !== 'ESRCH') throw error; }
}
export async function stopOwned(children) {
  for (const owned of [...children].reverse()) signalGroup(owned, 'SIGTERM');
  await Promise.race([Promise.all(children.map(owned => owned.done)), delay(5000, undefined, { ref: false })]);
  // A leader may exit before its descendants. Signal every owned group anyway.
  for (const owned of children) signalGroup(owned, 'SIGKILL');
  const joined = await Promise.race([Promise.all(children.map(owned => owned.done)).then(() => true), delay(2000, undefined, { ref: false }).then(() => false)]);
  if (!joined) throw new Error('owned components failed to join');
}
export async function ready(url, owned, signal) {
  const expires = performance.now() + 30_000;
  while (!owned.exited && performance.now() < expires) {
    signal.throwIfAborted();
    try { if ((await fetch(url, { signal: AbortSignal.any([signal, AbortSignal.timeout(1000)]) })).ok) return; } catch { signal.throwIfAborted(); }
    await delay(100, undefined, { signal });
  }
  throw new Error('owned component readiness failed');
}
