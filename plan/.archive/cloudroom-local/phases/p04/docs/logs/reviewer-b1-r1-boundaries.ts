import assert from 'node:assert/strict';
import openbot from '/Users/mac03/dev/openbot/integrations/cloudroom-plugin/src/server.ts';
import { Connector } from '/Users/mac03/dev/openbot/integrations/cloudroom-plugin/src/connector.ts';
import { Dispatcher } from '/Users/mac03/dev/openbot/integrations/cloudroom-plugin/src/dispatcher.ts';
import { recoverEnrollment } from '/Users/mac03/dev/openbot/integrations/cloudroom-plugin/src/enrollment.ts';
import { nativeInput } from '/Users/mac03/dev/openbot/integrations/cloudroom-plugin/src/protocol.ts';
import { fixture, operation, send } from '/Users/mac03/dev/openbot/integrations/cloudroom-plugin/test/support.ts';
import { event, nativeFixture } from '/Users/mac03/dev/openbot/integrations/cloudroom-plugin/test/native-fixture.ts';
async function main() {
  const f = fixture(); const command = send(); f.journal.bindSession(command.session_id, command.thread_id, f.selection);
  const n = nativeFixture(f.journal, f.selection); const op = operation(command);
  let values = { openbotOrigin: f.binding.origin, selectedProjects: f.selection.project_id,
    enrollment: JSON.stringify({ state: 'paired', version: 1, binding: f.binding, credential: 'a'.repeat(64) }) };
  let changed = (_next: any, _prev: any) => {}; let start: any; let reads = 0;
  let paused!: () => void; const atSecondRead = new Promise<void>(resolve => paused = resolve);
  let resume!: () => void; const resumed = new Promise<void>(resolve => resume = resolve);
  const bb: any = { ...n.bb, storage: { database: () => f.journal.db, migrate: () => {} },
    settings: { define: () => ({ get: async () => { if (++reads === 2) { paused(); await resumed; } return { ...values }; },
      experimental_set: async () => values, onChange: (listener: typeof changed) => changed = listener }) },
    background: { service: (_name: string, service: any) => start = service.start }, cli: { register: () => {} } };
  const originalRun = Connector.prototype.run;
  let connectorEntered = false;
  Connector.prototype.run = async function(_paired, signal) {
    connectorEntered = true;
    await this.dispatcher.process({ operation: op, mode: 'dispatch' }, { signal, canDispatch: () => true });
  };
  try {
    await openbot(bb); const running = start(new AbortController().signal);
    await atSecondRead;
    const old = values; values = { ...values, selectedProjects: '' }; changed(values, old); resume();
    await running;
    assert.equal(values.selectedProjects, ''); assert.equal(connectorEntered, true); assert.equal(n.calls.length, 1);
    console.log(JSON.stringify({ case: 'project withdrawal during startup', configuredProjects: values.selectedProjects,
      connectorEntered, mutationCallsAfterWithdrawal: n.calls.length, durableSdkCalls: f.journal.get(op.id)?.sdkCalls,
      transport: 'Connector.run replaced by direct production Dispatcher.process; public SDK stand-in, no network/native effect' }));
  } finally { Connector.prototype.run = originalRun; f.close(); }

  const p = fixture(); const c = send(); p.journal.bindSession(c.session_id, c.thread_id, p.selection);
  const native = nativeFixture(p.journal, p.selection); const dispatcher = new Dispatcher(p.journal, native.sdk); const operationRecord = operation(c);
  const signal = new AbortController().signal;
  try {
    await dispatcher.process({ operation: operationRecord, mode: 'dispatch' }, { signal, canDispatch: () => true });
    native.state.events = [event(1, 'client/turn/requested', { input: [{ type: 'text', text: nativeInput(operationRecord) }], source: 'tell', requestId: 'req' }),
      event(2, 'turn/input/accepted', { clientRequestId: 'req' }, 'turn')];
    native.state.interactions = [{ threadId: c.thread_id, turnId: 'turn', status: 'pending' }];
    await dispatcher.tick(signal); assert.equal(p.journal.get(operationRecord.id)?.nativeState, 'waiting_for_input');
    native.state.interactions = [{ threadId: c.thread_id, turnId: 'turn', status: 'resolved' }];
    native.state.events.push(event(3, 'system/interaction/lifecycle', { interaction: { id: 'permission', status: 'resolved' } }, 'turn'));
    await dispatcher.tick(signal);
    const after = p.journal.get(operationRecord.id)!;
    assert.equal(after.nativeState, 'waiting_for_input');
    console.log(JSON.stringify({ case: 'resolved permission with no item event', interactions: 'resolved', nativeState: after.nativeState,
      limitation: 'synthetic public SDK fixture; no native scheduler assertion' }));
  } finally { p.close(); }

  const enrollmentFixture = fixture({ initialized: false });
  let enrollmentValues: any = { openbotOrigin: enrollmentFixture.binding.origin, machineName: 'Disposable',
    selectedProjects: '', pairingCode: 'b'.repeat(32) };
  const settings = { get: async () => ({ ...enrollmentValues }), experimental_set: async (patch: any) => {
    for (const [key, value] of Object.entries(patch)) { if (value === null) delete enrollmentValues[key]; else enrollmentValues[key] = value; }
    return { ...enrollmentValues };
  } };
  const requests: any[] = [];
  const rejectPairing: any = async (_url: any, args: any) => {
    if (args.method === 'POST') { requests.push(JSON.parse(args.body)); return new Response(null, { status: 400 }); }
    return new Response(null, { status: 404 });
  };
  try {
    await assert.rejects(recoverEnrollment(settings, enrollmentFixture.journal, new AbortController().signal, rejectPairing), /Pairing was not accepted/);
    const rejectedInputRetained = enrollmentValues.pairingCode !== undefined;
    enrollmentValues.pairingCode = 'c'.repeat(32);
    await assert.rejects(recoverEnrollment(settings, enrollmentFixture.journal, new AbortController().signal, rejectPairing), /Pairing was not accepted/);
    assert(rejectedInputRetained); assert.deepEqual(requests[0], requests[1]);
    console.log(JSON.stringify({ case: 'definitive enrollment rejection', status: 400, rejectedInputRetained,
      enteredReplacementCodeIgnored: requests[1].code !== enrollmentValues.pairingCode,
      pendingStillContainsRejectedCode: JSON.parse(enrollmentValues.enrollment).request.code === requests[0].code,
      journalInitialized: enrollmentFixture.journal.initialized(), transport: 'synthetic complete HTTP status/response; no network' }));
  } finally { enrollmentFixture.close(); }
}
main().catch(error => { console.error(error); process.exitCode = 1; });
