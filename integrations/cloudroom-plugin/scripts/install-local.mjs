// One-step company installation. Runs the OPERATIONS.md build, packaging, installation and
// registration steps in order. Provider logins, plugin settings and pairing stay with the operator.
import assert from 'node:assert/strict';
import { execFileSync, spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, lstatSync, mkdirSync, readdirSync, readFileSync, realpathSync } from 'node:fs';
import { homedir } from 'node:os';
import { dirname, isAbsolute, relative, resolve, sep } from 'node:path';
import { createInterface } from 'node:readline';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { parseArgs } from 'node:util';
import { CLOUDROOM_REVISION } from '../compatibility/check-source.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const providerCommands = { 'claude-code': 'claude', codex: 'codex', 'acp-opencode': 'opencode' };

export function parseOptions(args) {
  const { values } = parseArgs({ args, allowPositionals: false, options: {
    cloudroom: { type: 'string' }, profile: { type: 'string' }, plugin: { type: 'string' },
    artifacts: { type: 'string' }, 'trusted-integrity': { type: 'string' },
    'server-port': { type: 'string', default: '4310' }, 'daemon-port': { type: 'string', default: '4311' },
    'ui-port': { type: 'string', default: '4312' },
    'skip-tests': { type: 'boolean', default: false }, 'keep-running': { type: 'boolean', default: false },
  } });
  for (const key of ['cloudroom', 'profile', 'plugin']) assert(values[key] && isAbsolute(values[key]), `--${key} needs an absolute path`);
  for (const key of ['artifacts', 'trusted-integrity']) assert(values[key] === undefined || isAbsolute(values[key]), `--${key} needs an absolute path`);
  const [cloudroom, profile, plugin] = ['cloudroom', 'profile', 'plugin'].map(key => resolve(values[key]));
  const within = (child, parent) => { const path = relative(parent, child); return path === '' || (path !== '..' && !path.startsWith(`..${sep}`) && !isAbsolute(path)); };
  for (const [a, b] of [[cloudroom, profile], [cloudroom, plugin], [profile, plugin]]) {
    assert(!within(a, b) && !within(b, a), '--cloudroom, --profile and --plugin must be separate directories');
  }
  const ports = ['server-port', 'daemon-port', 'ui-port'].map(key => {
    assert(/^\d+$/.test(values[key]), `--${key} must be a number`);
    const port = Number(values[key]); assert(port >= 1024 && port <= 65535, `--${key} must be 1024-65535`); return port;
  });
  assert.equal(new Set(ports).size, 3, 'server, daemon and UI ports must be distinct');
  return { cloudroom, profile, plugin, ports,
    artifacts: values.artifacts ? resolve(values.artifacts) : resolve(root, 'artifacts'),
    trustedIntegrity: values['trusted-integrity'], skipTests: values['skip-tests'], keepRunning: values['keep-running'] };
}

