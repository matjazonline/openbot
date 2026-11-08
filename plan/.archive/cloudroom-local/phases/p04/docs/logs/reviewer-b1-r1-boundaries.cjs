var __create = Object.create;
var __defProp = Object.defineProperty;
var __getOwnPropDesc = Object.getOwnPropertyDescriptor;
var __getOwnPropNames = Object.getOwnPropertyNames;
var __getProtoOf = Object.getPrototypeOf;
var __hasOwnProp = Object.prototype.hasOwnProperty;
var __copyProps = (to, from, except, desc) => {
  if (from && typeof from === "object" || typeof from === "function") {
    for (let key of __getOwnPropNames(from))
      if (!__hasOwnProp.call(to, key) && key !== except)
        __defProp(to, key, { get: () => from[key], enumerable: !(desc = __getOwnPropDesc(from, key)) || desc.enumerable });
  }
  return to;
};
var __toESM = (mod, isNodeMode, target) => (target = mod != null ? __create(__getProtoOf(mod)) : {}, __copyProps(
  // If the importer is in node compatibility mode or this is not an ESM
  // file that has been converted to a CommonJS file using a Babel-
  // compatible transform (i.e. "__esModule" has not been set), then set
  // "default" to the CommonJS "module.exports" for node compatibility.
  isNodeMode || !mod || !mod.__esModule ? __defProp(target, "default", { value: mod, enumerable: true }) : target,
  mod
));

// plan/cloudroom-local/phases/p04/docs/logs/reviewer-b1-r1-boundaries.ts
var import_strict = __toESM(require("node:assert/strict"));

// integrations/cloudroom-plugin/src/connector.ts
var import_ws = __toESM(require("/Users/mac03/dev/openbot/integrations/cloudroom-plugin/node_modules/ws/index.js"), 1);
var import_promises = require("node:timers/promises");

// integrations/cloudroom-plugin/src/enrollment.ts
var import_node_crypto2 = require("node:crypto");
var import_zod3 = require("/Users/mac03/dev/openbot/integrations/cloudroom-plugin/node_modules/zod/index.cjs");

// integrations/cloudroom-plugin/src/journal.ts
var import_zod2 = require("/Users/mac03/dev/openbot/integrations/cloudroom-plugin/node_modules/zod/index.cjs");

// integrations/cloudroom-plugin/src/protocol.ts
var import_node_crypto = require("node:crypto");
var import_zod = require("/Users/mac03/dev/openbot/integrations/cloudroom-plugin/node_modules/zod/index.cjs");
var LIMITS = Object.freeze({
  prompt: 32 * 1024,
  output: 256 * 1024,
  frame: 64 * 1024,
  chunk: 16 * 1024,
  logical: 2 * 1024 * 1024,
  assemblyMs: 3e4,
  outbox: 64 * 1024 * 1024,
  effects: 8,
  queued: 32,
  controls: 8,
  projects: 128,
  providers: 32,
  models: 256,
  page: 100,
  heartbeatMs: 15e3,
  ioMs: 5e3,
  handshakeMs: 1e4,
  history: 4096,
  terminalReserve: 3 * 1024 * 1024
});
var sequence = import_zod.z.number().int().min(0).max(Number.MAX_SAFE_INTEGER);
var positive = sequence.min(1);
var uuid = import_zod.z.string().regex(/^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/).refine((value) => value !== "00000000-0000-0000-0000-000000000000");
var external = import_zod.z.string().regex(/^[A-Za-z0-9_.:-]{1,128}$/);
var model = import_zod.z.string().refine((value) => value.length > 0 && Buffer.byteLength(value) <= 128 && !/[\p{White_Space}\p{Cc}]/u.test(value));
var text = (maximum) => import_zod.z.string().refine((value) => Buffer.byteLength(value) <= maximum && !value.includes("\0"));
var prompt = text(LIMITS.prompt).refine((value) => value.trim().length > 0);
var digestSchema = import_zod.z.string().regex(/^[0-9a-f]{64}$/);
var reasoning = import_zod.z.enum(["none", "low", "medium", "high", "xhigh", "max", "ultra", "ultracode"]);
var provider = import_zod.z.enum(["claude-code", "codex", "acp-opencode"]);
var policySchema = import_zod.z.object({
  workspace: import_zod.z.literal("project_directory"),
  permissions: import_zod.z.literal("provider_default"),
  reasoning
}).strict();
var selectionSchema = import_zod.z.object({
  machine_id: uuid,
  project_id: external,
  provider,
  model,
  policy: policySchema
}).strict();
var commandSchema = import_zod.z.discriminatedUnion("kind", [
  import_zod.z.object({ kind: import_zod.z.literal("create"), session_id: uuid, selection: selectionSchema, prompt }).strict(),
  import_zod.z.object({ kind: import_zod.z.literal("send"), session_id: uuid, thread_id: external, prompt }).strict(),
  import_zod.z.object({ kind: import_zod.z.literal("inspect"), session_id: uuid, thread_id: external }).strict(),
  import_zod.z.object({ kind: import_zod.z.literal("stop_thread"), session_id: uuid, thread_id: external }).strict(),
  import_zod.z.object({ kind: import_zod.z.literal("archive_thread"), session_id: uuid, thread_id: external }).strict(),
  import_zod.z.object({ kind: import_zod.z.literal("refresh_catalog") }).strict()
]);
var turnState = import_zod.z.enum(["queued", "running", "waiting_for_input", "completed", "failed", "stopped", "missing"]);
var dispatch = import_zod.z.discriminatedUnion("kind", [
  import_zod.z.object({ kind: import_zod.z.literal("not_dispatched") }).strict(),
  import_zod.z.object({ kind: import_zod.z.literal("accepted_locally"), sequence: positive }).strict(),
  import_zod.z.object({ kind: import_zod.z.literal("submission_intent"), sequence: positive }).strict(),
  import_zod.z.object({ kind: import_zod.z.literal("native_identity"), sequence: positive, thread_id: external, turn_id: external.nullable() }).strict(),
  import_zod.z.object({ kind: import_zod.z.literal("catalog_snapshot"), sequence: positive }).strict()
]);
var deadline = import_zod.z.string().refine((value) => {
  const matched = /^(\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d)(?:\.(\d{3}|\d{6}))?Z$/.exec(value);
  if (!matched || !Number.isFinite(Date.parse(value))) return false;
  if (matched[2] === "000" || matched[2]?.length === 6 && matched[2].endsWith("000")) return false;
  return new Date(value).toISOString().slice(0, 19) === matched[1];
});
var operationSchema = import_zod.z.object({
  id: uuid,
  command: commandSchema,
  deadline,
  payload_digest: digestSchema,
  dispatch,
  outcome: import_zod.z.enum(["succeeded", "failed", "cancelled", "expired", "uncertain"]).nullable()
}).strict();
var identitySchema = import_zod.z.object({
  installation_id: uuid,
  credential_generation: positive,
  connection_generation: positive
}).strict();
var catalogSchema = import_zod.z.object({
  projects: import_zod.z.array(external).max(LIMITS.projects),
  providers: import_zod.z.array(import_zod.z.object({ provider, models: import_zod.z.array(import_zod.z.object({
    model,
    reasoning_levels: import_zod.z.array(reasoning).max(8)
  }).strict()).max(LIMITS.models) }).strict()).max(LIMITS.providers)
}).strict();
var refusalSchema = import_zod.z.enum([
  "project_unavailable",
  "provider_unavailable",
  "model_unavailable",
  "reasoning_unavailable",
  "capacity",
  "expired",
  "recovery_required"
]);
var evidenceSchema = import_zod.z.discriminatedUnion("kind", [
  import_zod.z.object({ kind: import_zod.z.literal("receipt") }).strict(),
  import_zod.z.object({ kind: import_zod.z.literal("submission_intent") }).strict(),
  import_zod.z.object({ kind: import_zod.z.literal("submission"), thread_id: external, state: import_zod.z.enum(["accepted", "queued", "missing"]) }).strict(),
  import_zod.z.object({ kind: import_zod.z.literal("refused"), reason: refusalSchema }).strict(),
  import_zod.z.object({ kind: import_zod.z.literal("uncertain") }).strict(),
  import_zod.z.object({
    kind: import_zod.z.literal("turn"),
    thread_id: external,
    turn_id: external,
    state: turnState,
    output: text(LIMITS.output).nullable(),
    truncated: import_zod.z.boolean(),
    usage: import_zod.z.object({ input_tokens: sequence, output_tokens: sequence }).strict().nullable()
  }).strict()
]);
var observationSchema = import_zod.z.discriminatedUnion("kind", [
  import_zod.z.object({ kind: import_zod.z.literal("agent"), observation: import_zod.z.object({ sequence: positive, operation_id: uuid, evidence: evidenceSchema }).strict() }).strict(),
  import_zod.z.object({ kind: import_zod.z.literal("catalog"), sequence: positive, operation_id: uuid, catalog: catalogSchema }).strict(),
  import_zod.z.object({
    kind: import_zod.z.literal("control"),
    sequence: positive,
    operation_id: uuid,
    thread_id: external,
    evidence: import_zod.z.discriminatedUnion("kind", [
      import_zod.z.object({ kind: import_zod.z.literal("snapshot"), state: turnState }).strict(),
      import_zod.z.object({ kind: import_zod.z.literal("stop_accepted") }).strict(),
      import_zod.z.object({ kind: import_zod.z.literal("archive_accepted") }).strict()
    ])
  }).strict()
]);
var serverSchema = import_zod.z.discriminatedUnion("kind", [
  import_zod.z.object({ kind: import_zod.z.literal("welcome"), welcome: import_zod.z.object({
    identity: identitySchema,
    acknowledged_sequence: sequence,
    recovery_required: import_zod.z.boolean()
  }).strict() }).strict(),
  import_zod.z.object({
    kind: import_zod.z.literal("command"),
    identity: identitySchema,
    delivery: import_zod.z.object({ operation: operationSchema, mode: import_zod.z.enum(["dispatch", "reconcile"]) }).strict()
  }).strict(),
  import_zod.z.object({ kind: import_zod.z.literal("acknowledged"), identity: identitySchema, through: sequence }).strict(),
  import_zod.z.object({ kind: import_zod.z.literal("ready"), identity: identitySchema, ready: import_zod.z.boolean() }).strict()
]);
var sha256 = (bytes) => (0, import_node_crypto.createHash)("sha256").update(bytes).digest("hex");
var isEffect = (command) => command.kind === "create" || command.kind === "send";
var targetThread = (command) => "thread_id" in command ? command.thread_id : void 0;
var observationSequence = (observation) => observation.kind === "agent" ? observation.observation.sequence : observation.sequence;
function commandDigest(command, exactDeadline) {
  return sha256(JSON.stringify([commandSchema.parse(command), deadline.parse(exactDeadline)]));
}
function parseOperation(value) {
  const operation2 = operationSchema.parse(value);
  if (commandDigest(operation2.command, operation2.deadline) !== operation2.payload_digest) throw new Error("command digest mismatch");
  const evidence2 = operation2.dispatch;
  if (evidence2.kind === "native_identity" && targetThread(operation2.command) && evidence2.thread_id !== targetThread(operation2.command)) throw new Error("command thread mismatch");
  if (evidence2.kind === "catalog_snapshot" !== (operation2.command.kind === "refresh_catalog") && evidence2.kind === "catalog_snapshot") throw new Error("invalid catalog receipt");
  if (operation2.command.kind === "refresh_catalog" && evidence2.kind === "native_identity") throw new Error("invalid native receipt");
  if (operation2.outcome === "succeeded" && !(evidence2.kind === "catalog_snapshot" || evidence2.kind === "native_identity" && (!isEffect(operation2.command) || evidence2.turn_id !== null))) {
    throw new Error("success without native evidence");
  }
  return operation2;
}
function parseServer(value) {
  const parsed = serverSchema.parse(value);
  if (parsed.kind === "command") parsed.delivery.operation = parseOperation(parsed.delivery.operation);
  return parsed;
}
function nativeInput(operation2) {
  if (!("prompt" in operation2.command)) throw new Error("not an effect");
  const header = `[Openbot operation ${operation2.id}]
`;
  if (Buffer.byteLength(header) !== 57) throw new Error("invalid operation identity");
  return header + operation2.command.prompt;
}
function encodeFrames(message) {
  const bytes = Buffer.from(JSON.stringify(message));
  if (!bytes.length || bytes.length > LIMITS.logical) throw new Error("logical message bound");
  const transfer_id = (0, import_node_crypto.randomUUID)();
  const digest = sha256(bytes);
  const frames = [];
  for (let index = 0; index * LIMITS.chunk < bytes.length; index++) {
    frames.push(JSON.stringify({
      version: 1,
      transfer_id,
      index,
      parts: Math.ceil(bytes.length / LIMITS.chunk),
      total_bytes: bytes.length,
      digest,
      data: bytes.subarray(index * LIMITS.chunk, (index + 1) * LIMITS.chunk).toString("base64")
    }));
  }
  return frames;
}
var chunkSchema = import_zod.z.object({
  version: import_zod.z.literal(1),
  transfer_id: uuid,
  index: sequence,
  parts: positive.max(128),
  total_bytes: positive.max(LIMITS.logical),
  digest: digestSchema,
  data: import_zod.z.string().max(Math.ceil(LIMITS.chunk / 3) * 4)
}).strict();
var FrameAssembler = class {
  pending;
  now;
  constructor(now = () => performance.now()) {
    this.now = now;
  }
  expire() {
    if (this.pending && this.now() >= this.pending.expires) throw new Error("chunk assembly expired");
  }
  push(frame) {
    if (Buffer.byteLength(frame) > LIMITS.frame) throw new Error("physical frame bound");
    this.expire();
    const source = typeof frame === "string" ? frame : new TextDecoder("utf-8", { fatal: true }).decode(frame);
    const c = chunkSchema.parse(JSON.parse(source));
    if (c.parts !== Math.ceil(c.total_bytes / LIMITS.chunk) || c.index >= c.parts) throw new Error("invalid chunk bounds");
    const data = Buffer.from(c.data, "base64");
    const expected = c.index + 1 === c.parts ? c.total_bytes - c.index * LIMITS.chunk : LIMITS.chunk;
    if (data.length !== expected || data.toString("base64") !== c.data) throw new Error("invalid chunk encoding");
    this.pending ??= {
      transfer: c.transfer_id,
      next: 0,
      parts: c.parts,
      size: c.total_bytes,
      digest: c.digest,
      chunks: [],
      expires: this.now() + LIMITS.assemblyMs
    };
    const p = this.pending;
    if (p.next !== c.index || p.transfer !== c.transfer_id || p.parts !== c.parts || p.size !== c.total_bytes || p.digest !== c.digest) {
      throw new Error("interleaved or reused chunk");
    }
    p.chunks.push(data);
    p.next++;
    if (p.next !== p.parts) return void 0;
    this.pending = void 0;
    const bytes = Buffer.concat(p.chunks, p.size);
    if (sha256(bytes) !== p.digest) throw new Error("chunk digest mismatch");
    return JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(bytes));
  }
};

