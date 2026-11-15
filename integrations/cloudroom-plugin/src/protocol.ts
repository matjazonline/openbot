import { createHash, randomUUID } from 'node:crypto';
import { z } from 'zod';

export const PROTOCOL_VERSION = 2;

export const LIMITS = Object.freeze({ prompt: 32 * 1024, output: 256 * 1024, frame: 64 * 1024,
  chunk: 16 * 1024, logical: 2 * 1024 * 1024, assemblyMs: 30_000, outbox: 64 * 1024 * 1024,
  effects: 8, queued: 32, controls: 8, projects: 128, providers: 32, models: 256, page: 100,
  heartbeatMs: 15_000, ioMs: 5_000, handshakeMs: 10_000, history: 4096, terminalReserve: 3 * 1024 * 1024 });
const sequence = z.number().int().min(0).max(Number.MAX_SAFE_INTEGER);
const positive = sequence.min(1);
export const uuid = z.string().regex(/^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/)
  .refine(value => value !== '00000000-0000-0000-0000-000000000000');
export const external = z.string().regex(/^[A-Za-z0-9_.:-]{1,128}$/);
const model = z.string().refine(value => value.length > 0 && Buffer.byteLength(value) <= 128
  && !/[\p{White_Space}\p{Cc}]/u.test(value));
const text = (maximum: number) => z.string().refine(value => Buffer.byteLength(value) <= maximum && !value.includes('\0'));
const prompt = text(LIMITS.prompt).refine(value => value.trim().length > 0);
const digestSchema = z.string().regex(/^[0-9a-f]{64}$/);
export const reasoning = z.enum(['none', 'low', 'medium', 'high', 'xhigh', 'max', 'ultra', 'ultracode']);
export const provider = z.enum(['claude-code', 'codex', 'acp-opencode']);
export const permissions = z.enum(['full', 'auto', 'accept_edits']);
export const policySchema = z.object({ workspace: z.literal('project_directory'),
  permissions, reasoning }).strict();
export const selectionSchema = z.object({ machine_id: uuid, project_id: external, provider,
  model, policy: policySchema }).strict();
export type Selection = z.infer<typeof selectionSchema>;
export const harnessSchema = z.discriminatedUnion('kind', [
  z.object({ kind: z.literal('unconfigured') }).strict(),
  z.object({ kind: z.literal('agent'), agent_id: uuid, revision_id: uuid, digest: digestSchema }).strict(),
]);
export type Harness = z.infer<typeof harnessSchema>;
export const commandSchema = z.discriminatedUnion('kind', [
  z.object({ kind: z.literal('create'), session_id: uuid, selection: selectionSchema, harness: harnessSchema, prompt }).strict(),
  z.object({ kind: z.literal('send'), session_id: uuid, thread_id: external, prompt }).strict(),
  z.object({ kind: z.literal('inspect'), session_id: uuid, thread_id: external }).strict(),
  z.object({ kind: z.literal('stop_thread'), session_id: uuid, thread_id: external }).strict(),
  z.object({ kind: z.literal('archive_thread'), session_id: uuid, thread_id: external }).strict(),
  z.object({ kind: z.literal('refresh_catalog') }).strict(),
]);
export type Command = z.infer<typeof commandSchema>;
export const turnState = z.enum(['queued', 'running', 'waiting_for_input', 'completed', 'failed', 'stopped', 'missing']);
const dispatch = z.discriminatedUnion('kind', [
  z.object({ kind: z.literal('not_dispatched') }).strict(),
  z.object({ kind: z.literal('accepted_locally'), sequence: positive }).strict(),
  z.object({ kind: z.literal('submission_intent'), sequence: positive }).strict(),
  z.object({ kind: z.literal('native_identity'), sequence: positive, thread_id: external, turn_id: external.nullable() }).strict(),
  z.object({ kind: z.literal('catalog_snapshot'), sequence: positive }).strict(),
]);
const deadline = z.string().refine(value => {
  const matched = /^(\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d)(?:\.(\d{3}|\d{6}))?Z$/.exec(value);
  if (!matched || !Number.isFinite(Date.parse(value))) return false;
  if (matched[2] === '000' || (matched[2]?.length === 6 && matched[2].endsWith('000'))) return false;
  return new Date(value).toISOString().slice(0, 19) === matched[1];
});
const operationSchema = z.object({ id: uuid, command: commandSchema, deadline,
  payload_digest: digestSchema, dispatch,
  outcome: z.enum(['succeeded', 'failed', 'cancelled', 'expired', 'uncertain']).nullable() }).strict();
