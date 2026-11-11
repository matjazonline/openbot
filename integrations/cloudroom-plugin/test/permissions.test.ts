import test from 'node:test';
import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import { Dispatcher } from '../src/dispatcher.js';
import { policySchema, type Selection } from '../src/protocol.js';
import { nativeFixture } from './native-fixture.js';
import { fixture, operation, send } from './support.js';
const context = { signal: new AbortController().signal, canDispatch: () => true };
for (const mode of ['full', 'auto', 'accept_edits'] as const) {
  for (const kind of ['create', 'send'] as const) test(`${kind} sends explicit ${mode} native permissions`, async () => {
    const f = fixture(); f.selection.policy.permissions = mode;
    const n = nativeFixture(f.journal, f.selection);
    try {
      const command = kind === 'create'
        ? { kind, session_id: randomUUID(), selection: f.selection, prompt: 'authorized fixture' }
        : send();
      if (command.kind === 'send') f.journal.bindSession(command.session_id, command.thread_id, f.selection);
      await new Dispatcher(f.journal, n.sdk).process({ operation: operation(command), mode: 'dispatch' }, context);
      assert.equal(n.calls.length, 1);
      assert.equal(n.calls[0]!.input.permissionMode, mode === 'accept_edits' ? 'accept-edits' : mode);
    } finally { f.close(); }
  });
}
for (const boundary of ['provider', 'server', 'host', 'manual'] as const) {
  test(`unsupported full access at ${boundary} refuses before native intent`, async () => {
    const f = fixture(); const command = send(); f.journal.bindSession(command.session_id, command.thread_id, f.selection);
    const n = nativeFixture(f.journal, f.selection);
    if (boundary === 'provider') n.state.permissionModes = ['auto', 'accept-edits'];
    if (boundary === 'server') n.state.permissionCeiling = 'auto';
    if (boundary === 'host') n.state.hostPermissionCeiling = 'auto';
    if (boundary === 'manual') n.state.nativePermission = 'auto';
    try {
      const op = operation(command);
      await new Dispatcher(f.journal, n.sdk).process({ operation: op, mode: 'dispatch' }, context);
      assert.equal(n.calls.length, 0);
      const record = f.journal.get(op.id)!;
      assert.equal(record.sdkCalls, 0); assert.equal(record.stage, 'terminal');
      assert(record.lastObservation?.kind === 'agent');
      assert.deepEqual(record.lastObservation.observation.evidence, { kind: 'refused', reason: 'permissions_unavailable' });
    } finally { f.close(); }
  });
}
test('catalog advertises only permission modes allowed by provider and both ceilings', async () => {
  const f = fixture(); const n = nativeFixture(f.journal, f.selection);
  n.state.hostPermissionCeiling = 'auto'; n.state.permissionModes = ['auto', 'full'];
  try {
    const catalog = await n.sdk.catalog(context.signal);
    assert.deepEqual(catalog.providers[0]!.permission_modes, ['auto']);
  } finally { f.close(); }
});
test('permission policy has no implicit default or legacy permission decoder', () => {
  const base: Selection['policy'] = { workspace: 'project_directory', permissions: 'full', reasoning: 'low' };
  assert(policySchema.safeParse(base).success);
  assert(!policySchema.safeParse({ ...base, permissions: 'provider_default' }).success);
  const { permissions: ignored, ...missing } = base; void ignored;
  assert(!policySchema.safeParse(missing).success);
});
