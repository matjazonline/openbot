import { build } from 'esbuild';
import { readdir, mkdir } from 'node:fs/promises';
const tests = process.argv.includes('--test');
const entries = tests
  ? (await readdir('test')).filter(name => name.endsWith('.test.ts') || name.endsWith('-peer.ts')).map(name => `test/${name}`)
  : ['src/server.ts'];
if (!entries.length) throw new Error('no selected plugin tests');
const outdir = tests ? 'build-tests' : 'dist';
await mkdir(outdir, { recursive: true });
await build({ entryPoints: entries, outdir, bundle: true, platform: 'node', format: 'esm',
  target: 'node24', packages: 'external', sourcemap: true, logLevel: 'warning' });
console.log(`built ${entries.length} ${tests ? 'test entries' : 'plugin entry'}`);
