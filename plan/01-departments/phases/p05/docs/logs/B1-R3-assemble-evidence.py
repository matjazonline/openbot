"""Retain R2 evidence and add exercised R3 CI jobs; never rewrite older rounds."""
from pathlib import Path
import hashlib
import json
import sys

logs = Path(__file__).resolve().parent
artifact = sys.argv[1]
resource_root = Path((logs / 'B1-R3-resource-root.txt').read_text().strip())
acceptance = json.loads((resource_root / 'doop-acceptance-4/acceptance.json').read_text())
assert acceptance['status'] == 'PASS'
assert {run['name'] for run in acceptance['runs']} == {
    'all-disk', 'all-bucket', 'e2e-disk', 'e2e-bucket'}
assert all(run['status'] == 'PASS' for run in acceptance['runs'])
for result_file in ['B1-R3-knowledge-result.json', 'B1-R3-memory-result.json',
                    'B1-R3-doop-service-test-rerun-result.json',
                    'B1-R3-doop-image-result-2.json', 'B1-R3-doop-acceptance-result-4.json']:
    assert json.loads((logs / result_file).read_text())['status'] == 'PASS', result_file
focused = json.loads((logs / 'B1-R3-focused-checks.json').read_text())
assert len(focused) == 4 and all(result['status'] == 'PASS' for result in focused)
previous = json.loads((logs / '01DEP-P05-B1-R2.evidence.json').read_text())
checks = [check for check in previous['checks'] if check['status'] != 'NOT EXERCISED']


def entry(criteria, command, fixture, status, patterns, note):
    paths = sorted({path for pattern in patterns for path in logs.glob(pattern)})
    assert paths, patterns
    checks.append(dict(criteria=criteria, command=command, fixture=fixture, status=status,
                       files=[dict(path='logs/' + path.name,
                                   sha256=hashlib.sha256(path.read_bytes()).hexdigest())
                              for path in paths], note=note))


entry(['05A.3', 'CI-knowledge-image'],
      'docker build --tag openbot-p05-r3-knowledge . (BuildKit); python3 scripts/check-knowledge-image.py --image openbot-p05-r3-knowledge',
      'owned openbot-p05-r3 Docker engine; production Dockerfile; pinned image ID in TASK',
      'PASS', ['B1-R3-knowledge-build-2.txt', 'B1-R3-knowledge-check.txt', 'B1-R3-knowledge-result.json'],
      'Actual non-root/read-only/offline image, retrieval and RSS/model-integrity assertions. Later edit is cfg(test)-only teardown order.')
entry(['05A.3', 'CI-knowledge-image-attempt1'], 'docker build --tag openbot-p05-r3-knowledge . (legacy attempt)',
      'same owned engine; legacy builder before private Buildx configuration', 'FAIL',
      ['B1-R3-knowledge-build.txt'], 'Canceled initial attempt retained; successful BuildKit build supersedes it.')
for step in ['install', 'format-check', 'typecheck', 'lint', 'doc-lint', 'build']:
    entry(['05A.3', 'CI-doop-service-' + step], 'managed Bun frozen install or run ' + step,
          'pinned managed Doop source e0c063c4; clean dedicated HOME; Node22.23.4/Bun1.3.10',
          'PASS', ['B1-R3-doop-service-' + step + '.txt'], 'Exact commands and exits retained in service-results JSON.')
entry(['05A.3', 'CI-doop-service-tests'], 'bun run test -- --maxWorkers=2 (runner.clean_environment)',
      'same immutable managed source; phase Docker PATH adapter and isolated PostgreSQL', 'PASS',
      ['B1-R3-doop-service-test-rerun*', 'B1-R3-doop-service-results.json', 'B1-R3-doop-service-gate.py', 'B1-R3-docker-*', 'B1-R3-node22.txt', 'B1-R3-doop-setup.txt'],
      '2,711 passed/17 explicit clipboard skips; no source or test edits; source fingerprint unchanged.')
entry(['05A.3', 'CI-doop-service-tests-attempt1'], 'bun run test -- --maxWorkers=2 (initial clean environment)',
      'same source, before phase Docker discovery adapter', 'FAIL', ['B1-R3-doop-service-test.txt'],
      'Initial private storage fixture failures retained; identical full test step passes after environment correction.')
entry(['05A.3', 'CI-doop-private-image'], 'python3 scripts/doop-test.py image --managed --output <owned>/doop-image-2',
      'pinned managed source, owned Docker engine, unchanged300s command bound', 'PASS',
      ['B1-R3-doop-image-2.txt', 'B1-R3-doop-image-result-2.json', 'B1-R3-doop-image-metadata.json'],
      'Actual node-user image and archive/source identity validated.')
