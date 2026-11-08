import { Journal, type CommandRecord } from './journal.js';
import { CloudroomSdk, PreflightRefusal, sdkSignal } from './sdk.js';
import { isEffect, LIMITS, observationSequence, type Delivery, type Observation, type Refusal } from './protocol.js';
export type DispatchContext = { signal: AbortSignal; canDispatch: () => boolean };

export class Dispatcher {
  readonly journal: Journal; readonly sdk: CloudroomSdk;
  constructor(journal: Journal, sdk: CloudroomSdk) { this.journal = journal; this.sdk = sdk; }
  private cached(record: CommandRecord): Observation | undefined {
    return record.lastObservation && observationSequence(record.lastObservation) <= this.journal.acknowledged()
      ? record.lastObservation : undefined;
  }
  private refuse(record: CommandRecord, reason: Refusal): void {
    record.stage = 'terminal'; this.journal.evidence(record, { kind: 'refused', reason }, 'terminal');
  }
  private uncertain(record: CommandRecord): void {
    if (record.stage === 'terminal') return;
    record.stage = isEffect(record.operation.command) ? 'uncertain' : 'terminal';
    const old = record.lastObservation;
    if (old?.kind === 'agent' && old.observation.evidence.kind === 'uncertain') { this.journal.save(record); return; }
    this.journal.evidence(record, { kind: 'uncertain' }, record.stage === 'terminal' ? 'terminal' : 'critical');
  }
  private blocked(record: CommandRecord, context: DispatchContext): Refusal | undefined {
    if (Date.parse(record.operation.deadline) <= Date.now()) return 'expired';
    if (this.journal.recoveryRequired() || !context.canDispatch() || context.signal.aborted) return 'recovery_required';
    const counts = this.journal.capacity(); const effect = isEffect(record.operation.command);
    if (counts.queued > (effect ? LIMITS.queued - LIMITS.controls : LIMITS.queued)
        || (effect && counts.effects > LIMITS.effects)) return 'capacity';
    return undefined;
  }
  async process(delivery: Delivery, context: DispatchContext): Promise<Observation | undefined> {
    const admitted = this.journal.admit(delivery); const record = admitted.record;
    if (record.stage === 'terminal') return this.cached(record);
    const command = record.operation.command;
    try {
      if (command.kind === 'refresh_catalog' && record.sdkCalls === 0) {
        this.journal.catalog(record, await this.sdk.catalog(sdkSignal(context.signal)));
      } else if (command.kind === 'inspect' && record.sdkCalls === 0) {
        await this.sdk.prepare(record, sdkSignal(context.signal));
        const state = await this.sdk.inspect(command.thread_id, sdkSignal(context.signal));
        if (state) this.journal.control(record, command.thread_id, { kind: 'snapshot', state });
        else this.uncertain(record);
      } else if (record.sdkCalls !== 0) {
        await this.reconcile(record, context.signal);
      } else if (delivery.mode === 'reconcile' || record.stage !== 'received') {
        if (record.stage === 'received') this.refuse(record, 'recovery_required');
      } else {
        const blocked = this.blocked(record, context);
        if (blocked) { this.refuse(record, blocked); return this.cached(record); }
        const prepared = await this.sdk.prepare(record, sdkSignal(context.signal));
        await this.sdk.baseline(record, sdkSignal(context.signal));
        const withdrawn = this.blocked(record, context);
        if (withdrawn) { this.refuse(record, withdrawn); return this.cached(record); }
        this.journal.beginIntent(record);
        const signal = sdkSignal(context.signal);
        signal.throwIfAborted();
        await this.sdk.mutate(record, prepared, signal);
      }
    } catch (error) {
      // Reload current durable state: a failed local commit after SDK acceptance is
      // still an intent gap, never permission to repeat the SDK call.
      const current = this.journal.get(record.operation.id)!;
      if (current.stage !== 'terminal') {
        if (current.sdkCalls === 0 && current.stage === 'received') {
          this.refuse(current, error instanceof PreflightRefusal ? error.reason : 'provider_unavailable');
        } else this.uncertain(current);
      }
    }
    return this.cached(this.journal.get(record.operation.id)!);
  }
  private async reconcile(record: CommandRecord, signal: AbortSignal): Promise<void> {
    if (!isEffect(record.operation.command)) { this.uncertain(record); return; }
    const update = await this.sdk.poll(record, sdkSignal(signal));
    if (update.kind === 'uncertain') { this.uncertain(update.record); return; }
    if (update.evidence && update.kind !== 'unchanged') {
      this.journal.evidence(update.record, update.evidence, update.kind === 'terminal' ? 'terminal' : 'progress');
    } else this.journal.save(update.record);
  }
  async tick(signal: AbortSignal): Promise<void> {
    for (const record of this.journal.pending()) {
      if (signal.aborted) break;
      if (record.sdkCalls !== 1 || !isEffect(record.operation.command)) continue;
      try { await this.reconcile(record, signal); }
      catch { this.uncertain(this.journal.get(record.operation.id)!); }
    }
  }
}
