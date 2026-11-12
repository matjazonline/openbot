import { mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { execFileSync, spawn } from 'node:child_process';
const directory = mkdtempSync(join(tmpdir(), 'openbot-plugin-tls-'));
try {
  writeFileSync(join(directory, 'config'), '[req]\ndistinguished_name=dn\nprompt=no\nx509_extensions=ext\n[dn]\nCN=localhost\n[ext]\nsubjectAltName=DNS:localhost\nbasicConstraints=critical,CA:TRUE\nkeyUsage=digitalSignature,keyEncipherment,keyCertSign\nextendedKeyUsage=serverAuth\n');
  execFileSync('openssl', ['req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-days', '1', '-config', 'config', '-keyout', 'key.pem', '-out', 'cert.pem'], { cwd: directory, stdio: 'pipe' });
  const child = spawn(process.execPath, ['--test', ...(process.argv.length>2?process.argv.slice(2):['build-tests/*.test.js', 'services/*.test.mjs'])], { stdio: 'inherit',
    env: { ...process.env, NODE_EXTRA_CA_CERTS: join(directory, 'cert.pem'), OPENBOT_TEST_TLS: directory } });
  process.exitCode = await new Promise(resolve => child.on('exit', code => resolve(code ?? 1)));
} finally { rmSync(directory, { recursive: true, force: true }); }
