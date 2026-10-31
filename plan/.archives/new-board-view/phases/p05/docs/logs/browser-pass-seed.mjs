// Throwaway seed for the new-board-view 05 browser pass (not committed). Usage:
//   node scripts/tests/.nbv-seed.mjs <scratch-dir>
// Writes <scratch-dir>/seed.json with the ids the pass script needs.
import { request } from '@playwright/test';
import { execFileSync } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import { writeFileSync } from 'node:fs';

const dir = process.argv[2];
const base = 'http://localhost:3001';
const db = `postgres://${process.env.USER}@localhost:5432/mail_agents_nbv_pass`;
const psql = sql => execFileSync('psql', [db, '-Atqc', sql], { encoding: 'utf8' }).trim();
const password = 'Passw0rd!nbv-1234';

async function signedIn(username) {
    const ctx = await request.newContext({ baseURL: base, extraHTTPHeaders: { Origin: base } });
    await ctx.post('/register', { form: { username, email: `${username}@nbv.test`, password, confirm_password: password } });
    const login = await ctx.post('/login', { form: { email_or_username: username, password } });
    if (login.status() >= 400) throw new Error(`login ${username}: ${login.status()}`);
    await ctx.storageState({ path: `${dir}/${username}.json` });
    return ctx;
}

const owner = await request.newContext({ baseURL: base, extraHTTPHeaders: { Origin: base }, storageState: `${dir}/owner.json` });
const company = psql("SELECT id FROM companies WHERE slug = 'boardco'");
const ownerId = psql("SELECT id FROM users WHERE username = 'owner'");
await signedIn('admin');
const adminId = psql("SELECT id FROM users WHERE username = 'admin'");
psql(`INSERT INTO company_members (id, company_id, user_id, role) VALUES (gen_random_uuid(), '${company}', '${adminId}', 'admin') ON CONFLICT DO NOTHING`);

const by = `'{"actor_type":"user","actor_id":"${ownerId}","actor_name":"owner"}'::jsonb`;
const created = await owner.post(`/ui/channels?company_id=${company}`, { form: { name: 'Private', slug: 'private' } });
if (created.status() >= 400) throw new Error(`channel: ${created.status()}`);
const privateChannel = psql(`SELECT id FROM channels WHERE company_id = '${company}' AND name = 'Private'`);
if (!privateChannel) throw new Error('channel not created');
psql(`UPDATE channels SET access_mode = 'allowlist' WHERE id = '${privateChannel}'`);
const agents = {};
for (const slug of ['assistant', 'reviewer']) {
    agents[slug] = randomUUID();
    psql(`INSERT INTO agents (id, company_id, name, slug, provider, model, created_by) VALUES ('${agents[slug]}', '${company}', '${slug === 'assistant' ? 'Assistant' : 'Reviewer'}', '${slug}', 'openai', 'gpt-test', ${by})`);
}

const api = `/api/companies/${company}`;
async function ok(response, what) {
    const body = await response.text();
    if (response.status() >= 300) throw new Error(`${what}: ${response.status()} ${body}`);
    return JSON.parse(body || '{}');
}
const literal = values => Object.fromEntries(Object.entries(values).map(([k, v]) => [k, { literal: v }]));
const text = { type: 'string', maxLength: 128 };
function source(workflow, step, extra = {}) {
    return { format_version: 1, workflow_id: workflow, input_schema: true, parameter_schema: true, output_schema: true,
        resources: [], entry: 'start', steps: { start: step }, limits: { max_steps: 100, max_context_bytes: 65536 }, ...extra };
}
async function publish(title, build) {
    const workflow = randomUUID();
    await ok(await owner.put(`${api}/workflow-definitions/${workflow}`, { data: { expected_revision: null, title, source: JSON.stringify(build(workflow)) } }), `save ${title}`);
    const published = await ok(await owner.post(`${api}/workflow-definitions/${workflow}/publish`, { data: { expected_revision: 1, command_key: `publish-${workflow}` } }), `publish ${title}`);
    return { workflow, version: published.version_id };
}
async function bind(version, association = { kind: 'company' }) {
    const binding = randomUUID();
    const configured = await ok(await owner.put(`${api}/workflow-bindings/${binding}`, { data: { expected_state_revision: null, association, version_id: version } }), 'configure');
    await ok(await owner.post(`${api}/workflow-bindings/${binding}/activate`, { data: { expected_state_revision: configured.state_revision } }), 'activate');
    return binding;
}
async function start(binding, input = {}) {
    const started = await ok(await owner.post(`${api}/workflow-bindings/${binding}/runs`, { data: { input, command_key: randomUUID() } }), 'start');
    return started.run_id;
}

