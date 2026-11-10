import type { BbPluginApi } from '@get-bb/plugin-sdk';
import { Journal, RecoveryRequired, type CommandRecord } from './journal.js';
import { catalogSchema, LIMITS, provider, reasoning, type Catalog, type Evidence, type Selection, type Refusal } from './protocol.js';
import { applyNativeEvents, type TurnUpdate } from './turns.js';

type SDK = BbPluginApi['sdk'];
type ModelPage = Awaited<ReturnType<SDK['providers']['models']>>;
type ListedThread = Awaited<ReturnType<SDK['threads']['list']>>[number];
export class PreflightRefusal extends Error {
  readonly reason: Refusal;
  constructor(reason: Refusal) { super('local preflight refused'); this.reason = reason; }
}
export type Prepared = { selection: Selection; hostId: string; environmentProvider: string };
export type ProviderHealth = { provider: string; status: 'available' | 'auth_required' | 'unavailable' | 'oversized' };
export const sdkSignal = (parent: AbortSignal) => AbortSignal.any([parent, AbortSignal.timeout(LIMITS.ioMs)]);
function missing(error: unknown): boolean { return typeof error === 'object' && error !== null && 'status' in error && error.status === 404; }
function stateUpdate(record: CommandRecord, evidence: Evidence): TurnUpdate {
  const previous = record.lastObservation;
  return { kind: previous?.kind === 'agent' && JSON.stringify(previous.observation.evidence) === JSON.stringify(evidence)
    ? 'unchanged' : 'progress', record, evidence };
}

