import test from 'node:test';
import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import { join } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import type WebSocket from 'ws';
import Database from 'better-sqlite3';
import { Connector } from '../src/connector.js';
import { Dispatcher } from '../src/dispatcher.js';
import { Journal, RecoveryRequired, type MappingCursor } from '../src/journal.js';
import { FrameAssembler, LIMITS, nativeInput, observationSequence, type Identity, type Observation } from '../src/protocol.js';
import { fixture, operation, send } from './support.js';
import { event, nativeFixture } from './native-fixture.js';
import { server, waitFor, write } from './connection-support.js';

const OLD = Date.now() - 100 * 86_400_000;

test('bounded mapping traversal passes a retained oldest page and wraps after reclaiming full history', () => {
  const f = fixture();
  try {
    f.journal.db.transaction(() => {
      for (let i = 0; i < LIMITS.history; i++) f.journal.bindSession(randomUUID(), `thr_${i}`, f.selection);
      f.journal.db.prepare('UPDATE sessions SET last_used_at=?').run(OLD);
    })();
    assert(f.journal.diagnostics().newSessionsPaused);
    const retained = new Set(f.journal.mappingCandidates().map(candidate => candidate.id));
    assert.equal(retained.size, 100);
    let cursor: MappingCursor | undefined; const visited = new Set<string>(); let removed = 0;
    for (let page = 0; page < Math.ceil(LIMITS.history / 100); page++) {
      const candidates = f.journal.mappingCandidates(cursor);
      assert(candidates.length > 0 && candidates.length <= 100);
      for (const candidate of candidates) { assert(!visited.has(candidate.id)); visited.add(candidate.id); }
      cursor = candidates.at(-1)!;
      removed += f.journal.retireMappings(candidates, candidates.filter(candidate => !retained.has(candidate.id)).map(candidate => candidate.id));
    }
    assert.equal(visited.size, LIMITS.history); assert.equal(removed, LIMITS.history - retained.size);
    assert.deepEqual(f.journal.mappingCandidates(cursor), []);
    assert.deepEqual(new Set(f.journal.mappingCandidates().map(candidate => candidate.id)), retained);
    assert.equal(f.journal.diagnostics().mappings, 100); assert.equal(f.journal.diagnostics().newSessionsPaused, false);
  } finally { f.close(); }
});

for (const expected of [{ count: 4096, requests: 2, remaining: 3996 }, { count: 205, requests: 4, remaining: 100 }]) {
  test(`real WSS mapping traversal preserves the oldest page across timeout/reconnect and visits later mappings (${expected.count})`, async () => {
    const s = await server(); const f = fixture({ initialized: false });
    f.binding.origin = s.origin; f.journal.initialize(f.binding);
    f.journal.db.transaction(() => {
      for (let i = 0; i < expected.count; i++) f.journal.bindSession(randomUUID(), `thr_${i}`, f.selection);
      f.journal.db.prepare('UPDATE sessions SET last_used_at=?').run(OLD);
    })();
    const prefix = f.journal.mappingCandidates().map(candidate => candidate.id); const retained = new Set(prefix);
    const n = nativeFixture(f.journal, f.selection); const stop = new AbortController();
    const connector = new Connector(new Dispatcher(f.journal, n.sdk));
    const requests: string[][] = []; const tasks: Promise<void>[] = []; let generation = 0; let removed = 0; let failure: unknown;
    s.wss.on('connection', socket => {
      const identity = { installation_id: f.binding.installation_id, credential_generation: 1, connection_generation: ++generation };
      const assembler = new FrameAssembler();
      socket.on('message', data => {
        const message = assembler.push(data as Buffer) as Record<string, any> | undefined;
        if (message?.kind === 'hello') write(socket, { kind: 'welcome', welcome: { identity, acknowledged_sequence: 0, recovery_required: false } });
        if (message?.kind === 'ready') write(socket, { kind: 'ready', identity, ready: true });
        if (message?.kind !== 'retire_mappings') return;
        const query = message.request as { nonce: string; sessions: string[] };
        requests.push(query.sessions); assert(query.sessions.length <= 100);
        const task = (async () => {
          if (requests.length === 1) {
            assert.deepEqual(query.sessions, prefix);
            await delay(LIMITS.ioMs + 150, undefined, { signal: stop.signal });
            write(socket, { kind: 'mappings_retired', identity, nonce: query.nonce, sessions: [prefix[0]!] });
            await delay(100, undefined, { signal: stop.signal });
            assert(prefix.every(id => f.journal.session(id)), 'an expired response cannot retire its unchanged mapping');
            socket.terminate(); return;
          }
          const retired = query.sessions.filter(id => !retained.has(id)); removed += retired.length;
          write(socket, { kind: 'mappings_retired', identity, nonce: query.nonce, sessions: retired });
          await waitFor(() => f.journal.diagnostics().mappings === expected.count - removed);
          if (requests.length < expected.requests) socket.terminate();
        })();
        tasks.push(task); void task.catch(error => { if (!stop.signal.aborted) failure = error; });
      });
    });
    const running = connector.run({ state: 'paired', version: 1, binding: f.binding, credential: 'a'.repeat(64) }, stop.signal);
    try {
      await waitFor(() => { if (failure) throw failure; return requests.length === expected.requests && f.journal.diagnostics().mappings === expected.remaining; }, 15_000);
      assert(requests[1]!.every(id => !retained.has(id)), 'reconnect advances beyond a wholly retained/lost first page');
      if (expected.remaining === retained.size) assert.deepEqual(requests.at(-1), prefix, 'traversal wraps and revisits retained mappings');
      assert(prefix.every(id => f.journal.session(id))); assert.equal(n.calls.length, 0);
      assert.equal(f.journal.diagnostics().newSessionsPaused, false);
    } finally { stop.abort(); await running; await Promise.allSettled(tasks); await s.close(); f.close(); }
  });
}

