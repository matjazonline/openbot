"""Add exercised F3 correction evidence without rewriting historical rounds."""
from pathlib import Path
from collections import Counter
import hashlib
import json
import sys

logs = Path(__file__).resolve().parent
root = Path((logs / 'B1-R3-resource-root.txt').read_text().strip())
artifact = sys.argv[1]
previous = json.loads((logs / '01DEP-P05-B1-R3.evidence.json').read_text())
checks = previous['checks'].copy()
acceptance = json.loads((root / 'doop-acceptance-r4-2/acceptance.json').read_text())
assert acceptance['status'] == 'PASS' and len(acceptance['runs']) == 4
clipboard = json.loads((logs / 'B1-R4-clipboard-result-3.json').read_text())
assert clipboard['status'] == 'PASS' and clipboard['tests_passed'] == 17 and clipboard['tests_skipped'] == 0


def add(criteria, command, fixture, status, patterns, note):
    files = sorted({p for pattern in patterns for p in logs.glob(pattern)})
    assert files
    checks.append(dict(criteria=criteria, command=command, fixture=fixture, status=status,
                       files=[dict(path='logs/' + p.name, sha256=hashlib.sha256(p.read_bytes()).hexdigest())
                              for p in files], note=note))


add(['05A.3', 'CI-doop-service-clipboard', '01DEP-P05-B1-F3'],
    'bun run test -- --project clipboard tests/canvasClipboardBrowser.test.ts --maxWorkers=1 (isolated Linux fixture)',
    'pinned managed source351a2180; existing synthetic materializer provider; Linux/arm64, node user, sandboxed Chromium,2CPU/2GiB,networknone',
    'PASS', ['B1-R4-clipboard-tests-3.txt', 'B1-R4-clipboard-result-3.json', 'B1-R4-clipboard-image-3.json',
             'B1-R4-clipboard-build-3.txt', 'B1-R4-clipboard-Dockerfile-2.txt'],
    'All17 original clipboard assertions execute and pass, zero skips; test bodies/timeouts unchanged. Synthetic fixture proves interaction only, not server authorization/admission. Supplements unchanged2711-pass macOS service coverage.')
for number in [1, 2]:
    suffix = '' if number == 1 else '-2'
    add(['05A.3', 'CI-doop-service-clipboard-attempt' + str(number)], 'same focused Linux clipboard case file, earlier fixture attempt',
        'same pinned original source; first relocated fixture, second original/app with missing provider', 'FAIL',
        ['B1-R4-clipboard-tests' + suffix + '.txt', 'B1-R4-clipboard-result' + suffix + '.json',
         'B1-R4-clipboard-image' + suffix + '.json'],
        'All17 failed in harness/frame readiness before clipboard assertions. Failures retained; current test fixture supplies established materializer context, preserving every assertion.')
add(['05A.3', 'CI-doop-package'],
    'python3 scripts/doop-test.py package --directory <verified> --source <corrected-source> --source-fingerprint <351a2180...>',
    'separate pristine clone plus canonical patch; original reviewed source preserved', 'PASS',
    ['B1-R4-source-identity.json', 'B1-R4-source-delta.diff', 'B1-R4-package-2.txt', 'B1-R4-doop-setup.txt'],
    'Only canvasClipboard.html differs in source bytes. Canonical managed.patch/source.json regenerated with official producer. Pinned runtime source and dependency lock unchanged.')
add(['05A.3', 'CI-doop-private-image'],
    'python3 scripts/doop-test.py image --managed --directory <new-verified-setup> --output <new-image>',
    'new canonical source351a2180, same Docker engine/pinned dependencies and existing build bound', 'PASS',
    ['B1-R4-doop-image.txt', 'B1-R4-doop-image-result.json', 'B1-R4-doop-image-metadata.json'],
    'Actual image carries new source fingerprint and node user; archive/manifest identity retained. Production source bytes unchanged.')
add(['05A.3', 'CI-doop-integration'],
    'python3 scripts/doop-test.py acceptance --directory <new-setup> --image <new-image.json> --output <new-r4> --openbot-test-binary <current-immutable-helper> --openbot-test-database <owned-native-PG> --ui-chrome <system-Chrome>',
    'new canonical managed source/image; same current helper, private PostgreSQL/local TLS and8CPU fixture', 'PASS',
    ['B1-R4-doop-acceptance-2.txt', 'B1-R4-doop-acceptance-result-2.json', 'B1-R4-doop-acceptance-public-*',
     'B1-R4-helper-identity-2.json', 'B1-R4-helper-discovery-2.txt', 'B1-R4-helper-build-stderr-3.txt'],
    'Official four-profile aggregate repeats all104 assertions for the updated package identity with unchanged limits and confirmed runtime cleanup.')
