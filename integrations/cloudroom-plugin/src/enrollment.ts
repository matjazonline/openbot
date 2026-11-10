import type { BbPluginApi } from '@get-bb/plugin-sdk';
import { randomBytes, randomUUID } from 'node:crypto';
import { z } from 'zod';
import { Journal, RecoveryRequired, bindingSchema, type Binding } from './journal.js';
import { external, LIMITS, sha256, uuid } from './protocol.js';

export const settingDescriptors = {
  openbotOrigin: { type: 'string', label: 'Openbot HTTPS origin' },
  machineName: { type: 'string', label: 'Machine name', default: 'Local machine' },
  selectedProjects: { type: 'string', label: 'Selected project IDs', description: 'One local project ID per line.', experimental_multiline: true, default: '' },
  pairingCode: { type: 'string', label: 'Pairing code', secret: true },
  enrollment: { type: 'string', label: 'Connection credential (managed automatically)', secret: true },
} as const;
export function defineSettings(bb: Pick<BbPluginApi, 'settings'>) { return bb.settings.define(settingDescriptors); }
export type SettingsHandle = ReturnType<typeof defineSettings>;
export type SettingsStore = Pick<SettingsHandle, 'get' | 'experimental_set'>;
export class ConfigurationRequired extends Error {
  constructor(message: string) { super(message); this.name = 'NeedsConfigurationError'; }
}
const secret = z.string().regex(/^[0-9a-f]{64}$/);
const nameSchema = z.string().refine(value => value.trim().length > 0 && Buffer.byteLength(value) <= 128 && !/[\p{Cc}]/u.test(value));
const requestSchema = z.object({ code: z.string().regex(/^[0-9a-f]{32}$/), installation_id: uuid,
  name: nameSchema, claim_verifier: secret, credential_verifier: secret }).strict();
const pendingSchema = z.object({ state: z.literal('pending'), version: z.literal(1), origin: z.string(),
  installation_id: uuid, journal_id: uuid, claim_secret: secret, credential: secret, request: requestSchema }).strict();
const pairedSchema = z.object({ state: z.literal('paired'), version: z.literal(1), binding: bindingSchema, credential: secret }).strict();
export const enrollmentSchema = z.discriminatedUnion('state', [pendingSchema, pairedSchema]);
export type Paired = z.infer<typeof pairedSchema>;
type Pending = z.infer<typeof pendingSchema>;
const receiptSchema = z.object({ company_id: uuid, company_name: z.string().max(8192),
  machine: z.object({ id: uuid, company_id: uuid, installation_id: uuid, name: nameSchema,
    state: z.enum(['enabled', 'disabled', 'revoked']), credential_generation: z.number().int().positive().max(Number.MAX_SAFE_INTEGER),
    last_seen_at: z.string().nullable(), capabilities_sequence: z.number().int().positive().max(Number.MAX_SAFE_INTEGER).nullable() }).strict() }).strict();