function finishEvents(input: string) {
  return [event(1, 'client/turn/requested', { input: [{ type: 'text', text: input }], source: 'tell', requestId: 'request_owned' }),
    event(2, 'turn/input/accepted', { clientRequestId: 'request_owned' }, 'turn_owned'),
    event(3, 'item/completed', { item: { type: 'agentMessage', id: 'answer', text: 'preserved terminal summary' } }, 'turn_owned'),
    event(4, 'turn/completed', { status: 'completed' }, 'turn_owned')];
}

test('same-point journal restore retains mapping while SQL-only history rollback cannot reconstruct it at an equal cursor', async () => {
  const f = fixture(); const command = send(); const context = { signal: new AbortController().signal, canDispatch: () => true };
  let restored: Database.Database | undefined;
  try {
    f.journal.bindSession(command.session_id, command.thread_id, f.selection);
    f.journal.db.prepare('UPDATE sessions SET last_used_at=?').run(OLD);
    const snapshot = join(f.directory, 'coordinated-snapshot.db');
    await f.journal.db.backup(snapshot); const cursor = f.journal.acknowledged();
    assert.equal(f.journal.retireMappings(f.journal.mappingCandidates(), [command.session_id]), 1);
    assert.equal(f.journal.acknowledged(), cursor, 'mapping metadata deletion does not advance observation cursors');
    const stale = nativeFixture(f.journal, f.selection);
    await new Dispatcher(f.journal, stale.sdk).process({ operation: operation(command), mode: 'dispatch' }, context);
    assert.equal(stale.calls.length, 0); assert.equal(f.journal.session(command.session_id), undefined);
    const refused = f.journal.outbox().at(-1)!;
    assert(refused.kind === 'agent' && refused.observation.evidence.kind === 'refused'
      && refused.observation.evidence.reason === 'recovery_required');
    restored = new Database(snapshot); const journal = new Journal(restored); journal.validateBinding(f.binding);
    assert.equal(journal.acknowledged(), cursor); assert.equal(journal.session(command.session_id)!.threadId, command.thread_id);
    const native = nativeFixture(journal, f.selection); const op = operation(command);
    const dispatcher = new Dispatcher(journal, native.sdk);
    await dispatcher.process({ operation: op, mode: 'dispatch' }, context);
    await dispatcher.process({ operation: op, mode: 'reconcile' }, context);
    assert.equal(native.calls.length, 1); assert.equal(native.calls[0]!.input.threadId, command.thread_id);
  } finally { restored?.close(); f.close(); }
});