// integrations/cloudroom-plugin/src/journal.ts
var JOURNAL_SCHEMA = [
  `CREATE TABLE journal_meta(id INTEGER PRIMARY KEY CHECK(id=1),binding TEXT NOT NULL,
    allocated INTEGER NOT NULL DEFAULT 0 CHECK(allocated>=0 AND allocated<=9007199254740991),
    acknowledged INTEGER NOT NULL DEFAULT 0 CHECK(acknowledged>=0 AND acknowledged<=allocated),
    recovery_required INTEGER NOT NULL DEFAULT 0 CHECK(recovery_required IN (0,1)))`,
  `CREATE TABLE commands(id TEXT PRIMARY KEY,digest TEXT NOT NULL,state TEXT NOT NULL,
    reserve INTEGER NOT NULL CHECK(reserve>=0),terminal INTEGER NOT NULL CHECK(terminal IN (0,1)))`,
  `CREATE TABLE sessions(id TEXT PRIMARY KEY,thread_id TEXT NOT NULL UNIQUE,selection TEXT NOT NULL)`,
  `CREATE TABLE outbox(sequence INTEGER PRIMARY KEY,operation_id TEXT NOT NULL REFERENCES commands(id),
    payload TEXT NOT NULL,bytes INTEGER NOT NULL CHECK(bytes>0))`
];
var bindingSchema = import_zod2.z.object({
  origin: import_zod2.z.string().url(),
  installation_id: uuid,
  journal_id: uuid,
  machine_id: uuid,
  company_id: uuid,
  credential_generation: import_zod2.z.number().int().positive().max(Number.MAX_SAFE_INTEGER)
}).strict();
var stored = import_zod2.z.object({
  operation: import_zod2.z.unknown().transform(parseOperation),
  stage: import_zod2.z.enum(["received", "intent", "observing", "uncertain", "terminal"]),
  sdkInput: import_zod2.z.string().max(LIMITS.prompt + 57),
  sdkCalls: import_zod2.z.number().int().min(0).max(1),
  threadId: external.nullable(),
  requestId: external.nullable(),
  turnId: external.nullable(),
  queueId: external.nullable(),
  cursor: import_zod2.z.number().int().min(0).max(Number.MAX_SAFE_INTEGER),
  items: import_zod2.z.array(import_zod2.z.object({ id: import_zod2.z.string().max(128), text: import_zod2.z.string() }).strict()).max(1024),
  truncated: import_zod2.z.boolean(),
  correlationConflict: import_zod2.z.boolean(),
  nativeState: turnState.nullable(),
  usage: import_zod2.z.object({
    input_tokens: import_zod2.z.number().int().min(0).max(Number.MAX_SAFE_INTEGER),
    output_tokens: import_zod2.z.number().int().min(0).max(Number.MAX_SAFE_INTEGER)
  }).strict().nullable(),
  lastObservation: observationSchema.nullable()
}).strict().refine((value) => value.items.reduce((sum, item) => sum + Buffer.byteLength(item.text), 0) <= LIMITS.output);
var RecoveryRequired = class extends Error {
  constructor() {
    super("local journal recovery required");
  }
};
var JournalCapacity = class extends Error {
  constructor() {
    super("local journal capacity reached");
  }
};
var Journal = class {
  db;
  constructor(db) {
    this.db = db;
    db.pragma("foreign_keys = ON");
    db.pragma("synchronous = FULL");
    if (db.pragma("synchronous", { simple: true }) !== 2) throw new Error("durable journal unavailable");
  }
  initialize(binding) {
    const exact = bindingSchema.parse(binding);
    this.db.transaction(() => {
      const old = this.db.prepare("SELECT binding FROM journal_meta WHERE id=1").get();
      if (old) {
        if (old.binding !== JSON.stringify(exact)) throw new RecoveryRequired();
        return;
      }
      const residue = this.db.prepare("SELECT (SELECT count(*) FROM commands)+(SELECT count(*) FROM outbox)+(SELECT count(*) FROM sessions) AS count").get();
      if (residue.count !== 0) throw new RecoveryRequired();
      this.db.prepare("INSERT INTO journal_meta(id,binding) VALUES(1,?)").run(JSON.stringify(exact));
    })();
  }
  meta() {
    const meta = this.db.prepare("SELECT binding,allocated,acknowledged,recovery_required FROM journal_meta WHERE id=1").get();
    if (!meta) throw new RecoveryRequired();
    bindingSchema.parse(JSON.parse(meta.binding));
    if (!Number.isSafeInteger(meta.allocated) || meta.acknowledged > meta.allocated || meta.acknowledged < 0) throw new RecoveryRequired();
    return meta;
  }
  initialized() {
    return Boolean(this.db.prepare("SELECT 1 FROM journal_meta WHERE id=1").get());
  }
  binding() {
    return bindingSchema.parse(JSON.parse(this.meta().binding));
  }
  validateBinding(binding) {
    if (JSON.stringify(this.binding()) !== JSON.stringify(bindingSchema.parse(binding))) throw new RecoveryRequired();
  }
  sequence() {
    return this.meta().allocated;
  }
  acknowledged() {
    return this.meta().acknowledged;
  }
  recoveryRequired() {
    return this.meta().recovery_required !== 0;
  }
  requireRecovery() {
    this.db.prepare("UPDATE journal_meta SET recovery_required=1 WHERE id=1").run();
  }
  row(id) {
    return this.db.prepare("SELECT id,digest,state,reserve,terminal FROM commands WHERE id=?").get(id);
  }
  get(id) {
    const row = this.row(id);
    if (!row) return void 0;
    const record = stored.parse(JSON.parse(row.state));
    if (record.operation.id !== row.id || record.operation.payload_digest !== row.digest || record.stage === "terminal" !== Boolean(row.terminal) || record.sdkInput !== (isEffect(record.operation.command) ? nativeInput(record.operation) : "")) throw new RecoveryRequired();
    return record;
  }
  budget() {
    return this.db.prepare("SELECT COALESCE((SELECT sum(bytes) FROM outbox),0) AS used,COALESCE((SELECT sum(reserve) FROM commands),0) AS reserved").get();
  }
  append(record, observation, importance) {
    const row = this.row(record.operation.id);
    if (!row || row.digest !== record.operation.payload_digest) throw new RecoveryRequired();
    const exact = observationSchema.parse(observation);
    if (importance === "terminal" !== (record.stage === "terminal")) throw new RecoveryRequired();
    const meta = this.meta();
    if (meta.allocated >= Number.MAX_SAFE_INTEGER || observationSequence(exact) !== meta.allocated + 1) throw new JournalCapacity();
    const payload = JSON.stringify(exact);
    const bytes = Buffer.byteLength(payload);
    if (bytes > LIMITS.logical - 256) throw new JournalCapacity();
    const budget = this.budget();
    let reserve = row.reserve;
    if (importance === "progress") {
      if (budget.used + budget.reserved + bytes > LIMITS.outbox) {
        this.write(record);
        return;
      }
    } else {
      if (bytes > reserve) throw new JournalCapacity();
      reserve = importance === "terminal" ? 0 : reserve - bytes;
    }
    if (budget.used + bytes + budget.reserved - row.reserve + reserve > LIMITS.outbox) throw new JournalCapacity();
    this.db.prepare("INSERT INTO outbox(sequence,operation_id,payload,bytes) VALUES(?,?,?,?)").run(meta.allocated + 1, record.operation.id, payload, bytes);
    this.db.prepare("UPDATE journal_meta SET allocated=? WHERE id=1").run(meta.allocated + 1);
    record.lastObservation = exact;
    this.write(record, reserve);
  }
  write(record, reserve) {
    const exact = stored.parse(record);
    const old = this.row(exact.operation.id);
    if (!old || old.digest !== exact.operation.payload_digest) throw new RecoveryRequired();
    const previous = stored.parse(JSON.parse(old.state));
    if (old.terminal && exact.stage !== "terminal" || previous.sdkCalls > exact.sdkCalls || previous.sdkInput !== exact.sdkInput || previous.cursor > exact.cursor || previous.correlationConflict && !exact.correlationConflict || previous.threadId !== null && previous.threadId !== exact.threadId || previous.requestId !== null && previous.requestId !== exact.requestId || previous.turnId !== null && previous.turnId !== exact.turnId) throw new RecoveryRequired();
    this.db.prepare("UPDATE commands SET state=?,terminal=?,reserve=? WHERE id=?").run(
      JSON.stringify(exact),
      Number(exact.stage === "terminal"),
      exact.stage === "terminal" ? 0 : reserve ?? old.reserve,
      exact.operation.id
    );
  }
  admit(delivery) {
    const operation2 = parseOperation(delivery.operation);
    return this.db.transaction(() => {
      const old = this.get(operation2.id);
      if (old) {
        if (old.operation.payload_digest !== operation2.payload_digest) throw new Error("operation identity conflict");
        return { record: old, fresh: false };
      }
      const count = this.db.prepare("SELECT count(*) AS count FROM commands").get();
      const budget = this.budget();
      if (count.count >= LIMITS.history || budget.used + budget.reserved + LIMITS.terminalReserve > LIMITS.outbox) throw new JournalCapacity();
      const record = {
        operation: operation2,
        stage: "received",
        sdkInput: isEffect(operation2.command) ? nativeInput(operation2) : "",
        sdkCalls: 0,
        threadId: null,
        requestId: null,
        turnId: null,
        queueId: null,
        cursor: 0,
        items: [],
        truncated: false,
        correlationConflict: false,
        nativeState: null,
        usage: null,
        lastObservation: null
      };
      this.db.prepare("INSERT INTO commands(id,digest,state,reserve,terminal) VALUES(?,?,?,?,0)").run(
        operation2.id,
        operation2.payload_digest,
        JSON.stringify(record),
        LIMITS.terminalReserve
      );
      this.append(record, { kind: "agent", observation: {
        sequence: this.sequence() + 1,
        operation_id: operation2.id,
        evidence: { kind: "receipt" }
      } }, "critical");
      if (delivery.mode === "reconcile" || operation2.outcome !== null || operation2.dispatch.kind !== "not_dispatched") {
        this.requireRecovery();
        record.stage = "uncertain";
        this.append(record, { kind: "agent", observation: {
          sequence: this.sequence() + 1,
          operation_id: operation2.id,
          evidence: { kind: "uncertain" }
        } }, "critical");
      }
      return { record, fresh: true };
    })();
  }
  save(record) {
    this.db.transaction(() => {
      if (record.stage === "terminal" && !this.row(record.operation.id)?.terminal) throw new RecoveryRequired();
      this.write(record);
    })();
  }
  evidence(record, evidence2, importance) {
    this.db.transaction(() => {
      evidenceSchema.parse(evidence2);
      if (evidence2.kind === "refused" && record.sdkCalls !== 0) throw new RecoveryRequired();
      this.append(record, { kind: "agent", observation: {
        sequence: this.sequence() + 1,
        operation_id: record.operation.id,
        evidence: evidence2
      } }, importance);
    })();
  }
  beginIntent(record) {
    this.db.transaction(() => {
      const current = this.get(record.operation.id);
      if (!current || current.stage !== "received" || current.sdkCalls !== 0) throw new RecoveryRequired();
      record.stage = "intent";
      record.sdkCalls = 1;
      this.evidence(record, { kind: "submission_intent" }, "critical");
    })();
  }
  catalog(record, catalog) {
    this.db.transaction(() => {
      record.stage = "terminal";
      this.append(record, {
        kind: "catalog",
        sequence: this.sequence() + 1,
        operation_id: record.operation.id,
        catalog: catalogSchema.parse(catalog)
      }, "terminal");
    })();
  }
  control(record, thread_id, evidence2) {
    this.db.transaction(() => {
      record.stage = "terminal";
      this.append(record, {
        kind: "control",
        sequence: this.sequence() + 1,
        operation_id: record.operation.id,
        thread_id,
        evidence: evidence2
      }, "terminal");
    })();
  }
  bindSession(session, thread, selection) {
    this.db.transaction(() => {
      const exact = selectionSchema.parse(selection);
      external.parse(thread);
      if (exact.machine_id !== this.binding().machine_id) throw new RecoveryRequired();
      const old = this.session(session);
      if (old) {
        if (old.threadId !== thread || JSON.stringify(old.selection) !== JSON.stringify(exact)) throw new RecoveryRequired();
        return;
      }
      const count = this.db.prepare("SELECT count(*) AS count FROM sessions").get();
      if (count.count >= LIMITS.history) throw new JournalCapacity();
      this.db.prepare("INSERT INTO sessions(id,thread_id,selection) VALUES(?,?,?)").run(uuid.parse(session), thread, JSON.stringify(exact));
    })();
  }
  session(id) {
    const row = this.db.prepare("SELECT thread_id,selection FROM sessions WHERE id=?").get(id);
    return row ? { threadId: external.parse(row.thread_id), selection: selectionSchema.parse(JSON.parse(row.selection)) } : void 0;
  }
  submission(record, selection, state) {
    this.db.transaction(() => {
      if (!record.threadId || !("session_id" in record.operation.command)) throw new RecoveryRequired();
      this.bindSession(record.operation.command.session_id, record.threadId, selection);
      record.stage = state === "missing" ? "uncertain" : "observing";
      this.evidence(record, { kind: "submission", thread_id: record.threadId, state }, "critical");
    })();
  }
  pending() {
    const rows = this.db.prepare("SELECT id FROM commands WHERE terminal=0 ORDER BY rowid LIMIT ?").all(LIMITS.queued);
    return rows.map((row) => this.get(row.id));
  }
  capacity() {
    const pending = this.pending();
    return { queued: pending.length, effects: pending.filter((record) => isEffect(record.operation.command)).length };
  }
  outbox(after = this.acknowledged()) {
    const rows = this.db.prepare("SELECT payload FROM outbox WHERE sequence>? ORDER BY sequence LIMIT ?").all(after, LIMITS.queued);
    return rows.map((row) => observationSchema.parse(JSON.parse(row.payload)));
  }
  acknowledge(through) {
    this.db.transaction(() => {
      const meta = this.meta();
      if (!Number.isSafeInteger(through) || through < meta.acknowledged || through > meta.allocated) throw new RecoveryRequired();
      this.db.prepare("UPDATE journal_meta SET acknowledged=? WHERE id=1").run(through);
      this.db.prepare("DELETE FROM outbox WHERE sequence<=?").run(through);
    })();
  }
};

