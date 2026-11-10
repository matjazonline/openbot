import type { BbPluginApi } from '@get-bb/plugin-sdk';
import { Connector } from './connector.js';
import { Dispatcher } from './dispatcher.js';
import { canonicalOrigin, ConfigurationRequired, defineSettings, recoverEnrollment, selectedProjects, type SettingsStore } from './enrollment.js';
import { JOURNAL_SCHEMA, Journal } from './journal.js';
import { acquireProfileLock } from './lock.js';
import { CloudroomSdk, sdkSignal } from './sdk.js';

export default async function openbot(bb: BbPluginApi): Promise<void> {
  const settings = defineSettings(bb); const db = bb.storage.database();
  bb.storage.migrate(db, [...JOURNAL_SCHEMA]);
  const journal = new Journal(db);
  let selected: string[] = []; let connector: Connector | undefined;
  let running: AbortController | undefined;
  const expectedSecrets = new Set<string | undefined>();
  const enrollmentSettings: SettingsStore = { get: () => settings.get(),
    experimental_set: async patch => {
      running?.signal.throwIfAborted();
      const ownSecret = Object.hasOwn(patch, 'enrollment'); const nextSecret = patch.enrollment ?? undefined;
      if (ownSecret) expectedSecrets.add(nextSecret);
      try { return await settings.experimental_set(patch); }
      finally { if (ownSecret) expectedSecrets.delete(nextSecret); }
    } };
  const sdk = new CloudroomSdk(bb, journal, () => selected);
  settings.onChange((next, prev) => {
    if (next.openbotOrigin !== prev.openbotOrigin
        || (next.enrollment !== prev.enrollment && !expectedSecrets.has(next.enrollment))
        || next.selectedProjects !== prev.selectedProjects) running?.abort(new ConfigurationRequired('Connector settings changed; reload plugin'));
  });
  bb.background.service('openbot-connector', {
    async start(signal) {
      const release = acquireProfileLock(db.name); const stop = new AbortController();
      const lifetime = AbortSignal.any([signal, stop.signal]);
      running = stop;
      try {
        const values = await settings.get(); canonicalOrigin(values.openbotOrigin);
        selected = selectedProjects(values.selectedProjects ?? '');
        const paired = await recoverEnrollment(enrollmentSettings, journal, lifetime);
        const current = await settings.get(); lifetime.throwIfAborted();
        if (canonicalOrigin(current.openbotOrigin) !== paired.binding.origin
            || current.selectedProjects !== values.selectedProjects || current.enrollment !== JSON.stringify(paired)) {
          throw new ConfigurationRequired('Connector settings changed; reload plugin');
        }
        connector = new Connector(new Dispatcher(journal, sdk));
        await connector.run(paired, lifetime);
        if (stop.signal.aborted && !signal.aborted) throw new ConfigurationRequired('Connector settings changed; reload plugin');
      } finally { running = undefined; stop.abort(); release(); }
    },
  });
  bb.cli.register({ name: 'openbot-local', summary: 'Local connector status and project discovery',
    commands: [{ name: 'status', summary: 'Connection, journal and provider status', usage: 'openbot-local status' },
      { name: 'projects', summary: 'List local Cloudroom projects', usage: 'openbot-local projects' }],
    async run(argv, context) {
      if (argv.length === 1 && argv[0] === 'status') return { exitCode: 0, stdout: JSON.stringify({
        connection: connector?.status ?? 'offline', paired: journal.initialized(),
        recoveryRequired: journal.initialized() ? journal.recoveryRequired() : false,
        sequence: journal.initialized() ? journal.sequence() : 0,
        acknowledged: journal.initialized() ? journal.acknowledged() : 0, providers: [...sdk.health.values()],
        journal: journal.initialized() ? journal.diagnostics() : undefined,
      }) };
      if (argv.length === 1 && argv[0] === 'projects') {
        const signal = sdkSignal(context.signal ?? AbortSignal.timeout(5_000));
        const hostId = await sdk.localHost(signal);
        const projects = await bb.sdk.projects.list({ signal });
        if (projects.length > 128) return { exitCode: 1, stderr: 'Project catalog exceeds the supported bound' };
        return { exitCode: 0, stdout: JSON.stringify(projects.filter(project => project.sources.some(source => source.type === 'local_path' && source.hostId === hostId))
          .map(project => ({ id: project.id, name: project.name }))) };
      }
      return { exitCode: 1, stderr: 'Use openbot-local status or openbot-local projects' };
    },
  });
}
