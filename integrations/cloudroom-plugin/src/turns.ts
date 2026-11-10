import type { BbPluginApi } from '@get-bb/plugin-sdk';
import type { CommandRecord } from './journal.js';
import { LIMITS, type Evidence } from './protocol.js';
type Event = Awaited<ReturnType<BbPluginApi['sdk']['threads']['events']['list']>>[number];
export type TurnUpdate = { kind: 'progress' | 'terminal' | 'uncertain' | 'unchanged'; record: CommandRecord; evidence?: Evidence };

function ownInput(input: unknown, expected: string): boolean {
  if (!Array.isArray(input) || input.length !== 1) return false;
  const first: unknown = input[0];
  return typeof first === 'object' && first !== null && 'type' in first && first.type === 'text'
    && 'text' in first && first.text === expected
    && (!('mentions' in first) || (Array.isArray(first.mentions) && first.mentions.length === 0));
}
function replaceText(record: CommandRecord, id: string, value: string): void {
  if (!id || id.length > 128) { record.truncated = true; return; }
  let item = record.items.find(item => item.id === id);
  if (!item) {
    if (record.items.length >= 1024) { record.truncated = true; return; }
    item = { id, text: '' }; record.items.push(item);
  }
  const other = record.items.reduce((sum, candidate) => sum + (candidate === item ? 0 : Buffer.byteLength(candidate.text)), 0);
  const remaining = Math.max(0, LIMITS.output - other - Math.max(0, record.items.length - 1) * 2);
  const clipped = value.slice(0, LIMITS.output + 1).replaceAll('\0', '\ufffd');
  const bytes = Buffer.from(clipped, 'utf8'); let end = Math.min(bytes.length, remaining);
  while (end > 0 && end < bytes.length && (bytes[end]! & 0xc0) === 0x80) end--;
  item.text = bytes.subarray(0, end).toString('utf8');
  if (value !== item.text) record.truncated = true;
}
function output(record: CommandRecord): string | null {
  const result = record.items.map(item => item.text).filter(Boolean).join('\n\n');
  return result.length ? result : null;
}
function evidence(record: CommandRecord): Evidence | undefined {
  if (!record.threadId || !record.turnId || !record.nativeState) return undefined;
  const terminal = ['completed', 'failed', 'stopped'].includes(record.nativeState);
  return { kind: 'turn', thread_id: record.threadId, turn_id: record.turnId, state: record.nativeState,
    output: terminal ? output(record) : null, truncated: record.truncated, usage: record.usage };
}
export function applyNativeEvents(original: CommandRecord, events: readonly Event[]): TurnUpdate {
  if (events.length > LIMITS.page || !original.threadId) throw new Error('native event page bound');
  const record = structuredClone(original);
  if (record.correlationConflict) return { kind: 'uncertain', record };
  const conflict = (): TurnUpdate => {
    record.correlationConflict = true; record.stage = 'uncertain';
    return { kind: 'uncertain', record };
  };
  for (const event of events) {
    if (!Number.isSafeInteger(event.seq) || event.seq <= record.cursor) throw new Error('native event cursor did not advance');
    // Cloudroom prunes resolved streaming deltas without renumbering retained
    // events. A gap is not a lost request/turn binding; expose incomplete history
    // conservatively while still requiring every positive correlation below.
    if (event.seq > record.cursor + 1) record.truncated = true;
    record.cursor = event.seq;
    if (event.type === 'client/turn/requested' && ownInput(event.data.input, record.sdkInput)
        && event.data.source === (record.operation.command.kind === 'create' ? 'spawn' : 'tell')) {
      if (record.requestId && record.requestId !== event.data.requestId) return conflict();
      record.requestId = event.data.requestId;
    }
    if (event.type === 'turn/input/accepted' && record.requestId !== null && event.data.clientRequestId === record.requestId) {
      if (event.scope.kind !== 'turn' || (record.turnId && record.turnId !== event.scope.turnId)) return conflict();
      record.turnId = event.scope.turnId; record.nativeState = 'running'; record.stage = 'observing';
    }
    if (event.scope.kind !== 'turn' || event.scope.turnId !== record.turnId) continue;
    if (event.type.startsWith('item/') && record.nativeState === 'waiting_for_input') record.nativeState = 'running';
    if (event.type === 'item/completed'
        && event.data.item.type === 'agentMessage' && !event.data.item.parentToolCallId) {
      replaceText(record, event.data.item.id, event.data.item.text);
    }
    if (event.type === 'turn/completed') {
      record.nativeState = event.data.status === 'completed' ? 'completed' : event.data.status === 'interrupted' ? 'stopped' : 'failed';
      record.stage = 'terminal';
      return { kind: 'terminal', record, evidence: evidence(record) };
    }
  }
  const observed = evidence(record);
  const prior = evidence(original);
  return { kind: JSON.stringify(observed) === JSON.stringify(prior) ? 'unchanged' : 'progress', record, evidence: observed };
}