// integrations/cloudroom-plugin/src/enrollment.ts
var settingDescriptors = {
  openbotOrigin: { type: "string", label: "Openbot HTTPS origin" },
  machineName: { type: "string", label: "Machine name", default: "Local machine" },
  selectedProjects: { type: "string", label: "Selected project IDs", description: "One local project ID per line.", experimental_multiline: true, default: "" },
  pairingCode: { type: "string", label: "Pairing code", secret: true },
  enrollment: { type: "string", label: "Connection credential (managed automatically)", secret: true }
};
function defineSettings(bb) {
  return bb.settings.define(settingDescriptors);
}
var ConfigurationRequired = class extends Error {
  constructor(message) {
    super(message);
    this.name = "NeedsConfigurationError";
  }
};
var secret = import_zod3.z.string().regex(/^[0-9a-f]{64}$/);
var nameSchema = import_zod3.z.string().refine((value) => value.trim().length > 0 && Buffer.byteLength(value) <= 128 && !/[\p{Cc}]/u.test(value));
var requestSchema = import_zod3.z.object({
  code: import_zod3.z.string().regex(/^[0-9a-f]{32}$/),
  installation_id: uuid,
  name: nameSchema,
  claim_verifier: secret,
  credential_verifier: secret
}).strict();
var pendingSchema = import_zod3.z.object({
  state: import_zod3.z.literal("pending"),
  version: import_zod3.z.literal(1),
  origin: import_zod3.z.string(),
  installation_id: uuid,
  journal_id: uuid,
  claim_secret: secret,
  credential: secret,
  request: requestSchema
}).strict();
var pairedSchema = import_zod3.z.object({ state: import_zod3.z.literal("paired"), version: import_zod3.z.literal(1), binding: bindingSchema, credential: secret }).strict();
var enrollmentSchema = import_zod3.z.discriminatedUnion("state", [pendingSchema, pairedSchema]);
var receiptSchema = import_zod3.z.object({
  company_id: uuid,
  company_name: import_zod3.z.string().max(8192),
  machine: import_zod3.z.object({
    id: uuid,
    company_id: uuid,
    installation_id: uuid,
    name: nameSchema,
    state: import_zod3.z.enum(["enabled", "disabled", "revoked"]),
    credential_generation: import_zod3.z.number().int().positive().max(Number.MAX_SAFE_INTEGER),
    last_seen_at: import_zod3.z.string().nullable(),
    capabilities_sequence: import_zod3.z.number().int().positive().max(Number.MAX_SAFE_INTEGER).nullable()
  }).strict()
}).strict();
function canonicalOrigin(value) {
  if (!value || value.length > 2048) throw new ConfigurationRequired("Configure the Openbot HTTPS origin");
  let url;
  try {
    url = new URL(value);
  } catch {
    throw new ConfigurationRequired("Configure a valid Openbot HTTPS origin");
  }
  if (url.protocol !== "https:" || url.username || url.password || url.search || url.hash || url.pathname !== "/") {
    throw new ConfigurationRequired("Openbot origin must use HTTPS without credentials, path or query");
  }
  return url.origin;
}
function selectedProjects(value) {
  if (Buffer.byteLength(value) > LIMITS.projects * 130) throw new ConfigurationRequired("Too many selected projects");
  const ids = value.split(/\r?\n/).map((value2) => value2.trim()).filter(Boolean);
  if (ids.length > LIMITS.projects || new Set(ids).size !== ids.length || !ids.every((value2) => external.safeParse(value2).success)) {
    throw new ConfigurationRequired("Select at most 128 unique local project IDs");
  }
  return ids;
}
async function readBoundedJson(response, limit) {
  if (!response.body) throw new Error("empty HTTP response");
  const declared = Number(response.headers.get("content-length"));
  if (declared > limit) {
    await response.body.cancel();
    throw new Error("HTTP response bound");
  }
  const reader = response.body.getReader();
  const parts = [];
  let bytes = 0;
  try {
    while (true) {
      const result = await reader.read();
      if (result.done) break;
      bytes += result.value.byteLength;
      if (bytes > limit) throw new Error("HTTP response bound");
      parts.push(result.value);
    }
    return JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(Buffer.concat(parts)));
  } catch (error) {
    await reader.cancel().catch(() => {
    });
    throw error;
  } finally {
    reader.releaseLock();
  }
}
function validatePending(value) {
  if (canonicalOrigin(value.origin) !== value.origin || value.request.installation_id !== value.installation_id || sha256(value.claim_secret) !== value.request.claim_verifier || sha256(value.credential) !== value.request.credential_verifier) {
    throw new RecoveryRequired();
  }
}
async function currentCandidate(settings, candidate) {
  const current = await settings.get();
  if (canonicalOrigin(current.openbotOrigin) !== candidate.origin || current.enrollment !== JSON.stringify(candidate)) {
    throw new ConfigurationRequired("Pairing settings changed; restore the approved origin or repair pairing");
  }
}
async function exchange(settings, candidate, signal, fetcher) {
  await currentCandidate(settings, candidate);
  signal.throwIfAborted();
  const bounded = AbortSignal.any([signal, AbortSignal.timeout(LIMITS.handshakeMs)]);
  const status = await fetcher(`${candidate.origin}/v1/local-machines/pair-status`, {
    redirect: "error",
    signal: bounded,
    headers: { "x-openbot-pair-claim": candidate.claim_secret, "x-openbot-installation-id": candidate.installation_id }
  });
  if (status.ok) return receiptSchema.parse(await readBoundedJson(status, 16 * 1024));
  await status.body?.cancel();
  if (status.status !== 401 && status.status !== 404) throw new Error("pairing status unavailable");
  await currentCandidate(settings, candidate);
  signal.throwIfAborted();
  const enrolled = await fetcher(`${candidate.origin}/v1/local-machines/pair`, {
    method: "POST",
    redirect: "error",
    signal: bounded,
    headers: { "content-type": "application/json" },
    body: JSON.stringify(candidate.request)
  });
  if (!enrolled.ok) {
    await enrolled.body?.cancel();
    if (enrolled.status === 400 || enrolled.status === 401 || enrolled.status === 403 || enrolled.status === 404) {
      throw new ConfigurationRequired("Pairing was not accepted; recover or repair this pending enrollment");
    }
    throw new Error("pairing unavailable");
  }
  return receiptSchema.parse(await readBoundedJson(enrolled, 16 * 1024));
}
async function recoverEnrollment(settings, journal, signal, fetcher = fetch) {
  const values = await settings.get();
  const origin = canonicalOrigin(values.openbotOrigin);
  let state;
  if (values.enrollment === void 0) {
    if (journal.initialized()) throw new RecoveryRequired();
    const credential = (0, import_node_crypto2.randomBytes)(32).toString("hex");
    const claim_secret = (0, import_node_crypto2.randomBytes)(32).toString("hex");
    const installation_id = (0, import_node_crypto2.randomUUID)();
    const journal_id = (0, import_node_crypto2.randomUUID)();
    const request = requestSchema.safeParse({
      code: values.pairingCode,
      installation_id,
      name: values.machineName,
      claim_verifier: sha256(claim_secret),
      credential_verifier: sha256(credential)
    });
    if (!request.success) throw new ConfigurationRequired("Enter a valid pairing code and machine name");
    state = { state: "pending", version: 1, origin, installation_id, journal_id, claim_secret, credential, request: request.data };
    await settings.experimental_set({ enrollment: JSON.stringify(state) });
  } else {
    try {
      state = enrollmentSchema.parse(JSON.parse(values.enrollment));
    } catch {
      throw new RecoveryRequired();
    }
  }
  if (state.state === "paired") {
    if (origin !== state.binding.origin) throw new ConfigurationRequired("Approved Openbot origin changed; repair pairing explicitly");
    journal.validateBinding(state.binding);
    if (values.pairingCode !== void 0) await settings.experimental_set({ pairingCode: null });
    return state;
  }
  validatePending(state);
  if (origin !== state.origin) throw new ConfigurationRequired("Pending enrollment belongs to the original approved origin");
  const receipt = await exchange(settings, state, signal, fetcher);
  if (receipt.company_id !== receipt.machine.company_id || receipt.machine.installation_id !== state.installation_id || receipt.machine.state === "revoked") throw new RecoveryRequired();
  await currentCandidate(settings, state);
  signal.throwIfAborted();
  const binding = {
    origin,
    installation_id: state.installation_id,
    journal_id: state.journal_id,
    machine_id: receipt.machine.id,
    company_id: receipt.company_id,
    credential_generation: receipt.machine.credential_generation
  };
  journal.initialize(binding);
  const paired = { state: "paired", version: 1, binding, credential: state.credential };
  await settings.experimental_set({ enrollment: JSON.stringify(paired) });
  await settings.experimental_set({ pairingCode: null });
  return paired;
}

