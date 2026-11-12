import type Database from 'better-sqlite3';
import { z } from 'zod';
import { catalogSchema, evidenceSchema, isEffect, LIMITS, nativeInput, observationSchema,
  observationSequence, parseOperation, selectionSchema, harnessSchema, turnState, external, uuid, type Catalog, type Delivery,
  type Evidence, type Observation, type Operation, type Selection } from './protocol.js';

export const JOURNAL_SCHEMA = [
  `CREATE TABLE journal_meta(id INTEGER PRIMARY KEY CHECK(id=1),binding TEXT NOT NULL,
    allocated INTEGER NOT NULL DEFAULT 0 CHECK(allocated>=0 AND allocated<=9007199254740991),
    acknowledged INTEGER NOT NULL DEFAULT 0 CHECK(acknowledged>=0 AND acknowledged<=allocated),
    recovery_required INTEGER NOT NULL DEFAULT 0 CHECK(recovery_required IN (0,1)))`,
  `CREATE TABLE commands(id TEXT PRIMARY KEY,digest TEXT NOT NULL,state TEXT NOT NULL,
    reserve INTEGER NOT NULL CHECK(reserve>=0),terminal INTEGER NOT NULL CHECK(terminal IN (0,1)),
    terminal_at INTEGER,acknowledged_at INTEGER,
    session_id TEXT GENERATED ALWAYS AS (json_extract(state,'$.operation.command.session_id')) VIRTUAL,
    CHECK((terminal=0 AND terminal_at IS NULL) OR (terminal=1 AND terminal_at IS NOT NULL)),
    CHECK(acknowledged_at IS NULL OR terminal=1))`,
  `CREATE TABLE sessions(id TEXT PRIMARY KEY,mapping TEXT NOT NULL CHECK(json_valid(mapping) AND length(mapping)<=4096),
    thread_id TEXT GENERATED ALWAYS AS (json_extract(mapping,'$.threadId')) STORED NOT NULL UNIQUE,last_used_at INTEGER NOT NULL)`,
  `CREATE TABLE outbox(sequence INTEGER PRIMARY KEY,operation_id TEXT NOT NULL REFERENCES commands(id),
    payload TEXT NOT NULL,bytes INTEGER NOT NULL CHECK(bytes>0))`,
  `CREATE INDEX commands_session_mapping ON commands(session_id)`,
  `CREATE INDEX commands_retention ON commands(terminal,acknowledged_at,terminal_at,id)`,
  `CREATE INDEX sessions_retention ON sessions(last_used_at,id)`,
  `CREATE INDEX outbox_operation ON outbox(operation_id)`,
  `CREATE TABLE mcp_intents(key TEXT PRIMARY KEY,operation_id TEXT NOT NULL REFERENCES commands(id) ON DELETE CASCADE,
    state TEXT NOT NULL CHECK(json_valid(state)),bytes INTEGER NOT NULL CHECK(bytes=length(CAST(state AS BLOB)) AND bytes<=8192),
    resolved INTEGER NOT NULL CHECK(resolved IN (0,1)),reserve INTEGER NOT NULL CHECK(reserve IN (0,256)))`,
  `CREATE INDEX mcp_intents_operation ON mcp_intents(operation_id,resolved)`,
];
export const bindingSchema = z.object({ origin: z.string().url(), installation_id: uuid,
  journal_id: uuid, machine_id: uuid, company_id: uuid, credential_generation: z.number().int().positive().max(Number.MAX_SAFE_INTEGER) }).strict();
