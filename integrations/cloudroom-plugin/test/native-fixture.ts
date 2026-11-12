import type { BbPluginApi } from '@get-bb/plugin-sdk';
import { CloudroomSdk } from '../src/sdk.js';
import { type Journal } from '../src/journal.js';
import { fixture } from './support.js';
type Event = Awaited<ReturnType<BbPluginApi['sdk']['threads']['events']['list']>>[number];
export function event(seq: number, type: Event['type'], data: unknown, turnId?: string): Event {
  return { seq, type, data, scope: turnId ? { kind: 'turn', turnId } : { kind: 'thread' } } as Event;
}
export function nativeFixture(journal: Journal, selection: ReturnType<typeof fixture>['selection']) {
  const calls: { kind: string; input: Record<string, unknown> }[] = [];
  const state = { events: [] as Event[], thread: { id: 'thr_owned', projectId: selection.project_id,
    providerId: selection.provider, status: 'idle', environmentId: 'env_local', originPluginId: 'openbot-test' },
    metadata: {} as Record<string, string>, interactions: [] as unknown[], missing: false,
    providerError: null as null | { code: string }, model: selection.model, platform: process.platform as string,
    permissionModes: ['full', 'auto', 'accept-edits'], permissionCeiling: 'full', hostPermissionCeiling: 'full',
    nativePermission: selection.policy.permissions === 'accept_edits' ? 'accept-edits' : selection.policy.permissions,
    hostId: 'host_primary', requiredConfiguration: undefined as Parameters<BbPluginApi['agents']['configureRequired']>[0] | undefined, dispatchHook: undefined as ((context:any)=>any)|undefined, tools: [] as Parameters<BbPluginApi['agents']['registerTool']>[0][], rechecks: 0, lostResponse: false, held: false, entered: 0 };
  const mutate = async (kind: string, input: Record<string, unknown>) => {
    state.entered++; calls.push({ kind, input });
    if (kind === 'spawn') state.metadata = input.pluginMetadata as Record<string, string>;
    if (state.held) await new Promise<void>((resolve, reject) => {
      const signal = input.signal as AbortSignal;
      if (signal.aborted) reject(signal.reason);
      else signal.addEventListener('abort', () => reject(signal.reason), { once: true });
    });
    if (state.lostResponse) throw new Error('response lost after native acceptance');
  };
  const bb = { pluginId: 'openbot-test', agents: { nativeToolOriginVersion: 1, registerTool: (tool: Parameters<BbPluginApi['agents']['registerTool']>[0]) => {state.tools.push(tool);}, configureRequired: (provider: Parameters<BbPluginApi['agents']['configureRequired']>[0]) => {state.requiredConfiguration=provider;} }, experimental_hooks: { on: (_name: string, handler: (context:any)=>any) => {state.dispatchHook=handler;}, recheck: async () => {state.rechecks++;} }, sdk: {
    system: { config: async () => ({ primaryHostId: state.hostId }),
      executionOptions: async () => ({ permissionCeiling: state.permissionCeiling, modelLoadError: state.providerError,
        models: [{ model: state.model, supportedReasoningEfforts: [{ reasoningEffort: 'low' }], defaultReasoningEffort: 'low' }] }) },
    hosts: { get: async () => ({ id: state.hostId, status: 'connected', type: 'persistent',
      platform: state.platform, maxPermissionMode: state.hostPermissionCeiling, machineProviderId: null, connectMachineId: null }) },
    projects: { get: async () => ({ id: selection.project_id, sources: [{ type: 'local_path', hostId: state.hostId }] }) },
    providers: { list: async () => [{ id: selection.provider, capabilities: { permissionModes: state.permissionModes } }], models: async () => ({ modelLoadError: state.providerError,
      models: [{ model: state.model, supportedReasoningEfforts: [{ reasoningEffort: 'low' }], defaultReasoningEffort: 'low' }] }) },
    environments: { get: async () => ({ hostId: state.hostId, projectId: selection.project_id }),
      listProviders: async () => [{ id: 'project-checkout', machineProviderId: null, requires: { projectCheckout: true },
        acceptsEmptyInputs: true, machineAvailability: {}, availability: { status: 'available' } }] },
    threads: { defaultExecutionOptions: async () => ({ permissionMode: state.nativePermission }), get: async () => { if (state.missing) throw { status: 404 }; return state.thread; },
      list: async () => state.missing ? [] : [state.thread], getPluginMetadata: async () => state.metadata,
      spawn: async (input: Record<string, unknown>) => { await mutate('spawn', input); return state.thread; },
      send: async (input: Record<string, unknown>) => { await mutate('send', input); return { delivery: 'queued', queuedMessage: { id: 'queue_own' } }; },
      stop: async (input: Record<string, unknown>) => { await mutate('stop', input); },
      archive: async (input: Record<string, unknown>) => { await mutate('archive', input); },
      events: { list: async (input: { afterSeq?: string; order?: string; limit: string }) => {
        const selected = state.events.filter(event => event.seq > Number(input.afterSeq ?? 0));
        return (input.order === 'desc' ? selected.reverse() : selected).slice(0, Number(input.limit));
      } }, interactions: { list: async () => state.interactions } },
  } } as unknown as Pick<BbPluginApi, 'sdk' | 'pluginId' | 'agents' | 'experimental_hooks'>;
  return { bb, state, calls, sdk: new CloudroomSdk(bb, journal, () => [selection.project_id]) };
}