export function canonicalOrigin(value: string | undefined): string {
  if (!value || value.length > 2048) throw new ConfigurationRequired('Configure the Openbot HTTPS origin');
  let url: URL;
  try { url = new URL(value); } catch { throw new ConfigurationRequired('Configure a valid Openbot HTTPS origin'); }
  if (url.protocol !== 'https:' || url.username || url.password || url.search || url.hash || url.pathname !== '/') {
    throw new ConfigurationRequired('Openbot origin must use HTTPS without credentials, path or query');
  }
  return url.origin;
}
export function selectedProjects(value: string): string[] {
  if (Buffer.byteLength(value) > LIMITS.projects * 130) throw new ConfigurationRequired('Too many selected projects');
  const ids = value.split(/\r?\n/).map(value => value.trim()).filter(Boolean);
  if (ids.length > LIMITS.projects || new Set(ids).size !== ids.length || !ids.every(value => external.safeParse(value).success)) {
    throw new ConfigurationRequired('Select at most 128 unique local project IDs');
  }
  return ids;
}
async function readBoundedBody(response: Response, limit: number): Promise<Buffer> {
  if (!response.body) return Buffer.alloc(0);
  const declared = Number(response.headers.get('content-length'));
  if (declared > limit) { await response.body.cancel(); throw new Error('HTTP response bound'); }
  const reader = response.body.getReader(); const parts: Uint8Array[] = []; let bytes = 0;
  try {
    while (true) {
      const result = await reader.read();
      if (result.done) break;
      bytes += result.value.byteLength;
      if (bytes > limit) throw new Error('HTTP response bound');
      parts.push(result.value);
    }
    return Buffer.concat(parts);
  } catch (error) { await reader.cancel().catch(() => {}); throw error; }
  finally { reader.releaseLock(); }
}
export async function readBoundedJson(response: Response, limit: number): Promise<unknown> {
  return JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(await readBoundedBody(response, limit)));
}
function validatePending(value: Pending): void {
  if (canonicalOrigin(value.origin) !== value.origin || value.request.installation_id !== value.installation_id
      || sha256(value.claim_secret) !== value.request.claim_verifier || sha256(value.credential) !== value.request.credential_verifier) {
    throw new RecoveryRequired();
  }
}
async function currentCandidate(settings: SettingsStore, candidate: Pending): Promise<void> {
  const current = await settings.get();
  if (canonicalOrigin(current.openbotOrigin) !== candidate.origin || current.enrollment !== JSON.stringify(candidate)) {
    throw new ConfigurationRequired('Pairing settings changed; restore the approved origin or repair pairing');
  }
}
async function exchange(settings: SettingsStore, journal: Journal, candidate: Pending, signal: AbortSignal, fetcher: typeof fetch) {
  await currentCandidate(settings, candidate);
  signal.throwIfAborted();
  const bounded = AbortSignal.any([signal, AbortSignal.timeout(LIMITS.handshakeMs)]);
  const status = await fetcher(`${candidate.origin}/v1/local-machines/pair-status`, { redirect: 'error', signal: bounded,
    headers: { 'x-openbot-pair-claim': candidate.claim_secret, 'x-openbot-installation-id': candidate.installation_id } });
  if (status.ok) return receiptSchema.parse(await readBoundedJson(status, 16 * 1024));
  await status.body?.cancel();
  if (status.status !== 401 && status.status !== 404) throw new Error('pairing status unavailable');
  await currentCandidate(settings, candidate);
  signal.throwIfAborted();
  const enrolled = await fetcher(`${candidate.origin}/v1/local-machines/pair`, { method: 'POST', redirect: 'error', signal: bounded,
    headers: { 'content-type': 'application/json' }, body: JSON.stringify(candidate.request) });
  if (!enrolled.ok) {
    // Only a completely received rejection disposes an initial candidate. A
    // truncated/failed response retains its exact original recovery identity.
    await readBoundedBody(enrolled, 16 * 1024);
    if (enrolled.status === 400 || enrolled.status === 401 || enrolled.status === 403 || enrolled.status === 404) {
      await currentCandidate(settings, candidate); signal.throwIfAborted();
      if (journal.initialized()) throw new RecoveryRequired();
      // Clear input before the authoritative candidate: an interruption between
      // these separate secret writes cannot accidentally enroll the old code as
      // a new installation. An ambiguous Pending record is never cleared here.
      await settings.experimental_set({ pairingCode: null });
      await currentCandidate(settings, candidate); signal.throwIfAborted();
      await settings.experimental_set({ enrollment: null });
      throw new ConfigurationRequired('Pairing was not accepted; enter a new pairing code to retry');
    }
    throw new Error('pairing unavailable');
  }
  return receiptSchema.parse(await readBoundedJson(enrolled, 16 * 1024));
}
export async function recoverEnrollment(settings: SettingsStore, journal: Journal, signal: AbortSignal,
  fetcher: typeof fetch = fetch): Promise<Paired> {
  const values = await settings.get(); const origin = canonicalOrigin(values.openbotOrigin);
  let state: z.infer<typeof enrollmentSchema>;
  if (values.enrollment === undefined) {
    if (journal.initialized()) throw new RecoveryRequired();
    const credential = randomBytes(32).toString('hex'); const claim_secret = randomBytes(32).toString('hex');
    const installation_id = randomUUID(); const journal_id = randomUUID();
    const request = requestSchema.safeParse({ code: values.pairingCode, installation_id, name: values.machineName,
      claim_verifier: sha256(claim_secret), credential_verifier: sha256(credential) });
    if (!request.success) throw new ConfigurationRequired('Enter a valid pairing code and machine name');
    state = { state: 'pending', version: 1, origin, installation_id, journal_id, claim_secret, credential, request: request.data };
    await settings.experimental_set({ enrollment: JSON.stringify(state) });
  } else {
    try { state = enrollmentSchema.parse(JSON.parse(values.enrollment)); } catch { throw new RecoveryRequired(); }
  }
  if (state.state === 'paired') {
    if (origin !== state.binding.origin) throw new ConfigurationRequired('Approved Openbot origin changed; repair pairing explicitly');
    journal.validateBinding(state.binding);
    if (values.pairingCode !== undefined) await settings.experimental_set({ pairingCode: null });
    return state;
  }
  validatePending(state);
  if (origin !== state.origin) throw new ConfigurationRequired('Pending enrollment belongs to the original approved origin');
  const receipt = await exchange(settings, journal, state, signal, fetcher);
  if (receipt.company_id !== receipt.machine.company_id || receipt.machine.installation_id !== state.installation_id
      || receipt.machine.state === 'revoked') throw new RecoveryRequired();
  await currentCandidate(settings, state);
  signal.throwIfAborted();
  const binding: Binding = { origin, installation_id: state.installation_id, journal_id: state.journal_id,
    machine_id: receipt.machine.id, company_id: receipt.company_id, credential_generation: receipt.machine.credential_generation };
  journal.initialize(binding);
  const paired: Paired = { state: 'paired', version: 1, binding, credential: state.credential };
  await settings.experimental_set({ enrollment: JSON.stringify(paired) });
  await settings.experimental_set({ pairingCode: null });
  return paired;
}
