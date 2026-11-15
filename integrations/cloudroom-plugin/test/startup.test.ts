import test from 'node:test';
import assert from 'node:assert/strict';
import type { BbPluginApi } from '@get-bb/plugin-sdk';
import openbot from '../src/server.js';
import { Connector } from '../src/connector.js';
import { fixture, operation, send } from './support.js';
import { nativeFixture } from './native-fixture.js';

for (const stage of ['paired', 'enrolling']) for (const setting of ['selectedProjects', 'openbotOrigin', 'enrollment']) {
  test(`startup ${stage} ${setting} change prevents connector and native admission`, async () => {
    const f = fixture({ initialized: stage === 'paired' });
    const command = send(); if (stage === 'paired') f.journal.bindSession({sessionId:command.session_id,threadId:command.thread_id,selection:f.selection,harness:{kind:'unconfigured'},operationId:'00000000-0000-4000-8000-000000000001',hostId:null});
    const n = nativeFixture(f.journal, f.selection); const op = operation(command);
    let values: Record<string, string> = { openbotOrigin: f.binding.origin, machineName: 'Disposable', selectedProjects: f.selection.project_id,
      ...(stage === 'paired' ? { enrollment: JSON.stringify({ state: 'paired', version: 1, binding: f.binding, credential: 'a'.repeat(64) }) }
        : { pairingCode: 'b'.repeat(32) }) };
    let changed: (next: any, prev: any) => void = () => {}; let start!: (signal: AbortSignal) => Promise<void>; let reads = 0;
    let pause!: () => void; const paused = new Promise<void>(resolve => { pause = resolve; });
    let resume!: () => void; const resumed = new Promise<void>(resolve => { resume = resolve; });
    const bb = { ...n.bb, storage: { database: () => f.journal.db, migrate: () => {} },
      settings: { define: () => ({ get: async () => {
        if (++reads === 2 && stage === 'paired') { pause(); await resumed; }
        return { ...values };
      }, experimental_set: async (patch: Record<string, string | null>) => {
        const old = values; values = { ...values };
        for (const [key, value] of Object.entries(patch)) { if (value === null) delete values[key]; else values[key] = value; }
        changed(values, old); return { ...values };
      }, onChange: (listener: typeof changed) => { changed = listener; } }) },
      background: { service: (_name: string, service: { start: typeof start }) => { start = service.start; } }, cli: { register: () => {} },
    } as unknown as BbPluginApi;
    const originalRun = Connector.prototype.run; let entered = false;
    Connector.prototype.run = async function (_paired, signal) {
      entered = true; await this.dispatcher.process({ operation: op, mode: 'dispatch' }, { signal, canDispatch: () => true });
    };
    const originalFetch = globalThis.fetch;
    if (stage === 'enrolling') globalThis.fetch = (async (_url, init) => {
      if (init?.method !== 'POST') return new Response(null, { status: 404 });
      pause(); await resumed;
      const pending = JSON.parse(values.enrollment ?? '{}');
      return Response.json({ company_id: f.binding.company_id, company_name: 'Disposable', machine: { id: f.binding.machine_id,
        company_id: f.binding.company_id, installation_id: pending.installation_id ?? f.binding.installation_id, name: 'Disposable',
        state: 'enabled', credential_generation: 1, last_seen_at: null, capabilities_sequence: null } });
    }) as typeof fetch;
    try {
      await openbot(bb); const running = start(new AbortController().signal); const rejection = assert.rejects(running);
      await paused; const old = values; values = { ...values, [setting]: setting === 'selectedProjects' ? ''
        : setting === 'openbotOrigin' ? 'https://changed.example' : '{}' };
      changed(values, old); resume(); await rejection;
      assert.equal(entered, false); assert.equal(n.calls.length, 0);
      if (stage === 'paired') assert.equal(f.journal.sequence(), 0); else assert.equal(f.journal.initialized(), false);
    } finally { resume(); Connector.prototype.run = originalRun; globalThis.fetch = originalFetch; f.close(); }
  });
}
test('legitimate internal Pending to Paired secret writes do not cancel connector startup', async () => {
  const f = fixture({ initialized: false }); const n = nativeFixture(f.journal, f.selection);
  let values: Record<string, string> = { openbotOrigin: f.binding.origin, machineName: 'Disposable', selectedProjects: f.selection.project_id, pairingCode: 'c'.repeat(32) };
  let changed: (next: any, prev: any) => void = () => {}; let start!: (signal: AbortSignal) => Promise<void>;
  const bb = { ...n.bb, storage: { database: () => f.journal.db, migrate: () => {} }, settings: { define: () => ({
    get: async () => ({ ...values }), experimental_set: async (patch: Record<string, string | null>) => {
      const old = values; values = { ...values };
      for (const [key, value] of Object.entries(patch)) { if (value === null) delete values[key]; else values[key] = value; }
      changed(values, old); return { ...values };
    }, onChange: (listener: typeof changed) => { changed = listener; } }) },
    background: { service: (_name: string, service: { start: typeof start }) => { start = service.start; } }, cli: { register: () => {} },
  } as unknown as BbPluginApi;
  const originalFetch = globalThis.fetch; const originalRun = Connector.prototype.run; let entered = false;
  globalThis.fetch = (async (_url, init) => {
    if (init?.method !== 'POST') return new Response(null, { status: 404 });
    const pending = JSON.parse(values.enrollment!);
    return Response.json({ company_id: f.binding.company_id, company_name: 'Disposable', machine: { id: f.binding.machine_id,
      company_id: f.binding.company_id, installation_id: pending.installation_id, name: 'Disposable', state: 'enabled',
      credential_generation: 1, last_seen_at: null, capabilities_sequence: null } });
  }) as typeof fetch;
  Connector.prototype.run = async function (_paired, signal) { signal.throwIfAborted(); entered = true; };
  try { await openbot(bb); await start(new AbortController().signal); assert(entered); assert(f.journal.initialized()); assert.equal(values.pairingCode, undefined); }
  finally { globalThis.fetch = originalFetch; Connector.prototype.run = originalRun; f.close(); }
});