const mapper = await publish('Mapper', w => source(w, { type: 'data.map', with: literal({ value: 'hello', output_schema: text }), routes: { success: '$end' } }));
const hostile = await publish('<img src=x onerror=alert(1)> "quoted"', w => source(w, { type: 'data.map', with: literal({ value: 'hello', output_schema: text }), routes: { success: '$end' } }));
const assistant = await publish('Assistant flow', w => source(w, { type: 'agent.run', with: literal({ agent: 'assistant', context: { message: 'hello' }, output_schema: text }), routes: { success: '$end' } }));
const review = { type: 'decision.human', with: literal({ proposal: { summary: 'review' }, reviewers: [{ kind: 'user', id: ownerId }], deadline: '2030-01-01T00:00:00Z', data_schema: text, feedback_required: ['revise'] }), routes: { choices: { continue: '$end', revise: '$end' } } };
const reviewed = await publish('Review', w => source(w, review));
const child = await publish('Child review', w => source(w, review, { input_schema: { type: 'object', properties: { text }, required: ['text'] } }));
const parent = await publish('Parent', w => source(w, { type: 'workflow.call', child_workflow_id: child.workflow, child_version_id: child.version, with: literal({ input: { text: 'hello' } }), routes: { success: '$end' } }));

const mapperBinding = await bind(mapper.version);
const privateBinding = await bind(mapper.version, { kind: 'channel', channel_id: privateChannel });
const hostileBinding = await bind(hostile.version);
const assistantBinding = await bind(assistant.version);
const reviewBinding = await bind(reviewed.version);
const parentBinding = await bind(parent.version);

const mapped = [];
for (let i = 0; i < 62; i++) mapped.push(await start(mapperBinding));
const hidden = [await start(privateBinding), await start(privateBinding)];
const hostileRun = await start(hostileBinding);
const assistantRuns = [await start(assistantBinding), await start(assistantBinding)];
const reviewRuns = [await start(reviewBinding), await start(reviewBinding)];
const parentRun = await start(parentBinding);

// Let the workers settle every run, then move the mapper's runs across the columns.
const all = [...mapped, ...hidden, hostileRun, ...assistantRuns, ...reviewRuns, parentRun];
for (let tries = 0; tries < 120; tries++) {
    const open = Number(psql(`SELECT count(*) FROM workflow_runs WHERE company_id = '${company}' AND state IN ('queued','running')`));
    if (open === 0) break;
    await new Promise(resolve => setTimeout(resolve, 500));
}
const list = ids => ids.map(id => `'${id}'`).join(',');
psql(`UPDATE workflow_runs SET state = 'failed' WHERE id IN (${list(mapped.slice(0, 55))})`);
psql(`UPDATE workflow_runs SET state = 'cancelled' WHERE id IN (${list(mapped.slice(55, 57))})`);
psql(`UPDATE workflow_runs SET state = 'running', finished_at = NULL WHERE id IN (${list(mapped.slice(57, 59))})`);
psql(`UPDATE workflow_runs SET state = 'queued', finished_at = NULL WHERE id IN (${list(mapped.slice(59, 60))})`);
const states = psql(`SELECT state || ':' || count(*) FROM workflow_runs WHERE company_id = '${company}' AND parent_run_id IS NULL GROUP BY state ORDER BY state`);
const childRun = psql(`SELECT child_run_id FROM workflow_run_parents WHERE parent_run_id = '${parentRun}'`);
const seed = { company, ownerId, adminId, privateChannel, agents, mapper, assistant, reviewed, parent, mapperBinding,
    mapped, hidden, hostileRun, assistantRuns, reviewRuns, parentRun, childRun,
    running: mapped.slice(57, 59), states: states.split('\n') };
writeFileSync(`${dir}/seed.json`, JSON.stringify(seed, null, 2));
console.log(JSON.stringify({ states: seed.states, parent: psql(`SELECT state || '/' || coalesce(waiting_reason,'') FROM workflow_runs WHERE id = '${parentRun}'`), child: childRun }));
