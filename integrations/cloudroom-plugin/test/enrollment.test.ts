import test from 'node:test';
import assert from 'node:assert/strict';
import Database from 'better-sqlite3';
import { readFileSync, writeFileSync, renameSync, existsSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import { Journal } from '../src/journal.js';
import { recoverEnrollment, type SettingsStore } from '../src/enrollment.js';
import { fixture } from './support.js';

type Values = Awaited<ReturnType<SettingsStore['get']>>;
function store(directory: string, cut?: string): SettingsStore {
  const path = join(directory, 'settings.json');
  return { get: async () => JSON.parse(readFileSync(path, 'utf8')) as Values,
    experimental_set: async patch => {
      const values = { ...JSON.parse(readFileSync(path, 'utf8')) };
      for (const [key, value] of Object.entries(patch)) { if (value === null) delete values[key]; else values[key] = value; }
      const state = values.enrollment ? JSON.parse(values.enrollment).state as string : 'empty';
      if (cut === 'before-pending' && state === 'pending') process.kill(process.pid, 'SIGKILL');
      writeFileSync(`${path}.tmp`, JSON.stringify(values), { mode: 0o600 }); renameSync(`${path}.tmp`, path);
      if ((cut === 'pending' && state === 'pending') || (cut === 'paired' && state === 'paired')
          || (cut === 'cleaned' && values.pairingCode === undefined)
          || (cut === 'rejected' && values.pairingCode === undefined && state === 'empty')) process.kill(process.pid, 'SIGKILL');
      return values as Values;
    } };
}
function exchange(directory: string, cut?: string): typeof fetch {
  const receiptPath = join(directory, 'server.json');
  return (async (_url: string | URL | Request, init?: RequestInit) => {
    const values = await store(directory).get(); const pending = JSON.parse(values.enrollment!);
    const headers = new Headers(init?.headers);
    if (init?.method !== 'POST') {
      assert.equal(headers.get('x-openbot-installation-id'), pending.installation_id);
      assert.equal(headers.get('x-openbot-pair-claim'), pending.claim_secret);
      if (!existsSync(receiptPath)) return new Response(null, { status: 404 });
      if (cut === 'receipt') process.kill(process.pid, 'SIGKILL');
      return Response.json(JSON.parse(readFileSync(receiptPath, 'utf8')));
    }
    assert(!existsSync(receiptPath), 'a second enrollment is forbidden');
    assert.deepEqual(JSON.parse(init.body as string), pending.request);
    const ids = JSON.parse(readFileSync(join(directory, 'ids.json'), 'utf8'));
    const receipt = { company_id: ids.company_id, company_name: 'Disposable company',
      machine: { id: ids.machine_id, company_id: ids.company_id, installation_id: pending.installation_id,
        name: pending.request.name, state: 'enabled', credential_generation: 1, last_seen_at: null, capabilities_sequence: null } };
    writeFileSync(receiptPath, JSON.stringify(receipt));
    if (cut === 'server-commit') process.kill(process.pid, 'SIGKILL');
    return Response.json(receipt);
  }) as typeof fetch;
}
if (process.env.OPENBOT_ENROLLMENT_WORKER) {
  const directory = process.env.OPENBOT_ENROLLMENT_WORKER; const cut = process.env.OPENBOT_ENROLLMENT_CUT;
  const db = new Database(join(directory, 'data.db')); db.pragma('journal_mode=WAL');
  const journal = new Journal(db); const original = journal.initialize.bind(journal);
  journal.initialize = binding => { original(binding); if (cut === 'journal') process.kill(process.pid, 'SIGKILL'); };
  const fetcher = process.env.OPENBOT_ENROLLMENT_REJECT ? (async (_url, init) => new Response(null,
    { status: init?.method === 'POST' ? 400 : 404 })) as typeof fetch : exchange(directory, cut);
  await recoverEnrollment(store(directory, cut), journal, new AbortController().signal, fetcher);
  db.close(); process.exit(0);
}
for (const cut of ['before-pending', 'pending', 'server-commit', 'receipt', 'journal', 'paired', 'cleaned']) {
  test(`real process interruption at enrollment ${cut} recovers the same durable claim`, async () => {
    const f = fixture({ initialized: false });
    try {
      writeFileSync(join(f.directory, 'settings.json'), JSON.stringify({ openbotOrigin: f.binding.origin,
        machineName: 'Disposable machine', selectedProjects: '', pairingCode: 'a'.repeat(32) }), { mode: 0o600 });
      writeFileSync(join(f.directory, 'ids.json'), JSON.stringify(f.binding)); f.journal.db.close();
      // The receipt cut requires an already committed receipt and original pending record.
      if (cut === 'receipt') {
        const first = spawnSync(process.execPath, [fileURLToPath(import.meta.url)], { env: { ...process.env,
          OPENBOT_ENROLLMENT_WORKER: f.directory, OPENBOT_ENROLLMENT_CUT: 'server-commit' }, timeout: 10_000 });
        assert.equal(first.signal, 'SIGKILL');
      }
      const crashed = spawnSync(process.execPath, [fileURLToPath(import.meta.url)], { env: { ...process.env,
        OPENBOT_ENROLLMENT_WORKER: f.directory, OPENBOT_ENROLLMENT_CUT: cut }, timeout: 10_000 });
      assert.equal(crashed.signal, 'SIGKILL', crashed.stderr.toString());
      const prior = await store(f.directory).get(); const candidate = prior.enrollment ? JSON.parse(prior.enrollment) : null;
      const reopened = f.open(); const paired = await recoverEnrollment(store(f.directory), reopened, new AbortController().signal, exchange(f.directory));
      if (candidate?.state === 'pending') {
        assert.equal(paired.binding.installation_id, candidate.installation_id); assert.equal(paired.binding.journal_id, candidate.journal_id);
        assert.equal(paired.credential, candidate.credential);
      } else if (candidate?.state === 'paired') assert.deepEqual(paired, candidate);
      assert.equal((await store(f.directory).get()).pairingCode, undefined);
      assert.equal(statSync(join(f.directory, 'settings.json')).mode & 0o777, 0o600);
      assert.equal(reopened.sequence(), 0); assert.equal(reopened.capacity().effects, 0);
      assert(!JSON.stringify(reopened.binding()).includes(paired.credential));
      // A promoted secret cannot silently initialize an erased journal.
      reopened.db.prepare('DELETE FROM journal_meta').run();
      await assert.rejects(recoverEnrollment(store(f.directory), reopened, new AbortController().signal, exchange(f.directory)), /recovery/);
    } finally { f.close(); }
  });
}
test('changed origin never receives a saved pending credential or pairing claim', async () => {
  const f = fixture({ initialized: false }); let network = 0;
  try {
    writeFileSync(join(f.directory, 'settings.json'), JSON.stringify({ openbotOrigin: f.binding.origin,
      machineName: 'Disposable machine', selectedProjects: '', pairingCode: 'b'.repeat(32) }), { mode: 0o600 });
    await assert.rejects(recoverEnrollment(store(f.directory), f.journal, new AbortController().signal,
      (async () => { network++; throw new Error('offline'); }) as typeof fetch));
    assert.equal(network, 1);
    await store(f.directory).experimental_set({ openbotOrigin: 'https://another.example' });
    await assert.rejects(recoverEnrollment(store(f.directory), f.journal, new AbortController().signal,
      (async () => { network++; throw new Error('credential leaked'); }) as typeof fetch), /origin/);
    assert.equal(network, 1);
  } finally { f.close(); }
});
for (const status of [400, 401, 403, 404]) {
  test(`complete initial pairing ${status} clears rejected input and explicit replacement starts a fresh attempt`, async () => {
    const f = fixture({ initialized: false });
    try {
      writeFileSync(join(f.directory, 'settings.json'), JSON.stringify({ openbotOrigin: f.binding.origin,
        machineName: 'Disposable', selectedProjects: '', pairingCode: 'b'.repeat(32) }), { mode: 0o600 });
      writeFileSync(join(f.directory, 'ids.json'), JSON.stringify(f.binding));
      let rejected: Record<string, string> | undefined;
      await assert.rejects(recoverEnrollment(store(f.directory), f.journal, new AbortController().signal,
        (async (_url, init) => { if (init?.method === 'POST') { rejected = JSON.parse(init.body as string); return new Response(null, { status }); }
          return new Response(null, { status: 404 }); }) as typeof fetch), /enter a new pairing code/);
      const cleared = await store(f.directory).get(); assert.equal(cleared.pairingCode, undefined); assert.equal(cleared.enrollment, undefined);
      assert.equal(f.journal.initialized(), false);
      await store(f.directory).experimental_set({ pairingCode: 'c'.repeat(32), machineName: 'Replacement' });
      const paired = await recoverEnrollment(store(f.directory), f.journal, new AbortController().signal, exchange(f.directory));
      assert.notEqual(paired.binding.installation_id, rejected!.installation_id);
      const receipt = JSON.parse(readFileSync(join(f.directory, 'server.json'), 'utf8')); assert.equal(receipt.machine.name, 'Replacement');
      assert.equal((await store(f.directory).get()).pairingCode, undefined);
    } finally { f.close(); }
  });
}
for (const failure of ['transport', 'incomplete_rejection']) {
  test(`${failure} retains original pending secrets and ignores replacement input until outcome is resolved`, async () => {
    const f = fixture({ initialized: false });
    try {
      writeFileSync(join(f.directory, 'settings.json'), JSON.stringify({ openbotOrigin: f.binding.origin,
        machineName: 'Disposable', selectedProjects: '', pairingCode: 'd'.repeat(32) }), { mode: 0o600 });
      writeFileSync(join(f.directory, 'ids.json'), JSON.stringify(f.binding));
      await assert.rejects(recoverEnrollment(store(f.directory), f.journal, new AbortController().signal,
        (async (_url, init) => {
          if (init?.method !== 'POST') return new Response(null, { status: 404 });
          if (failure === 'transport') throw new Error('response lost');
          return new Response(new ReadableStream({ start(controller) { controller.enqueue(Buffer.from('partial')); controller.error(new Error('incomplete body')); } }), { status: 400 });
        }) as typeof fetch));
      const before = await store(f.directory).get(); const original = JSON.parse(before.enrollment!);
      assert.equal(before.pairingCode, 'd'.repeat(32)); assert.equal(original.request.code, 'd'.repeat(32));
      await store(f.directory).experimental_set({ pairingCode: 'e'.repeat(32) });
      const paired = await recoverEnrollment(store(f.directory), f.journal, new AbortController().signal, exchange(f.directory));
      assert.equal(paired.binding.installation_id, original.installation_id); assert.equal(paired.credential, original.credential);
    } finally { f.close(); }
  });
}
for (const cut of ['cleaned', 'rejected']) {
  test(`actual process interruption during definitive rejection ${cut} cannot enroll stale input as a new identity`, async () => {
    const f = fixture({ initialized: false });
    try {
      writeFileSync(join(f.directory, 'settings.json'), JSON.stringify({ openbotOrigin: f.binding.origin,
        machineName: 'Disposable', selectedProjects: '', pairingCode: 'f'.repeat(32) }), { mode: 0o600 });
      writeFileSync(join(f.directory, 'ids.json'), JSON.stringify(f.binding)); f.journal.db.close();
      const crashed = spawnSync(process.execPath, [fileURLToPath(import.meta.url)], { env: { ...process.env,
        OPENBOT_ENROLLMENT_WORKER: f.directory, OPENBOT_ENROLLMENT_CUT: cut, OPENBOT_ENROLLMENT_REJECT: '1' }, timeout: 10_000 });
      assert.equal(crashed.signal, 'SIGKILL', crashed.stderr.toString());
      const prior = await store(f.directory).get(); assert.equal(prior.pairingCode, undefined);
      const reopened = f.open(); let posts = 0;
      await assert.rejects(recoverEnrollment(store(f.directory), reopened, new AbortController().signal,
        (async (_url, init) => { if (init?.method === 'POST') { posts++; return new Response(null, { status: 400 }); } return new Response(null, { status: 404 }); }) as typeof fetch));
      assert.equal(posts, cut === 'cleaned' ? 1 : 0); assert.equal(reopened.initialized(), false);
      assert.equal((await store(f.directory).get()).enrollment, undefined);
    } finally { f.close(); }
  });
}
test('a complete rejection cannot clear a Pending record or secret input once its journal is initialized', async () => {
  const f = fixture({ initialized: false });
  try {
    writeFileSync(join(f.directory, 'settings.json'), JSON.stringify({ openbotOrigin: f.binding.origin,
      machineName: 'Disposable', selectedProjects: '', pairingCode: '1'.repeat(32) }), { mode: 0o600 });
    await assert.rejects(recoverEnrollment(store(f.directory), f.journal, new AbortController().signal,
      (async () => { throw new Error('network interrupted'); }) as typeof fetch));
    const prior = await store(f.directory).get(); const pending = JSON.parse(prior.enrollment!);
    f.journal.initialize({ ...f.binding, installation_id: pending.installation_id, journal_id: pending.journal_id });
    await assert.rejects(recoverEnrollment(store(f.directory), f.journal, new AbortController().signal,
      (async (_url, init) => new Response(null, { status: init?.method === 'POST' ? 400 : 404 })) as typeof fetch), /recovery/);
    assert.equal((await store(f.directory).get()).enrollment, prior.enrollment);
    assert.equal((await store(f.directory).get()).pairingCode, prior.pairingCode); assert(f.journal.initialized());
  } finally { f.close(); }
});
