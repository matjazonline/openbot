import test from 'node:test';
import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import { Dispatcher } from '../src/dispatcher.js';
import { CloudroomSdk } from '../src/sdk.js';
import { LIMITS, nativeInput } from '../src/protocol.js';
import { applyNativeEvents } from '../src/turns.js';
import { fixture, operation, send } from './support.js';
import { event, nativeFixture } from './native-fixture.js';
const context = { signal: new AbortController().signal, canDispatch: () => true };
test('lost read-only deliveries reconcile without poisoning native admission', async () => {
  const f = fixture(); const command = send();
  f.journal.bindSession({sessionId:command.session_id,threadId:command.thread_id,selection:f.selection,harness:{kind:'unconfigured'},operationId:'00000000-0000-4000-8000-000000000001',hostId:null});
  const n = nativeFixture(f.journal, f.selection); const d = new Dispatcher(f.journal, n.sdk);
  try {
    for (const op of [operation(), operation({ kind: 'inspect', session_id: command.session_id, thread_id: command.thread_id })]) {
      await d.process({ operation: op, mode: 'reconcile' }, { ...context, canDispatch: () => false });
      assert.equal(f.journal.get(op.id)!.stage, 'terminal');
      assert.equal(f.journal.recoveryRequired(), false);
    }
    const effect = operation(command);
    await d.process({ operation: effect, mode: 'dispatch' }, context);
    assert.equal(n.calls.length, 1); assert.equal(n.calls[0]!.kind, 'send');
    assert.equal(f.journal.get(effect.id)!.sdkCalls, 1);
    const missing = operation({ kind: 'stop_thread', session_id: command.session_id, thread_id: command.thread_id });
    await d.process({ operation: missing, mode: 'reconcile' }, context);
    assert.equal(n.calls.length, 1); assert.equal(f.journal.recoveryRequired(), true);
  } finally { f.close(); }
});
function completed(input: string, source: 'spawn' | 'tell' = 'tell') {
  return [event(1, 'client/turn/requested', { input: [{ type: 'text', text: input }], source, requestId: 'request_owned' }),
    event(2, 'turn/input/accepted', { clientRequestId: 'request_owned' }, 'turn_owned'),
    event(3, 'item/completed', { item: { type: 'agentMessage', id: 'answer_owned', text: 'bounded local answer' } }, 'turn_owned'),
    event(4, 'turn/completed', { status: 'completed' }, 'turn_owned')];
}

test('native startup error before turn acceptance stays uncertain across restart without resubmission', async () => {
  const f = fixture(); const n = nativeFixture(f.journal, f.selection);
  const op = operation({ kind: 'create', harness: {kind:'unconfigured'}, session_id: randomUUID(), selection: f.selection, prompt: 'authorized work' });
  const d = new Dispatcher(f.journal, n.sdk);
  try {
    await d.process({ operation: op, mode: 'dispatch' }, context);
    n.state.events = [event(1, 'client/turn/requested', { input: [{ type: 'text', text: nativeInput(op) }], source: 'spawn', requestId: 'request_owned' })];
    await d.tick(context.signal);
    assert.equal(f.journal.get(op.id)!.stage, 'observing', 'a pending acceptance is not a startup failure');
    n.state.thread.status = 'error';
    await d.tick(context.signal);
    assert.equal(f.journal.get(op.id)!.stage, 'uncertain');
    assert.equal(f.journal.get(op.id)!.turnId, null);
    const observation = f.journal.get(op.id)!.lastObservation!;
    assert(observation.kind === 'agent');
    assert.equal(observation.observation.evidence.kind, 'uncertain');
    f.journal.db.close(); const reopened = f.open();
    const recovered = new Dispatcher(reopened, new CloudroomSdk(n.bb, reopened, () => [f.selection.project_id]));
    await recovered.process({ operation: op, mode: 'reconcile' }, context);
    assert.equal(reopened.get(op.id)!.stage, 'uncertain');
    assert.equal(n.calls.length, 1);
    n.state.events = completed(nativeInput(op), 'spawn');
    await recovered.tick(context.signal);
    assert.equal(reopened.get(op.id)!.stage, 'terminal', 'positive matching turn evidence can still reconcile late');
    assert.equal(reopened.get(op.id)!.nativeState, 'completed');
    assert.equal(n.calls.length, 1);
  } finally { f.close(); }
});