export function probe(command, args = ['--version'], cwd = undefined) {
  try { return execFileSync(command, args, { cwd, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'], timeout: 30_000 }).trim(); }
  catch { return undefined; }
}

// Every problem is reported at once, before the long build starts.
export function toolProblems(expected, found, required) {
  const problems = [];
  if (found.root) problems.push('run as an unprivileged company account, not root');
  for (const [tool, version] of Object.entries(expected)) {
    if (found[tool] !== version) problems.push(`${tool} ${version} is required (found ${found[tool] ?? 'none'})`);
  }
  for (const tool of required) if (!found[tool]) problems.push(`${tool} was not found on PATH`);
  return problems;
}

export function assertEmptyDirectory(path) {
  assert(!existsSync(path) || (lstatSync(path).isDirectory() && readdirSync(path).length === 0),
    `${path} must be absent or an empty directory: Cloudroom runs the plugin from it in place`);
}

// The packaging run's own integrity file only detects a damaged archive; a trusted copy is the trust check.
export function verifyArtifact(tarball, integrityFile, trustedFile) {
  const sha256 = createHash('sha256').update(readFileSync(tarball)).digest('hex');
  assert.equal(JSON.parse(readFileSync(integrityFile, 'utf8')).sha256, sha256, 'the archive differs from its integrity file');
  if (trustedFile !== undefined) {
    assert.equal(JSON.parse(readFileSync(trustedFile, 'utf8')).sha256, sha256, 'the archive differs from the trusted integrity file');
  }
  return sha256;
}

// Cloudroom CLI calls address only this profile; inherited BB_/ROOM_ overrides could redirect them.
export function cloudroomEnv(environment, profile, serverPort) {
  const inherited = Object.fromEntries(Object.entries(environment).filter(([key]) => !/^(BB_|ROOM_)/.test(key)));
  return { ...inherited, BB_DATA_DIR: profile, ROOM_SERVER_URL: `http://127.0.0.1:${serverPort}` };
}

// Resolves on the wrapper's redacted ready status. Any other state, or exit, means it never became ready.
export function untilReady(lines) {
  return new Promise((ready, reject) => {
    lines.on('line', line => {
      let status; try { status = JSON.parse(line); } catch { return; }
      if (status.state === 'ready') ready();
      else if (status.state !== undefined) reject(new Error(`the Cloudroom wrapper reported ${status.state} at stage ${status.stage}`));
    });
    lines.once('close', () => reject(new Error('the Cloudroom wrapper exited before it was ready')));
  });
}

export const quote = value => /^[\w@%+=:,./-]+$/.test(value) ? value : `'${value.replaceAll("'", `'\\''`)}'`;

function run(command, args, options = {}) {
  console.log(`$ ${[command, ...args].map(quote).join(' ')}`);
  execFileSync(command, args, { stdio: 'inherit', ...options });
}

function step(title) { console.log(`\n==> ${title}`); }

async function register(options, cli, manifest) {
  const [serverPort, daemonPort, uiPort] = options.ports.map(String);
  const wrapper = spawn(process.execPath, [resolve(options.plugin, 'services/run.mjs'), '--source', options.cloudroom,
    '--profile', options.profile, '--server-port', serverPort, '--daemon-port', daemonPort, '--ui-port', uiPort],
  { stdio: ['ignore', 'pipe', 'inherit'] });
  const closed = new Promise(done => { wrapper.once('error', () => done(1)); wrapper.once('close', code => done(code ?? 1)); });
  // The wrapper drains its owned services on TERM. Wait for it rather than orphaning them.
  const stop = () => { if (wrapper.exitCode === null && wrapper.signalCode === null) wrapper.kill('SIGTERM'); };
  process.on('SIGINT', stop); process.on('SIGTERM', stop);
  const lines = createInterface({ input: wrapper.stdout });
  lines.on('line', line => console.log(line));
  try {
    await untilReady(lines);
    const env = cloudroomEnv(process.env, options.profile, serverPort);
    run(process.execPath, [cli, 'plugin', 'install', `path:${options.plugin}`, '--yes'], { env });
    run(process.execPath, [cli, 'plugin', 'reload', 'openbot-local'], { env });
    if (!options.keepRunning) stop();
    else { nextSteps(options, cli, manifest); console.log(`\nCloudroom is running at http://127.0.0.1:${uiPort}. Press Ctrl-C to stop it.`); }
    const code = await closed;
    assert.equal(code, 0, `the Cloudroom wrapper exited with status ${code}`);
    if (!options.keepRunning) nextSteps(options, cli, manifest);
  } catch (error) {
    stop(); await closed; throw error;
  } finally {
    process.removeListener('SIGINT', stop); process.removeListener('SIGTERM', stop);
  }
}

function nextSteps(options, cli, manifest) {
  const [serverPort, daemonPort, uiPort] = options.ports.map(String);
  const env = `BB_DATA_DIR=${quote(options.profile)} ROOM_SERVER_URL=http://127.0.0.1:${serverPort}`;
  const providers = Object.entries(manifest.nativeClients).map(([id, pin]) => {
    const command = providerCommands[id]; const found = probe(command)?.split('\n')[0];
    return `       ${command}: ${found ? `found ${found}` : 'not found on PATH'} (tested ${typeof pin === 'string' ? pin : pin.version})`;
  });
  console.log([
    '', `Installed the Openbot local agents plugin into ${options.profile}.`, '',
    'Still manual (OPERATIONS.md, "Providers, pairing and a first task"):',
    '  1. Install and log in to the provider CLIs as this account. The service PATH must contain them.',
    ...providers,
    `  2. Open http://127.0.0.1:${uiPort} and, in the plugin's settings, set Openbot HTTPS origin,`,
    '     Machine name and Selected project IDs.',
    "  3. Generate a pairing code in Openbot's company settings, enter it as the plugin's Pairing code, then reload:",
    `       ${env} node ${quote(cli)} plugin reload openbot-local`,
    '', 'Start Cloudroom with:',
    `  node ${quote(resolve(options.plugin, 'services/run.mjs'))} --source ${quote(options.cloudroom)} --profile ${quote(options.profile)} --server-port ${serverPort} --daemon-port ${daemonPort} --ui-port ${uiPort}`,
  ].join('\n'));
}

async function main() {
  const options = parseOptions(process.argv.slice(2));
  const manifest = JSON.parse(readFileSync(resolve(root, 'compatibility/manifest.json'), 'utf8'));
  const pkg = JSON.parse(readFileSync(resolve(root, 'package.json'), 'utf8'));
  const gui = resolve(options.cloudroom, 'gui');
  const required = ['git', 'c++', 'tar', ...(options.skipTests ? [] : ['openssl'])];
  const problems = toolProblems(
    { node: manifest.runtime.node, npm: pkg.packageManager.split('@')[1], pnpm: manifest.runtime.packageManager.split('@')[1] },
    { root: process.getuid?.() === 0, node: process.version.slice(1), npm: probe('npm'),
      pnpm: probe('pnpm', ['--version'], existsSync(gui) ? gui : homedir()), git: probe('git'), 'c++': probe('c++'),
      tar: probe('tar'), openssl: probe('openssl', ['version']) },
    required);
  if (problems.length) throw new Error(`missing prerequisites:\n  - ${problems.join('\n  - ')}`);
  assertEmptyDirectory(options.plugin);
  if (options.trustedIntegrity) readFileSync(options.trustedIntegrity);

  step('Build and verify the plugin');
  run('npm', ['ci'], { cwd: root });
  run('npm', ['run', 'typecheck'], { cwd: root });
  if (!options.skipTests) { run('npm', ['test'], { cwd: root }); run('npm', ['run', 'test:guards'], { cwd: root }); }

  step('Prepare the pinned Cloudroom source');
  if (!existsSync(options.cloudroom)) {
    run('git', ['clone', '--quiet', manifest.cloudroom.repository, options.cloudroom]);
    run('git', ['-C', options.cloudroom, 'checkout', '--quiet', '--detach', CLOUDROOM_REVISION]);
  }
  run(process.execPath, [resolve(root, 'scripts/prepare-runtime.mjs'), options.cloudroom], { cwd: root });

  step('Package and install the plugin');
  run('npm', ['run', 'package', '--', options.artifacts], { cwd: root });
  const tarball = resolve(options.artifacts, `${pkg.name}-${pkg.version}.tgz`);
  const sha256 = verifyArtifact(tarball, `${tarball}.integrity.json`, options.trustedIntegrity);
  console.log(`${tarball} sha256 ${sha256} (${options.trustedIntegrity ? 'matches the trusted integrity file' : 'self-consistent only; no trusted integrity file given'})`);
  mkdirSync(options.plugin, { recursive: true });
  run('tar', ['-xzf', tarball, '-C', options.plugin, '--strip-components=1']);
  run('npm', ['ci', '--omit=dev'], { cwd: options.plugin });

  step('Register the plugin in the Cloudroom profile');
  await register(options, resolve(gui, 'apps/cli/dist/index.js'), manifest);
}

if (process.argv[1] && import.meta.url === pathToFileURL(realpathSync(process.argv[1])).href) {
  try { await main(); } catch (error) { console.error(`\ninstall-local: ${error.message}`); process.exitCode = 1; }
}