// integrations/cloudroom-plugin/src/connector.ts
var Connector = class {
  dispatcher;
  status = "offline";
  queue = /* @__PURE__ */ new Map();
  constructor(dispatcher) {
    this.dispatcher = dispatcher;
  }
  async run(paired, signal) {
    const stop = new AbortController();
    const lifetime = AbortSignal.any([signal, stop.signal]);
    const worker = this.work(lifetime).catch((error) => {
      stop.abort(error);
      throw error;
    });
    void worker.catch(() => {
    });
    try {
      let attempt = 0;
      while (!lifetime.aborted) {
        try {
          await this.connect(paired, lifetime);
          attempt = 0;
        } catch (error) {
          if (lifetime.aborted) break;
          if (error instanceof ConfigurationRequired || error instanceof RecoveryRequired) throw error;
        }
        if (!lifetime.aborted) {
          this.status = "offline";
          await (0, import_promises.setTimeout)(Math.min(3e4, 500 * 2 ** Math.min(attempt++, 6)) * (0.75 + Math.random() * 0.5), void 0, { signal: lifetime });
        }
      }
    } catch (error) {
      if (!lifetime.aborted) throw error;
    } finally {
      stop.abort();
      await worker;
      this.queue.clear();
      if (this.status !== "authorization_required") this.status = "offline";
    }
  }
  async work(signal) {
    try {
      while (!signal.aborted) {
        const next = this.queue.entries().next().value;
        if (next) {
          const [id, work] = next;
          try {
            const cached = await this.dispatcher.process(work.delivery, { signal, canDispatch: work.allowed });
            if (cached) work.reply(cached);
          } finally {
            this.queue.delete(id);
          }
        } else {
          await this.dispatcher.tick(signal);
          await (0, import_promises.setTimeout)(250, void 0, { signal });
        }
      }
    } catch (error) {
      if (!signal.aborted) throw error;
    }
  }
  async connect(paired, signal) {
    this.status = "connecting";
    const journal = this.dispatcher.journal;
    const binding = paired.binding;
    const endpoint = new URL("/v1/local-machines/connect", binding.origin);
    endpoint.protocol = "wss:";
    const socket = new import_ws.default(endpoint, {
      headers: {
        authorization: `Bearer ${paired.credential}`,
        "x-openbot-installation-id": binding.installation_id,
        "x-openbot-protocol-version": "1"
      },
      followRedirects: false,
      handshakeTimeout: LIMITS.handshakeMs,
      maxPayload: LIMITS.frame,
      perMessageDeflate: false
    });
    let identity;
    let allowed = false;
    let active = true;
    let lastReceived = performance.now();
    let sent = journal.acknowledged();
    let frames = 0;
    let window = performance.now();
    let failure;
    const assembler = new FrameAssembler();
    const cached = [];
    const stop = new AbortController();
    const connected = AbortSignal.any([signal, stop.signal]);
    const fail = (error) => {
      failure ??= error;
      allowed = false;
      stop.abort();
      socket.terminate();
    };
    const abort = () => fail(signal.reason ?? new Error("connector stopped"));
    signal.addEventListener("abort", abort, { once: true });
    socket.on("error", () => fail(new Error("Openbot connection failed")));
    socket.on("close", () => fail(new Error("Openbot connection closed")));
    socket.on("unexpected-response", (_request, response) => {
      response.destroy();
      if (response.statusCode === 401 || response.statusCode === 403) {
        this.status = "authorization_required";
        fail(new ConfigurationRequired("Machine authorization rejected; repair pairing"));
      } else fail(new Error("Openbot handshake rejected"));
    });
    socket.on("ping", () => {
      lastReceived = performance.now();
    });
    socket.on("pong", () => {
      lastReceived = performance.now();
    });
    socket.on("message", (data, binary) => {
      try {
        if (!active || binary) throw new Error("expected text frame");
        if (performance.now() - window >= 6e4) {
          frames = 0;
          window = performance.now();
        }
        if (++frames > 1024) throw new Error("incoming frame budget exhausted");
        lastReceived = performance.now();
        const value = assembler.push(Buffer.isBuffer(data) ? data : Buffer.concat(Array.isArray(data) ? data : [Buffer.from(data)]));
        if (value === void 0) return;
        const message = parseServer(value);
        if (message.kind === "welcome") {
          if (identity || message.welcome.identity.installation_id !== binding.installation_id || message.welcome.identity.credential_generation !== binding.credential_generation) throw new RecoveryRequired();
          if (message.welcome.recovery_required) {
            journal.requireRecovery();
            throw new RecoveryRequired();
          }
          journal.acknowledge(message.welcome.acknowledged_sequence);
          identity = message.welcome.identity;
          sent = journal.acknowledged();
          this.status = "recovering";
        } else {
          if (!identity || JSON.stringify(message.identity) !== JSON.stringify(identity)) throw new Error("stale connection identity");
          if (message.kind === "acknowledged") journal.acknowledge(message.through);
          if (message.kind === "ready") {
            allowed = message.ready && !journal.recoveryRequired();
            this.status = allowed ? "connected" : "recovering";
          }
          if (message.kind === "command") {
            const id = message.delivery.operation.id;
            const existing = this.queue.get(id);
            if (existing) {
              if (existing.delivery.operation.payload_digest !== message.delivery.operation.payload_digest) throw new Error("operation identity conflict");
              return;
            }
            const effect = ["create", "send"].includes(message.delivery.operation.command.kind);
            if (this.queue.size >= (effect ? LIMITS.queued - LIMITS.controls : LIMITS.queued)) throw new Error("incoming command capacity");
            this.queue.set(id, {
              delivery: message.delivery,
              allowed: () => active && allowed,
              reply: (value2) => {
                if (active && cached.length < LIMITS.queued) cached.push(value2);
              }
            });
          }
        }
      } catch (error) {
        fail(error);
      }
    });
    const write = async (message) => {
      for (const frame of encodeFrames(message)) {
        connected.throwIfAborted();
        await new Promise((resolve, reject) => {
          const cancel = () => {
            cleanup();
            reject(new Error("socket write cancelled"));
          };
          const timer = setTimeout(() => {
            cleanup();
            reject(new Error("socket write timed out"));
            socket.terminate();
          }, LIMITS.ioMs);
          const cleanup = () => {
            clearTimeout(timer);
            connected.removeEventListener("abort", cancel);
          };
          connected.addEventListener("abort", cancel, { once: true });
          socket.send(frame, (error) => {
            cleanup();
            if (error) reject(new Error("socket write failed"));
            else resolve();
          });
        });
      }
    };
    try {
      const opened = performance.now();
      while (socket.readyState === import_ws.default.CONNECTING) await (0, import_promises.setTimeout)(10, void 0, { signal: connected });
      connected.throwIfAborted();
      await write({ kind: "hello", hello: {
        version: 1,
        installation_id: binding.installation_id,
        journal_id: binding.journal_id,
        journal_sequence: journal.sequence()
      } });
      while (!identity) {
        if (performance.now() - opened >= LIMITS.handshakeMs) throw new Error("welcome timed out");
        await (0, import_promises.setTimeout)(10, void 0, { signal: connected });
      }
      let heartbeat = 0;
      let ready = 0;
      while (!connected.aborted) {
        assembler.expire();
        const now = performance.now();
        if (now - lastReceived >= 45e3) throw new Error("Openbot heartbeat expired");
        if (now - heartbeat >= LIMITS.heartbeatMs) {
          await write({ kind: "heartbeat", identity });
          heartbeat = now;
        }
        const observations = journal.outbox(sent);
        for (const observation of observations) {
          if (sent - journal.acknowledged() >= LIMITS.queued) break;
          await write({ kind: "observation", identity, observation });
          sent = observationSequence(observation);
        }
        for (const observation of cached.splice(0)) await write({ kind: "observation", identity, observation });
        if (sent === journal.sequence() && journal.acknowledged() === sent && now - ready >= 500) {
          await write({ kind: "ready", identity });
          ready = now;
        }
        await (0, import_promises.setTimeout)(25, void 0, { signal: connected });
      }
    } catch (error) {
      throw failure ?? error;
    } finally {
      active = false;
      allowed = false;
      stop.abort();
      signal.removeEventListener("abort", abort);
      socket.terminate();
    }
  }
};