export class CloudroomSdk {
  readonly sdk: SDK; readonly pluginId: string; readonly journal: Journal;
  readonly health = new Map<string, ProviderHealth>();
  private readonly projects: () => readonly string[];
  constructor(bb: Pick<BbPluginApi, 'sdk' | 'pluginId'>, journal: Journal, projects: () => readonly string[]) {
    this.sdk = bb.sdk; this.pluginId = bb.pluginId; this.journal = journal; this.projects = projects;
  }
  async localHost(signal: AbortSignal): Promise<string> {
    const config = await this.sdk.system.config({ signal });
    if (!config.primaryHostId) throw new PreflightRefusal('project_unavailable');
    const host = await this.sdk.hosts.get({ hostId: config.primaryHostId, signal });
    if (host.status !== 'connected' || host.type !== 'persistent' || host.machineProviderId !== null
        || host.connectMachineId !== null || !['darwin', 'linux'].includes(host.platform ?? '')
        || host.platform !== process.platform) throw new PreflightRefusal('project_unavailable');
    return host.id;
  }
  private async project(projectId: string, hostId: string, signal: AbortSignal): Promise<void> {
    if (!this.projects().includes(projectId)) throw new PreflightRefusal('project_unavailable');
    const project = await this.sdk.projects.get({ projectId, signal });
    if (project.id !== projectId || !project.sources.some(source => source.type === 'local_path' && source.hostId === hostId)) {
      throw new PreflightRefusal('project_unavailable');
    }
  }
  private async models(providerId: Selection['provider'], hostId: string, signal: AbortSignal): Promise<ModelPage> {
    const page = await this.sdk.providers.models({ providerId, hostId, signal });
    const status = page.modelLoadError?.code === 'auth_required' ? 'auth_required'
      : page.modelLoadError ? 'unavailable' : page.models.length > LIMITS.models ? 'oversized' : 'available';
    this.health.set(providerId, { provider: providerId, status });
    if (page.modelLoadError) throw new PreflightRefusal('provider_unavailable');
    if (page.models.length > LIMITS.models) throw new PreflightRefusal('capacity');
    return page;
  }
  async catalog(signal: AbortSignal): Promise<Catalog> {
    const hostId = await this.localHost(signal); const projects = [...this.projects()];
    if (projects.length > LIMITS.projects) throw new PreflightRefusal('capacity');
    for (const project of projects) await this.project(project, hostId, signal);
    const installed = await this.sdk.providers.list({ hostId, signal });
    if (installed.length > LIMITS.providers) throw new PreflightRefusal('capacity');
    const providers: Catalog['providers'] = [];
    for (const id of provider.options) {
      if (!installed.some(item => item.id === id)) { this.health.set(id, { provider: id, status: 'unavailable' }); continue; }
      try {
        const page = await this.models(id, hostId, signal);
        const models = page.models.map(model => ({ model: model.model, reasoning_levels: [...new Set(
          [...model.supportedReasoningEfforts.map(option => option.reasoningEffort), model.defaultReasoningEffort]
            .filter(value => reasoning.safeParse(value).success))] }));
        providers.push({ provider: id, models });
      } catch (error) { if (signal.aborted) throw error; if (!(error instanceof PreflightRefusal)) throw error; }
    }
    return catalogSchema.parse({ projects, providers });
  }
  async prepare(record: CommandRecord, signal: AbortSignal): Promise<Prepared> {
    const command = record.operation.command; const hostId = await this.localHost(signal);
    let selection: Selection;
    if (command.kind === 'create') {
      if (this.journal.session(command.session_id)) throw new PreflightRefusal('recovery_required');
      selection = command.selection;
      if (selection.machine_id !== this.journal.binding().machine_id) throw new PreflightRefusal('recovery_required');
    } else if ('session_id' in command) {
      const mapped = this.journal.session(command.session_id);
      if (!mapped || !('thread_id' in command) || mapped.threadId !== command.thread_id) throw new PreflightRefusal('recovery_required');
      selection = mapped.selection;
      const thread = await this.sdk.threads.get({ threadId: command.thread_id, signal });
      if (thread.projectId !== selection.project_id || thread.providerId !== selection.provider) throw new PreflightRefusal('recovery_required');
      record.threadId = command.thread_id;
      if (thread.environmentId) {
        const environment = await this.sdk.environments.get({ environmentId: thread.environmentId, signal });
        if (environment.hostId !== hostId || environment.projectId !== selection.project_id) throw new PreflightRefusal('project_unavailable');
      }
    } else throw new PreflightRefusal('recovery_required');
    if (command.kind === 'create' || command.kind === 'send') {
      await this.project(selection.project_id, hostId, signal);
      const installed = await this.sdk.providers.list({ hostId, signal });
      if (installed.length > LIMITS.providers) throw new PreflightRefusal('capacity');
      if (!installed.some(provider => provider.id === selection.provider)) throw new PreflightRefusal('provider_unavailable');
      const page = await this.models(selection.provider, hostId, signal);
      const model = page.models.find(model => model.model === selection.model);
      if (!model) throw new PreflightRefusal('model_unavailable');
      if (model.defaultReasoningEffort !== selection.policy.reasoning
          && !model.supportedReasoningEfforts.some(level => level.reasoningEffort === selection.policy.reasoning)) {
        throw new PreflightRefusal('reasoning_unavailable');
      }
    }
    let environmentProvider = 'project-checkout';
    if (command.kind === 'create') {
      const providers = await this.sdk.environments.listProviders({ projectId: selection.project_id, hostId, signal });
      const workspace = providers.find(item => (item.environmentProviderId ?? item.id) === 'project-checkout'
        && item.machineProviderId === null && item.requires.projectCheckout && item.acceptsEmptyInputs);
      const available = workspace?.machineAvailability[hostId] ?? workspace?.availability;
      if (!workspace || (available && available.status !== 'available')) throw new PreflightRefusal('project_unavailable');
      environmentProvider = workspace.environmentProviderId ?? workspace.id;
    }
    return { selection, hostId, environmentProvider };
  }
  metadata(record: CommandRecord) {
    const command = record.operation.command;
    if (!('session_id' in command)) throw new RecoveryRequired();
    return { installation: this.journal.binding().installation_id, session: command.session_id,
      operation: record.operation.id, digest: record.operation.payload_digest };
  }
  async mutate(record: CommandRecord, prepared: Prepared, signal: AbortSignal): Promise<void> {
    const command = record.operation.command;
    if (command.kind === 'create') {
      const thread = await this.sdk.threads.spawn({ projectId: prepared.selection.project_id, executionTarget: 'local',
        providerId: prepared.selection.provider, model: prepared.selection.model, reasoningLevel: prepared.selection.policy.reasoning,
        environment: { type: 'provider', environmentProviderId: prepared.environmentProvider,
          machine: { type: 'existing', hostId: prepared.hostId }, inputs: {} },
        title: 'Openbot local session', prompt: record.sdkInput, pluginMetadata: this.metadata(record), signal });
      record.threadId = thread.id;
      this.journal.submission(record, prepared.selection, thread.status === 'pending' ? 'queued' : 'accepted');
    } else if (command.kind === 'send') {
      const result = await this.sdk.threads.send({ threadId: command.thread_id, mode: 'queue-if-active', model: prepared.selection.model,
        reasoningLevel: prepared.selection.policy.reasoning, input: [{ type: 'text', text: record.sdkInput, mentions: [] }], signal });
      record.threadId = command.thread_id;
      if (result.delivery === 'queued') record.queueId = result.queuedMessage.id;
      this.journal.submission(record, prepared.selection, result.delivery === 'queued' ? 'queued' : 'accepted');
    } else if (command.kind === 'stop_thread') {
      await this.sdk.threads.stop({ threadId: command.thread_id, signal });
      this.journal.control(record, command.thread_id, { kind: 'stop_accepted' });
    } else if (command.kind === 'archive_thread') {
      await this.sdk.threads.archive({ threadId: command.thread_id, signal });
      this.journal.control(record, command.thread_id, { kind: 'archive_accepted' });
    } else throw new Error('not a mutation');
  }
  async baseline(record: CommandRecord, signal: AbortSignal): Promise<void> {
    if (!record.threadId) return;
    const page = await this.sdk.threads.events.list({ threadId: record.threadId, limit: '1', order: 'desc', signal });
    if (page.length > 1) throw new Error('native baseline bound');
    const cursor = page[0]?.seq ?? 0;
    if (!Number.isSafeInteger(cursor) || cursor < 0) throw new Error('invalid native cursor');
    record.cursor = cursor;
  }
  async recoverCreate(record: CommandRecord, signal: AbortSignal): Promise<boolean> {
    const command = record.operation.command;
    if (command.kind !== 'create' || record.threadId) return false;
    const expected = this.metadata(record); const matches: ListedThread[] = [];
    for (let offset = 0; offset < 400; offset += 100) {
      const page = await this.sdk.threads.list({ projectId: command.selection.project_id, originPluginId: this.pluginId,
        limit: 100, offset, signal });
      if (page.length > 100) throw new Error('native recovery page bound');
      for (const thread of page) {
        if (thread.projectId !== command.selection.project_id || thread.providerId !== command.selection.provider
            || thread.originPluginId !== this.pluginId) continue;
        const metadata = await this.sdk.threads.getPluginMetadata({ threadId: thread.id, signal });
        if (Object.entries(expected).every(([key, value]) => metadata[key] === value)) matches.push(thread);
        if (matches.length > 1) return false;
      }
      if (page.length < 100) break;
    }
    const thread = matches[0];
    if (!thread) return false;
    record.threadId = thread.id;
    this.journal.submission(record, command.selection, thread.status === 'pending' ? 'queued' : 'accepted');
    return true;
  }
  async poll(record: CommandRecord, signal: AbortSignal): Promise<TurnUpdate> {
    if (!record.threadId && record.operation.command.kind === 'send') {
      const mapping = this.journal.session(record.operation.command.session_id);
      if (mapping?.threadId === record.operation.command.thread_id) record.threadId = mapping.threadId;
    }
    if (!record.threadId) await this.recoverCreate(record, signal);
    if (!record.threadId) return { kind: 'uncertain', record };
    let nativeFailed: boolean;
    try { nativeFailed = (await this.sdk.threads.get({ threadId: record.threadId, signal })).status === 'error'; }
    catch (error) {
      if (!missing(error)) throw error;
      if (record.turnId) {
        record.nativeState = 'missing'; record.stage = 'uncertain';
        return stateUpdate(record, { kind: 'turn', thread_id: record.threadId, turn_id: record.turnId,
          state: 'missing', output: null, truncated: record.truncated, usage: null });
      }
      const command = record.operation.command;
      const selected = command.kind === 'create' ? command.selection : 'session_id' in command ? this.journal.session(command.session_id)?.selection : undefined;
      if (selected && !(record.lastObservation?.kind === 'agent'
          && record.lastObservation.observation.evidence.kind === 'submission'
          && record.lastObservation.observation.evidence.state === 'missing')) this.journal.submission(record, selected, 'missing');
      return { kind: 'unchanged', record };
    }
    const events = await this.sdk.threads.events.list({ threadId: record.threadId, afterSeq: String(record.cursor), limit: '100', order: 'asc', signal });
    const update = applyNativeEvents(record, events);
    // Startup can fail before any turn is accepted (for example, no provider
    // login). Report uncertainty without inventing a turn result or resending.
    if (nativeFailed && !update.record.turnId) return { kind: 'uncertain', record: update.record };
    if (update.kind !== 'terminal' && update.kind !== 'uncertain' && update.record.turnId) {
      const interactions = await this.sdk.threads.interactions.list({ threadId: record.threadId, signal });
      if (interactions.length > LIMITS.page) throw new Error('native interaction bound');
      if (interactions.some(interaction => interaction.threadId === record.threadId && interaction.turnId === update.record.turnId
          && (interaction.status === 'pending' || interaction.status === 'resolving'))) {
        update.record.nativeState = 'waiting_for_input';
        return stateUpdate(update.record, { kind: 'turn', thread_id: record.threadId, turn_id: update.record.turnId,
          state: 'waiting_for_input', output: null, truncated: update.record.truncated, usage: update.record.usage });
      }
      if (update.record.nativeState === 'waiting_for_input' && interactions.some(interaction => interaction.threadId === record.threadId
          && interaction.turnId === update.record.turnId && interaction.status === 'resolved')) {
        update.record.nativeState = 'running';
        return stateUpdate(update.record, { kind: 'turn', thread_id: record.threadId, turn_id: update.record.turnId,
          state: 'running', output: null, truncated: update.record.truncated, usage: update.record.usage });
      }
    }
    return update;
  }
  async inspect(threadId: string, signal: AbortSignal): Promise<Extract<CommandRecord['nativeState'], string> | null> {
    const thread = await this.sdk.threads.get({ threadId, signal });
    if (thread.status === 'pending') return 'queued';
    if (thread.status === 'active' || thread.status === 'starting' || thread.status === 'stopping') return 'running';
    const events = await this.sdk.threads.events.list({ threadId, limit: '100', order: 'desc', signal });
    if (events.length > 100) throw new Error('native inspect bound');
    const completed = events.find(event => event.type === 'turn/completed');
    if (completed?.type !== 'turn/completed') return null;
    return completed.data.status === 'completed' ? 'completed' : completed.data.status === 'interrupted' ? 'stopped' : 'failed';
  }
}