entry(['05A.3', 'CI-doop-private-image-attempt1'], 'python3 scripts/doop-test.py image --managed --output <owned>/doop-image',
      'cold owned Docker dependency cache; same source and bound', 'FAIL',
      ['B1-R3-doop-image.txt', 'B1-R3-doop-image-result.json', 'B1-R3-doop-image-build-attempt1.txt'],
      'Cold dependency installation exceeded existing300s bound; unchanged retry passed after warming cache.')
entry(['05A.3', 'CI-doop-integration'], 'python3 scripts/doop-test.py acceptance --directory <owned>/doop --image <owned>/doop-image-2/image.json --output <owned>/doop-acceptance-4 --openbot-test-binary <immutable-current-helper> --openbot-test-database <owned-native-PG> --ui-chrome <system-Chrome>',
      'real private image/local verified TLS; pinned managed source; identical required four profiles; owned native Openbot PG18.6',
      'PASS', ['B1-R3-doop-acceptance-4.txt', 'B1-R3-doop-acceptance-result-4.json', 'B1-R3-doop-acceptance-attempt4-*', 'B1-R3-helper-identity-2.json', 'B1-R3-helper-discovery-2.txt', 'B1-R3-helper-build-stderr-2.txt', 'B1-R3-native-pg-resource.json'],
      'All four required profiles exercise the exact current immutable helper; report hashes and confirmed runtime cleanup retained.')
for number in [1, 2, 3]:
    suffix = '' if number == 1 else '-' + str(number)
    entry(['05A.3', 'CI-doop-integration-attempt' + str(number)], 'same official four-profile acceptance (earlier attempt)',
          'owned VM Openbot DB for attempt1; owned native Openbot DB for attempt2; old preserved helper', 'FAIL',
          ['B1-R3-doop-acceptance' + suffix + '.txt', 'B1-R3-doop-acceptance-result' + suffix + '.json', 'B1-R3-doop-acceptance-attempt' + str(number) + '-*'],
          'Attempt1 lifecycle hit80s on VM DB; attempt2 model teardown exceeded60s; attempt3 passed three profiles and hit80s in S3 E2E. See TASK fixture correction and capacity retry; every failed attempt retained.')
entry(['05A.3', 'CI-doop-capacity-focused'], 'python3 scripts/doop-test.py run --managed --suite e2e --private-deployment bucket (same helper/image/assertions)',
      'owned Docker engine reallocated4 to8 CPUs; RAM8GiB;80s helper/60s model/probe bounds unchanged', 'PASS',
      ['B1-R3-capacity-retry.txt', 'B1-R3-doop-capacity-e2e-2.txt', 'B1-R3-doop-capacity-e2e-result-2.json', 'B1-R3-doop-capacity-public-*'],
      'Focused real S3 end-to-end check before official full acceptance. Increased local CPU allocation; no acceptance threshold changed.')
entry(['05A.3', 'CI-memory-integration'], 'scripts/memory-integration.sh',
      'owned pinned pgvector PG18 container; actual pinned native ORT/model; service/fake ports18095/18096', 'PASS',
      ['B1-R3-memory-integration.txt', 'B1-R3-memory-result.json', 'B1-R3-memory-cleanup-and-model.txt'],
      'Six required ignored tests execute, role-isolation assertions pass and fake Messages API observed; per-test resources cleaned.')
entry(['05A.2', 'R3-fixture-correction'], 'cargo fmt/check/clippy and focused doop_e2e_ tests (locked/offline)',
      'current worktree; cfg(test)-only helper teardown-order correction', 'PASS',
      ['B1-R3-focused-checks*', 'B1-R3-helper-discovery-2.txt'],
      'No test name/assertion/limit removed or weakened; completed model fixtures are checked/closed before independent lost-receipt work.')
entry(['05A.2', '05A.3', 'retention-and-cleanup'], 'git diff --check; evidence validators; owned resource cleanup; graft build',
      'p05 worktree and owned resources only; no shared server/global Docker changes', 'PASS',
      ['B1-R3-final-whitespace.txt', 'B1-R3-graft-build.txt', 'B1-R3-cleanup.txt', 'B1-R3-resource-root.txt', 'B1-R3-helper-identity.json'],
      'Required historical evidence retained; cleanup scope documented. Full/stock unrelated failures remain explicit in reused R2 evidence.')
pin = logs / 'coordination-r16.md'
manifest = dict(version=1, round='01DEP-P05-B1-R3', artifact=artifact,
                reuses_round='01DEP-P05-B1-R2',
                coordination=dict(revision=16, file='logs/' + pin.name,
                                  sha256=hashlib.sha256(pin.read_bytes()).hexdigest()), checks=checks)
out = logs / '01DEP-P05-B1-R3.evidence.json'
assert not out.exists(), 'do not rewrite a frozen review round'
out.write_text(json.dumps(manifest, indent=2) + '\n')
from collections import Counter
print(out, dict(Counter(check['status'] for check in checks)))
