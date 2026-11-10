import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdirSync, mkdtempSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createInterface } from 'node:readline';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { assertEmptyDirectory, cloudroomEnv, parseOptions, quote, toolProblems, untilReady, verifyArtifact } from './install-local.mjs';

const paths = ['--cloudroom', '/srv/cloudroom', '--profile', '/srv/profile', '--plugin', '/srv/plugin'];

test('options require separate absolute directories and distinct unprivileged ports', () => {
  const options = parseOptions(paths);
  assert.deepEqual(options.ports, [4310, 4311, 4312]);
  assert.equal(options.artifacts, fileURLToPath(new URL('../artifacts', import.meta.url)));
  assert.equal(options.trustedIntegrity, undefined);
  assert.equal(options.skipTests, false); assert.equal(options.keepRunning, false);
  assert.deepEqual(parseOptions([...paths, '--server-port', '5000', '--skip-tests']).ports, [5000, 4311, 4312]);
  assert.throws(() => parseOptions(['--cloudroom', 'cloudroom', '--profile', '/srv/profile', '--plugin', '/srv/plugin']), /--cloudroom needs an absolute path/);
  assert.throws(() => parseOptions(paths.slice(2)), /--cloudroom needs an absolute path/);
  assert.throws(() => parseOptions([...paths, '--artifacts', 'out']), /--artifacts needs an absolute path/);
  assert.throws(() => parseOptions(['--cloudroom', '/srv/a', '--profile', '/srv/a/profile', '--plugin', '/srv/plugin']), /separate directories/);
  assert.throws(() => parseOptions(['--cloudroom', '/srv/a', '--profile', '/srv/b', '--plugin', '/srv/b/../a']), /separate directories/);
  assert.throws(() => parseOptions(['--cloudroom', '/srv/a', '--profile', '/srv/a/..profile', '--plugin', '/srv/plugin']), /separate directories/);
  assert.doesNotThrow(() => parseOptions(['--cloudroom', '/srv/a', '--profile', '/srv/a-profile', '--plugin', '/srv/plugin']));
  assert.throws(() => parseOptions([...paths, '--ui-port', '80']), /--ui-port must be 1024-65535/);
  assert.throws(() => parseOptions([...paths, '--ui-port', '4312x']), /--ui-port must be a number/);
  assert.throws(() => parseOptions([...paths, '--ui-port', '4310']), /distinct/);
  assert.throws(() => parseOptions([...paths, 'extra']));
  assert.throws(() => parseOptions([...paths, '--pairing-code', 'secret']));
});

test('preflight reports every missing or mismatched prerequisite together', () => {
  const expected = { node: '24.21.0', npm: '11.19.0', pnpm: '10.34.9' };
  const ready = { node: '24.21.0', npm: '11.19.0', pnpm: '10.34.9', git: 'git version 2', 'c++': 'clang', tar: 'bsdtar', openssl: 'OpenSSL 3' };
  assert.deepEqual(toolProblems(expected, ready, ['git', 'c++', 'tar', 'openssl']), []);
  assert.deepEqual(toolProblems(expected, { ...ready, root: true, node: '26.5.0', pnpm: undefined, 'c++': undefined, openssl: undefined },
    ['git', 'c++', 'tar']), [
    'run as an unprivileged company account, not root',
    'node 24.21.0 is required (found 26.5.0)',
    'pnpm 10.34.9 is required (found none)',
    'c++ was not found on PATH',
  ]);
});

test('the plugin directory must be absent or empty because Cloudroom runs it in place', () => {
  const directory = mkdtempSync(join(tmpdir(), 'openbot-install-plugin-'));
  try {
    assertEmptyDirectory(join(directory, 'absent'));
    mkdirSync(join(directory, 'empty')); assertEmptyDirectory(join(directory, 'empty'));
    writeFileSync(join(directory, 'empty/package.json'), '{}');
    assert.throws(() => assertEmptyDirectory(join(directory, 'empty')), /absent or an empty directory/);
    writeFileSync(join(directory, 'file'), '');
    assert.throws(() => assertEmptyDirectory(join(directory, 'file')), /absent or an empty directory/);
    mkdirSync(join(directory, 'target')); symlinkSync(join(directory, 'target'), join(directory, 'alias'));
    assert.throws(() => assertEmptyDirectory(join(directory, 'alias')), /absent or an empty directory/);
  } finally { rmSync(directory, { recursive: true, force: true }); }
});

test('the archive must match its own integrity file and any trusted copy', () => {
  const directory = mkdtempSync(join(tmpdir(), 'openbot-install-artifact-'));
  const tarball = join(directory, 'plugin.tgz'); const own = `${tarball}.integrity.json`; const trusted = join(directory, 'trusted.json');
  const sha256 = createHash('sha256').update('archive').digest('hex');
  try {
    writeFileSync(tarball, 'archive'); writeFileSync(own, JSON.stringify({ sha256 })); writeFileSync(trusted, JSON.stringify({ sha256 }));
    assert.equal(verifyArtifact(tarball, own), sha256);
    assert.equal(verifyArtifact(tarball, own, trusted), sha256);
    writeFileSync(trusted, JSON.stringify({ sha256: '0'.repeat(64) }));
    assert.throws(() => verifyArtifact(tarball, own, trusted), /differs from the trusted integrity file/);
    writeFileSync(tarball, 'changed archive');
    assert.throws(() => verifyArtifact(tarball, own), /differs from its integrity file/);
  } finally { rmSync(directory, { recursive: true, force: true }); }
});

test('Cloudroom CLI calls address only the installed profile', () => {
  const env = cloudroomEnv({ PATH: '/bin', BB_DATA_DIR: '/other', ROOM_SERVER_URL: 'http://remote', BB_HOST_ENROLL_KEY: 'key', HOME: '/home/a' }, '/srv/profile', '4410');
  assert.deepEqual(env, { PATH: '/bin', HOME: '/home/a', BB_DATA_DIR: '/srv/profile', ROOM_SERVER_URL: 'http://127.0.0.1:4410' });
});

const wrapper = lines => {
  const child = spawn(process.execPath, ['-e', `for (const line of ${JSON.stringify(lines)}) console.log(line)`], { stdio: ['ignore', 'pipe', 'ignore'] });
  return untilReady(createInterface({ input: child.stdout }));
};

test('readiness follows the wrapper status lines', async () => {
  const status = (stage, state) => JSON.stringify({ component: 'cloudroom-local', stage, state });
  await wrapper(['not json', status('services', 'ready'), status('services', 'stopped')]);
  await assert.rejects(wrapper([status('ports', 'failed'), status('ports', 'stopped')]), /reported failed at stage ports/);
  await assert.rejects(wrapper(['{"component":"cloudroom-local"}']), /exited before it was ready/);
});

test('shell quoting keeps printed commands copyable', () => {
  assert.equal(quote('/srv/cloud-room/gui/apps/cli/dist/index.js'), '/srv/cloud-room/gui/apps/cli/dist/index.js');
  assert.equal(quote('/Users/a b/profile'), "'/Users/a b/profile'");
  assert.equal(quote("/srv/it's"), "'/srv/it'\\''s'");
});

test('the command line refuses bad options before running anything', () => {
  const script = fileURLToPath(new URL('./install-local.mjs', import.meta.url));
  const result = spawnSync(process.execPath, [script, '--cloudroom', 'relative', '--profile', '/srv/profile', '--plugin', '/srv/plugin'], { encoding: 'utf8', timeout: 10_000 });
  assert.equal(result.status, 1);
  assert.equal(result.stdout, '');
  assert.match(result.stderr, /install-local: --cloudroom needs an absolute path/);
});
