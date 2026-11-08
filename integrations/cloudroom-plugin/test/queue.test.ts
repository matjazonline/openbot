import test from 'node:test';
import assert from 'node:assert/strict';
import { Dispatcher } from '../src/dispatcher.js';
import { CloudroomSdk } from '../src/sdk.js';
import { nativeInput } from '../src/protocol.js';
import { nativeQueueFixture } from './native-queue-fixture.js';
import { fixture, operation, send } from './support.js';
const context = { signal: new AbortController().signal, canDispatch: () => true };

for (const initial of ['idle', 'human', 'openbot'] as const) {
  test(`native ${initial} admission preserves prompt order, queue identity and separate originating turns`, async () => {
    const f = fixture(); const command = send(); f.journal.bindSession(command.session_id, command.thread_id, f.selection);
    const n = nativeQueueFixture(f); if (initial !== 'idle') n.preexisting(initial);
    const d = new Dispatcher(f.journal, n.sdk); const first = operation({ ...command, prompt: 'first local request' });
    const second = operation({ ...command, prompt: 'second local request' });
    try {
      await d.process({ operation: first, mode: 'dispatch' }, context);
      await d.process({ operation: second, mode: 'dispatch' }, context);
      const receipt = f.journal.get(first.id)!.lastObservation!;
      assert(receipt.kind === 'agent' && receipt.observation.evidence.kind === 'submission');
      assert.equal(receipt.observation.evidence.state, initial === 'idle' ? 'accepted' : 'queued');
      assert.equal(f.journal.get(first.id)!.queueId, initial === 'idle' ? null : 'queue_native_1');
      assert.equal(f.journal.get(second.id)!.queueId, 'queue_native_2');
      await d.tick(context.signal); assert.equal(f.journal.get(second.id)!.turnId, null);
      assert.equal(f.journal.capacity().effects, 2);
      if (initial !== 'idle') {
        assert.equal(f.journal.get(first.id)!.turnId, null);
        n.finish('preexisting human or Openbot result'); await d.tick(context.signal);
        assert.notEqual(f.journal.get(first.id)!.stage, 'terminal'); assert.notEqual(f.journal.get(second.id)!.stage, 'terminal');
      }
      assert.equal(f.journal.get(first.id)!.turnId, 'turn_native_1');
      n.finish('first local result'); await d.tick(context.signal);
      assert.equal(f.journal.get(first.id)!.nativeState, 'completed');
      assert.equal(f.journal.get(second.id)!.turnId, 'turn_native_2'); assert.notEqual(f.journal.get(second.id)!.stage, 'terminal');
      n.finish('second local result'); await d.tick(context.signal);
      const results = [first, second].map(op => f.journal.get(op.id)!.lastObservation!);
      for (const result of results) assert(result.kind === 'agent' && result.observation.evidence.kind === 'turn');
      assert.deepEqual(results.map(result => result.kind === 'agent' && result.observation.evidence.kind === 'turn' ? result.observation.evidence.output : null), ['first local result', 'second local result']);
      assert.deepEqual(n.started.slice(initial === 'idle' ? 0 : 1), [nativeInput(first), nativeInput(second)]);
      assert.equal(n.calls.length, 2); assert.equal(f.journal.capacity().effects, 0);
    } finally { f.close(); }
  });
}
for (const race of ['idle_to_busy', 'busy_to_idle'] as const) {
  test(`native queue ${race} race is decided at admission without steering or a second SDK call`, async () => {
    const f = fixture(); const command = send(); f.journal.bindSession(command.session_id, command.thread_id, f.selection);
    const n = nativeQueueFixture(f); if (race === 'busy_to_idle') n.preexisting('human');
    n.beforeSend(() => { if (race === 'idle_to_busy') n.preexisting('human'); else n.finish('human completed before send entry'); });
    const d = new Dispatcher(f.journal, n.sdk); const op = operation(command);
    try {
      await d.process({ operation: op, mode: 'dispatch' }, context);
      const receipt = f.journal.get(op.id)!.lastObservation!;
      assert(receipt.kind === 'agent' && receipt.observation.evidence.kind === 'submission');
      assert.equal(receipt.observation.evidence.state, race === 'idle_to_busy' ? 'queued' : 'accepted');
      await d.tick(context.signal); assert.notEqual(f.journal.get(op.id)!.stage, 'terminal');
      if (race === 'idle_to_busy') { assert.equal(f.journal.get(op.id)!.turnId, null); n.finish('human answer'); await d.tick(context.signal); }
      n.finish('race-owned local answer'); await d.tick(context.signal);
      assert.equal(f.journal.get(op.id)!.nativeState, 'completed'); assert.equal(n.calls.length, 1);
    } finally { f.close(); }
  });
}
test('accepted native queue work survives lost SDK response and replay with one mutation and original order', async () => {
  const f = fixture(); const command = send(); f.journal.bindSession(command.session_id, command.thread_id, f.selection);
  const n = nativeQueueFixture(f); n.preexisting('human'); n.loseNextResponse(); const op = operation(command);
  try {
    const d = new Dispatcher(f.journal, n.sdk); await d.process({ operation: op, mode: 'dispatch' }, context);
    assert.equal(f.journal.get(op.id)!.stage, 'uncertain'); assert.equal(n.pending.length, 1); assert.equal(n.calls.length, 1);
    f.journal.db.close(); const reopened = f.open();
    const restored = new Dispatcher(reopened, new CloudroomSdk(n.bb, reopened, () => [f.selection.project_id]));
    await restored.process({ operation: op, mode: 'reconcile' }, context);
    assert.equal(n.calls.length, 1); assert.equal(reopened.get(op.id)!.turnId, null);
    n.finish('human result'); await restored.tick(context.signal);
    assert.equal(reopened.get(op.id)!.turnId, 'turn_native_1'); assert.notEqual(reopened.get(op.id)!.stage, 'terminal');
    n.finish('recovered local result'); await restored.tick(context.signal);
    assert.equal(reopened.get(op.id)!.nativeState, 'completed'); assert.equal(n.calls.length, 1);
    assert.deepEqual(n.started, ['Human authorized work', nativeInput(op)]);
  } finally { f.close(); }
});
