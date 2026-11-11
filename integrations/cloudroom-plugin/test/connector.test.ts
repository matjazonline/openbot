import test from 'node:test';
import assert from 'node:assert/strict';
import { setTimeout as delay } from 'node:timers/promises';
import { Connector } from '../src/connector.js';
import { Dispatcher } from '../src/dispatcher.js';
import { encodeFrames, FrameAssembler, observationSequence, nativeInput, type Identity, type Observation } from '../src/protocol.js';
import { fixture, operation, send } from './support.js';
import { event, nativeFixture } from './native-fixture.js';
import type { BbPluginApi } from '@get-bb/plugin-sdk';
import openbot from '../src/server.js';

import { waitFor, server, write } from './connection-support.js';

test('real trusted WSS reconnect rewraps immutable outbox and native tracking continues offline without duplicate SDK calls', async () => {
  const s = await server(); const f = fixture({ initialized: false }); f.binding.origin = s.origin; f.journal.initialize(f.binding);
  const command = send(); f.journal.bindSession(command.session_id, command.thread_id, f.selection);
  const n = nativeFixture(f.journal, f.selection); const connector = new Connector(new Dispatcher(f.journal, n.sdk));
  const stop = new AbortController(); const op = operation(command);
  let generation = 0; let delivered = false; let cut = false; let terminal = false;
  const seen = new Map<number, Observation>(); const identities: number[] = []; const failures: unknown[] = [];
  s.wss.on('connection', (socket, request) => {
    try {
      assert.equal(request.headers.authorization, `Bearer ${'a'.repeat(64)}`);
      assert.equal(request.headers['x-openbot-installation-id'], f.binding.installation_id);
      assert.equal(request.headers['x-openbot-protocol-version'], '2'); assert.equal(request.url, '/v1/local-machines/connect');
    } catch (error) { failures.push(error); }
    const identity: Identity = { installation_id: f.binding.installation_id, credential_generation: 1, connection_generation: ++generation };
    const assembler = new FrameAssembler(); let through = 0;
    socket.on('message', data => {
      try {
        const message = assembler.push(data as Buffer) as Record<string, any> | undefined;
        if (!message) return;
        if (message.kind === 'hello') {
          assert.equal(message.hello.version, 2);
          assert.equal(message.hello.journal_id, f.binding.journal_id);
          // Replay the last unacknowledged observation after the deliberate cut.
          through = f.journal.acknowledged();
          write(socket, { kind: 'welcome', welcome: { identity, acknowledged_sequence: through, recovery_required: false } });
        }
        if (message.kind === 'ready') {
          write(socket, { kind: 'ready', identity, ready: true });
          if (!delivered) { delivered = true; write(socket, { kind: 'command', identity, delivery: { operation: op, mode: 'dispatch' } }); }
        }
        if (message.kind === 'observation') {
          assert.deepEqual(message.identity, identity); identities.push(identity.connection_generation);
          const observation = message.observation as Observation; const sequence = observationSequence(observation);
          if (seen.has(sequence)) assert.deepEqual(observation, seen.get(sequence)); else seen.set(sequence, observation);
          if (!cut && observation.kind === 'agent' && observation.observation.evidence.kind === 'submission') {
            cut = true; socket.terminate();
            n.state.events = [event(1, 'client/turn/requested', { input: [{ type: 'text', text: nativeInput(op) }], source: 'tell', requestId: 'request_owned' }),
              event(2, 'turn/input/accepted', { clientRequestId: 'request_owned' }, 'turn_owned'),
              event(3, 'item/completed', { item: { type: 'agentMessage', id: 'answer', text: 'only the owned turn' } }, 'turn_owned'),
              event(4, 'turn/completed', { status: 'completed' }, 'turn_owned')];
            return;
          }
          through = Math.max(through, sequence); write(socket, { kind: 'acknowledged', identity, through });
          if (observation.kind === 'agent' && observation.observation.evidence.kind === 'turn'
              && observation.observation.evidence.state === 'completed') terminal = true;
        }
      } catch (error) { failures.push(error); socket.terminate(); }
    });
  });
  const running = connector.run({ state: 'paired', version: 1, binding: f.binding, credential: 'a'.repeat(64) }, stop.signal);
  try {
    await waitFor(() => terminal && f.journal.outbox().length === 0);
    assert.equal(failures.length, 0, String(failures[0])); assert(generation >= 2); assert(cut);
    assert(identities.includes(1) && identities.includes(2)); assert.equal(n.calls.length, 1);
    assert.equal(f.journal.get(op.id)!.nativeState, 'completed');
    assert.equal(f.journal.capacity().effects, 0);
  } finally { stop.abort(); await running; await s.close(); f.close(); }
});
test('authorization rejection stops reconnect attempts and never enters the SDK', async () => {
  const s = await server(); const f = fixture({ initialized: false }); f.binding.origin = s.origin; f.journal.initialize(f.binding);
  let attempts = 0;
  // Replace upgrade handling with a real HTTP authorization rejection.
  s.https.removeAllListeners('upgrade');
  s.https.on('upgrade', (_request, socket) => { attempts++; socket.end('HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n'); });
  const n = nativeFixture(f.journal, f.selection); const connector = new Connector(new Dispatcher(f.journal, n.sdk));
  try {
    await assert.rejects(connector.run({ state: 'paired', version: 1, binding: f.binding, credential: 'a'.repeat(64) }, new AbortController().signal), /authorization/);
    assert.equal(attempts, 1); assert.equal(connector.status, 'authorization_required'); assert.equal(n.calls.length, 0);
  } finally { await s.close(); f.close(); }
});
test('shutdown during a stalled TLS handshake joins network and dispatcher within the host drain', async () => {
  const s = await server(); const f = fixture({ initialized: false }); f.binding.origin = s.origin; f.journal.initialize(f.binding);
  s.https.removeAllListeners('upgrade'); const held = new Set<import('node:stream').Duplex>();
  s.https.on('upgrade', (_request, socket) => { held.add(socket); socket.on('close', () => held.delete(socket)); });
  const n = nativeFixture(f.journal, f.selection); const stop = new AbortController();
  const running = new Connector(new Dispatcher(f.journal, n.sdk)).run({ state: 'paired', version: 1, binding: f.binding, credential: 'a'.repeat(64) }, stop.signal);
  try {
    await waitFor(() => held.size > 0); const began = Date.now(); stop.abort(); await running;
    assert(Date.now() - began < 1_000); assert.equal(n.calls.length, 0);
  } finally { stop.abort(); await running; for (const socket of held) socket.destroy(); await s.close(); f.close(); }
});
for (const invalid of ['stale_identity', 'changed_digest', 'binary', 'unknown_field']) {
  test(`real WSS rejects ${invalid} before journal receipt or SDK entry`, async () => {
    const s = await server(); const f = fixture({ initialized: false }); f.binding.origin = s.origin; f.journal.initialize(f.binding);
    let closed = false;
    s.wss.on('connection', socket => {
      const identity = { installation_id: f.binding.installation_id, credential_generation: 1, connection_generation: 1 };
      const assembler = new FrameAssembler();
      socket.on('close', () => { closed = true; });
      socket.on('message', data => {
        const message = assembler.push(data as Buffer) as { kind: string } | undefined;
        if (message?.kind !== 'hello') return;
        write(socket, { kind: 'welcome', welcome: { identity, acknowledged_sequence: 0, recovery_required: false } });
        const op = operation(send()); if (invalid === 'changed_digest') op.payload_digest = '0'.repeat(64);
        const command = { kind: 'command', identity: invalid === 'stale_identity' ? { ...identity, connection_generation: 99 } : identity,
          delivery: { operation: op, mode: 'dispatch' }, ...(invalid === 'unknown_field' ? { injected: true } : {}) };
        if (invalid === 'binary') socket.send(Buffer.from(encodeFrames(command)[0]!)); else write(socket, command);
      });
    });
    const n = nativeFixture(f.journal, f.selection); const stop = new AbortController();
    const running = new Connector(new Dispatcher(f.journal, n.sdk)).run({ state: 'paired', version: 1, binding: f.binding, credential: 'a'.repeat(64) }, stop.signal);
    try { await waitFor(() => closed); assert.equal(f.journal.sequence(), 0); assert.equal(n.calls.length, 0); }
    finally { stop.abort(); await running; await s.close(); f.close(); }
  });
}
test('public plugin service reload joins SDK work before storage closes and settings changes stop admission', async () => {
  const s = await server(); const f = fixture({ initialized: false }); f.binding.origin = s.origin; f.journal.initialize(f.binding);
  const command = send(); f.journal.bindSession(command.session_id, command.thread_id, f.selection);
  const n = nativeFixture(f.journal, f.selection); n.state.held = true; const op = operation(command);
  let changed: (next: any, prev: any) => void = () => {};
  let start!: (signal: AbortSignal) => Promise<void>; let runCli!: (argv: string[], context: {}) => Promise<any>;
  const values = { openbotOrigin: s.origin, selectedProjects: f.selection.project_id,
    enrollment: JSON.stringify({ state: 'paired', version: 1, binding: f.binding, credential: 'a'.repeat(64) }) };
  const bb = { ...n.bb, storage: { database: () => f.journal.db, migrate: () => {} },
    settings: { define: () => ({ get: async () => values, experimental_set: async () => values,
      onChange: (listener: typeof changed) => { changed = listener; } }) },
    background: { service: (_name: string, service: { start: typeof start }) => { start = service.start; } },
    cli: { register: (registration: { run: typeof runCli }) => { runCli = registration.run; } },
  } as unknown as BbPluginApi;
  s.wss.on('connection', socket => {
    const assembler = new FrameAssembler(); const identity = { installation_id: f.binding.installation_id, credential_generation: 1, connection_generation: 1 };
    let delivered = false;
    socket.on('message', data => {
      const message = assembler.push(data as Buffer) as Record<string, any> | undefined;
      if (message?.kind === 'hello') write(socket, { kind: 'welcome', welcome: { identity, acknowledged_sequence: 0, recovery_required: false } });
      if (message?.kind === 'ready') {
        write(socket, { kind: 'ready', identity, ready: true });
        if (!delivered) { delivered = true; write(socket, { kind: 'command', identity, delivery: { operation: op, mode: 'dispatch' } }); }
      }
      if (message?.kind === 'observation') write(socket, { kind: 'acknowledged', identity, through: observationSequence(message.observation) });
    });
  });
  const hostStop = new AbortController();
  await openbot(bb); const running = start(hostStop.signal);
  // Install a rejection handler before the deliberate configuration cancellation.
  const ended = running.then(() => undefined, error => error as Error);
  try {
    await waitFor(() => n.state.entered === 1);
    const before = JSON.parse((await runCli(['status'], {})).stdout);
    assert.equal(before.connection, 'connected'); assert(!JSON.stringify(before).includes('a'.repeat(64)));
    // A second instance cannot open a socket or enter SDK while this service owns the profile.
    await assert.rejects(start(new AbortController().signal), /another connector/);
    const began = Date.now(); changed({ ...values, selectedProjects: '' }, values);
    const error = await ended; assert(error instanceof Error && error.name === 'NeedsConfigurationError');
    assert(Date.now() - began < 1_000); assert(f.journal.db.open);
    assert.equal(f.journal.get(op.id)!.stage, 'uncertain'); assert.equal(n.calls.length, 1);
    const sequence = f.journal.sequence(); f.journal.db.close(); await delay(50);
    assert.equal(f.open().sequence(), sequence, 'no detached task writes after storage disposal');
  } finally { hostStop.abort(); await ended; await s.close(); f.close(); }
});
