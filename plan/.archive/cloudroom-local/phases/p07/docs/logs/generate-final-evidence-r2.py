from pathlib import Path
import hashlib
import json
import re
import shlex
import sys

records = Path(__file__).resolve().parent.parent
logs = records / 'logs'
artifact = sys.argv[1]
assert re.fullmatch(r'[0-9a-f]{64}', artifact)
checks = []


def files(paths):
    result = []
    for name in paths:
        path = records / name
        assert path.is_file() and path.resolve().is_relative_to(records.resolve())
        result.append({'path': name, 'sha256': hashlib.sha256(path.read_bytes()).hexdigest()})
    return result


def add(criteria, command, fixture, status, paths=(), reason=None):
    if isinstance(command, list):
        command = shlex.join(command)
    entry = dict(criteria=criteria, command=command, fixture=fixture, status=status)
    if paths:
        entry['files'] = files(paths)
    if reason:
        entry['reason'] = reason
    checks.append(entry)


results = json.loads((logs / 'final-results-r2.json').read_text())
corrections = json.loads((logs / 'final-correction-results-r2.json').read_text())
images = json.loads((logs / 'lima-image-results-r3.json').read_text())
replacements = {item['name']: item for item in [*corrections, *images]}
assert replacements['stack']['status'] == 'PASS'
assert replacements['production-image-build']['status'] == 'PASS'
assert replacements['production-image-knowledge-memory']['status'] == 'PASS'
required = {
    'versions', 'plugin-install', 'plugin-typecheck', 'plugin-build', 'plugin-tests',
    'plugin-guards', 'plugin-package', 'plugin-extract', 'plugin-runtime-install',
    'format', 'whitespace', 'offline-check', 'clippy', 'migrations',
    'query-statistics-probe', 'query-statistics', 'sqlx', 'compile-tests', 'inventory',
    'suite', 'stack', 'lease', 'css', 'run-counts', 'js-protocols', 'deploy',
    'key-rotation', 'transport-tests', 'transport', 'sample-tests', 'sample',
    'knowledge-prepare', 'decision-browser', 'production-image-knowledge-memory',
    'headless-linux-native', 'decision_browser_markup_and_closed_draft_recovery',
    'source_message_outside_window_is_exact_and_authorized',
}
for prefix in ['html', 'knowledge', 'agent-authoring', 'local-agents', 'workflow-agent', 'session']:
    required.update([prefix + '-fixtures', prefix + '-browser'])
assert required <= {item['name'] for item in results}
for item in results:
    item = replacements.get(item['name'], item)
    assert item['status'] in ['PASS', 'NOT EXERCISED'], item
    paths = [item['file']] if 'file' in item else []
    reason = item.get('reason')
    if item['name'] == 'stack':
        paths.extend(['logs/final-stack-r2.txt', 'logs/stock-local-resolution-after-fix-results.json',
                      'logs/source-delta-audit-r6.json', item['progress'],
                      'logs/stock-correction-timeout-r1.json', 'logs/stock-timeout-process-audit-r1.json',
                      'logs/stock-timeout-pre-run-observations-r1.md', 'logs/cargo-stream-wrapper-r1.sh',
                      'logs/stock-stream-proof-r1.txt'])
        reason = 'Full corrected stock-stack run supersedes the earlier one-test fixture failure and a later outer-supervisor timeout. Exact2MiB/network scripts, production/per-test bounds and all assertions are unchanged. Fresh database, real Cargo with pipefail/tee output duplication,3600second outer watchdog; failed/timed-out observations retained.'
    if item['name'] == 'production-image-knowledge-memory':
        paths.extend(['logs/lima-image-build-r3.txt', 'logs/lima-image-resource-r3.json',
                      'logs/lima-image-context-r1.json', 'logs/lima-image-context-source-audit-r1.json',
                      'logs/reviewer-b1-r2-image-input-audit.json', 'logs/lima-image-runtime-info-r1.json',
                      'logs/lima-image-final-identity-r1.json', 'logs/reviewer-b1-r2-image-check.txt',
                      'logs/reviewer-b1-r2-image-results.json'])
        reason = 'Host Docker was unavailable; the exact required image/check script ran in a task-owned native ARM64 Linux VM. All context inputs match the frozen source.'
    if item['name'] == 'plugin-runtime-install':
        paths = ['logs/final-r2-plugin-runtime-install.txt']
    criteria = ['AC3', 'AC5'] if item['name'].startswith('plugin-') else ['AC5']
    if item['name'] in ['suite', 'stack']:
        criteria = ['AC1', 'AC2', 'AC5']
    if item['name'] == 'headless-linux-native':
        criteria = ['AC4']
    add(criteria, item.get('command', item['name']),
        'Fresh owned PostgreSQL 18.6 loopback56388; final-databases-r2.json; '
        'exact CI-equivalent checks. Full-suite controller walltime includes its deliberate resource hold.',
        item['status'], paths, reason)