export type Binding = z.infer<typeof bindingSchema>;
const stored = z.object({ operation: z.unknown().transform(parseOperation),
  stage: z.enum(['received', 'intent', 'observing', 'uncertain', 'terminal']),
  sdkInput: z.string().max(LIMITS.prompt + 57), sdkCalls: z.number().int().min(0).max(1),
  hostId: external.nullable(), threadId: external.nullable(), requestId: external.nullable(),
  turnId: external.nullable(), queueId: external.nullable(),
  cursor: z.number().int().min(0).max(Number.MAX_SAFE_INTEGER),
  items: z.array(z.object({ id: z.string().max(128), text: z.string() }).strict()).max(1024),
  truncated: z.boolean(), correlationConflict: z.boolean(), nativeState: turnState.nullable(),
  usage: z.object({ input_tokens: z.number().int().min(0).max(Number.MAX_SAFE_INTEGER),
    output_tokens: z.number().int().min(0).max(Number.MAX_SAFE_INTEGER) }).strict().nullable(),
  lastObservation: observationSchema.nullable(),
}).strict().refine(value => value.items.reduce((sum, item) => sum + Buffer.byteLength(item.text), 0) <= LIMITS.output);
export type CommandRecord = z.infer<typeof stored>;
export type Importance = 'progress' | 'critical' | 'terminal';
type Row = { id: string; digest: string; state: string; reserve: number; terminal: number };
export type MappingCandidate = { id: string; thread_id: string; mapping: string; last_used_at: number };
export type MappingCursor = Pick<MappingCandidate, 'last_used_at' | 'id'>;
const RETENTION_MS = 90 * 24 * 60 * 60 * 1000;
type Meta = { binding: string; allocated: number; acknowledged: number; recovery_required: number };
export class RecoveryRequired extends Error { constructor() { super('local journal recovery required'); } }
export class JournalCapacity extends Error { constructor() { super('local journal capacity reached'); } }

export const mappingSchema = z.object({ sessionId: uuid, threadId: external, selection: selectionSchema,
  harness: harnessSchema, operationId: uuid, hostId: external.nullable() }).strict()
  .refine(mapping => mapping.harness.kind === 'unconfigured' || mapping.hostId !== null);
