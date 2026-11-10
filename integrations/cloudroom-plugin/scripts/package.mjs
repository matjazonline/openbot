import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync, realpathSync, mkdirSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const cwd = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const pkg = JSON.parse(readFileSync(resolve(cwd, 'package.json')));
assert.equal(process.version, `v${pkg.engines.node}`, 'use the pinned Node runtime');
assert.equal(`npm@${execFileSync('npm', ['--version'], { encoding: 'utf8' }).trim()}`, pkg.packageManager);
const lock = JSON.parse(readFileSync(resolve(cwd, 'npm-shrinkwrap.json')));
assert.equal(lock.packages[''].version, pkg.version);
assert.deepEqual(lock.packages[''].dependencies, pkg.dependencies);
const output = resolve(process.argv[2] ?? resolve(cwd, 'artifacts')); mkdirSync(output, { recursive: true });
assert(!realpathSync(output).startsWith(cwd + '/dist/'), 'artifact output must be outside dist');
execFileSync(process.execPath, ['scripts/build.mjs'], { cwd, stdio: 'inherit' });
const [pack] = JSON.parse(execFileSync('npm', ['pack', '--json', '--pack-destination', output], { cwd, encoding: 'utf8', maxBuffer: 1024 * 1024 }));
for (const path of ['npm-shrinkwrap.json', 'dist/server.js', 'services/run.mjs', 'OPERATIONS.md', 'compatibility/UPSTREAM-NOTICE']) {
  assert(pack.files.some(file => file.path === path), `missing package input ${path}`);
}
const sha256 = createHash('sha256').update(readFileSync(resolve(output, pack.filename))).digest('hex');
// Keep an enduring reproducibility signal in CI, including build output.
execFileSync(process.execPath, ['scripts/build.mjs'], { cwd, stdio: 'inherit' });
execFileSync('npm', ['pack', '--json', '--pack-destination', output], { cwd, encoding: 'utf8', maxBuffer: 1024 * 1024 });
assert.equal(createHash('sha256').update(readFileSync(resolve(output, pack.filename))).digest('hex'), sha256,
  'repeated build/package changed artifact bytes');
writeFileSync(resolve(output, `${pack.filename}.integrity.json`), JSON.stringify({ version: pkg.version,
  filename: pack.filename, sha256, integrity: pack.integrity,
  compatibilitySha256: createHash('sha256').update(readFileSync(resolve(cwd, 'compatibility/manifest.json'))).digest('hex'),
}, null, 2) + '\n');
console.log(JSON.stringify({ filename: pack.filename, sha256 }));
