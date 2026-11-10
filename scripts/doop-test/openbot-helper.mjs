// Private pipe only: both helpers share the same input, output, and process bounds.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { readFile, writeFile } from 'node:fs/promises';
import { join } from 'node:path';

export function companyInput(origin, actors) {
  return { origin, source: {
    revision: process.env.DOOP_TEST_SOURCE_REVISION, fingerprint: process.env.DOOP_TEST_SOURCE_FINGERPRINT,
  }, companies: ['a', 'b'].map((company) => ({
    workspace: actors[`bot-${company}`].workspace, bot: actors[`bot-${company}`].id,
    verification_canvas: actors[`bot-${company}`].verificationCanvas, token: actors[`bot-${company}`].token,
    humans: [{ id: actors[`human-${company}`].id, email: actors[`human-${company}`].email }],
  })) };
}

export async function runOpenbotHelper({ testName, marker, recordName, input, output, secrets }) {
  const encoded = JSON.stringify(input);
  assert.ok(Buffer.byteLength(encoded) <= 65536, 'native helper input exceeds 64 KiB');
  const child = spawn(process.env.DOOP_TEST_OPENBOT_BINARY, ['--ignored', '--exact', testName, '--nocapture'], {
    env: { ...process.env, TEST_DATABASE_URL: process.env.DOOP_TEST_OPENBOT_DATABASE,
      DOOP_TEST_SETUP_RECORD: join(output, recordName + '.json') }, stdio: ['pipe', 'pipe', 'pipe'],
  });
  let bytes = 0;
  let exceeded = false;
  let inputFailed = false;
  const chunks = [];
  const capture = (chunk) => {
    bytes += chunk.length;
    if (bytes > 16384) { exceeded = true; child.kill('SIGTERM'); return; }
    chunks.push(chunk);
  };
  child.stdout.on('data', capture);
  child.stderr.on('data', capture);
  child.stdin.on('error', () => { inputFailed = true; });
  let force;
  const deadline = setTimeout(() => {
    child.kill('SIGTERM');
    force = setTimeout(() => child.kill('SIGKILL'), 5000);
  }, 80000);
  try {
    const finished = new Promise((resolve, reject) => {
      child.once('error', reject);
      child.once('close', (code, signal) => resolve({ code, signal }));
    });
    child.stdin.end(encoded);
    const result = await finished;
    const diagnostic = Buffer.concat(chunks).toString('utf8');
    if (result.code !== 0 || result.signal !== null || exceeded || inputFailed || !diagnostic.includes(marker)) {
      let redacted = diagnostic;
      for (const secret of secrets) redacted = redacted.split(secret).join('[redacted]');
      await writeFile(join(output, recordName + '-helper-failure.txt'), redacted, { mode: 0o600 });
    }
    assert.equal(exceeded || inputFailed, false);
    assert.equal(result.signal, null);
    assert.equal(result.code, 0);
    assert.ok(diagnostic.includes(marker), 'the exact native helper must actually run');
    const record = await readFile(join(output, recordName + '.json'));
    assert.ok(record.length <= 16384);
    const text = record.toString('utf8');
    for (const secret of secrets) assert.ok(!text.includes(secret), 'secret in native helper record');
    return JSON.parse(text);
  } finally {
    clearTimeout(deadline);
    clearTimeout(force);
    if (child.exitCode === null && child.signalCode === null) child.kill('SIGTERM');
  }
}