// integrations/cloudroom-plugin/src/turns.ts
function ownInput(input, expected) {
  if (!Array.isArray(input) || input.length !== 1) return false;
  const first = input[0];
  return typeof first === "object" && first !== null && "type" in first && first.type === "text" && "text" in first && first.text === expected && (!("mentions" in first) || Array.isArray(first.mentions) && first.mentions.length === 0);
}
function replaceText(record, id, value) {
  if (!id || id.length > 128) {
    record.truncated = true;
    return;
  }
  let item = record.items.find((item2) => item2.id === id);
  if (!item) {
    if (record.items.length >= 1024) {
      record.truncated = true;
      return;
    }
    item = { id, text: "" };
    record.items.push(item);
  }
  const other = record.items.reduce((sum, candidate) => sum + (candidate === item ? 0 : Buffer.byteLength(candidate.text)), 0);
  const remaining = Math.max(0, LIMITS.output - other - Math.max(0, record.items.length - 1) * 2);
  const clipped = value.slice(0, LIMITS.output + 1).replaceAll("\0", "\uFFFD");
  const bytes = Buffer.from(clipped, "utf8");
  let end = Math.min(bytes.length, remaining);
  while (end > 0 && end < bytes.length && (bytes[end] & 192) === 128) end--;
  item.text = bytes.subarray(0, end).toString("utf8");
  if (value !== item.text) record.truncated = true;
}
function output(record) {
  const result = record.items.map((item) => item.text).filter(Boolean).join("\n\n");
  return result.length ? result : null;
}
function evidence(record) {
  if (!record.threadId || !record.turnId || !record.nativeState) return void 0;
  const terminal = ["completed", "failed", "stopped"].includes(record.nativeState);
  return {
    kind: "turn",
    thread_id: record.threadId,
    turn_id: record.turnId,
    state: record.nativeState,
    output: terminal ? output(record) : null,
    truncated: record.truncated,
    usage: record.usage
  };
}
function applyNativeEvents(original, events) {
  if (events.length > LIMITS.page || !original.threadId) throw new Error("native event page bound");
  const record = structuredClone(original);
  if (record.correlationConflict) return { kind: "uncertain", record };
  const conflict = () => {
    record.correlationConflict = true;
    record.stage = "uncertain";
    return { kind: "uncertain", record };
  };
  for (const event2 of events) {
    if (!Number.isSafeInteger(event2.seq) || event2.seq <= record.cursor) throw new Error("native event cursor did not advance");
    if (event2.seq > record.cursor + 1) {
      record.cursor = event2.seq;
      return conflict();
    }
    record.cursor = event2.seq;
    if (event2.type === "client/turn/requested" && ownInput(event2.data.input, record.sdkInput) && event2.data.source === (record.operation.command.kind === "create" ? "spawn" : "tell")) {
      if (record.requestId && record.requestId !== event2.data.requestId) return conflict();
      record.requestId = event2.data.requestId;
    }
    if (event2.type === "turn/input/accepted" && record.requestId !== null && event2.data.clientRequestId === record.requestId) {
      if (event2.scope.kind !== "turn" || record.turnId && record.turnId !== event2.scope.turnId) return conflict();
      record.turnId = event2.scope.turnId;
      record.nativeState = "running";
      record.stage = "observing";
    }
    if (event2.scope.kind !== "turn" || event2.scope.turnId !== record.turnId) continue;
    if (event2.type.startsWith("item/") && record.nativeState === "waiting_for_input") record.nativeState = "running";
    if (event2.type === "item/agentMessage/delta" && !event2.data.parentToolCallId) {
      const prior2 = record.items.find((item) => item.id === event2.data.itemId)?.text ?? "";
      replaceText(record, event2.data.itemId, prior2 + event2.data.delta.slice(0, LIMITS.output + 1));
    }
    if ((event2.type === "item/started" || event2.type === "item/completed") && event2.data.item.type === "agentMessage" && !event2.data.item.parentToolCallId) {
      replaceText(record, event2.data.item.id, event2.data.item.text);
    }
    if (event2.type === "turn/completed") {
      record.nativeState = event2.data.status === "completed" ? "completed" : event2.data.status === "interrupted" ? "stopped" : "failed";
      record.stage = "terminal";
      return { kind: "terminal", record, evidence: evidence(record) };
    }
  }
  const observed = evidence(record);
  const prior = evidence(original);
  return { kind: JSON.stringify(observed) === JSON.stringify(prior) ? "unchanged" : "progress", record, evidence: observed };
}