add(['AC5'], replacements['production-image-build']['command'],
    'Pinned production Dockerfile in task-owned native ARM64 Lima VM; readonly host source, isolated Docker cache',
    'PASS', [replacements['production-image-build']['file'], 'logs/lima-image-context-r1.json',
             'logs/lima-image-build-r1.txt', 'logs/lima-image-build-oom-r1.txt',
             'logs/lima-image-build-r2.txt', 'logs/lima-image-resize-r1.txt'])

for item in json.loads((logs / 'post-native-results-r1.json').read_text()):
    assert item['status'] == 'PASS'
    add(['AC5', 'AC6'], item['command'], 'Retained checks before the final test-only lease-fixture correction; current affected checks follow',
        'PASS', [item['file']])

final_fixture = json.loads((logs / 'final-fixture-results-r1.json').read_text())
assert {item['name'] for item in final_fixture} == {'format', 'whitespace', 'clippy', 'inventory', 'graft'}
for item in final_fixture:
    assert item['status'] == 'PASS'
    add(['AC5', 'AC6'], item['command'], 'Current clean frozen source after the one-line gate-fixture correction',
        'PASS', [item['file']])

add(['AC5'], 'cargo check --locked --offline --all-targets; cargo test --locked --offline --lib --no-run',
    'Current frozen native acceptance helpers, explicit owned databases, no missing-database skip', 'PASS',
    ['logs/offline-native-assertion-r1.txt', 'logs/compile-native-final-r1.txt',
     'logs/plugin-corrections-typecheck-r12.txt', 'logs/build-peers-native-final-r1.txt',
     'logs/compile-local-resolution-r1.txt'])

native = json.loads((logs / 'native-results-r21.json').read_text())
assert {item['provider'] for item in native} == {'codex', 'claude', 'opencode'}
for item in native:
    assert item['status'] == 'PASS'
    provider = {'claude': 'claude-code', 'opencode': 'acp-opencode'}.get(item['provider'], item['provider'])
    folder = logs / 'native-r21' / provider
    result = json.loads((folder / 'result.json').read_text())
    assert result['status'] == 'PASS' and result['final']['archived'] is True
    assert result['outputRecoveredAfterNativeOutputBeforeServerReceipt'] is True
    assert result['nativeHumanPermission']['status'] == 'PASS'
    assert result['isolatedLoggedOutProvider']['workflow'] == 'failed'
    evidence = [str(path.relative_to(records)) for path in sorted(folder.rglob('*'))
                if path.is_file() and path.suffix in ['.json', '.png', '.txt']]
    add(['AC1', 'AC4'], 'Explicitly authorized native runner run-native-r21.py: ' + item['provider'],
        'macOS; packaged production plugin SHA561d6b3b3e3d0fa038130cd20e1fcdb95e1e1cb07316d2cf0e285089dcfe8bde; '
        'actual HTTP/WSS, two workflow workers, provider login, UI denial/human turn, unconsumed-output restart, '
        'one SDK entry/control, exact interruption and public archivedAt. Actual uncertain ACK remains uncertain.',
        'PASS', [item['file'], *evidence, 'logs/native-final-versions-r1.json',
                 'logs/native-package-r3.integrity.json'])

add(['AC1', 'AC2', 'AC3'], 'Independent B: node test/run.mjs; current focused Cargo e2e inventory and five composed tests',
    'Serialized reviewer slot; 90 plugin tests; 5 real PostgreSQL/WSS tests; 3 explicit native ignores separately exercised by root',
    'PASS', ['logs/reviewer-b1-r2-plugin-tests.txt', 'logs/reviewer-b1-r2-e2e-inventory.txt',
             'logs/reviewer-b1-r2-e2e-tests.txt', 'logs/reviewer-b1-r2-runtime-results.json'])

add(['AC6'], 'Historical R1 installation-output retention audit', 'Rejected historical preliminary round',
    'NOT EXERCISED', ['logs/runtime-install-retention-loss-r1.json',
                      'logs/reviewer-b1-r2-retained-evidence-audit.json'],
    'An earlier stdout file was overwritten by an author filename collision; original bytes could not be recovered '
    'and were not regenerated. Historical R1 manifest remains unchanged and invalid at that entry. '
    'Current installation was independently exercised and retained under a unique path above; no aggregate historical-validation PASS.')

manifest = dict(version=1, round='CL-P07-B1-R2-final-v2', artifact=artifact, checks=checks)
with (logs / 'CL-P07-B1-R2.final-v2.evidence.json').open('x') as stream:
    json.dump(manifest, stream, indent=2)
    stream.write('\n')
print('Saved final manifest:', len(checks), 'checks for', artifact)