test('terminal receipts remain for90days after ACK and old compacted effects cannot enter the SDK again', async t => {
  const f = fixture(); const command = send(); f.journal.bindSession(command.session_id, command.thread_id, f.selection);
  const n = nativeFixture(f.journal, f.selection); const dispatcher = new Dispatcher(f.journal, n.sdk);
  const op = operation(command); const context = { signal: new AbortController().signal, canDispatch: () => true };
  try {
    await dispatcher.process({ operation: op, mode: 'dispatch' }, context);
    n.state.events = finishEvents(nativeInput(op)); await dispatcher.tick(context.signal);
    assert.equal(f.journal.get(op.id)!.stage, 'terminal'); assert.equal(n.calls.length, 1);
    const terminal = f.journal.outbox();
    f.journal.db.prepare('UPDATE commands SET terminal_at=? WHERE id=?').run(OLD, op.id);
    assert.equal(f.journal.compactAcknowledged(), 0); assert.deepEqual(f.journal.outbox(), terminal);
    f.journal.acknowledge(f.journal.sequence());
    assert.equal(f.journal.compactAcknowledged(), 0, 'a delayed ACK begins the remaining reconciliation window');
    const future = Date.now() + 91 * 86_400_000;
    t.mock.method(Date, 'now', () => future);
    assert.equal(f.journal.compactAcknowledged(), 1); assert.equal(f.journal.get(op.id), undefined);
    assert(f.journal.session(command.session_id), 'local age alone never retires a server-visible mapping');
    await dispatcher.process({ operation: op, mode: 'dispatch' }, context);
    assert.equal(n.calls.length, 1, 'the original immutable elapsed deadline refuses a fresh SDK claim');
    assert.equal(f.journal.get(op.id)!.stage, 'terminal');
  } finally { f.close(); }
});

test('uncertainty and pending local references survive compaction and only unchanged confirmed mappings retire', () => {
  const f = fixture();
  try {
    const command = send(); const unused = randomUUID();
    f.journal.bindSession(command.session_id, command.thread_id, f.selection);
    f.journal.bindSession(unused, 'thr_unused', f.selection);
    f.journal.db.prepare('UPDATE sessions SET last_used_at=?').run(OLD);
    const record = f.journal.admit({ operation: operation(command), mode: 'dispatch' }).record;
    f.journal.beginIntent(record); record.stage = 'uncertain';
    f.journal.evidence(record, { kind: 'uncertain' }, 'critical');
    f.journal.db.prepare('UPDATE sessions SET last_used_at=?').run(OLD);
    f.journal.acknowledge(f.journal.sequence());
    assert.equal(f.journal.compactAcknowledged(), 0); assert.equal(f.open().get(record.operation.id)!.stage, 'uncertain');
    const candidates = f.journal.mappingCandidates(); assert.deepEqual(candidates.map(value => value.id), [unused]);
    assert.throws(() => f.journal.retireMappings(candidates, [command.session_id]), RecoveryRequired);
    assert(f.journal.session(command.session_id));
    f.journal.bindSession(unused, 'thr_unused', f.selection);
    assert.equal(f.journal.retireMappings(candidates, [unused]), 0, 'new use invalidates a captured candidate');
    f.journal.db.prepare('UPDATE sessions SET last_used_at=? WHERE id=?').run(OLD, unused);
    assert.equal(f.journal.retireMappings(f.journal.mappingCandidates(), [unused]), 1);
    assert.equal(f.open().session(unused), undefined); assert(f.journal.session(command.session_id));
  } finally { f.close(); }
});