export type SessionMapping = z.infer<typeof mappingSchema>;
export class Journal {
  readonly db: Database.Database;
  constructor(db: Database.Database) {
    this.db = db;
    db.pragma('foreign_keys = ON');
    db.pragma('synchronous = FULL');
    if (db.pragma('synchronous', { simple: true }) !== 2) throw new Error('durable journal unavailable');
  }
  initialize(binding: Binding): void {
    const exact = bindingSchema.parse(binding);
    this.db.transaction(() => {
      const old = this.db.prepare('SELECT binding FROM journal_meta WHERE id=1').get() as { binding: string } | undefined;
      if (old) { if (old.binding !== JSON.stringify(exact)) throw new RecoveryRequired(); return; }
      const residue = this.db.prepare('SELECT (SELECT count(*) FROM commands)+(SELECT count(*) FROM outbox)+(SELECT count(*) FROM sessions) AS count').get() as { count: number };
      if (residue.count !== 0) throw new RecoveryRequired();
      this.db.prepare('INSERT INTO journal_meta(id,binding) VALUES(1,?)').run(JSON.stringify(exact));
    })();
  }
  private meta(): Meta {
    const meta = this.db.prepare('SELECT binding,allocated,acknowledged,recovery_required FROM journal_meta WHERE id=1').get() as Meta | undefined;
    if (!meta) throw new RecoveryRequired();
    bindingSchema.parse(JSON.parse(meta.binding));
    if (!Number.isSafeInteger(meta.allocated) || meta.acknowledged > meta.allocated || meta.acknowledged < 0) throw new RecoveryRequired();
    return meta;
  }
  initialized(): boolean { return Boolean(this.db.prepare('SELECT 1 FROM journal_meta WHERE id=1').get()); }
  binding(): Binding { return bindingSchema.parse(JSON.parse(this.meta().binding)); }
  validateBinding(binding: Binding): void { if (JSON.stringify(this.binding()) !== JSON.stringify(bindingSchema.parse(binding))) throw new RecoveryRequired(); }
  sequence(): number { return this.meta().allocated; }
  acknowledged(): number { return this.meta().acknowledged; }
  recoveryRequired(): boolean { return this.meta().recovery_required !== 0; }
  requireRecovery(): void { this.db.prepare('UPDATE journal_meta SET recovery_required=1 WHERE id=1').run(); }
  private row(id: string): Row | undefined {
    return this.db.prepare('SELECT id,digest,state,reserve,terminal FROM commands WHERE id=?').get(id) as Row | undefined;
  }
  get(id: string): CommandRecord | undefined {
    const row = this.row(id);
    if (!row) return undefined;
    const record = stored.parse(JSON.parse(row.state));
    if (record.operation.id !== row.id || record.operation.payload_digest !== row.digest
        || (record.stage === 'terminal') !== Boolean(row.terminal)
        || record.sdkInput !== (isEffect(record.operation.command) ? nativeInput(record.operation) : '')) throw new RecoveryRequired();
    return record;
  }
  private budget(): { used: number; reserved: number } {
    return this.db.prepare('SELECT COALESCE((SELECT sum(bytes) FROM outbox),0)+COALESCE((SELECT sum(bytes+reserve) FROM mcp_intents),0) AS used,COALESCE((SELECT sum(reserve) FROM commands),0) AS reserved').get() as { used: number; reserved: number };
  }
  private append(record: CommandRecord, observation: Observation, importance: Importance): void {
    const row = this.row(record.operation.id);
    if (!row || row.digest !== record.operation.payload_digest) throw new RecoveryRequired();
    const exact = observationSchema.parse(observation);
    if ((importance === 'terminal') !== (record.stage === 'terminal')) throw new RecoveryRequired();
    const meta = this.meta();
    if (meta.allocated >= Number.MAX_SAFE_INTEGER || observationSequence(exact) !== meta.allocated + 1) throw new JournalCapacity();
    const payload = JSON.stringify(exact); const bytes = Buffer.byteLength(payload);
    if (bytes > LIMITS.logical - 256) throw new JournalCapacity();
    const budget = this.budget();
    let reserve = row.reserve;
    if (importance === 'progress') {
      if (budget.used + budget.reserved + bytes > LIMITS.outbox) { this.write(record); return; }
    } else {
      if (bytes > reserve) throw new JournalCapacity();
      reserve = importance === 'terminal' ? 0 : reserve - bytes;
    }
    if (budget.used + bytes + budget.reserved - row.reserve + reserve > LIMITS.outbox) throw new JournalCapacity();
    this.db.prepare('INSERT INTO outbox(sequence,operation_id,payload,bytes) VALUES(?,?,?,?)').run(meta.allocated + 1, record.operation.id, payload, bytes);
    this.db.prepare('UPDATE journal_meta SET allocated=? WHERE id=1').run(meta.allocated + 1);
    record.lastObservation = exact;
    if (importance === 'terminal') this.db.prepare('UPDATE commands SET acknowledged_at=NULL WHERE id=?').run(record.operation.id);
    this.write(record, reserve);
  }
  private write(record: CommandRecord, reserve?: number): void {
    const exact = stored.parse(record);
    const old = this.row(exact.operation.id);
    if (!old || old.digest !== exact.operation.payload_digest) throw new RecoveryRequired();
    const previous = stored.parse(JSON.parse(old.state));
    if ((old.terminal && exact.stage !== 'terminal') || previous.sdkCalls > exact.sdkCalls
        || previous.sdkInput !== exact.sdkInput || previous.cursor > exact.cursor
        || (previous.correlationConflict && !exact.correlationConflict)
        || (previous.threadId !== null && previous.threadId !== exact.threadId)
        || (previous.requestId !== null && previous.requestId !== exact.requestId)
        || (previous.turnId !== null && previous.turnId !== exact.turnId)) throw new RecoveryRequired();
    const terminal = Number(exact.stage === 'terminal');
    this.db.prepare('UPDATE commands SET state=?,terminal=?,reserve=?,terminal_at=CASE WHEN ?=1 THEN COALESCE(terminal_at,?) ELSE NULL END WHERE id=?').run(JSON.stringify(exact),
      terminal, terminal ? 0 : (reserve ?? old.reserve), terminal, Date.now(), exact.operation.id);
  }
  admit(delivery: Delivery): { record: CommandRecord; fresh: boolean } {
    const operation = parseOperation(delivery.operation);
    return this.db.transaction(() => {
      const old = this.get(operation.id);
      if (old) {
        if (old.operation.payload_digest !== operation.payload_digest) throw new Error('operation identity conflict');
        return { record: old, fresh: false };
      }
      if (!this.canAdmit(operation)) throw new JournalCapacity();
      const record: CommandRecord = { operation, stage: 'received', sdkInput: isEffect(operation.command) ? nativeInput(operation) : '',
        sdkCalls: 0, hostId: null, threadId: null, requestId: null, turnId: null, queueId: null, cursor: 0,
        items: [], truncated: false, correlationConflict: false, nativeState: null, usage: null, lastObservation: null };
      this.db.prepare('INSERT INTO commands(id,digest,state,reserve,terminal) VALUES(?,?,?,?,0)').run(operation.id, operation.payload_digest,
        JSON.stringify(record), LIMITS.terminalReserve);
      if ('session_id' in operation.command) this.db.prepare('UPDATE sessions SET last_used_at=max(last_used_at,?) WHERE id=?').run(Date.now(), operation.command.session_id);
      this.append(record, { kind: 'agent', observation: { sequence: this.sequence() + 1,
        operation_id: operation.id, evidence: { kind: 'receipt' } } }, 'critical');
      // Read-only diagnostics can be repeated after a lost receipt. Their absence
      // is not evidence that a native mutation lost its durable intent.
      const mutation = !['refresh_catalog', 'inspect'].includes(operation.command.kind);
      if (mutation && (delivery.mode === 'reconcile' || operation.outcome !== null || operation.dispatch.kind !== 'not_dispatched')) {
        this.requireRecovery();
        record.stage = 'uncertain';
        this.append(record, { kind: 'agent', observation: { sequence: this.sequence() + 1,
          operation_id: operation.id, evidence: { kind: 'uncertain' } } }, 'critical');
      }
      return { record, fresh: true };
    })();
  }
  save(record: CommandRecord): void {
    this.db.transaction(() => {
      if (record.stage === 'terminal' && !this.row(record.operation.id)?.terminal) throw new RecoveryRequired();
      this.write(record);
    })();
  }
  evidence(record: CommandRecord, evidence: Evidence, importance: Importance): void {
    this.db.transaction(() => {
      evidenceSchema.parse(evidence);
      if (evidence.kind === 'refused' && record.sdkCalls !== 0) throw new RecoveryRequired();
      this.append(record, { kind: 'agent', observation: { sequence: this.sequence() + 1,
        operation_id: record.operation.id, evidence } }, importance);
    })();
  }
  beginIntent(record: CommandRecord): void {
    this.db.transaction(() => {
      const current = this.get(record.operation.id);
      if (!current || current.stage !== 'received' || current.sdkCalls !== 0) throw new RecoveryRequired();
      record.stage = 'intent'; record.sdkCalls = 1;
      this.evidence(record, { kind: 'submission_intent' }, 'critical');
    })();
  }
  catalog(record: CommandRecord, catalog: Catalog): void {
    this.db.transaction(() => {
      record.stage = 'terminal';
      this.append(record, { kind: 'catalog', sequence: this.sequence() + 1,
        operation_id: record.operation.id, catalog: catalogSchema.parse(catalog) }, 'terminal');
    })();
  }
  control(record: CommandRecord, thread_id: string, evidence: Extract<Observation, { kind: 'control' }>['evidence']): void {
    this.db.transaction(() => {
      record.stage = 'terminal';
      this.append(record, { kind: 'control', sequence: this.sequence() + 1,
        operation_id: record.operation.id, thread_id, evidence }, 'terminal');
    })();
  }
  bindSession(mapping: SessionMapping): void {
    const exact = mappingSchema.parse(mapping);
    this.db.transaction(() => {
      if (exact.selection.machine_id !== this.binding().machine_id) throw new RecoveryRequired();
      const old = this.session(exact.sessionId);
      if (old) {
        const {operationId:_priorOperation,...prior}=old; const {operationId:_newOperation,...next}=exact;
        if (JSON.stringify(prior) !== JSON.stringify(next)) throw new RecoveryRequired();
        this.db.prepare('UPDATE sessions SET last_used_at=max(last_used_at+1,?) WHERE id=?').run(Date.now(), exact.sessionId);
        return;
      }
      const count = this.db.prepare('SELECT count(*) AS count FROM sessions').get() as {count:number};
      if (count.count >= LIMITS.history) throw new JournalCapacity();
      this.db.prepare('INSERT INTO sessions(id,mapping,last_used_at) VALUES(?,?,?)').run(exact.sessionId,JSON.stringify(exact),Date.now());
    })();
  }
  session(id: string): SessionMapping | undefined {
    const row = this.db.prepare('SELECT mapping FROM sessions WHERE id=?').get(id) as {mapping:string}|undefined;
    return row ? mappingSchema.parse(JSON.parse(row.mapping)) : undefined;
  }
  thread(id: string): SessionMapping | undefined {
    const row = this.db.prepare('SELECT mapping FROM sessions WHERE thread_id=?').get(id) as {mapping:string}|undefined;
    return row ? mappingSchema.parse(JSON.parse(row.mapping)) : undefined;
  }
  referenceOperation(session: string, operation: string): void {
    const mapping = this.session(session); if (!mapping) throw new RecoveryRequired();
    mapping.operationId = uuid.parse(operation);
    this.db.prepare('UPDATE sessions SET mapping=?,last_used_at=max(last_used_at+1,?) WHERE id=?').run(JSON.stringify(mapping),Date.now(),session);
  }
  bundleReferences(): Set<string> {
    const references = new Set<string>();
    const rows = this.db.prepare('SELECT mapping FROM sessions LIMIT ?').all(LIMITS.history+1) as {mapping:string}[];
    if(rows.length>LIMITS.history) throw new RecoveryRequired();
    for(const row of rows) { const mapping=mappingSchema.parse(JSON.parse(row.mapping)); if(mapping.harness.kind==='agent') references.add(mapping.harness.digest); }
    const commands=this.db.prepare('SELECT id FROM commands LIMIT ?').all(LIMITS.history+1) as {id:string}[];
    if(commands.length>LIMITS.history) throw new RecoveryRequired();
    for(const row of commands) { const command=this.get(row.id)!.operation.command; if(command.kind==='create' && command.harness.kind==='agent') references.add(command.harness.digest); }
    return references;
  }
  hasMappedBundle(digest: string): boolean {
    const rows=this.db.prepare('SELECT mapping FROM sessions LIMIT ?').all(LIMITS.history+1) as {mapping:string}[];
    if(rows.length>LIMITS.history) throw new RecoveryRequired();
    return rows.some(row=>{const mapping=mappingSchema.parse(JSON.parse(row.mapping));return mapping.harness.kind==='agent' && mapping.harness.digest===digest;});
  }
  hasDispatchedBundle(digest:string):boolean {
    const rows=this.db.prepare('SELECT id FROM commands LIMIT ?').all(LIMITS.history+1) as {id:string}[];
    if(rows.length>LIMITS.history) throw new RecoveryRequired();
    return rows.some(row=>{const record=this.get(row.id)!;const command=record.operation.command;
      return record.sdkCalls!==0 && command.kind==='create' && command.harness.kind==='agent' && command.harness.digest===digest;});
  }
  submission(record: CommandRecord, selection: Selection, state: 'accepted'|'queued'|'missing'): void {
    this.db.transaction(() => {
      const command=record.operation.command;
      if(!record.threadId || !('session_id' in command)) throw new RecoveryRequired();
      if(command.kind==='create') this.bindSession({sessionId:command.session_id,threadId:record.threadId,selection,
        harness:command.harness,operationId:record.operation.id,hostId:command.harness.kind==='agent'?record.hostId:null});
      else if (!this.session(command.session_id) || this.session(command.session_id)!.threadId!==record.threadId) throw new RecoveryRequired();
      record.stage=state==='missing'?'uncertain':'observing';
      this.evidence(record,{kind:'submission',thread_id:record.threadId,state},'critical');
    })();
  }
  pending(): CommandRecord[] {
    const rows = this.db.prepare('SELECT id FROM commands WHERE terminal=0 ORDER BY rowid LIMIT ?').all(LIMITS.queued) as { id: string }[];
    return rows.map(row => this.get(row.id)!);
  }
  capacity(): { effects: number; queued: number } {
    const pending = this.pending();
    return { queued: pending.length, effects: pending.filter(record => isEffect(record.operation.command)).length };
  }
  outbox(after = this.acknowledged()): Observation[] {
    const rows = this.db.prepare('SELECT payload FROM outbox WHERE sequence>? ORDER BY sequence LIMIT ?').all(after, LIMITS.queued) as { payload: string }[];
    return rows.map(row => observationSchema.parse(JSON.parse(row.payload)));
  }
  acknowledge(through: number): void {
    this.db.transaction(() => {
      const meta = this.meta();
      if (!Number.isSafeInteger(through) || through < meta.acknowledged || through > meta.allocated) throw new RecoveryRequired();
      this.db.prepare('UPDATE journal_meta SET acknowledged=? WHERE id=1').run(through);
      this.db.prepare("UPDATE commands SET acknowledged_at=COALESCE(acknowledged_at,?) WHERE terminal=1 AND acknowledged_at IS NULL AND COALESCE(json_extract(state,'$.lastObservation.sequence'),json_extract(state,'$.lastObservation.observation.sequence'))<=?").run(Date.now(), through);
      this.db.prepare('DELETE FROM outbox WHERE sequence<=?').run(through);
    })();
  }
  canAdmit(operation: Operation): boolean {
    if (this.row(operation.id)) return true;
    const counts = this.diagnostics();
    return !counts.newCommandsPaused && (operation.command.kind !== 'create' || !counts.newSessionsPaused);
  }
  diagnostics(): { commands: number; mappings: number; outboxBytes: number; reservedBytes: number;
    newCommandsPaused: boolean; newSessionsPaused: boolean } {
    const counts = this.db.prepare('SELECT (SELECT count(*) FROM commands) AS commands,(SELECT count(*) FROM sessions) AS mappings').get() as { commands: number; mappings: number };
    const budget = this.budget();
    const newCommandsPaused = counts.commands >= LIMITS.history || budget.used + budget.reserved + LIMITS.terminalReserve > LIMITS.outbox;
    return { ...counts, outboxBytes: budget.used, reservedBytes: budget.reserved,
      newCommandsPaused, newSessionsPaused: newCommandsPaused || counts.mappings >= LIMITS.history };
  }
  compactAcknowledged(): number {
    return this.db.transaction(() => this.db.prepare(`DELETE FROM commands WHERE id IN (
      SELECT command.id FROM commands AS command WHERE command.terminal=1
        AND command.terminal_at<? AND command.acknowledged_at<?
        AND (julianday(json_extract(command.state,'$.operation.deadline'))-2440587.5)*86400000<?
        AND NOT EXISTS(SELECT 1 FROM outbox WHERE operation_id=command.id)
        AND NOT EXISTS(SELECT 1 FROM mcp_intents WHERE operation_id=command.id AND resolved=0)
      ORDER BY command.terminal_at,command.id LIMIT 128)`).run(Date.now() - RETENTION_MS, Date.now() - RETENTION_MS, Date.now()).changes)();
  }
  mappingCandidates(after?: MappingCursor): MappingCandidate[] {
    return this.db.prepare(`SELECT session.id,session.thread_id,session.mapping,session.last_used_at
      FROM sessions AS session WHERE session.last_used_at<?
        AND (? IS NULL OR (session.last_used_at,session.id)>(?,?))
        AND NOT EXISTS(SELECT 1 FROM commands WHERE commands.session_id=session.id)
      ORDER BY session.last_used_at,session.id LIMIT 100`).all(Date.now() - RETENTION_MS,
        after?.last_used_at ?? null, after?.last_used_at ?? null, after?.id ?? null) as MappingCandidate[];
  }
  retireMappings(candidates: readonly MappingCandidate[], retired: readonly string[]): number {
    if (candidates.length > 100 || retired.length > 100 || new Set(retired).size !== retired.length) throw new RecoveryRequired();
    const exact = new Map(candidates.map(candidate => [candidate.id, candidate]));
    if (retired.some(id => !exact.has(id))) throw new RecoveryRequired();
    return this.db.transaction(() => {
      let removed = 0;
      for (const id of retired) {
        const candidate = exact.get(id)!;
        removed += this.db.prepare(`DELETE FROM sessions WHERE id=? AND thread_id=? AND mapping=? AND last_used_at=? AND last_used_at<?
          AND NOT EXISTS(SELECT 1 FROM commands WHERE commands.session_id=sessions.id)`)
          .run(uuid.parse(id), candidate.thread_id, candidate.mapping, candidate.last_used_at, Date.now() - RETENTION_MS).changes;
      }
      return removed;
    })();
  }

}