export type Operation = z.infer<typeof operationSchema>;
export type Delivery = { operation: Operation; mode: 'dispatch' | 'reconcile' };
export const identitySchema = z.object({ installation_id: uuid, credential_generation: positive,
  connection_generation: positive }).strict();
export type Identity = z.infer<typeof identitySchema>;
export const catalogSchema = z.object({ projects: z.array(external).max(LIMITS.projects),
  providers: z.array(z.object({ provider, permission_modes: z.array(permissions).max(3), models: z.array(z.object({ model,
    reasoning_levels: z.array(reasoning).max(8) }).strict()).max(LIMITS.models) }).strict()).max(LIMITS.providers) }).strict();
export type Catalog = z.infer<typeof catalogSchema>;
export const refusalSchema = z.enum(['project_unavailable', 'provider_unavailable', 'model_unavailable',
  'reasoning_unavailable', 'permissions_unavailable', 'capacity', 'expired', 'recovery_required']);
export type Refusal = z.infer<typeof refusalSchema>;
export const evidenceSchema = z.discriminatedUnion('kind', [
  z.object({ kind: z.literal('receipt') }).strict(),
  z.object({ kind: z.literal('submission_intent') }).strict(),
  z.object({ kind: z.literal('submission'), thread_id: external, state: z.enum(['accepted', 'queued', 'missing']) }).strict(),
  z.object({ kind: z.literal('refused'), reason: refusalSchema }).strict(),
  z.object({ kind: z.literal('uncertain') }).strict(),
  z.object({ kind: z.literal('turn'), thread_id: external, turn_id: external, state: turnState,
    output: text(LIMITS.output).nullable(), truncated: z.boolean(),
    usage: z.object({ input_tokens: sequence, output_tokens: sequence }).strict().nullable() }).strict(),
]);
export type Evidence = z.infer<typeof evidenceSchema>;
export const observationSchema = z.discriminatedUnion('kind', [
  z.object({ kind: z.literal('agent'), observation: z.object({ sequence: positive, operation_id: uuid, evidence: evidenceSchema }).strict() }).strict(),
  z.object({ kind: z.literal('catalog'), sequence: positive, operation_id: uuid, catalog: catalogSchema }).strict(),
  z.object({ kind: z.literal('control'), sequence: positive, operation_id: uuid, thread_id: external,
    evidence: z.discriminatedUnion('kind', [z.object({ kind: z.literal('snapshot'), state: turnState }).strict(),
      z.object({ kind: z.literal('stop_accepted') }).strict(), z.object({ kind: z.literal('archive_accepted') }).strict()]) }).strict(),
]);
export type Observation = z.infer<typeof observationSchema>;
export const serverSchema = z.discriminatedUnion('kind', [
  z.object({ kind: z.literal('welcome'), welcome: z.object({ identity: identitySchema, acknowledged_sequence: sequence,
    recovery_required: z.boolean() }).strict() }).strict(),
  z.object({ kind: z.literal('command'), identity: identitySchema,
    delivery: z.object({ operation: operationSchema, mode: z.enum(['dispatch', 'reconcile']) }).strict() }).strict(),
  z.object({ kind: z.literal('acknowledged'), identity: identitySchema, through: sequence }).strict(),
  z.object({ kind: z.literal('ready'), identity: identitySchema, ready: z.boolean() }).strict(),
  z.object({ kind: z.literal('mappings_retired'), identity: identitySchema, nonce: uuid, sessions: z.array(uuid).max(100) }).strict(),
]);
export type ServerMessage = z.infer<typeof serverSchema>;
export const sha256 = (bytes: string | Buffer) => createHash('sha256').update(bytes).digest('hex');
export const isEffect = (command: Command) => command.kind === 'create' || command.kind === 'send';
export const sessionId = (command: Command) => 'session_id' in command ? command.session_id : undefined;
export const targetThread = (command: Command) => 'thread_id' in command ? command.thread_id : undefined;
export const observationSequence = (observation: Observation) => observation.kind === 'agent' ? observation.observation.sequence : observation.sequence;
export function commandDigest(command: Command, exactDeadline: string): string {
  return sha256(JSON.stringify([commandSchema.parse(command), deadline.parse(exactDeadline)]));
}
export function parseOperation(value: unknown): Operation {
  const operation = operationSchema.parse(value);
  if (commandDigest(operation.command, operation.deadline) !== operation.payload_digest) throw new Error('command digest mismatch');
  const evidence = operation.dispatch;
  if (evidence.kind === 'native_identity' && targetThread(operation.command)
      && evidence.thread_id !== targetThread(operation.command)) throw new Error('command thread mismatch');
  if ((evidence.kind === 'catalog_snapshot') !== (operation.command.kind === 'refresh_catalog')
      && evidence.kind === 'catalog_snapshot') throw new Error('invalid catalog receipt');
  if (operation.command.kind === 'refresh_catalog' && evidence.kind === 'native_identity') throw new Error('invalid native receipt');
  if (operation.outcome === 'succeeded' && !(evidence.kind === 'catalog_snapshot'
      || (evidence.kind === 'native_identity' && (!isEffect(operation.command) || evidence.turn_id !== null)))) {
    throw new Error('success without native evidence');
  }
  return operation;
}
export function parseServer(value: unknown): ServerMessage {
  const parsed = serverSchema.parse(value);
  if (parsed.kind === 'command') parsed.delivery.operation = parseOperation(parsed.delivery.operation);
  return parsed;
}
export function nativeInput(operation: Operation): string {
  if (!('prompt' in operation.command)) throw new Error('not an effect');
  const header = `[Openbot operation ${operation.id}]\n`;
  if (Buffer.byteLength(header) !== 57) throw new Error('invalid operation identity');
  return header + operation.command.prompt;
}
export function encodeFrames(message: unknown): string[] {
  const bytes = Buffer.from(JSON.stringify(message));
  if (!bytes.length || bytes.length > LIMITS.logical) throw new Error('logical message bound');
  const transfer_id = randomUUID(); const digest = sha256(bytes);
  const frames: string[] = [];
  for (let index = 0; index * LIMITS.chunk < bytes.length; index++) {
    frames.push(JSON.stringify({ version: 1, transfer_id, index, parts: Math.ceil(bytes.length / LIMITS.chunk),
      total_bytes: bytes.length, digest, data: bytes.subarray(index * LIMITS.chunk, (index + 1) * LIMITS.chunk).toString('base64') }));
  }
  return frames;
}
const chunkSchema = z.object({ version: z.literal(1), transfer_id: uuid, index: sequence,
  parts: positive.max(128), total_bytes: positive.max(LIMITS.logical), digest: digestSchema,
  data: z.string().max(Math.ceil(LIMITS.chunk / 3) * 4) }).strict();