test('64MiB outbox pressure coalesces progress and preserves its reserved unacknowledged terminal summary', () => {
  const f = fixture();
  try {
    const command = send(); f.journal.bindSession(command.session_id, command.thread_id, f.selection);
    const op = operation(command); const record = f.journal.admit({ operation: op, mode: 'dispatch' }).record;
    f.journal.beginIntent(record);
    for (let i = 1; i < 21; i++) f.journal.admit({ operation: operation(), mode: 'dispatch' });
    const pending = f.journal.outbox();
    assert.equal(f.journal.diagnostics().commands, 21, '64MiB with3MiB terminal reservations admits21 records');
    assert(f.journal.diagnostics().newCommandsPaused, 'the fixed bound must keep its early admission signal');
    const output = '€'.repeat(87381); record.items = [{ id: 'answer', text: output }]; record.nativeState = 'running';
    let before = f.journal.sequence(); let coalesced = false;
    for (let i = 0; i < 8; i++) {
      record.cursor++;
      f.journal.evidence(record, { kind: 'turn', thread_id: command.thread_id, turn_id: 'turn_owned', state: 'running', output, truncated: false, usage: null }, 'progress');
      if (f.journal.sequence() === before) { coalesced = true; break; }
      before = f.journal.sequence();
    }
    assert(coalesced); assert.equal(f.journal.get(op.id)!.items[0]!.text, output);
    record.stage = 'terminal'; record.nativeState = 'completed';
    f.journal.evidence(record, { kind: 'turn', thread_id: command.thread_id, turn_id: 'turn_owned', state: 'completed', output, truncated: false, usage: null }, 'terminal');
    assert.equal(f.journal.sequence(), before + 1);
    const terminal = f.journal.get(op.id)!.lastObservation!;
    assert.equal(terminal.kind, 'agent');
    assert.deepEqual(f.journal.outbox(0).slice(0, pending.length), pending);
    const budget = f.journal.diagnostics(); assert(budget.outboxBytes + budget.reservedBytes <= LIMITS.outbox);
    f.journal.db.prepare('UPDATE commands SET terminal_at=? WHERE id=?').run(OLD, op.id);
    assert.equal(f.journal.compactAcknowledged(), 0);
    assert.deepEqual(f.open().get(op.id)!.lastObservation, terminal);
    assert.equal(f.journal.get(op.id)!.items[0]!.text, output);
  } finally { f.close(); }
});

test('real WSS retirement requires current nonce and unchanged binding after server confirmation', async () => {
  const s = await server(); const f = fixture({ initialized: false }); f.binding.origin = s.origin; f.journal.initialize(f.binding);
  const keep = randomUUID(); const retire = randomUUID();
  f.journal.bindSession(keep, 'thr_keep', f.selection); f.journal.bindSession(retire, 'thr_retire', f.selection);
  f.journal.db.prepare('UPDATE sessions SET last_used_at=?').run(OLD);
  const n = nativeFixture(f.journal, f.selection); const connector = new Connector(new Dispatcher(f.journal, n.sdk));
  const identity: Identity = { installation_id: f.binding.installation_id, credential_generation: 1, connection_generation: 1 };
  let peer: WebSocket | undefined; let query: { nonce: string; sessions: string[] } | undefined; let ready = false;
  s.wss.on('connection', socket => {
    peer = socket; const assembler = new FrameAssembler();
    socket.on('message', data => {
      const message = assembler.push(data as Buffer) as Record<string, any> | undefined;
      if (message?.kind === 'hello') write(socket, { kind: 'welcome', welcome: { identity, acknowledged_sequence: 0, recovery_required: false } });
      if (message?.kind === 'ready' && !ready) { ready = true; write(socket, { kind: 'ready', identity, ready: true }); }
      if (message?.kind === 'retire_mappings') {
        query = message.request; assert.deepEqual(message.identity, identity);
        write(socket, { kind: 'mappings_retired', identity, nonce: randomUUID(), sessions: [keep, retire] });
        write(socket, { kind: 'ready', identity, ready: false });
      }
    });
  });
  const stop = new AbortController();
  const running = connector.run({ state: 'paired', version: 1, binding: f.binding, credential: 'a'.repeat(64) }, stop.signal);
  try {
    await waitFor(() => !!query && connector.status === 'recovering');
    assert(f.journal.session(keep)); assert(f.journal.session(retire));
    assert.deepEqual(new Set(query!.sessions), new Set([keep, retire]));
    f.journal.bindSession(keep, 'thr_keep', f.selection);
    write(peer!, { kind: 'mappings_retired', identity, nonce: query!.nonce, sessions: [keep, retire] });
    write(peer!, { kind: 'ready', identity, ready: true });
    await waitFor(() => connector.status === 'connected');
    assert(f.journal.session(keep)); assert.equal(f.journal.session(retire), undefined); assert.equal(n.calls.length, 0);
  } finally { stop.abort(); await running; await s.close(); f.close(); }
});