// integrations/cloudroom-plugin/src/sdk.ts
var PreflightRefusal = class extends Error {
  reason;
  constructor(reason) {
    super("local preflight refused");
    this.reason = reason;
  }
};
var sdkSignal = (parent) => AbortSignal.any([parent, AbortSignal.timeout(LIMITS.ioMs)]);
function missing(error) {
  return typeof error === "object" && error !== null && "status" in error && error.status === 404;
}
function stateUpdate(record, evidence2) {
  const previous = record.lastObservation;
  return { kind: previous?.kind === "agent" && JSON.stringify(previous.observation.evidence) === JSON.stringify(evidence2) ? "unchanged" : "progress", record, evidence: evidence2 };
}
var CloudroomSdk = class {
  sdk;
  pluginId;
  journal;
  health = /* @__PURE__ */ new Map();
  projects;
  constructor(bb, journal, projects) {
    this.sdk = bb.sdk;
    this.pluginId = bb.pluginId;
    this.journal = journal;
    this.projects = projects;
  }
  async localHost(signal) {
    const config = await this.sdk.system.config({ signal });
    if (!config.primaryHostId) throw new PreflightRefusal("project_unavailable");
    const host = await this.sdk.hosts.get({ hostId: config.primaryHostId, signal });
    if (host.status !== "connected" || host.type !== "persistent" || host.machineProviderId !== null || host.connectMachineId !== null || !["darwin", "linux"].includes(host.platform ?? "") || host.platform !== process.platform) throw new PreflightRefusal("project_unavailable");
    return host.id;
  }
  async project(projectId, hostId, signal) {
    if (!this.projects().includes(projectId)) throw new PreflightRefusal("project_unavailable");
    const project = await this.sdk.projects.get({ projectId, signal });
    if (project.id !== projectId || !project.sources.some((source) => source.type === "local_path" && source.hostId === hostId)) {
      throw new PreflightRefusal("project_unavailable");
    }
  }
  async models(providerId, hostId, signal) {
    const page = await this.sdk.providers.models({ providerId, hostId, signal });
    const status = page.modelLoadError?.code === "auth_required" ? "auth_required" : page.modelLoadError ? "unavailable" : page.models.length > LIMITS.models ? "oversized" : "available";
    this.health.set(providerId, { provider: providerId, status });
    if (page.modelLoadError) throw new PreflightRefusal("provider_unavailable");
    if (page.models.length > LIMITS.models) throw new PreflightRefusal("capacity");
    return page;
  }
  async catalog(signal) {
    const hostId = await this.localHost(signal);
    const projects = [...this.projects()];
    if (projects.length > LIMITS.projects) throw new PreflightRefusal("capacity");
    for (const project of projects) await this.project(project, hostId, signal);
    const installed = await this.sdk.providers.list({ hostId, signal });
    if (installed.length > LIMITS.providers) throw new PreflightRefusal("capacity");
    const providers = [];
    for (const id of provider.options) {
      if (!installed.some((item) => item.id === id)) {
        this.health.set(id, { provider: id, status: "unavailable" });
        continue;
      }
      try {
        const page = await this.models(id, hostId, signal);
        const models = page.models.map((model2) => ({ model: model2.model, reasoning_levels: [...new Set(
          [...model2.supportedReasoningEfforts.map((option) => option.reasoningEffort), model2.defaultReasoningEffort].filter((value) => reasoning.safeParse(value).success)
        )] }));
        providers.push({ provider: id, models });
      } catch (error) {
        if (signal.aborted) throw error;
        if (!(error instanceof PreflightRefusal)) throw error;
      }
    }
    return catalogSchema.parse({ projects, providers });
  }
  async prepare(record, signal) {
    const command = record.operation.command;
    const hostId = await this.localHost(signal);
    let selection;
    if (command.kind === "create") {
      if (this.journal.session(command.session_id)) throw new PreflightRefusal("recovery_required");
      selection = command.selection;
      if (selection.machine_id !== this.journal.binding().machine_id) throw new PreflightRefusal("recovery_required");
    } else if ("session_id" in command) {
      const mapped = this.journal.session(command.session_id);
      if (!mapped || !("thread_id" in command) || mapped.threadId !== command.thread_id) throw new PreflightRefusal("recovery_required");
      selection = mapped.selection;
      const thread = await this.sdk.threads.get({ threadId: command.thread_id, signal });
      if (thread.projectId !== selection.project_id || thread.providerId !== selection.provider) throw new PreflightRefusal("recovery_required");
      record.threadId = command.thread_id;
      if (thread.environmentId) {
        const environment = await this.sdk.environments.get({ environmentId: thread.environmentId, signal });
        if (environment.hostId !== hostId || environment.projectId !== selection.project_id) throw new PreflightRefusal("project_unavailable");
      }
    } else throw new PreflightRefusal("recovery_required");
    if (command.kind === "create" || command.kind === "send") {
      await this.project(selection.project_id, hostId, signal);
      const installed = await this.sdk.providers.list({ hostId, signal });
      if (installed.length > LIMITS.providers) throw new PreflightRefusal("capacity");
      if (!installed.some((provider2) => provider2.id === selection.provider)) throw new PreflightRefusal("provider_unavailable");
      const page = await this.models(selection.provider, hostId, signal);
      const model2 = page.models.find((model3) => model3.model === selection.model);
      if (!model2) throw new PreflightRefusal("model_unavailable");
      if (model2.defaultReasoningEffort !== selection.policy.reasoning && !model2.supportedReasoningEfforts.some((level) => level.reasoningEffort === selection.policy.reasoning)) {
        throw new PreflightRefusal("reasoning_unavailable");
      }
    }
    let environmentProvider = "project-checkout";
    if (command.kind === "create") {
      const providers = await this.sdk.environments.listProviders({ projectId: selection.project_id, hostId, signal });
      const workspace = providers.find((item) => (item.environmentProviderId ?? item.id) === "project-checkout" && item.machineProviderId === null && item.requires.projectCheckout && item.acceptsEmptyInputs);
      const available = workspace?.machineAvailability[hostId] ?? workspace?.availability;
      if (!workspace || available && available.status !== "available") throw new PreflightRefusal("project_unavailable");
      environmentProvider = workspace.environmentProviderId ?? workspace.id;
    }
    return { selection, hostId, environmentProvider };
  }
  metadata(record) {
    const command = record.operation.command;
    if (!("session_id" in command)) throw new RecoveryRequired();
    return {
      installation: this.journal.binding().installation_id,
      session: command.session_id,
      operation: record.operation.id,
      digest: record.operation.payload_digest
    };
  }
  async mutate(record, prepared, signal) {
    const command = record.operation.command;
    if (command.kind === "create") {
      const thread = await this.sdk.threads.spawn({
        projectId: prepared.selection.project_id,
        executionTarget: "local",
        providerId: prepared.selection.provider,
        model: prepared.selection.model,
        reasoningLevel: prepared.selection.policy.reasoning,
        environment: {
          type: "provider",
          environmentProviderId: prepared.environmentProvider,
          machine: { type: "existing", hostId: prepared.hostId },
          inputs: {}
        },
        title: "Openbot local session",
        prompt: record.sdkInput,
        pluginMetadata: this.metadata(record),
        signal
      });
      record.threadId = thread.id;
      this.journal.submission(record, prepared.selection, thread.status === "pending" ? "queued" : "accepted");
    } else if (command.kind === "send") {
      const result = await this.sdk.threads.send({
        threadId: command.thread_id,
        mode: "queue-if-active",
        model: prepared.selection.model,
        reasoningLevel: prepared.selection.policy.reasoning,
        input: [{ type: "text", text: record.sdkInput, mentions: [] }],
        signal
      });
      record.threadId = command.thread_id;
      if (result.delivery === "queued") record.queueId = result.queuedMessage.id;
      this.journal.submission(record, prepared.selection, result.delivery === "queued" ? "queued" : "accepted");
    } else if (command.kind === "stop_thread") {
      await this.sdk.threads.stop({ threadId: command.thread_id, signal });
      this.journal.control(record, command.thread_id, { kind: "stop_accepted" });
    } else if (command.kind === "archive_thread") {
      await this.sdk.threads.archive({ threadId: command.thread_id, signal });
      this.journal.control(record, command.thread_id, { kind: "archive_accepted" });
    } else throw new Error("not a mutation");
  }
  async baseline(record, signal) {
    if (!record.threadId) return;
    const page = await this.sdk.threads.events.list({ threadId: record.threadId, limit: "1", order: "desc", signal });
    if (page.length > 1) throw new Error("native baseline bound");
    const cursor = page[0]?.seq ?? 0;
    if (!Number.isSafeInteger(cursor) || cursor < 0) throw new Error("invalid native cursor");
    record.cursor = cursor;
  }
  async recoverCreate(record, signal) {
    const command = record.operation.command;
    if (command.kind !== "create" || record.threadId) return false;
    const expected = this.metadata(record);
    const matches = [];
    for (let offset = 0; offset < 400; offset += 100) {
      const page = await this.sdk.threads.list({
        projectId: command.selection.project_id,
        originPluginId: this.pluginId,
        limit: 100,
        offset,
        signal
      });
      if (page.length > 100) throw new Error("native recovery page bound");
      for (const thread2 of page) {
        if (thread2.projectId !== command.selection.project_id || thread2.providerId !== command.selection.provider || thread2.originPluginId !== this.pluginId) continue;
        const metadata = await this.sdk.threads.getPluginMetadata({ threadId: thread2.id, signal });
        if (Object.entries(expected).every(([key, value]) => metadata[key] === value)) matches.push(thread2);
        if (matches.length > 1) return false;
      }
      if (page.length < 100) break;
    }
    const thread = matches[0];
    if (!thread) return false;
    record.threadId = thread.id;
    this.journal.submission(record, command.selection, thread.status === "pending" ? "queued" : "accepted");
    return true;
  }
  async poll(record, signal) {
    if (!record.threadId && record.operation.command.kind === "send") {
      const mapping = this.journal.session(record.operation.command.session_id);
      if (mapping?.threadId === record.operation.command.thread_id) record.threadId = mapping.threadId;
    }
    if (!record.threadId) await this.recoverCreate(record, signal);
    if (!record.threadId) return { kind: "uncertain", record };
    try {
      await this.sdk.threads.get({ threadId: record.threadId, signal });
    } catch (error) {
      if (!missing(error)) throw error;
      if (record.turnId) {
        record.nativeState = "missing";
        record.stage = "uncertain";
        return stateUpdate(record, {
          kind: "turn",
          thread_id: record.threadId,
          turn_id: record.turnId,
          state: "missing",
          output: null,
          truncated: record.truncated,
          usage: null
        });
      }
      const command = record.operation.command;
      const selected = command.kind === "create" ? command.selection : "session_id" in command ? this.journal.session(command.session_id)?.selection : void 0;
      if (selected && !(record.lastObservation?.kind === "agent" && record.lastObservation.observation.evidence.kind === "submission" && record.lastObservation.observation.evidence.state === "missing")) this.journal.submission(record, selected, "missing");
      return { kind: "unchanged", record };
    }
    const events = await this.sdk.threads.events.list({ threadId: record.threadId, afterSeq: String(record.cursor), limit: "100", order: "asc", signal });
    const update = applyNativeEvents(record, events);
    if (update.kind !== "terminal" && update.kind !== "uncertain" && update.record.turnId) {
      const interactions = await this.sdk.threads.interactions.list({ threadId: record.threadId, signal });
      if (interactions.length > LIMITS.page) throw new Error("native interaction bound");
      if (interactions.some((interaction) => interaction.threadId === record.threadId && interaction.turnId === update.record.turnId && (interaction.status === "pending" || interaction.status === "resolving"))) {
        update.record.nativeState = "waiting_for_input";
        return stateUpdate(update.record, {
          kind: "turn",
          thread_id: record.threadId,
          turn_id: update.record.turnId,
          state: "waiting_for_input",
          output: null,
          truncated: update.record.truncated,
          usage: update.record.usage
        });
      }
    }
    return update;
  }
  async inspect(threadId, signal) {
    const thread = await this.sdk.threads.get({ threadId, signal });
    if (thread.status === "pending") return "queued";
    if (thread.status === "active" || thread.status === "starting" || thread.status === "stopping") return "running";
    const events = await this.sdk.threads.events.list({ threadId, limit: "100", order: "desc", signal });
    if (events.length > 100) throw new Error("native inspect bound");
    const completed = events.find((event2) => event2.type === "turn/completed");
    if (completed?.type !== "turn/completed") return null;
    return completed.data.status === "completed" ? "completed" : completed.data.status === "interrupted" ? "stopped" : "failed";
  }
};