type PartialMessage = { transfer: string; next: number; parts: number; size: number; digest: string; chunks: Buffer[]; expires: number };
export class FrameAssembler {
  private pending: PartialMessage | undefined;
  private readonly now: () => number;
  constructor(now: () => number = () => performance.now()) { this.now = now; }
  expire(): void { if (this.pending && this.now() >= this.pending.expires) throw new Error('chunk assembly expired'); }
  push(frame: Buffer | string): unknown | undefined {
    if (Buffer.byteLength(frame) > LIMITS.frame) throw new Error('physical frame bound');
    this.expire();
    const source = typeof frame === 'string' ? frame : new TextDecoder('utf-8', { fatal: true }).decode(frame);
    const c = chunkSchema.parse(JSON.parse(source));
    if (c.parts !== Math.ceil(c.total_bytes / LIMITS.chunk) || c.index >= c.parts) throw new Error('invalid chunk bounds');
    const data = Buffer.from(c.data, 'base64');
    const expected = c.index + 1 === c.parts ? c.total_bytes - c.index * LIMITS.chunk : LIMITS.chunk;
    if (data.length !== expected || data.toString('base64') !== c.data) throw new Error('invalid chunk encoding');
    this.pending ??= { transfer: c.transfer_id, next: 0, parts: c.parts, size: c.total_bytes,
      digest: c.digest, chunks: [], expires: this.now() + LIMITS.assemblyMs };
    const p = this.pending;
    if (p.next !== c.index || p.transfer !== c.transfer_id || p.parts !== c.parts || p.size !== c.total_bytes || p.digest !== c.digest) {
      throw new Error('interleaved or reused chunk');
    }
    p.chunks.push(data); p.next++;
    if (p.next !== p.parts) return undefined;
    this.pending = undefined;
    const bytes = Buffer.concat(p.chunks, p.size);
    if (sha256(bytes) !== p.digest) throw new Error('chunk digest mismatch');
    return JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(bytes));
  }
}