test('create freezes explicit local checkout/provider/model and duplicate concurrent delivery submits once', async () => {
  const f = fixture(); const n = nativeFixture(f.journal, f.selection); const d = new Dispatcher(f.journal, n.sdk);
  try {
    const op = operation({ kind: 'create', harness: {kind:'unconfigured'}, session_id: randomUUID(), selection: f.selection, prompt: 'authorized work' });
    await Promise.all([d.process({ operation: op, mode: 'dispatch' }, context), d.process({ operation: op, mode: 'dispatch' }, context)]);
    assert.equal(n.calls.length, 1); assert.equal(n.calls[0]!.kind, 'spawn');
    const args = n.calls[0]!.input;
    assert.deepEqual(args.environment, { type: 'provider', environmentProviderId: 'project-checkout', machine: { type: 'existing', hostId: 'host_primary' }, inputs: {} });
    assert.equal(args.providerId, 'codex'); assert.equal(args.model, f.selection.model);
    assert.equal(args.prompt, nativeInput(op)); assert.equal(args.executionTarget, 'local');
    assert.equal(f.journal.session(op.command.kind === 'create' ? op.command.session_id : '')!.threadId, 'thr_owned');
    n.state.events = completed(nativeInput(op), 'spawn'); await d.tick(context.signal);
    assert.equal(f.journal.get(op.id)!.stage, 'terminal'); assert.equal(f.journal.get(op.id)!.nativeState, 'completed');
    assert.equal(f.journal.get(op.id)!.usage, null); assert.equal(n.calls.length, 1);
  } finally { f.close(); }
});
test('lost send response reopens intent and recovers exact turn without sending again', async () => {
  const f = fixture(); const command = send(); f.journal.bindSession({sessionId:command.session_id,threadId:command.thread_id,selection:f.selection,harness:{kind:'unconfigured'},operationId:'00000000-0000-4000-8000-000000000001',hostId:null});
  const n = nativeFixture(f.journal, f.selection); const op = operation(command); n.state.lostResponse = true;
  try {
    await new Dispatcher(f.journal, n.sdk).process({ operation: op, mode: 'dispatch' }, context);
    assert.equal(f.journal.get(op.id)!.stage, 'uncertain'); assert.equal(n.calls.length, 1);
    assert.equal(n.calls[0]!.input.mode, 'queue-if-active');
    f.journal.db.close(); const reopened = f.open();
    n.state.events = completed(nativeInput(op));
    const d = new Dispatcher(reopened, new CloudroomSdk(n.bb, reopened, () => [f.selection.project_id]));
    await d.process({ operation: op, mode: 'reconcile' }, context);
    assert.equal(reopened.get(op.id)!.stage, 'terminal'); assert.equal(n.calls.length, 1);
    const last = reopened.get(op.id)!.lastObservation!;
    assert(last.kind === 'agent' && last.observation.evidence.kind === 'turn');
    assert.equal(last.observation.evidence.output, 'bounded local answer');
  } finally { f.close(); }
});
test('spawn gap recovers positive plugin metadata, while absent threads never recreate', async () => {
  const f = fixture(); const n = nativeFixture(f.journal, f.selection); n.state.lostResponse = true;
  const op = operation({ kind: 'create', harness: {kind:'unconfigured'}, session_id: randomUUID(), selection: f.selection, prompt: 'recover only' });
  const d = new Dispatcher(f.journal, n.sdk);
  try {
    await d.process({ operation: op, mode: 'dispatch' }, context); assert.equal(f.journal.get(op.id)!.threadId, null);
    n.state.missing = true; await d.tick(context.signal);
    assert.equal(n.calls.length, 1); assert.equal(f.journal.get(op.id)!.stage, 'uncertain');
    n.state.missing = false; n.state.events = completed(nativeInput(op), 'spawn'); await d.tick(context.signal);
    assert.equal(f.journal.get(op.id)!.threadId, 'thr_owned'); assert.equal(f.journal.get(op.id)!.stage, 'terminal'); assert.equal(n.calls.length, 1);
  } finally { f.close(); }
});
test('human turn, idle thread and unrelated permission prompts cannot complete or take ownership', async () => {
  const f = fixture(); const command = send(); f.journal.bindSession({sessionId:command.session_id,threadId:command.thread_id,selection:f.selection,harness:{kind:'unconfigured'},operationId:'00000000-0000-4000-8000-000000000001',hostId:null});
  const n = nativeFixture(f.journal, f.selection); const d = new Dispatcher(f.journal, n.sdk); const op = operation(command);
  try {
    await d.process({ operation: op, mode: 'dispatch' }, context);
    n.state.events = [event(1, 'client/turn/requested', { input: [{ type: 'text', text: 'human prompt' }], source: 'tell', requestId: 'human' }),
      event(2, 'turn/input/accepted', { clientRequestId: 'human' }, 'turn_human'), event(3, 'turn/completed', { status: 'completed' }, 'turn_human')];
    await d.tick(context.signal); assert.notEqual(f.journal.get(op.id)!.stage, 'terminal'); assert.equal(f.journal.get(op.id)!.turnId, null);
    n.state.events.push(event(4, 'client/turn/requested', { input: [{ type: 'text', text: nativeInput(op) }], source: 'tell', requestId: 'owned' }),
      event(5, 'turn/input/accepted', { clientRequestId: 'owned' }, 'turn_owned'));
    n.state.interactions = [{ threadId: command.thread_id, turnId: 'turn_human', status: 'pending' }];
    await d.tick(context.signal); assert.equal(f.journal.get(op.id)!.nativeState, 'running');
    n.state.interactions = [{ threadId: command.thread_id, turnId: 'turn_owned', status: 'pending' }];
    await d.tick(context.signal); assert.equal(f.journal.get(op.id)!.nativeState, 'waiting_for_input');
    const waitingSequence = f.journal.sequence();
    await d.tick(context.signal); await d.tick(context.signal);
    assert.equal(f.journal.sequence(), waitingSequence, 'unchanged permission waits must not allocate observations');
    assert.equal(n.calls.length, 1);
  } finally { f.close(); }
});
test('persistent missing native thread coalesces evidence and retains effect capacity', async () => {
  const f = fixture(); const command = send(); f.journal.bindSession({sessionId:command.session_id,threadId:command.thread_id,selection:f.selection,harness:{kind:'unconfigured'},operationId:'00000000-0000-4000-8000-000000000001',hostId:null});
  const n = nativeFixture(f.journal, f.selection); const d = new Dispatcher(f.journal, n.sdk); const op = operation(command);
  try {
    await d.process({ operation: op, mode: 'dispatch' }, context); n.state.missing = true;
    await d.tick(context.signal); const sequence = f.journal.sequence();
    for (let i = 0; i < 30; i++) await d.tick(context.signal);
    assert.equal(f.journal.sequence(), sequence); assert.equal(f.journal.capacity().effects, 1);
    assert.equal(f.journal.get(op.id)!.stage, 'uncertain'); assert.equal(n.calls.length, 1);
  } finally { f.close(); }
});
test('resolved matching permission updates running without item event across journal reopen', async () => {
  const f = fixture(); const command = send(); f.journal.bindSession({sessionId:command.session_id,threadId:command.thread_id,selection:f.selection,harness:{kind:'unconfigured'},operationId:'00000000-0000-4000-8000-000000000001',hostId:null});
  const n = nativeFixture(f.journal, f.selection); const d = new Dispatcher(f.journal, n.sdk); const op = operation(command);
  try {
    await d.process({ operation: op, mode: 'dispatch' }, context);
    n.state.events = completed(nativeInput(op)).slice(0, 2);
    n.state.interactions = [{ threadId: command.thread_id, turnId: 'turn_owned', status: 'pending' }];
    await d.tick(context.signal); const pending = f.journal.sequence();
    n.state.interactions[0] = { threadId: command.thread_id, turnId: 'turn_owned', status: 'resolving' };
    await d.tick(context.signal); assert.equal(f.journal.sequence(), pending);
    n.state.interactions.push({ threadId: command.thread_id, turnId: 'turn_human', status: 'resolved' });
    await d.tick(context.signal); assert.equal(f.journal.get(op.id)!.nativeState, 'waiting_for_input');
    f.journal.db.close(); const reopened = f.open();
    const continued = new Dispatcher(reopened, new CloudroomSdk(n.bb, reopened, () => [f.selection.project_id]));
    n.state.interactions[0] = { threadId: command.thread_id, turnId: 'turn_owned', status: 'resolved' };
    n.state.events.push(event(3, 'system/interaction/lifecycle', { interaction: { id: 'permission', status: 'resolved' } }, 'turn_owned'));
    await continued.tick(context.signal); assert.equal(reopened.get(op.id)!.nativeState, 'running');
    assert.notEqual(reopened.get(op.id)!.stage, 'terminal'); const resolved = reopened.sequence();
    await continued.tick(context.signal); assert.equal(reopened.sequence(), resolved);
    n.state.events.push(event(4, 'turn/completed', { status: 'completed' }, 'turn_owned'));
    await continued.tick(context.signal); assert.equal(reopened.get(op.id)!.nativeState, 'completed');
    assert.equal(n.calls.length, 1);
  } finally { f.close(); }
});
for (const cause of ['model', 'provider', 'project', 'expired', 'gate', 'foreign'] as const) {
  test(`preflight ${cause} refusal is durable before any SDK mutation`, async () => {
    const f = fixture(); const n = nativeFixture(f.journal, f.selection);
    const selected = { ...f.selection }; if (cause === 'foreign') selected.machine_id = randomUUID();
    if (cause === 'model') n.state.model = 'another-model';
    if (cause === 'provider') n.state.providerError = { code: 'auth_required' };
    if (cause === 'project') n.state.platform = 'win32';
    const op = operation({ kind: 'create', harness: {kind:'unconfigured'}, session_id: randomUUID(), selection: selected, prompt: 'refuse safely' });
    if (cause === 'expired') { op.deadline = '2020-01-01T00:00:00Z'; const { commandDigest } = await import('../src/protocol.js'); op.payload_digest = commandDigest(op.command, op.deadline); }
    try {
      await new Dispatcher(f.journal, n.sdk).process({ operation: op, mode: 'dispatch' }, { ...context, canDispatch: () => cause !== 'gate' });
      const record = f.journal.get(op.id)!;
      assert.equal(n.calls.length, 0); assert.equal(record.sdkCalls, 0); assert.equal(record.stage, 'terminal');
      assert(record.lastObservation?.kind === 'agent' && record.lastObservation.observation.evidence.kind === 'refused');
      if (cause === 'provider') assert.equal(n.sdk.health.get('codex')!.status, 'auth_required');
    } finally { f.close(); }
  });
}
test('unknown work consumes eight effect slots but explicit mapped-thread stop retains headroom', async () => {
  const f = fixture(); const n = nativeFixture(f.journal, f.selection); const d = new Dispatcher(f.journal, n.sdk);
  const command = send(); f.journal.bindSession({sessionId:command.session_id,threadId:command.thread_id,selection:f.selection,harness:{kind:'unconfigured'},operationId:'00000000-0000-4000-8000-000000000001',hostId:null});
  try {
    for (let i = 0; i < LIMITS.effects; i++) await d.process({ operation: operation(command), mode: 'dispatch' }, context);
    const ninth = operation(command); await d.process({ operation: ninth, mode: 'dispatch' }, context);
    assert.equal(n.calls.length, 8); assert.equal(f.journal.get(ninth.id)!.sdkCalls, 0);
    const stop = operation({ kind: 'stop_thread', session_id: command.session_id, thread_id: command.thread_id });
    await d.process({ operation: stop, mode: 'dispatch' }, context);
    assert.equal(n.calls[8]!.kind, 'stop'); assert.equal(f.journal.capacity().effects, 8);
    assert.equal(f.journal.get(stop.id)!.stage, 'terminal');
  } finally { f.close(); }
});
test('service abort cancels actual SDK wait and retains ambiguous intent without broad stop', async () => {
  const f = fixture(); const command = send(); f.journal.bindSession({sessionId:command.session_id,threadId:command.thread_id,selection:f.selection,harness:{kind:'unconfigured'},operationId:'00000000-0000-4000-8000-000000000001',hostId:null});
  const n = nativeFixture(f.journal, f.selection); n.state.held = true; const stop = new AbortController(); const op = operation(command);
  try {
    const pending = new Dispatcher(f.journal, n.sdk).process({ operation: op, mode: 'dispatch' }, { ...context, signal: stop.signal });
    while (!n.state.entered) await new Promise(resolve => setImmediate(resolve));
    stop.abort(); await pending;
    assert.equal(f.journal.get(op.id)!.stage, 'uncertain'); assert.equal(n.calls.length, 1); assert.equal(n.calls[0]!.kind, 'send');
  } finally { f.close(); }
});
test('matching terminal output stays within UTF8 bound and unknown usage remains null', () => {
  const f = fixture(); const op = operation(send());
  try {
    const record = f.journal.admit({ operation: op, mode: 'dispatch' }).record;
    record.threadId = 'thr_owned'; f.journal.beginIntent(record);
    const events = completed(nativeInput(op));
    events[2] = event(3, 'item/completed', { item: { type: 'agentMessage', id: 'answer_owned', text: `\0${'😀'.repeat(100_000)}` } }, 'turn_owned');
    const update = applyNativeEvents(record, events);
    assert.equal(update.kind, 'terminal'); assert(update.evidence?.kind === 'turn');
    assert(update.evidence.truncated); assert(update.evidence.output);
    assert(Buffer.byteLength(update.evidence.output) <= LIMITS.output);
    assert(!update.evidence.output.includes('\0')); assert.equal(update.evidence.usage, null);
    // The bounded result is accepted by the real durable journal validator.
    f.journal.evidence(update.record, update.evidence, 'terminal');
    assert.equal(f.journal.get(op.id)!.stage, 'terminal');
  } finally { f.close(); }
});
test('ambiguous duplicate matching requests cannot attribute a human or second turn', () => {
  const f = fixture(); const op = operation(send());
  try {
    const record = f.journal.admit({ operation: op, mode: 'dispatch' }).record; record.threadId = 'thr_owned';
    const events = [event(1, 'client/turn/requested', { input: [{ type: 'text', text: nativeInput(op) }], source: 'tell', requestId: 'first' }),
      event(2, 'client/turn/requested', { input: [{ type: 'text', text: nativeInput(op) }], source: 'tell', requestId: 'second' }),
      event(3, 'turn/input/accepted', { clientRequestId: 'second' }, 'turn_second'), event(4, 'turn/completed', { status: 'completed' }, 'turn_second')];
    const update = applyNativeEvents(record, events); assert.equal(update.kind, 'uncertain'); assert.equal(update.record.turnId, null);
    assert(update.record.correlationConflict);
    const later = applyNativeEvents(update.record, [event(3, 'turn/input/accepted', { clientRequestId: 'first' }, 'turn_first'),
      event(4, 'turn/completed', { status: 'completed' }, 'turn_first')]);
    assert.equal(later.kind, 'uncertain'); assert.equal(later.record.turnId, null);
  } finally { f.close(); }
});
test('compacted sparse native replay survives restart and uses completed snapshots', () => {
  const f = fixture(); const op = operation(send());
  try {
    const record = f.journal.admit({ operation: op, mode: 'dispatch' }).record; record.threadId = 'thr_owned'; f.journal.beginIntent(record);
    const requested = event(1, 'client/turn/requested', { input: [{ type: 'text', text: nativeInput(op) }], source: 'tell', requestId: 'request_owned' });
    const bound = applyNativeEvents(record, [requested, event(3, 'turn/input/accepted', { clientRequestId: 'request_owned' }, 'turn_owned'),
      event(5, 'item/agentMessage/delta', { itemId: 'answer', delta: 'partial' }, 'turn_owned')]);
    f.journal.save(bound.record); f.journal.db.close(); const reopened = f.open(); const recovered = reopened.get(op.id)!;
    const update = applyNativeEvents(recovered, [event(9, 'item/completed', { item: { type: 'agentMessage', id: 'answer', text: 'complete retained snapshot' } }, 'turn_owned'),
      event(11, 'turn/completed', { status: 'completed' }, 'turn_owned')]);
    assert.equal(update.kind, 'terminal'); assert(!update.record.correlationConflict); assert.equal(update.record.sdkCalls, 1);
    assert(update.evidence?.kind === 'turn'); assert.equal(update.evidence.output, 'complete retained snapshot'); assert(update.evidence.truncated);
    reopened.evidence(update.record, update.evidence, 'terminal'); assert.equal(reopened.get(op.id)!.stage, 'terminal');
  } finally { f.close(); }
});
for (const omitted of ['request', 'acceptance'] as const) test(`sparse native replay never invents missing ${omitted} correlation`, () => {
  const f = fixture(); const op = operation(send());
  try {
    const record = f.journal.admit({ operation: op, mode: 'dispatch' }).record; record.threadId = 'thr_owned'; f.journal.beginIntent(record);
    const events = completed(nativeInput(op));
    const update = applyNativeEvents(record, events.filter((_, i) => i !== (omitted === 'request' ? 0 : 1)));
    assert.notEqual(update.kind, 'terminal'); assert.equal(update.record.turnId, null); assert(update.record.truncated);
    assert.notEqual(update.evidence?.kind === 'turn' && update.evidence.state, 'completed');
  } finally { f.close(); }
});
test('prunable partial or started text cannot become final output without a completed snapshot', () => {
  const f = fixture(); const op = operation(send());
  try {
    const record = f.journal.admit({ operation: op, mode: 'dispatch' }).record; record.threadId = 'thr_owned'; f.journal.beginIntent(record);
    const events = completed(nativeInput(op));
    const update = applyNativeEvents(record, [events[0]!, events[1]!,
      event(3, 'item/started', { item: { type: 'agentMessage', id: 'answer', text: 'unfinished' } }, 'turn_owned'),
      event(4, 'item/agentMessage/delta', { itemId: 'answer', delta: 'partial' }, 'turn_owned'),
      event(8, 'item/completed', { item: { type: 'agentMessage', id: 'human_answer', text: 'foreign' } }, 'turn_human'),
      event(9, 'turn/completed', { status: 'completed' }, 'turn_owned')]);
    assert.equal(update.kind, 'terminal'); assert(update.evidence?.kind === 'turn'); assert.equal(update.evidence.output, null); assert(update.evidence.truncated);
    assert.throws(() => applyNativeEvents(update.record, [events[0]!]), /cursor/);
  } finally { f.close(); }
});
