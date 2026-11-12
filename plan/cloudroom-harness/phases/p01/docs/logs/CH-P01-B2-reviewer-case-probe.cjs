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

// plan/cloudroom-harness/phases/p01/docs/logs/CH-P01-B2-reviewer-case-probe.ts
var import_node_crypto4 = require("node:crypto");

// integrations/cloudroom-plugin/src/native-bundle.ts
var import_node_crypto2 = require("node:crypto");
var import_node_fs = require("node:fs");
var fs = __toESM(require("node:fs/promises"), 1);
var import_node_path = __toESM(require("node:path"), 1);
var import_yaml = require("yaml");
var import_zod3 = require("zod");

// integrations/cloudroom-plugin/src/journal.ts
var import_zod2 = require("zod");

// integrations/cloudroom-plugin/src/protocol.ts
var import_node_crypto = require("node:crypto");
var import_zod = require("zod");
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
var permissions = import_zod.z.enum(["full", "auto", "accept_edits"]);
var policySchema = import_zod.z.object({
  workspace: import_zod.z.literal("project_directory"),
  permissions,
  reasoning
}).strict();
var selectionSchema = import_zod.z.object({
  machine_id: uuid,
  project_id: external,
  provider,
  model,
  policy: policySchema
}).strict();
var harnessSchema = import_zod.z.discriminatedUnion("kind", [
  import_zod.z.object({ kind: import_zod.z.literal("unconfigured") }).strict(),
  import_zod.z.object({ kind: import_zod.z.literal("agent"), agent_id: uuid, revision_id: uuid, digest: digestSchema }).strict()
]);
var commandSchema = import_zod.z.discriminatedUnion("kind", [
  import_zod.z.object({ kind: import_zod.z.literal("create"), session_id: uuid, selection: selectionSchema, harness: harnessSchema, prompt }).strict(),
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
  providers: import_zod.z.array(import_zod.z.object({ provider, permission_modes: import_zod.z.array(permissions).max(3), models: import_zod.z.array(import_zod.z.object({
    model,
    reasoning_levels: import_zod.z.array(reasoning).max(8)
  }).strict()).max(LIMITS.models) }).strict()).max(LIMITS.providers)
}).strict();
var refusalSchema = import_zod.z.enum([
  "project_unavailable",
  "provider_unavailable",
  "model_unavailable",
  "reasoning_unavailable",
  "permissions_unavailable",
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
  import_zod.z.object({ kind: import_zod.z.literal("ready"), identity: identitySchema, ready: import_zod.z.boolean() }).strict(),
  import_zod.z.object({ kind: import_zod.z.literal("mappings_retired"), identity: identitySchema, nonce: uuid, sessions: import_zod.z.array(uuid).max(100) }).strict()
]);
var sha256 = (bytes2) => (0, import_node_crypto.createHash)("sha256").update(bytes2).digest("hex");
var isEffect = (command) => command.kind === "create" || command.kind === "send";
var targetThread = (command) => "thread_id" in command ? command.thread_id : void 0;
var observationSequence = (observation) => observation.kind === "agent" ? observation.observation.sequence : observation.sequence;
function commandDigest(command, exactDeadline) {
  return sha256(JSON.stringify([commandSchema.parse(command), deadline.parse(exactDeadline)]));
}
function parseOperation(value) {
  const operation2 = operationSchema.parse(value);
  if (commandDigest(operation2.command, operation2.deadline) !== operation2.payload_digest) throw new Error("command digest mismatch");
  const evidence = operation2.dispatch;
  if (evidence.kind === "native_identity" && targetThread(operation2.command) && evidence.thread_id !== targetThread(operation2.command)) throw new Error("command thread mismatch");
  if (evidence.kind === "catalog_snapshot" !== (operation2.command.kind === "refresh_catalog") && evidence.kind === "catalog_snapshot") throw new Error("invalid catalog receipt");
  if (operation2.command.kind === "refresh_catalog" && evidence.kind === "native_identity") throw new Error("invalid native receipt");
  if (operation2.outcome === "succeeded" && !(evidence.kind === "catalog_snapshot" || evidence.kind === "native_identity" && (!isEffect(operation2.command) || evidence.turn_id !== null))) {
    throw new Error("success without native evidence");
  }
  return operation2;
}
function nativeInput(operation2) {
  if (!("prompt" in operation2.command)) throw new Error("not an effect");
  const header = `[Openbot operation ${operation2.id}]
`;
  if (Buffer.byteLength(header) !== 57) throw new Error("invalid operation identity");
  return header + operation2.command.prompt;
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

// integrations/cloudroom-plugin/src/journal.ts
var JOURNAL_SCHEMA = [
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
  `CREATE INDEX outbox_operation ON outbox(operation_id)`
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
  hostId: external.nullable(),
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
var RETENTION_MS = 90 * 24 * 60 * 60 * 1e3;
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
var mappingSchema = import_zod2.z.object({
  sessionId: uuid,
  threadId: external,
  selection: selectionSchema,
  harness: harnessSchema,
  operationId: uuid,
  hostId: external.nullable()
}).strict().refine((mapping) => mapping.harness.kind === "unconfigured" || mapping.hostId !== null);
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
    const bytes2 = Buffer.byteLength(payload);
    if (bytes2 > LIMITS.logical - 256) throw new JournalCapacity();
    const budget = this.budget();
    let reserve = row.reserve;
    if (importance === "progress") {
      if (budget.used + budget.reserved + bytes2 > LIMITS.outbox) {
        this.write(record);
        return;
      }
    } else {
      if (bytes2 > reserve) throw new JournalCapacity();
      reserve = importance === "terminal" ? 0 : reserve - bytes2;
    }
    if (budget.used + bytes2 + budget.reserved - row.reserve + reserve > LIMITS.outbox) throw new JournalCapacity();
    this.db.prepare("INSERT INTO outbox(sequence,operation_id,payload,bytes) VALUES(?,?,?,?)").run(meta.allocated + 1, record.operation.id, payload, bytes2);
    this.db.prepare("UPDATE journal_meta SET allocated=? WHERE id=1").run(meta.allocated + 1);
    record.lastObservation = exact;
    if (importance === "terminal") this.db.prepare("UPDATE commands SET acknowledged_at=NULL WHERE id=?").run(record.operation.id);
    this.write(record, reserve);
  }
  write(record, reserve) {
    const exact = stored.parse(record);
    const old = this.row(exact.operation.id);
    if (!old || old.digest !== exact.operation.payload_digest) throw new RecoveryRequired();
    const previous = stored.parse(JSON.parse(old.state));
    if (old.terminal && exact.stage !== "terminal" || previous.sdkCalls > exact.sdkCalls || previous.sdkInput !== exact.sdkInput || previous.cursor > exact.cursor || previous.correlationConflict && !exact.correlationConflict || previous.threadId !== null && previous.threadId !== exact.threadId || previous.requestId !== null && previous.requestId !== exact.requestId || previous.turnId !== null && previous.turnId !== exact.turnId) throw new RecoveryRequired();
    const terminal = Number(exact.stage === "terminal");
    this.db.prepare("UPDATE commands SET state=?,terminal=?,reserve=?,terminal_at=CASE WHEN ?=1 THEN COALESCE(terminal_at,?) ELSE NULL END WHERE id=?").run(
      JSON.stringify(exact),
      terminal,
      terminal ? 0 : reserve ?? old.reserve,
      terminal,
      Date.now(),
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
      if (!this.canAdmit(operation2)) throw new JournalCapacity();
      const record = {
        operation: operation2,
        stage: "received",
        sdkInput: isEffect(operation2.command) ? nativeInput(operation2) : "",
        sdkCalls: 0,
        hostId: null,
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
      if ("session_id" in operation2.command) this.db.prepare("UPDATE sessions SET last_used_at=max(last_used_at,?) WHERE id=?").run(Date.now(), operation2.command.session_id);
      this.append(record, { kind: "agent", observation: {
        sequence: this.sequence() + 1,
        operation_id: operation2.id,
        evidence: { kind: "receipt" }
      } }, "critical");
      const mutation = !["refresh_catalog", "inspect"].includes(operation2.command.kind);
      if (mutation && (delivery.mode === "reconcile" || operation2.outcome !== null || operation2.dispatch.kind !== "not_dispatched")) {
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
  evidence(record, evidence, importance) {
    this.db.transaction(() => {
      evidenceSchema.parse(evidence);
      if (evidence.kind === "refused" && record.sdkCalls !== 0) throw new RecoveryRequired();
      this.append(record, { kind: "agent", observation: {
        sequence: this.sequence() + 1,
        operation_id: record.operation.id,
        evidence
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
  control(record, thread_id, evidence) {
    this.db.transaction(() => {
      record.stage = "terminal";
      this.append(record, {
        kind: "control",
        sequence: this.sequence() + 1,
        operation_id: record.operation.id,
        thread_id,
        evidence
      }, "terminal");
    })();
  }
  bindSession(mapping) {
    const exact = mappingSchema.parse(mapping);
    this.db.transaction(() => {
      if (exact.selection.machine_id !== this.binding().machine_id) throw new RecoveryRequired();
      const old = this.session(exact.sessionId);
      if (old) {
        const { operationId: _priorOperation, ...prior } = old;
        const { operationId: _newOperation, ...next } = exact;
        if (JSON.stringify(prior) !== JSON.stringify(next)) throw new RecoveryRequired();
        this.db.prepare("UPDATE sessions SET last_used_at=max(last_used_at+1,?) WHERE id=?").run(Date.now(), exact.sessionId);
        return;
      }
      const count = this.db.prepare("SELECT count(*) AS count FROM sessions").get();
      if (count.count >= LIMITS.history) throw new JournalCapacity();
      this.db.prepare("INSERT INTO sessions(id,mapping,last_used_at) VALUES(?,?,?)").run(exact.sessionId, JSON.stringify(exact), Date.now());
    })();
  }
  session(id) {
    const row = this.db.prepare("SELECT mapping FROM sessions WHERE id=?").get(id);
    return row ? mappingSchema.parse(JSON.parse(row.mapping)) : void 0;
  }
  thread(id) {
    const row = this.db.prepare("SELECT mapping FROM sessions WHERE thread_id=?").get(id);
    return row ? mappingSchema.parse(JSON.parse(row.mapping)) : void 0;
  }
  referenceOperation(session, operation2) {
    const mapping = this.session(session);
    if (!mapping) throw new RecoveryRequired();
    mapping.operationId = uuid.parse(operation2);
    this.db.prepare("UPDATE sessions SET mapping=?,last_used_at=max(last_used_at+1,?) WHERE id=?").run(JSON.stringify(mapping), Date.now(), session);
  }
  bundleReferences() {
    const references = /* @__PURE__ */ new Set();
    const rows = this.db.prepare("SELECT mapping FROM sessions LIMIT ?").all(LIMITS.history + 1);
    if (rows.length > LIMITS.history) throw new RecoveryRequired();
    for (const row of rows) {
      const mapping = mappingSchema.parse(JSON.parse(row.mapping));
      if (mapping.harness.kind === "agent") references.add(mapping.harness.digest);
    }
    const commands = this.db.prepare("SELECT id FROM commands LIMIT ?").all(LIMITS.history + 1);
    if (commands.length > LIMITS.history) throw new RecoveryRequired();
    for (const row of commands) {
      const command = this.get(row.id).operation.command;
      if (command.kind === "create" && command.harness.kind === "agent") references.add(command.harness.digest);
    }
    return references;
  }
  hasMappedBundle(digest) {
    const rows = this.db.prepare("SELECT mapping FROM sessions LIMIT ?").all(LIMITS.history + 1);
    if (rows.length > LIMITS.history) throw new RecoveryRequired();
    return rows.some((row) => {
      const mapping = mappingSchema.parse(JSON.parse(row.mapping));
      return mapping.harness.kind === "agent" && mapping.harness.digest === digest;
    });
  }
  hasDispatchedBundle(digest) {
    const rows = this.db.prepare("SELECT id FROM commands LIMIT ?").all(LIMITS.history + 1);
    if (rows.length > LIMITS.history) throw new RecoveryRequired();
    return rows.some((row) => {
      const record = this.get(row.id);
      const command = record.operation.command;
      return record.sdkCalls !== 0 && command.kind === "create" && command.harness.kind === "agent" && command.harness.digest === digest;
    });
  }
  submission(record, selection, state) {
    this.db.transaction(() => {
      const command = record.operation.command;
      if (!record.threadId || !("session_id" in command)) throw new RecoveryRequired();
      if (command.kind === "create") this.bindSession({
        sessionId: command.session_id,
        threadId: record.threadId,
        selection,
        harness: command.harness,
        operationId: record.operation.id,
        hostId: command.harness.kind === "agent" ? record.hostId : null
      });
      else if (!this.session(command.session_id) || this.session(command.session_id).threadId !== record.threadId) throw new RecoveryRequired();
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
      this.db.prepare("UPDATE commands SET acknowledged_at=COALESCE(acknowledged_at,?) WHERE terminal=1 AND acknowledged_at IS NULL AND COALESCE(json_extract(state,'$.lastObservation.sequence'),json_extract(state,'$.lastObservation.observation.sequence'))<=?").run(Date.now(), through);
      this.db.prepare("DELETE FROM outbox WHERE sequence<=?").run(through);
    })();
  }
  canAdmit(operation2) {
    if (this.row(operation2.id)) return true;
    const counts = this.diagnostics();
    return !counts.newCommandsPaused && (operation2.command.kind !== "create" || !counts.newSessionsPaused);
  }
  diagnostics() {
    const counts = this.db.prepare("SELECT (SELECT count(*) FROM commands) AS commands,(SELECT count(*) FROM sessions) AS mappings").get();
    const budget = this.budget();
    const newCommandsPaused = counts.commands >= LIMITS.history || budget.used + budget.reserved + LIMITS.terminalReserve > LIMITS.outbox;
    return {
      ...counts,
      outboxBytes: budget.used,
      reservedBytes: budget.reserved,
      newCommandsPaused,
      newSessionsPaused: newCommandsPaused || counts.mappings >= LIMITS.history
    };
  }
  compactAcknowledged() {
    return this.db.transaction(() => this.db.prepare(`DELETE FROM commands WHERE id IN (
      SELECT command.id FROM commands AS command WHERE command.terminal=1
        AND command.terminal_at<? AND command.acknowledged_at<?
        AND (julianday(json_extract(command.state,'$.operation.deadline'))-2440587.5)*86400000<?
        AND NOT EXISTS(SELECT 1 FROM outbox WHERE operation_id=command.id)
      ORDER BY command.terminal_at,command.id LIMIT 128)`).run(Date.now() - RETENTION_MS, Date.now() - RETENTION_MS, Date.now()).changes)();
  }
  mappingCandidates(after) {
    return this.db.prepare(`SELECT session.id,session.thread_id,session.mapping,session.last_used_at
      FROM sessions AS session WHERE session.last_used_at<?
        AND (? IS NULL OR (session.last_used_at,session.id)>(?,?))
        AND NOT EXISTS(SELECT 1 FROM commands WHERE commands.session_id=session.id)
      ORDER BY session.last_used_at,session.id LIMIT 100`).all(
      Date.now() - RETENTION_MS,
      after?.last_used_at ?? null,
      after?.last_used_at ?? null,
      after?.id ?? null
    );
  }
  retireMappings(candidates, retired) {
    if (candidates.length > 100 || retired.length > 100 || new Set(retired).size !== retired.length) throw new RecoveryRequired();
    const exact = new Map(candidates.map((candidate) => [candidate.id, candidate]));
    if (retired.some((id) => !exact.has(id))) throw new RecoveryRequired();
    return this.db.transaction(() => {
      let removed = 0;
      for (const id of retired) {
        const candidate = exact.get(id);
        removed += this.db.prepare(`DELETE FROM sessions WHERE id=? AND thread_id=? AND mapping=? AND last_used_at=? AND last_used_at<?
          AND NOT EXISTS(SELECT 1 FROM commands WHERE commands.session_id=sessions.id)`).run(uuid.parse(id), candidate.thread_id, candidate.mapping, candidate.last_used_at, Date.now() - RETENTION_MS).changes;
      }
      return removed;
    })();
  }
};

// integrations/cloudroom-plugin/src/native-bundle.ts
var NATIVE_CACHE_LIMITS = Object.freeze({ bytes: 64 * 1024 * 1024, bundles: 128, concurrent: 8 });
var MAX_NATIVE_BUNDLE_BYTES = 2 * 1024 * 1024;
var bytes = (limit) => (value) => Buffer.byteLength(value) <= limit;
var fileSchema = import_zod3.z.object({ path: import_zod3.z.string().max(240), content_base64: import_zod3.z.string().max(4 * Math.ceil(256 * 1024 / 3)), executable: import_zod3.z.boolean() }).strict();
var nativeBundleSchema = import_zod3.z.object({
  format: import_zod3.z.literal("openbot_native_v1"),
  instructions: import_zod3.z.string().max(16384).refine(bytes(16384)),
  permissions,
  skills: import_zod3.z.array(import_zod3.z.object({ name: import_zod3.z.string().regex(/^[A-Za-z0-9_.:-]{1,128}$/), entry: import_zod3.z.string().max(20 * 1024).refine(bytes(20 * 1024)), files: import_zod3.z.array(fileSchema).max(64) }).strict()).max(16)
}).strict();
var hex = (bytes2) => (0, import_node_crypto2.createHash)("sha256").update(bytes2).digest("hex");
function hashNativeTree(files) {
  const hash = (0, import_node_crypto2.createHash)("sha256").update("bb-skill-tree-v1");
  for (const file of [...files].sort((a, b) => a.path < b.path ? -1 : a.path > b.path ? 1 : 0)) hash.update("\0file\0").update(file.path).update("\0").update(file.mode.toString(8)).update("\0").update(String(file.bytes.length)).update("\0").update(file.bytes);
  return hash.digest("hex");
}
function assetPath(value) {
  if (!value || !/^[A-Za-z0-9._/ -]+$/.test(value) || Buffer.byteLength(value) > 240 || value.split("/").some((part) => !part || part === "." || part === ".." || part.startsWith(" ") || /[. ]$/.test(part)) || value.split("/")[0].toLowerCase() === "skill.md") throw new RecoveryRequired();
}
function renamedEntry(skill, name) {
  const match = /^\uFEFF?---\r?\n([\s\S]*?)\r?\n---(?=\r?\n|$)/.exec(skill.entry);
  if (!match) throw new RecoveryRequired();
  const metadata = (0, import_yaml.parse)(match[1], { uniqueKeys: true, maxAliasCount: 100 });
  if (!metadata || typeof metadata !== "object" || Array.isArray(metadata) || !("name" in metadata) || metadata.name !== skill.name || !("description" in metadata) || typeof metadata.description !== "string") throw new RecoveryRequired();
  const result = Buffer.from(`---
${(0, import_yaml.stringify)({ ...metadata, name })}---${skill.entry.slice(match[0].length)}`);
  if (result.length > 20 * 1024) throw new RecoveryRequired();
  return result;
}
function plan(raw, digest) {
  if (raw.length > MAX_NATIVE_BUNDLE_BYTES || hex(raw) !== digest) throw new RecoveryRequired();
  const bundle = nativeBundleSchema.parse(JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(raw)));
  if (JSON.stringify(bundle) !== raw.toString("utf8")) throw new RecoveryRequired();
  const names = /* @__PURE__ */ new Set();
  const trees = [];
  for (const skill of bundle.skills) {
    const slug = skill.name.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "").slice(0, 28).replace(/-$/, "") || "skill";
    const name = `ob-${slug}-${hex(JSON.stringify(skill)).slice(0, 32)}`;
    if (names.has(name)) throw new RecoveryRequired();
    names.add(name);
    const files2 = [{ path: "SKILL.md", bytes: renamedEntry(skill, name), mode: 420 }];
    let total = 0;
    for (const file of skill.files) {
      assetPath(file.path);
      const buffer = Buffer.from(file.content_base64, "base64");
      total += buffer.length;
      if (buffer.toString("base64") !== file.content_base64 || buffer.length > 256 * 1024 || total > 512 * 1024) throw new RecoveryRequired();
      files2.push({ path: file.path, bytes: buffer, mode: file.executable ? 493 : 420 });
    }
    const paths = files2.map((file) => file.path.toLowerCase()).sort();
    if (paths.some((value, index) => index > 0 && (value === paths[index - 1] || value.startsWith(`${paths[index - 1]}/`)))) throw new RecoveryRequired();
    trees.push({ name, hash: hashNativeTree(files2), files: files2 });
  }
  const files = /* @__PURE__ */ new Map([["bundle.json", { path: "bundle.json", bytes: raw, mode: 420 }]]);
  const directories = /* @__PURE__ */ new Set(["", "skills"]);
  for (const tree of trees) for (const file of tree.files) {
    const relative = `skills/${tree.name}/${file.path}`;
    files.set(relative, { ...file, path: relative });
    for (let parent = import_node_path.default.posix.dirname(relative); parent !== "."; parent = import_node_path.default.posix.dirname(parent)) directories.add(parent);
  }
  const instructions = bundle.instructions + (trees.length ? "\n\nOpenbot skill names:\n" + trees.map((tree, index) => `${bundle.skills[index].name} => ${tree.name}`).join("\n") : "");
  if (Buffer.byteLength(instructions) > 32768) throw new RecoveryRequired();
  const cost = 4096 * (files.size + directories.size) + [...files.values()].reduce((sum, file) => sum + Math.ceil(file.bytes.length / 4096) * 4096, 0);
  return { bundle, instructions, trees, files, directories, cost };
}
async function privateDirectory(directory) {
  const stat = await fs.lstat(directory);
  if (!stat.isDirectory() || stat.isSymbolicLink() || stat.uid !== process.getuid() || (stat.mode & 511) !== 448) throw new RecoveryRequired();
}
async function exactFile(file, expected, signal) {
  signal.throwIfAborted();
  const handle = await fs.open(file, import_node_fs.constants.O_RDONLY | import_node_fs.constants.O_NOFOLLOW);
  try {
    const stat = await handle.stat();
    if (!stat.isFile() || stat.uid !== process.getuid() || stat.size !== expected.bytes.length || (stat.mode & 511) !== expected.mode) throw new RecoveryRequired();
    const buffer = Buffer.alloc(stat.size);
    let offset = 0;
    while (offset < buffer.length) {
      signal.throwIfAborted();
      const { bytesRead } = await handle.read(buffer, offset, buffer.length - offset, null);
      if (!bytesRead) throw new RecoveryRequired();
      offset += bytesRead;
    }
    if ((await handle.read(Buffer.alloc(1), 0, 1, null)).bytesRead || !buffer.equals(expected.bytes)) throw new RecoveryRequired();
    const after = await handle.stat();
    if (after.size !== stat.size || after.mode !== stat.mode || after.ctimeMs !== stat.ctimeMs || after.mtimeMs !== stat.mtimeMs) throw new RecoveryRequired();
  } finally {
    await handle.close();
  }
}
async function validateDirectory(root, expected, signal) {
  const pending = [""];
  const seen = /* @__PURE__ */ new Set();
  let count = 0;
  while (pending.length) {
    const relative = pending.pop();
    const directory = import_node_path.default.join(root, relative);
    await privateDirectory(directory);
    const entries = await fs.opendir(directory);
    for await (const entry of entries) {
      signal.throwIfAborted();
      if (++count > expected.files.size + expected.directories.size) throw new RecoveryRequired();
      const child = relative ? `${relative}/${entry.name}` : entry.name;
      if (expected.directories.has(child)) {
        pending.push(child);
        continue;
      }
      const file = expected.files.get(child);
      if (!file) throw new RecoveryRequired();
      await exactFile(import_node_path.default.join(root, child), file, signal);
      seen.add(child);
    }
  }
  if (seen.size !== expected.files.size) throw new RecoveryRequired();
}
async function boundedRaw(file) {
  const handle = await fs.open(file, import_node_fs.constants.O_RDONLY | import_node_fs.constants.O_NOFOLLOW);
  try {
    const stat = await handle.stat();
    if (!stat.isFile() || stat.size > MAX_NATIVE_BUNDLE_BYTES) throw new RecoveryRequired();
    const buffer = Buffer.alloc(stat.size);
    let offset = 0;
    while (offset < buffer.length) {
      const { bytesRead } = await handle.read(buffer, offset, buffer.length - offset, null);
      if (!bytesRead) throw new RecoveryRequired();
      offset += bytesRead;
    }
    if ((await handle.read(Buffer.alloc(1), 0, 1, null)).bytesRead) throw new RecoveryRequired();
    return buffer;
  } finally {
    await handle.close();
  }
}
var NativeBundleCache = class {
  constructor(journal, limits = NATIVE_CACHE_LIMITS) {
    this.journal = journal;
    this.limits = limits;
  }
  journal;
  limits;
  tail = Promise.resolve();
  active = 0;
  root;
  async directory() {
    if (this.root) return this.root;
    if (!import_node_path.default.isAbsolute(this.journal.db.name)) throw new RecoveryRequired();
    const parent = import_node_path.default.dirname(await fs.realpath(this.journal.db.name));
    const root = import_node_path.default.join(parent, "openbot-native-bundles");
    try {
      await fs.mkdir(root, { mode: 448 });
    } catch (error) {
      if (error.code !== "EEXIST") throw error;
    }
    await privateDirectory(root);
    this.root = root;
    return root;
  }
  serial(work) {
    const result = this.tail.then(work);
    this.tail = result.then(() => {
    }, () => {
    });
    return result;
  }
  async inventory(signal) {
    const root = await this.directory();
    const plans = /* @__PURE__ */ new Map();
    const entries = await fs.opendir(root);
    let total = 0;
    for await (const entry of entries) {
      signal.throwIfAborted();
      if (!/^[0-9a-f]{64}$/.test(entry.name) || plans.size >= this.limits.bundles) throw new RecoveryRequired();
      const target = import_node_path.default.join(root, entry.name);
      await privateDirectory(target);
      const raw = await boundedRaw(import_node_path.default.join(target, "bundle.json"));
      const expected = plan(raw, entry.name);
      total += expected.cost;
      if (total > this.limits.bytes) throw new JournalCapacity();
      await validateDirectory(target, expected, signal);
      plans.set(entry.name, expected);
    }
    return plans;
  }
  async load(reader, operation2, reference, purpose, signal) {
    if (this.active >= this.limits.concurrent) throw new JournalCapacity();
    this.active++;
    try {
      const raw = await reader(operation2, signal);
      signal.throwIfAborted();
      const expected = plan(raw, reference.digest);
      return await this.serial(async () => {
        signal.throwIfAborted();
        const root = await this.directory();
        const inventory = await this.inventory(signal);
        const target = import_node_path.default.join(root, reference.digest);
        if (!inventory.has(reference.digest)) {
          if (purpose === "required_configuration" || this.journal.hasMappedBundle(reference.digest) || this.journal.hasDispatchedBundle(reference.digest)) throw new RecoveryRequired();
          if (inventory.size >= this.limits.bundles || expected.cost + [...inventory.values()].reduce((sum, item) => sum + item.cost, 0) > this.limits.bytes) throw new JournalCapacity();
          await this.publish(target, expected, signal);
        }
        await validateDirectory(target, expected, signal);
        return { bundle: expected.bundle, configuration: { instructions: expected.instructions, skillDirectories: expected.trees.map((tree) => ({ rootPath: import_node_path.default.join(target, "skills", tree.name), expectedTreeHash: tree.hash })) } };
      });
    } catch (error) {
      if (error instanceof JournalCapacity || error instanceof RecoveryRequired || signal.aborted) throw error;
      throw new RecoveryRequired();
    } finally {
      this.active--;
    }
  }
  async publish(target, expected, signal) {
    const temporary = import_node_path.default.join(import_node_path.default.dirname(target), `tmp-${(0, import_node_crypto2.randomUUID)()}`);
    let created = false;
    try {
      await fs.mkdir(temporary, { mode: 448 });
      created = true;
      for (const directory of [...expected.directories].filter(Boolean).sort((a, b) => a.length - b.length)) {
        signal.throwIfAborted();
        await fs.mkdir(import_node_path.default.join(temporary, directory), { mode: 448 });
      }
      for (const file of expected.files.values()) {
        signal.throwIfAborted();
        const handle = await fs.open(import_node_path.default.join(temporary, file.path), "wx", file.mode);
        try {
          await handle.writeFile(file.bytes, { signal });
          await handle.chmod(file.mode);
          await handle.sync();
        } finally {
          await handle.close();
        }
      }
      await validateDirectory(temporary, expected, signal);
      signal.throwIfAborted();
      await fs.rename(temporary, target);
      created = false;
    } finally {
      if (created) await fs.rm(temporary, { recursive: true, force: true });
    }
  }
  async cleanup(signal) {
    await this.serial(async () => {
      const inventory = await this.inventory(signal);
      const references = this.journal.bundleReferences();
      const root = await this.directory();
      for (const [digest, expected] of inventory) if (!references.has(digest)) {
        signal.throwIfAborted();
        const target = import_node_path.default.join(root, digest);
        await validateDirectory(target, expected, signal);
        await fs.rm(target, { recursive: true });
      }
    });
  }
};

// integrations/cloudroom-plugin/test/support.ts
var import_better_sqlite3 = __toESM(require("better-sqlite3"), 1);
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
  const open2 = () => {
    const existing = (0, import_node_fs2.existsSync)(file);
    const db = new import_better_sqlite3.default(file);
    handles.push(db);
    db.pragma("journal_mode=WAL");
    if (!existing) for (const statement of JOURNAL_SCHEMA) db.exec(statement);
    return new Journal(db);
  };
  const journal = open2();
  if (options.initialized !== false) journal.initialize(binding);
  const selection = {
    machine_id: binding.machine_id,
    project_id: "proj_selected",
    provider: "codex",
    model: "gpt-test",
    policy: { workspace: "project_directory", permissions: "full", reasoning: "low" }
  };
  return {
    directory,
    file,
    journal,
    binding,
    selection,
    open: open2,
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

// plan/cloudroom-harness/phases/p01/docs/logs/CH-P01-B2-reviewer-case-probe.ts
async function main() {
  const f = fixture();
  const value = { format: "openbot_native_v1", instructions: "Use both assets.", permissions: "full", skills: [{ name: "case-proof", entry: "---\nname: case-proof\ndescription: Case directory proof\n---\nRead both files.", files: [{ path: "Scripts/one.txt", content_base64: "YQ==", executable: false }, { path: "scripts/two.txt", content_base64: "Yg==", executable: false }] }] };
  const raw = Buffer.from(JSON.stringify(value));
  const reference = { kind: "agent", agent_id: (0, import_node_crypto4.randomUUID)(), revision_id: (0, import_node_crypto4.randomUUID)(), digest: (0, import_node_crypto4.createHash)("sha256").update(raw).digest("hex") };
  const op = operation({ kind: "create", session_id: (0, import_node_crypto4.randomUUID)(), selection: f.selection, harness: reference, prompt: "case proof" });
  f.journal.admit({ operation: op, mode: "dispatch" });
  try {
    const result = await new NativeBundleCache(f.journal).load(async () => raw, op.id, reference, "admission", new AbortController().signal);
    console.log("CACHE ACCEPTED", result.configuration.skillDirectories);
  } catch (error) {
    console.log("CACHE REFUSED", error.constructor.name, error.message);
  } finally {
    f.close();
  }
}
main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
