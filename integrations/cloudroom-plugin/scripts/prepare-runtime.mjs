// Run at installation time, never as a service startup hook.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { prepareSource } from '../compatibility/check-source.mjs';

assert.equal(process.argv.length, 3, 'usage: node scripts/prepare-runtime.mjs /absolute/cloudroom-checkout');
const source = resolve(process.argv[2]); const cwd = resolve(source, 'gui');
const manifest = JSON.parse(readFileSync(new URL('../compatibility/manifest.json', import.meta.url)));
assert.equal(process.version, `v${manifest.runtime.node}`, 'use the pinned Node runtime');
const identity = prepareSource(source);
const version = execFileSync('pnpm', ['--version'], { cwd, encoding: 'utf8', timeout: 30_000 }).trim();
assert.equal(`pnpm@${version}`, manifest.runtime.packageManager, 'use the source package-manager pin');
for (const args of [
  ['install', '--frozen-lockfile'],
  ['--filter', '@get-bb/plugin-sdk', 'build'],
  ['--filter', '@get-bb/plugin-sdk', 'build:types'],
  ['--filter', '@cloudroom/server', 'build'],
  ['--filter', '@cloudroom/host-daemon', 'build'],
  ['--filter', '@cloudroom/cli', 'build'],
  ['--filter', '@cloudroom/app', 'build'],
]) execFileSync('pnpm', args, { cwd, stdio: 'inherit', timeout: 15 * 60_000 });
prepareSource(source);
writeFileSync(resolve(source, '.openbot-runtime.json'), JSON.stringify({ version: 1, node: process.version,
  packageManager: manifest.runtime.packageManager, ...identity }, null, 2) + '\n', { mode: 0o600 });
console.log('Prepared pinned Cloudroom runtime. Provider authentication and inference require separate smoke tests.');