test('real WSS mapping retirement rejects a stale connection before deleting local ownership', async () => {
  const s = await server(); const f = fixture({ initialized: false }); f.binding.origin = s.origin; f.journal.initialize(f.binding);
  const id = randomUUID(); f.journal.bindSession(id, 'thr_keep', f.selection); f.journal.db.prepare('UPDATE sessions SET last_used_at=?').run(OLD);
  const n = nativeFixture(f.journal, f.selection); const stop = new AbortController(); let closed = false;
  s.wss.on('connection', socket => {
    const identity = { installation_id: f.binding.installation_id, credential_generation: 1, connection_generation: 1 };
    const assembler = new FrameAssembler(); socket.on('close', () => { closed = true; });
    socket.on('message', data => {
      const message = assembler.push(data as Buffer) as Record<string, any> | undefined;
      if (message?.kind === 'hello') write(socket, { kind: 'welcome', welcome: { identity, acknowledged_sequence: 0, recovery_required: false } });
      if (message?.kind === 'ready') write(socket, { kind: 'ready', identity, ready: true });
      if (message?.kind === 'retire_mappings') write(socket, { kind: 'mappings_retired', identity: { ...identity, connection_generation: 99 }, nonce: message.request.nonce, sessions: [id] });
    });
  });
  const running = new Connector(new Dispatcher(f.journal, n.sdk)).run({ state: 'paired', version: 1, binding: f.binding, credential: 'a'.repeat(64) }, stop.signal);
  try { await waitFor(() => closed); assert(f.journal.session(id)); assert.equal(n.calls.length, 0); }
  finally { stop.abort(); await running; await s.close(); f.close(); }
});

test('full mapping history pauses new creates while existing native work completes and ACKs drain', async () => {
  const s = await server(); const f = fixture({ initialized: false }); f.binding.origin = s.origin; f.journal.initialize(f.binding);
  const command = send(); f.journal.db.transaction(() => {
    f.journal.bindSession(command.session_id, command.thread_id, f.selection);
    for (let i = 1; i < 4096; i++) f.journal.bindSession(randomUUID(), `thr_old_${i}`, f.selection);
  })();
  const n = nativeFixture(f.journal, f.selection); const connector = new Connector(new Dispatcher(f.journal, n.sdk));
  const blocked = operation({ kind: 'create', session_id: randomUUID(), selection: f.selection, prompt: 'never dispatch this new session' });
  const admitted = operation(command); let delivered = false; let terminal = false;
  s.wss.on('connection', socket => {
    const identity = { installation_id: f.binding.installation_id, credential_generation: 1, connection_generation: 1 };
    const assembler = new FrameAssembler();
    socket.on('message', data => {
      const message = assembler.push(data as Buffer) as Record<string, any> | undefined;
      if (message?.kind === 'hello') write(socket, { kind: 'welcome', welcome: { identity, acknowledged_sequence: f.journal.acknowledged(), recovery_required: false } });
      if (message?.kind === 'ready') {
        write(socket, { kind: 'ready', identity, ready: true });
        if (!delivered) { delivered = true; for (const op of [blocked, admitted]) write(socket, { kind: 'command', identity, delivery: { operation: op, mode: 'dispatch' } }); }
      }
      if (message?.kind === 'observation') {
        const observation = message.observation as Observation;
        if (observation.kind === 'agent' && observation.observation.evidence.kind === 'submission') n.state.events = finishEvents(nativeInput(admitted));
        if (observation.kind === 'agent' && observation.observation.evidence.kind === 'turn' && observation.observation.evidence.state === 'completed') terminal = true;
        write(socket, { kind: 'acknowledged', identity, through: observationSequence(observation) });
      }
    });
  });
  const stop = new AbortController(); const running = connector.run({ state: 'paired', version: 1, binding: f.binding, credential: 'a'.repeat(64) }, stop.signal);
  try {
    await waitFor(() => terminal && f.journal.outbox().length === 0);
    assert.deepEqual(n.calls.map(call => call.kind), ['send']); assert.equal(f.journal.get(blocked.id), undefined);
    assert.equal(f.journal.get(admitted.id)!.nativeState, 'completed'); assert(f.journal.diagnostics().newSessionsPaused);
    assert.equal(connector.status, 'connected'); assert.equal(f.journal.capacity().effects, 0);
  } finally { stop.abort(); await running; await s.close(); f.close(); }
});