add(['05A.3', 'CI-doop-integration-r4-attempt1'], 'same official aggregate with earlier helper',
    'new canonical source/image, prior helper; three profiles pass, final S3 model deadline fails', 'FAIL',
    ['B1-R4-doop-acceptance.txt', 'B1-R4-doop-acceptance-result.json', 'B1-R4-doop-acceptance-attempt1-*'],
    'Models outlived their actual editor work while downstream verification ran; corrected lifetime follows persisted edit completion with exact-five/secret checks intact.')
add(['05A.3', 'CI-doop-helper-focused-attempt1'], 'same official focused S3 E2E with corrected helper',
    'current helper, overlapping own Rust checks', 'FAIL',
    ['B1-R4-doop-helper-focused.txt', 'B1-R4-doop-helper-focused-result.json', 'B1-R4-helper-focused-attempt1-*'],
    'Both model closures pass exact-five checks; later80s outer limit reached. Serialized rerun after own CPU-heavy checks passes; existing bounds retained.')
add(['05A.3', 'CI-doop-helper-focused'], 'python3 scripts/doop-test.py run --managed --suite e2e --private-deployment bucket (serialized)',
    'current helper1d94d46c, new canonical source351a2180/image, same8CPU Docker/native PG fixtures', 'PASS',
    ['B1-R4-doop-helper-focused-2.txt', 'B1-R4-doop-helper-focused-result-2.json', 'B1-R4-helper-focused-pass-*'],
    'All10 S3 end-to-end checks pass with original60s/80s/probe bounds; persisted edit completion closes each consumed model before downstream work.')
add(['05A.2', 'R4-helper-lifetime'], 'cargo fmt/check/clippy and focused doop_e2e_ tests (locked/offline)',
    'cfg(test)-only lifetime callback in existing Driver poll; no new queries/claim protocols or production changes', 'PASS',
    ['B1-R4-focused-checks*', 'B1-R4-helper-discovery-2.txt'],
    'All four focused checks pass; exact3 required test names unchanged. Actual end-to-end case checks exact-five exchanges and token secrecy for both companies.')
add(['05A.3', 'CI-doop-format'], 'bun run format:check',
    'new canonical managed source, same pinned Bun/Node dependency setup', 'PASS', ['B1-R4-managed-format.txt'],
    'Changed HTML fixture format checked. Other service command/assertion bodies and production source unchanged; reuse unaffected R3 evidence.')
add(['05A.3', 'CI-doop-runner-protocol-attempt1'], 'python3 scripts/tests/doop-test.py',
    'unchanged runner/protocol sources on macOS', 'FAIL', ['B1-R4-runner-protocol.txt'],
    'Existing process-identity/reaper race; ownership protection refused to stop an unconfirmed PID. Isolated test and complete46-test rerun pass; no identity check weakened.')
add(['05A.3', 'CI-doop-runner-protocol'],
    'python3 scripts/tests/doop-test.py DeploymentOwnershipTests.test_recovery_stops_only_the_recorded_positive_process_group; python3 scripts/tests/doop-test.py',
    'same unchanged runner sources and corrected canonical package', 'PASS',
    ['B1-R4-runner-process-isolated.txt', 'B1-R4-runner-protocol-2.txt'],
    'Isolated recovery assertion and full46 protocol assertions pass. First failed attempt retained.')
add(['05A.2', '05A.3', 'R4-resource-retention'],
    'git diff --check; git diff --cached --check; owned runtime cleanup; graft build',
    'phase-owned fixtures only; preserved historical manifests and shared resources', 'PASS',
    ['B1-R4-final-whitespace.txt', 'B1-R4-cleanup.txt', 'B1-R4-graft-build.txt', 'B1-R4-retained-identities.txt'],
    'R1/R2/R3 files unchanged; all required current checks exercised. Known unrelated full/stock failures remain qualified, not a wholly green-suite claim.')
pin = logs / 'coordination-r18.md'
manifest = dict(version=1, round='01DEP-P05-B1-R4', artifact=artifact, reuses_round='01DEP-P05-B1-R3',
                coordination=dict(revision=18, file='logs/' + pin.name,
                                  sha256=hashlib.sha256(pin.read_bytes()).hexdigest()), checks=checks)
out = logs / '01DEP-P05-B1-R4.evidence.json'
assert not out.exists()
out.write_text(json.dumps(manifest, indent=2) + '\n')
print(dict(Counter(c['status'] for c in checks)))