// integrations/cloudroom-plugin/src/dispatcher.ts
var Dispatcher = class {
  journal;
  sdk;
  constructor(journal, sdk) {
    this.journal = journal;
    this.sdk = sdk;
  }
  cached(record) {
    return record.lastObservation && observationSequence(record.lastObservation) <= this.journal.acknowledged() ? record.lastObservation : void 0;
  }
  refuse(record, reason) {
    record.stage = "terminal";
    this.journal.evidence(record, { kind: "refused", reason }, "terminal");
  }
  uncertain(record) {
    if (record.stage === "terminal") return;
    record.stage = isEffect(record.operation.command) ? "uncertain" : "terminal";
    const old = record.lastObservation;
    if (old?.kind === "agent" && old.observation.evidence.kind === "uncertain") {
      this.journal.save(record);
      return;
    }
    this.journal.evidence(record, { kind: "uncertain" }, record.stage === "terminal" ? "terminal" : "critical");
  }
  blocked(record, context) {
    if (Date.parse(record.operation.deadline) <= Date.now()) return "expired";
    if (this.journal.recoveryRequired() || !context.canDispatch() || context.signal.aborted) return "recovery_required";
    const counts = this.journal.capacity();
    const effect = isEffect(record.operation.command);
    if (counts.queued > (effect ? LIMITS.queued - LIMITS.controls : LIMITS.queued) || effect && counts.effects > LIMITS.effects) return "capacity";
    return void 0;
  }
  async process(delivery, context) {
    const admitted = this.journal.admit(delivery);
    const record = admitted.record;
    if (record.stage === "terminal") return this.cached(record);
    const command = record.operation.command;
    try {
      if (command.kind === "refresh_catalog" && record.sdkCalls === 0) {
        this.journal.catalog(record, await this.sdk.catalog(sdkSignal(context.signal)));
      } else if (command.kind === "inspect" && record.sdkCalls === 0) {
        await this.sdk.prepare(record, sdkSignal(context.signal));
        const state = await this.sdk.inspect(command.thread_id, sdkSignal(context.signal));
        if (state) this.journal.control(record, command.thread_id, { kind: "snapshot", state });
        else this.uncertain(record);
      } else if (record.sdkCalls !== 0) {
        await this.reconcile(record, context.signal);
      } else if (delivery.mode === "reconcile" || record.stage !== "received") {
        if (record.stage === "received") this.refuse(record, "recovery_required");
      } else {
        const blocked = this.blocked(record, context);
        if (blocked) {
          this.refuse(record, blocked);
          return this.cached(record);
        }
        const prepared = await this.sdk.prepare(record, sdkSignal(context.signal));
        await this.sdk.baseline(record, sdkSignal(context.signal));
        const withdrawn = this.blocked(record, context);
        if (withdrawn) {
          this.refuse(record, withdrawn);
          return this.cached(record);
        }
        this.journal.beginIntent(record);
        const signal = sdkSignal(context.signal);
        signal.throwIfAborted();
        await this.sdk.mutate(record, prepared, signal);
      }
    } catch (error) {
      const current = this.journal.get(record.operation.id);
      if (current.stage !== "terminal") {
        if (current.sdkCalls === 0 && current.stage === "received") {
          this.refuse(current, error instanceof PreflightRefusal ? error.reason : "provider_unavailable");
        } else this.uncertain(current);
      }
    }
    return this.cached(this.journal.get(record.operation.id));
  }
  async reconcile(record, signal) {
    if (!isEffect(record.operation.command)) {
      this.uncertain(record);
      return;
    }
    const update = await this.sdk.poll(record, sdkSignal(signal));
    if (update.kind === "uncertain") {
      this.uncertain(update.record);
      return;
    }
    if (update.evidence && update.kind !== "unchanged") {
      this.journal.evidence(update.record, update.evidence, update.kind === "terminal" ? "terminal" : "progress");
    } else this.journal.save(update.record);
  }
  async tick(signal) {
    for (const record of this.journal.pending()) {
      if (signal.aborted) break;
      if (record.sdkCalls !== 1 || !isEffect(record.operation.command)) continue;
      try {
        await this.reconcile(record, signal);
      } catch {
        this.uncertain(this.journal.get(record.operation.id));
      }
    }
  }
};

// integrations/cloudroom-plugin/src/lock.ts
var import_node_fs = require("node:fs");
var import_node_path = require("node:path");
var import_fs_ext = require("/Users/mac03/dev/openbot/integrations/cloudroom-plugin/node_modules/fs-ext/fs-ext.js");
function acquireProfileLock(databaseName) {
  if (!(0, import_node_path.isAbsolute)(databaseName)) throw new Error("persistent plugin database required");
  const fd = (0, import_node_fs.openSync)(
    (0, import_node_path.join)((0, import_node_path.dirname)(databaseName), "openbot-connector.lock"),
    import_node_fs.constants.O_RDWR | import_node_fs.constants.O_CREAT | import_node_fs.constants.O_NOFOLLOW,
    384
  );
  try {
    (0, import_fs_ext.flockSync)(fd, "exnb");
  } catch {
    (0, import_node_fs.closeSync)(fd);
    throw new Error("another connector owns this profile");
  }
  let held = true;
  return () => {
    if (held) {
      held = false;
      (0, import_node_fs.closeSync)(fd);
    }
  };
}

// integrations/cloudroom-plugin/src/server.ts
async function openbot(bb) {
  const settings = defineSettings(bb);
  const db = bb.storage.database();
  bb.storage.migrate(db, [...JOURNAL_SCHEMA]);
  const journal = new Journal(db);
  let selected = [];
  let connector;
  let running;
  const sdk = new CloudroomSdk(bb, journal, () => selected);
  settings.onChange((next, prev) => {
    if (next.openbotOrigin !== prev.openbotOrigin || next.enrollment !== prev.enrollment || next.selectedProjects !== prev.selectedProjects) running?.abort(new ConfigurationRequired("Connector settings changed; reload plugin"));
  });
  bb.background.service("openbot-connector", {
    async start(signal) {
      const release = acquireProfileLock(db.name);
      const stop = new AbortController();
      const lifetime = AbortSignal.any([signal, stop.signal]);
      try {
        const values = await settings.get();
        canonicalOrigin(values.openbotOrigin);
        selected = selectedProjects(values.selectedProjects ?? "");
        const paired = await recoverEnrollment(settings, journal, lifetime);
        lifetime.throwIfAborted();
        running = stop;
        connector = new Connector(new Dispatcher(journal, sdk));
        await connector.run(paired, lifetime);
        if (stop.signal.aborted && !signal.aborted) throw new ConfigurationRequired("Connector settings changed; reload plugin");
      } finally {
        running = void 0;
        stop.abort();
        release();
      }
    }
  });
  bb.cli.register({
    name: "openbot-local",
    summary: "Local connector status and project discovery",
    commands: [
      { name: "status", summary: "Connection, journal and provider status", usage: "openbot-local status" },
      { name: "projects", summary: "List local Cloudroom projects", usage: "openbot-local projects" }
    ],
    async run(argv, context) {
      if (argv.length === 1 && argv[0] === "status") return { exitCode: 0, stdout: JSON.stringify({
        connection: connector?.status ?? "offline",
        paired: journal.initialized(),
        recoveryRequired: journal.initialized() ? journal.recoveryRequired() : false,
        sequence: journal.initialized() ? journal.sequence() : 0,
        acknowledged: journal.initialized() ? journal.acknowledged() : 0,
        providers: [...sdk.health.values()]
      }) };
      if (argv.length === 1 && argv[0] === "projects") {
        const signal = sdkSignal(context.signal ?? AbortSignal.timeout(5e3));
        const hostId = await sdk.localHost(signal);
        const projects = await bb.sdk.projects.list({ signal });
        if (projects.length > 128) return { exitCode: 1, stderr: "Project catalog exceeds the supported bound" };
        return { exitCode: 0, stdout: JSON.stringify(projects.filter((project) => project.sources.some((source) => source.type === "local_path" && source.hostId === hostId)).map((project) => ({ id: project.id, name: project.name }))) };
      }
      return { exitCode: 1, stderr: "Use openbot-local status or openbot-local projects" };
    }
  });
}

