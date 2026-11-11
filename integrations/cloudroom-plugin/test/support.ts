import Database from 'better-sqlite3';
import { mkdtempSync, rmSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { randomUUID } from 'node:crypto';
import { JOURNAL_SCHEMA, Journal, type Binding } from '../src/journal.js';
import { commandDigest, type Command, type Operation, type Selection } from '../src/protocol.js';

export function fixture(options: { initialized?: boolean } = {}) {
  const directory = mkdtempSync(join(tmpdir(), 'openbot-plugin-unit-')); const file = join(directory, 'data.db');
  const binding: Binding = { origin: 'https://openbot.example', installation_id: randomUUID(), journal_id: randomUUID(),
    machine_id: randomUUID(), company_id: randomUUID(), credential_generation: 1 };
  const handles: Database.Database[] = [];
  const open = () => {
    const existing = existsSync(file); const db = new Database(file); handles.push(db);
    db.pragma('journal_mode=WAL');
    if (!existing) for (const statement of JOURNAL_SCHEMA) db.exec(statement);
    return new Journal(db);
  };
  const journal = open(); if (options.initialized !== false) journal.initialize(binding);
  const selection: Selection = { machine_id: binding.machine_id, project_id: 'proj_selected', provider: 'codex',
    model: 'gpt-test', policy: { workspace: 'project_directory', permissions: 'full', reasoning: 'low' } };
  return { directory, file, journal, binding, selection, open,
    close: () => { for (const db of handles) if (db.open) db.close(); rmSync(directory, { recursive: true, force: true }); } };
}
export function operation(command: Command = { kind: 'refresh_catalog' }): Operation {
  const deadline = new Date(Date.now() + 30 * 60 * 1000).toISOString().replace('.000Z', 'Z');
  return { id: randomUUID(), command, deadline, payload_digest: commandDigest(command, deadline), dispatch: { kind: 'not_dispatched' }, outcome: null };
}
export function send(): Extract<Command, { kind: 'send' }> { return { kind: 'send', session_id: randomUUID(), thread_id: 'thr_owned', prompt: 'bounded authorized work' }; }
