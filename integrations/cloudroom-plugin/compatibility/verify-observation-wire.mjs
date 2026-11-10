import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { observationSchema } from '../src/protocol.ts';
const observations = JSON.parse(readFileSync(0, 'utf8'));
assert.equal(observations.length, 3);
assert.deepEqual(observations.map(value => value.observation.evidence.state), ['accepted', 'queued', 'missing']);
for (const observation of observations) {
  assert.deepEqual(observationSchema.parse(observation), observation);
  const changed = structuredClone(observation);
  changed.observation.evidence.state = 'completed';
  assert.throws(() => observationSchema.parse(changed));
  changed.observation.evidence.state = 'queued';
  changed.observation.evidence.turn_id = 'turn_fabricated';
  assert.throws(() => observationSchema.parse(changed));
}
process.stdout.write('PASS Rust→Node submission evidence: accepted/queued/missing, no turn fabrication or terminal variant\n');
