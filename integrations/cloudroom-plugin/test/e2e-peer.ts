// Counted native-runtime stand-in, driven by real PostgreSQL/workflow/WSS tests.
// Durable provider state is separate from the plugin journal, as it is in Cloudroom.
import Database from 'better-sqlite3';
import { existsSync, readFileSync, writeFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { createInterface } from 'node:readline';
import { Connector } from '../src/connector.js';
import { Dispatcher } from '../src/dispatcher.js';
import { Journal, JOURNAL_SCHEMA, type CommandRecord } from '../src/journal.js';
import { nativeInput, selectionSchema } from '../src/protocol.js';
import { acquireProfileLock } from '../src/lock.js';
import type { Paired } from '../src/enrollment.js';
import { nativeFixture, event } from './native-fixture.js';

const config = JSON.parse(readFileSync(process.argv[2]!, 'utf8')) as {
  directory: string; paired: Paired; selection: unknown; cut: string;
};
process.umask(0o077);
const selection = selectionSchema.parse(config.selection);
const db = new Database(join(config.directory, 'journal.db')); db.pragma('journal_mode=WAL');
const release = acquireProfileLock(db.name);
if (!db.prepare("SELECT 1 FROM sqlite_master WHERE name='journal_meta'").get()) for (const statement of JOURNAL_SCHEMA) db.exec(statement);
const journal = new Journal(db); journal.initialize(config.paired.binding);
const native = nativeFixture(journal, selection);
const providerFile = join(config.directory, 'provider.json'); const cutFile = join(config.directory, 'cut.json');
if (existsSync(providerFile)) {
  if (statSync(providerFile).size > 2 * 1024 * 1024) throw new Error('provider fixture bound');
  const saved = JSON.parse(readFileSync(providerFile, 'utf8')) as { state: typeof native.state; calls: string[] };
  Object.assign(native.state, saved.state);
  for (const kind of saved.calls) native.calls.push({ kind, input: {} });
}
const persistProvider = () => writeFileSync(providerFile, JSON.stringify({ state: native.state, calls: native.calls.map(call => call.kind) }), { mode: 0o600 });
persistProvider();
if (config.cut === 'after_admission' && !existsSync(cutFile)) {
  // The Rust owner has committed admission; this peer has not opened a socket.
  writeFileSync(cutFile, JSON.stringify({ stage: config.cut }), { mode: 0o600 });
  process.kill(process.pid, 'SIGKILL');
}
function cut(stage: string, record: CommandRecord) {
  if (config.cut !== stage || existsSync(cutFile) || !['create', 'send'].includes(record.operation.command.kind)) return;
  writeFileSync(cutFile, JSON.stringify({ stage, operation: record.operation.id }), { mode: 0o600 });
  process.kill(process.pid, 'SIGKILL');
}
const admit = journal.admit.bind(journal);
journal.admit = delivery => {
  const provisional = { operation: delivery.operation } as CommandRecord;
  cut('before_receipt', provisional);
  const result = admit(delivery); cut('after_receipt', result.record); return result;
};
const intent = journal.beginIntent.bind(journal);
journal.beginIntent = record => { intent(record); cut('after_intent', record); };
const submission = journal.submission.bind(journal);
journal.submission = (record, selected, state) => { submission(record, selected, state); cut('after_mapping', record); };
const evidence = journal.evidence.bind(journal);
journal.evidence = (record, value, importance) => { evidence(record, value, importance); if (importance === 'terminal') cut('after_terminal', record); };
const acknowledge = journal.acknowledge.bind(journal);
journal.acknowledge = through => {
  for (const observation of journal.outbox()) {
    if (observation.kind !== 'agent' || observation.observation.sequence > through) continue;
    const record = journal.get(observation.observation.operation_id);
    if (record?.stage === 'terminal' && observation.observation.evidence.kind === 'turn'
        && observation.observation.evidence.state === 'completed') cut('after_server_receipt', record);
  }
  acknowledge(through);
};
function completeNative(record: CommandRecord) {
  const sequence = native.state.events.at(-1)?.seq ?? 0;
  if (sequence >= 400) throw new Error('native event fixture bound');
  const request = `request_${record.operation.id}`; const turn = `turn_${record.operation.id}`;
  native.state.events.push(event(sequence + 1, 'client/turn/requested', {
    input: [{ type: 'text', text: nativeInput(record.operation) }], source: record.operation.command.kind === 'create' ? 'spawn' : 'tell', requestId: request,
  }), event(sequence + 2, 'turn/input/accepted', { clientRequestId: request }, turn),
  event(sequence + 3, 'item/completed', { item: { type: 'agentMessage', id: `answer_${sequence}`, text: 'counted e2e native result' } }, turn),
  event(sequence + 4, 'turn/completed', { status: 'completed' }, turn));
  persistProvider(); cut('after_sdk', record);
}
for (const kind of ['spawn', 'send'] as const) {
  const original = native.bb.sdk.threads[kind].bind(native.bb.sdk.threads);
  // The fixture records actual SDK mutator entry, then independently persists the
  // native transcript before a crash can interrupt the plugin's mapping write.
  (native.bb.sdk.threads as any)[kind] = async (input: unknown) => {
    const result = await (original as (input: unknown) => Promise<unknown>)(input);
    const record = journal.pending().find(candidate => candidate.sdkCalls === 1 && candidate.operation.command.kind === (kind === 'spawn' ? 'create' : 'send'));
    if (!record) throw new Error('native effect without journal intent'); completeNative(record); return result;
  };
}
const emit = (value: unknown) => process.stdout.write(JSON.stringify(value) + '\n');
const stop = new AbortController(); const connector = new Connector(new Dispatcher(journal, native.sdk));
const running = connector.run(config.paired, stop.signal); void running.catch(() => emit({ event: 'failure', stage: 'connector' }));
const lines = createInterface({ input: process.stdin }); const deadline = setTimeout(() => stop.abort(), 120_000);
emit({ event: 'started' });
try {
  for await (const line of lines) {
    if (Buffer.byteLength(line) > 1024) throw new Error('fixture input bound');
    const input = JSON.parse(line) as { command: string };
    if (input.command === 'snapshot') emit({ event: 'snapshot', calls: native.calls.map(call => call.kind),
      connection: connector.status, sequence: journal.sequence(), acknowledged: journal.acknowledged(),
      recovery: journal.recoveryRequired(), outbox: journal.outbox().length });
    else if (input.command === 'close') break;
    else throw new Error('unknown fixture command');
  }
} catch { emit({ event: 'failure', stage: 'fixture' }); process.exitCode = 1; }
finally { stop.abort(); clearTimeout(deadline); await running.catch(() => {}); lines.close(); persistProvider(); db.close(); release(); }