// integrations/cloudroom-plugin/test/support.ts
var import_better_sqlite3 = __toESM(require("/Users/mac03/dev/openbot/integrations/cloudroom-plugin/node_modules/better-sqlite3/lib/index.js"), 1);
var import_node_fs2 = require("node:fs");
var import_node_os = require("node:os");
var import_node_path2 = require("node:path");
var import_node_crypto3 = require("node:crypto");
function fixture(options = {}) {
  const directory = (0, import_node_fs2.mkdtempSync)((0, import_node_path2.join)((0, import_node_os.tmpdir)(), "openbot-plugin-unit-"));
  const file = (0, import_node_path2.join)(directory, "data.db");
  const binding = {
    origin: "https://openbot.example",
    installation_id: (0, import_node_crypto3.randomUUID)(),
    journal_id: (0, import_node_crypto3.randomUUID)(),
    machine_id: (0, import_node_crypto3.randomUUID)(),
    company_id: (0, import_node_crypto3.randomUUID)(),
    credential_generation: 1
  };
  const handles = [];
  const open = () => {
    const existing = (0, import_node_fs2.existsSync)(file);
    const db = new import_better_sqlite3.default(file);
    handles.push(db);
    db.pragma("journal_mode=WAL");
    if (!existing) for (const statement of JOURNAL_SCHEMA) db.exec(statement);
    return new Journal(db);
  };
  const journal = open();
  if (options.initialized !== false) journal.initialize(binding);
  const selection = {
    machine_id: binding.machine_id,
    project_id: "proj_selected",
    provider: "codex",
    model: "gpt-test",
    policy: { workspace: "project_directory", permissions: "provider_default", reasoning: "low" }
  };
  return {
    directory,
    file,
    journal,
    binding,
    selection,
    open,
    close: () => {
      for (const db of handles) if (db.open) db.close();
      (0, import_node_fs2.rmSync)(directory, { recursive: true, force: true });
    }
  };
}
function operation(command = { kind: "refresh_catalog" }) {
  const deadline2 = new Date(Date.now() + 30 * 60 * 1e3).toISOString().replace(".000Z", "Z");
  return { id: (0, import_node_crypto3.randomUUID)(), command, deadline: deadline2, payload_digest: commandDigest(command, deadline2), dispatch: { kind: "not_dispatched" }, outcome: null };
}
function send() {
  return { kind: "send", session_id: (0, import_node_crypto3.randomUUID)(), thread_id: "thr_owned", prompt: "bounded authorized work" };
}

// integrations/cloudroom-plugin/test/native-fixture.ts
function event(seq, type, data, turnId) {
  return { seq, type, data, scope: turnId ? { kind: "turn", turnId } : { kind: "thread" } };
}
function nativeFixture(journal, selection) {
  const calls = [];
  const state = {
    events: [],
    thread: {
      id: "thr_owned",
      projectId: selection.project_id,
      providerId: selection.provider,
      status: "idle",
      environmentId: "env_local",
      originPluginId: "openbot-test"
    },
    metadata: {},
    interactions: [],
    missing: false,
    providerError: null,
    model: selection.model,
    platform: process.platform,
    hostId: "host_primary",
    lostResponse: false,
    held: false,
    entered: 0
  };
  const mutate = async (kind, input) => {
    state.entered++;
    calls.push({ kind, input });
    if (kind === "spawn") state.metadata = input.pluginMetadata;
    if (state.held) await new Promise((resolve, reject) => {
      const signal = input.signal;
      if (signal.aborted) reject(signal.reason);
      else signal.addEventListener("abort", () => reject(signal.reason), { once: true });
    });
    if (state.lostResponse) throw new Error("response lost after native acceptance");
  };
  const bb = { pluginId: "openbot-test", sdk: {
    system: { config: async () => ({ primaryHostId: state.hostId }) },
    hosts: { get: async () => ({
      id: state.hostId,
      status: "connected",
      type: "persistent",
      platform: state.platform,
      machineProviderId: null,
      connectMachineId: null
    }) },
    projects: { get: async () => ({ id: selection.project_id, sources: [{ type: "local_path", hostId: state.hostId }] }) },
    providers: { list: async () => [{ id: selection.provider }], models: async () => ({
      modelLoadError: state.providerError,
      models: [{ model: state.model, supportedReasoningEfforts: [{ reasoningEffort: "low" }], defaultReasoningEffort: "low" }]
    }) },
    environments: {
      get: async () => ({ hostId: state.hostId, projectId: selection.project_id }),
      listProviders: async () => [{
        id: "project-checkout",
        machineProviderId: null,
        requires: { projectCheckout: true },
        acceptsEmptyInputs: true,
        machineAvailability: {},
        availability: { status: "available" }
      }]
    },
    threads: {
      get: async () => {
        if (state.missing) throw { status: 404 };
        return state.thread;
      },
      list: async () => state.missing ? [] : [state.thread],
      getPluginMetadata: async () => state.metadata,
      spawn: async (input) => {
        await mutate("spawn", input);
        return state.thread;
      },
      send: async (input) => {
        await mutate("send", input);
        return { delivery: "queued", queuedMessage: { id: "queue_own" } };
      },
      stop: async (input) => {
        await mutate("stop", input);
      },
      archive: async (input) => {
        await mutate("archive", input);
      },
      events: { list: async (input) => {
        const selected = state.events.filter((event2) => event2.seq > Number(input.afterSeq ?? 0));
        return (input.order === "desc" ? selected.reverse() : selected).slice(0, Number(input.limit));
      } },
      interactions: { list: async () => state.interactions }
    }
  } };
  return { bb, state, calls, sdk: new CloudroomSdk(bb, journal, () => [selection.project_id]) };
}

// plan/cloudroom-local/phases/p04/docs/logs/reviewer-b1-r1-boundaries.ts
async function main() {
  const f = fixture();
  const command = send();
  f.journal.bindSession(command.session_id, command.thread_id, f.selection);
  const n = nativeFixture(f.journal, f.selection);
  const op = operation(command);
  let values = {
    openbotOrigin: f.binding.origin,
    selectedProjects: f.selection.project_id,
    enrollment: JSON.stringify({ state: "paired", version: 1, binding: f.binding, credential: "a".repeat(64) })
  };
  let changed = (_next, _prev) => {
  };
  let start;
  let reads = 0;
  let paused;
  const atSecondRead = new Promise((resolve) => paused = resolve);
  let resume;
  const resumed = new Promise((resolve) => resume = resolve);
  const bb = {
    ...n.bb,
    storage: { database: () => f.journal.db, migrate: () => {
    } },
    settings: { define: () => ({
      get: async () => {
        if (++reads === 2) {
          paused();
          await resumed;
        }
        return { ...values };
      },
      experimental_set: async () => values,
      onChange: (listener) => changed = listener
    }) },
    background: { service: (_name, service) => start = service.start },
    cli: { register: () => {
    } }
  };
  const originalRun = Connector.prototype.run;
  let connectorEntered = false;
  Connector.prototype.run = async function(_paired, signal2) {
    connectorEntered = true;
    await this.dispatcher.process({ operation: op, mode: "dispatch" }, { signal: signal2, canDispatch: () => true });
  };
  try {
    await openbot(bb);
    const running = start(new AbortController().signal);
    await atSecondRead;
    const old = values;
    values = { ...values, selectedProjects: "" };
    changed(values, old);
    resume();
    await running;
    import_strict.default.equal(values.selectedProjects, "");
    import_strict.default.equal(connectorEntered, true);
    import_strict.default.equal(n.calls.length, 1);
    console.log(JSON.stringify({
      case: "project withdrawal during startup",
      configuredProjects: values.selectedProjects,
      connectorEntered,
      mutationCallsAfterWithdrawal: n.calls.length,
      durableSdkCalls: f.journal.get(op.id)?.sdkCalls,
      transport: "Connector.run replaced by direct production Dispatcher.process; public SDK stand-in, no network/native effect"
    }));
  } finally {
    Connector.prototype.run = originalRun;
    f.close();
  }
  const p = fixture();
  const c = send();
  p.journal.bindSession(c.session_id, c.thread_id, p.selection);
  const native = nativeFixture(p.journal, p.selection);
  const dispatcher = new Dispatcher(p.journal, native.sdk);
  const operationRecord = operation(c);
  const signal = new AbortController().signal;
  try {
    await dispatcher.process({ operation: operationRecord, mode: "dispatch" }, { signal, canDispatch: () => true });
    native.state.events = [
      event(1, "client/turn/requested", { input: [{ type: "text", text: nativeInput(operationRecord) }], source: "tell", requestId: "req" }),
      event(2, "turn/input/accepted", { clientRequestId: "req" }, "turn")
    ];
    native.state.interactions = [{ threadId: c.thread_id, turnId: "turn", status: "pending" }];
    await dispatcher.tick(signal);
    import_strict.default.equal(p.journal.get(operationRecord.id)?.nativeState, "waiting_for_input");
    native.state.interactions = [{ threadId: c.thread_id, turnId: "turn", status: "resolved" }];
    native.state.events.push(event(3, "system/interaction/lifecycle", { interaction: { id: "permission", status: "resolved" } }, "turn"));
    await dispatcher.tick(signal);
    const after = p.journal.get(operationRecord.id);
    import_strict.default.equal(after.nativeState, "waiting_for_input");
    console.log(JSON.stringify({
      case: "resolved permission with no item event",
      interactions: "resolved",
      nativeState: after.nativeState,
      limitation: "synthetic public SDK fixture; no native scheduler assertion"
    }));
  } finally {
    p.close();
  }
  const enrollmentFixture = fixture({ initialized: false });
  let enrollmentValues = {
    openbotOrigin: enrollmentFixture.binding.origin,
    machineName: "Disposable",
    selectedProjects: "",
    pairingCode: "b".repeat(32)
  };
  const settings = { get: async () => ({ ...enrollmentValues }), experimental_set: async (patch) => {
    for (const [key, value] of Object.entries(patch)) {
      if (value === null) delete enrollmentValues[key];
      else enrollmentValues[key] = value;
    }
    return { ...enrollmentValues };
  } };
  const requests = [];
  const rejectPairing = async (_url, args) => {
    if (args.method === "POST") {
      requests.push(JSON.parse(args.body));
      return new Response(null, { status: 400 });
    }
    return new Response(null, { status: 404 });
  };
  try {
    await import_strict.default.rejects(recoverEnrollment(settings, enrollmentFixture.journal, new AbortController().signal, rejectPairing), /Pairing was not accepted/);
    const rejectedInputRetained = enrollmentValues.pairingCode !== void 0;
    enrollmentValues.pairingCode = "c".repeat(32);
    await import_strict.default.rejects(recoverEnrollment(settings, enrollmentFixture.journal, new AbortController().signal, rejectPairing), /Pairing was not accepted/);
    (0, import_strict.default)(rejectedInputRetained);
    import_strict.default.deepEqual(requests[0], requests[1]);
    console.log(JSON.stringify({
      case: "definitive enrollment rejection",
      status: 400,
      rejectedInputRetained,
      enteredReplacementCodeIgnored: requests[1].code !== enrollmentValues.pairingCode,
      pendingStillContainsRejectedCode: JSON.parse(enrollmentValues.enrollment).request.code === requests[0].code,
      journalInitialized: enrollmentFixture.journal.initialized(),
      transport: "synthetic complete HTTP status/response; no network"
    }));
  } finally {
    enrollmentFixture.close();
  }
}
main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
