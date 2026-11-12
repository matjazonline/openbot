// Counted public-SDK peer for the real PostgreSQL/WSS lifecycle fixture. Secrets
// enter through a private configuration file and never appear in its event stream.
import Database from 'better-sqlite3';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { createInterface } from 'node:readline';
import { Connector } from '../src/connector.js';
import { Dispatcher } from '../src/dispatcher.js';
import { Journal, JOURNAL_SCHEMA, bindingSchema } from '../src/journal.js';
import { nativeInput, selectionSchema, type Selection } from '../src/protocol.js';
import type { Paired } from '../src/enrollment.js';
import { event, nativeFixture } from './native-fixture.js';

const cfg = JSON.parse(readFileSync(process.argv[2]!, 'utf8')) as {
  directory: string; paired: Paired; selection: Selection; session: string; thread: string; sequence: number;
};
cfg.paired.binding = bindingSchema.parse(cfg.paired.binding); cfg.selection = selectionSchema.parse(cfg.selection);
const db = new Database(join(cfg.directory, 'peer.db')); db.pragma('journal_mode=WAL');
for (const statement of JOURNAL_SCHEMA) db.exec(statement);
const journal = new Journal(db); journal.initialize(cfg.paired.binding);
db.prepare('UPDATE journal_meta SET allocated=?,acknowledged=? WHERE id=1').run(cfg.sequence, cfg.sequence);
journal.bindSession({sessionId:cfg.session,threadId:cfg.thread,selection:cfg.selection,harness:{kind:'unconfigured'},operationId:'00000000-0000-4000-8000-000000000001',hostId:null});
db.prepare('UPDATE sessions SET last_used_at=?').run(Date.now() - 100 * 86_400_000);
const n = nativeFixture(journal, cfg.selection); n.state.thread.id = cfg.thread;
const emit = (value: unknown) => process.stdout.write(`${JSON.stringify(value)}\n`);
const retire = journal.retireMappings.bind(journal);
journal.retireMappings = (candidates, retired) => {
  const removed = retire(candidates, retired); emit({ event: 'retirement', retired, removed }); return removed;
};
const acknowledge = journal.acknowledge.bind(journal);
journal.acknowledge = through => { acknowledge(through); emit({ event: 'ack', through, outbox: journal.outbox().length }); };
let connector: Connector | undefined; let stop: AbortController | undefined; let running: Promise<void> | undefined;
async function stopPeer() {
  stop?.abort(); await running; running = undefined; connector = undefined;
  emit({ event: 'stopped', sequence: journal.sequence(), acknowledged: journal.acknowledged() });
}
const finished = new Set<string>();
const observation = setInterval(() => {
  for (const record of journal.pending()) {
    if (record.operation.command.kind !== 'send' || record.sdkCalls !== 1 || finished.has(record.operation.id)
        || !n.calls.some(call => call.kind === 'send')) continue;
    finished.add(record.operation.id);
    n.state.events = [event(1, 'client/turn/requested', { input: [{ type: 'text', text: nativeInput(record.operation) }], source: 'tell', requestId: 'request_owned' }),
      event(2, 'turn/input/accepted', { clientRequestId: 'request_owned' }, 'turn_owned'),
      event(3, 'item/completed', { item: { type: 'agentMessage', id: 'answer', text: 'combined mapping proof' } }, 'turn_owned'),
      event(4, 'turn/completed', { status: 'completed' }, 'turn_owned')];
  }
}, 10);
const deadline = setTimeout(() => process.exit(1), 30_000);
const lines = createInterface({ input: process.stdin });
try {
  for await (const line of lines) {
    const input = JSON.parse(line) as { command: string };
    if (input.command === 'start') {
      if (running) throw new Error('peer already running');
      stop = new AbortController(); connector = new Connector(new Dispatcher(journal, n.sdk));
      running = connector.run(cfg.paired, stop.signal); void running.catch(() => emit({ event: 'failure' }));
      emit({ event: 'started' });
    } else if (input.command === 'stop') await stopPeer();
    else if (input.command === 'age') {
      const future = Date.now() + 91 * 86_400_000; Date.now = () => future; emit({ event: 'aged' });
    } else if (input.command === 'snapshot') emit({ event: 'snapshot', calls: n.calls.filter(call => call.kind === 'send').length,
      mapped: !!journal.session(cfg.session), sequence: journal.sequence(), acknowledged: journal.acknowledged(),
      outbox: journal.outbox().length, connection: connector?.status ?? 'offline' });
    else if (input.command === 'close') { await stopPeer(); break; }
    else throw new Error('unknown fixture command');
  }
} catch {
  emit({ event: 'failure' }); process.exitCode = 1;
} finally {
  clearInterval(observation); clearTimeout(deadline); stop?.abort(); await running?.catch(() => {}); lines.close(); db.close();
}
