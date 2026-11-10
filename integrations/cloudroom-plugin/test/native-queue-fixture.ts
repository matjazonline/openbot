import assert from 'node:assert/strict';
import type { BbPluginApi } from '@get-bb/plugin-sdk';
import { nativeFixture, event } from './native-fixture.js';
import { fixture } from './support.js';
type Send = BbPluginApi['sdk']['threads']['send'];
type Prompt = { input: string; request: string; turn: string; queue: string };

// Test the native queue authority, not a second queue in the plugin. This models
// the inspected upstream queue-if-active decision at SDK entry: start when idle,
// otherwise append; accepted native requests appear when the queued item starts.
export function nativeQueueFixture(f: ReturnType<typeof fixture>) {
  const n = nativeFixture(f.journal, f.selection); const pending: Prompt[] = []; const started: string[] = [];
  let active: Prompt | undefined; let next = 0;
  let beforeSend: (() => void) | undefined; let loseResponse = false;
  const emit = (type: Parameters<typeof event>[1], data: unknown, turn?: string) => {
    n.state.events.push(event((n.state.events.at(-1)?.seq ?? 0) + 1, type, data, turn));
  };
  const start = (prompt: Prompt) => {
    assert(!active); active = prompt; started.push(prompt.input); n.state.thread.status = 'active';
    emit('client/turn/requested', { input: [{ type: 'text', text: prompt.input, mentions: [] }], source: 'tell', requestId: prompt.request });
    emit('turn/input/accepted', { clientRequestId: prompt.request }, prompt.turn);
  };
  const preexisting = (kind: 'human' | 'openbot') => start({ input: kind === 'human' ? 'Human authorized work'
    : '[Openbot operation 11111111-1111-1111-1111-111111111111]\nEarlier local work', request: `request_${kind}`, turn: `turn_${kind}`, queue: '' });
  const finish = (text: string) => {
    assert(active); const turn = active.turn;
    emit('item/completed', { item: { type: 'agentMessage', id: `item_${turn}`, text } }, turn);
    emit('turn/completed', { status: 'completed' }, turn); active = undefined; n.state.thread.status = 'idle';
    const next = pending.shift(); if (next) start(next);
  };
  const originalSend = n.bb.sdk.threads.send.bind(n.bb.sdk.threads);
  n.bb.sdk.threads.send = (async args => {
    await originalSend(args); assert.equal(args.mode, 'queue-if-active');
    const hook = beforeSend; beforeSend = undefined; hook?.();
    const block = args.input[0]; assert(block?.type === 'text');
    const prompt: Prompt = { input: block.text, request: `request_native_${++next}`, turn: `turn_native_${next}`, queue: `queue_native_${next}` };
    const queued = active !== undefined;
    if (queued) pending.push(prompt); else start(prompt);
    if (loseResponse) { loseResponse = false; throw new Error('native accepted then SDK response lost'); }
    return queued ? { delivery: 'queued', queuedMessage: { id: prompt.queue } } : { delivery: 'sent' };
  }) as Send;
  return { ...n, pending, started, preexisting, finish,
    beforeSend: (hook: () => void) => { beforeSend = hook; }, loseNextResponse: () => { loseResponse = true; } };
}
