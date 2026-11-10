import test from 'node:test';
import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import { FrameAssembler, encodeFrames, LIMITS, commandDigest, nativeInput, parseOperation } from '../src/protocol.js';
import { JournalCapacity, RecoveryRequired } from '../src/journal.js';
import { canonicalOrigin, selectedProjects } from '../src/enrollment.js';
import { fixture, operation, send } from './support.js';

for (const origin of ['http://example.test', 'https://user:secret@example.test', 'https://example.test/path',
  'https://example.test/?query=secret', 'https://example.test/#secret']) {
  test(`configuration refuses non-origin URL ${new URL(origin).protocol}/${new URL(origin).pathname}`, () => assert.throws(() => canonicalOrigin(origin)));
}
test('configuration canonicalizes HTTPS and refuses arbitrary project paths/duplicates/bounds', () => {
  assert.equal(canonicalOrigin('https://EXAMPLE.test:443/'), 'https://example.test');
  assert.deepEqual(selectedProjects('proj_one\nproj_two'), ['proj_one', 'proj_two']);
  for (const selected of ['../secret', 'proj_one\nproj_one', Array.from({ length: 129 }, (_, i) => `proj_${i}`).join('\n')]) {
    assert.throws(() => selectedProjects(selected));
  }
});
test('user UTF8 prompt bound remains32KiB with fixed native operation header', () => {
  const command = { ...send(), prompt: 'é'.repeat(16384) } as ReturnType<typeof send>;
  const op = operation(command);
  assert.equal(Buffer.byteLength(nativeInput(parseOperation(op))), 32768 + 57);
  assert.throws(() => operation({ ...command, prompt: `${command.prompt}x` }));
});
test('actual peer command decoder canonicalizes field order and rejects hash/date/field changes', () => {
  const op = operation(send()); op.deadline = '2030-01-02T03:04:05.123456Z'; op.payload_digest = commandDigest(op.command, op.deadline);
  assert.deepEqual(parseOperation({ ...op, command: Object.fromEntries(Object.entries(op.command).reverse()) }), op);
  assert.throws(() => parseOperation({ ...op, deadline: new Date(op.deadline).toISOString() }));
  assert.throws(() => parseOperation({ ...op, extra: true }));
});
test('chunk transport roundtrips the exact2MiB limit and rejects overflow before send', () => {
  const overhead = Buffer.byteLength(JSON.stringify({ value: '' }));
  const payload = { value: 'x'.repeat(LIMITS.logical - overhead) };
  const frames = encodeFrames(payload); assert.equal(frames.length, 128);
  const assembler = new FrameAssembler(); let result;
  for (const frame of frames) { assert(Buffer.byteLength(frame) <= LIMITS.frame); result = assembler.push(frame); }
  assert.deepEqual(result, payload);
  assert.throws(() => encodeFrames({ value: `${payload.value}x` }));
});
test('chunks reject interleaving, invalid UTF8/base64, unsafe integers and time expiry', () => {
  const frames = encodeFrames({ value: 'x'.repeat(20_000) });
  let now = 0; const assembler = new FrameAssembler(() => now);
  assert.equal(assembler.push(frames[0]!), undefined); now = 30_000;
  assert.throws(() => assembler.expire());
  assert.throws(() => new FrameAssembler().push(Buffer.from([255])));
  const chunk = JSON.parse(frames[0]!);
  assert.throws(() => new FrameAssembler().push(JSON.stringify({ ...chunk, index: Number.MAX_SAFE_INTEGER + 1 })));
  assert.throws(() => new FrameAssembler().push(JSON.stringify({ ...chunk, data: `!${chunk.data.slice(1)}` })));
  const other = new FrameAssembler(); other.push(frames[0]!);
  assert.throws(() => other.push(JSON.stringify({ ...JSON.parse(frames[1]!), transfer_id: randomUUID() })));
});
test('real SQLite survives receipt/intent reopen and replay keeps immutable payloads', () => {
  const f = fixture();
  try {
    const op = operation(send()); const admitted = f.journal.admit({ operation: op, mode: 'dispatch' });
    const receipt = f.journal.outbox(); assert.equal(receipt.length, 1);
    f.journal.db.close(); const reopened = f.open(); reopened.validateBinding(f.binding);
    const record = reopened.get(op.id)!; assert.equal(record.stage, 'received');
    reopened.beginIntent(record); assert.equal(reopened.sequence(), 2);
    reopened.db.close(); const again = f.open();
    assert.equal(again.get(op.id)!.stage, 'intent');
    assert.throws(() => again.beginIntent(again.get(op.id)!), RecoveryRequired);
    assert.deepEqual(again.outbox()[0], receipt[0]);
    assert.equal(again.admit({ operation: op, mode: 'reconcile' }).fresh, false);
    assert.equal(again.sequence(), 2);
    const changed = send();
    assert.throws(() => again.admit({ operation: { ...op, command: changed, payload_digest: commandDigest(changed, op.deadline) }, mode: 'dispatch' }), /identity conflict/);
  } finally { f.close(); }
});
test('competing stale intent claimants have exactly one durable SDK entry marker', async () => {
  const f = fixture();
  try {
    const op = operation(send()); f.journal.admit({ operation: op, mode: 'dispatch' });
    const snapshots = [f.journal.get(op.id)!, f.journal.get(op.id)!];
    let release!: () => void; const barrier = new Promise<void>(done => { release = done; }); let sdkEntries = 0;
    const claims = snapshots.map(record => (async () => { await barrier; f.journal.beginIntent(record); sdkEntries++; })());
    release(); const results = await Promise.allSettled(claims);
    assert.equal(results.filter(value => value.status === 'fulfilled').length, 1);
    assert.equal(sdkEntries, 1); assert.equal(f.journal.sequence(), 2);
    assert.equal(f.journal.get(op.id)!.sdkCalls, 1);
    assert.throws(() => f.journal.save({ ...snapshots[1]!, stage: 'received', sdkCalls: 0 }));
  } finally { f.close(); }
});
test('missing command under reconcile enters durable recovery without a fresh SDK claim', () => {
  const f = fixture();
  try {
    const op = operation(send()); const recovered = f.journal.admit({ operation: op, mode: 'reconcile' });
    assert.equal(recovered.record.stage, 'uncertain'); assert.equal(recovered.record.sdkCalls, 0);
    assert(f.journal.recoveryRequired()); assert.throws(() => f.journal.beginIntent(recovered.record));
    f.journal.db.close(); assert(f.open().recoveryRequired());
  } finally { f.close(); }
});
test('acknowledgment cannot forget an unknown future or restore an already-pruned cursor', () => {
  const f = fixture();
  try {
    const op = operation(); f.journal.admit({ operation: op, mode: 'dispatch' });
    assert.throws(() => f.journal.acknowledge(2)); f.journal.acknowledge(1);
    assert.equal(f.journal.outbox().length, 0); assert.throws(() => f.journal.acknowledge(0));
    assert.equal(f.journal.get(op.id)!.lastObservation?.kind, 'agent');
  } finally { f.close(); }
});
test('preflight refusal is durable but cannot follow the SDK intent cut', () => {
  const f = fixture();
  try {
    const op = operation(send()); const record = f.journal.admit({ operation: op, mode: 'dispatch' }).record;
    f.journal.beginIntent(record);
    assert.throws(() => f.journal.evidence(record, { kind: 'refused', reason: 'expired' }, 'critical'));
    assert.equal(f.journal.sequence(), 2); assert.equal(f.journal.capacity().effects, 1);
    const other = f.journal.admit({ operation: operation(), mode: 'dispatch' }).record;
    other.stage = 'terminal'; f.journal.evidence(other, { kind: 'refused', reason: 'capacity' }, 'terminal');
    assert.equal(f.journal.get(other.operation.id)!.stage, 'terminal');
  } finally { f.close(); }
});
test('fixed4096 history bound fails controlled admission and never allocates a dangling sequence', () => {
  const f = fixture();
  try {
    f.journal.db.transaction(() => {
      for (let i = 0; i < 4096; i++) {
        const record = f.journal.admit({ operation: operation(), mode: 'dispatch' }).record;
        record.stage = 'terminal'; f.journal.evidence(record, { kind: 'refused', reason: 'capacity' }, 'terminal');
        f.journal.acknowledge(f.journal.sequence());
      }
    })();
    const before = f.journal.sequence(); assert.throws(() => f.journal.admit({ operation: operation(), mode: 'dispatch' }), JournalCapacity);
    assert.equal(f.journal.sequence(), before);
  } finally { f.close(); }
});
