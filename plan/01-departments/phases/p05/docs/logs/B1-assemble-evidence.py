"""Assemble retained check receipts; never infer behavioral truth from validator success."""
import hashlib
import json
from pathlib import Path
import re

logs = Path(__file__).resolve().parent
snapshot = (logs / 'B1-snapshot.txt').read_text()
artifact = re.search(r'^digest: ([0-9a-f]{64})$', snapshot, re.M).group(1)
checks = []

def add(criteria, command, status, files, fixture='p05 local fixtures; see TASK resource identities', **extra):
    entry = dict(criteria=criteria, command=command, fixture=fixture, status=status,
                 files=[dict(path='logs/' + name, sha256=hashlib.sha256((logs/name).read_bytes()).hexdigest()) for name in files])
    entry.update(extra)
    checks.append(entry)

mapping = {
    'Generated CSS is current': ('generated_css_is_current (scripts/check.sh)', ['05A.2', 'css']),
    'SSE navigation lifecycle': ('node --test scripts/tests/sse-navigation.mjs scripts/tests/agent-runtime.mjs scripts/tests/channel-response-authoring.mjs', ['05A.2', 'CI-navigation']),
    'Deployment script tests': ('scripts/tests/deploy.sh && scripts/tests/credential-key-rotation.sh', ['05A.2', 'CI-deployment']),
    'Formatting': ('cargo fmt --all -- --check', ['05A.4-format']),
    'Patch whitespace': ('git diff HEAD --check', ['05A.4-whitespace']),
    'Transport abstraction boundary': ('scripts/tests/transport-boundary.sh && scripts/transport-boundary-check.sh', ['05A.2', 'CI-transport']),
    'Workflow evidence validator fixtures': ('python3 scripts/tests/workflow-evidence.py', ['05A.2', 'G1', 'G2']),
    'Retained workflow evidence': ('python3 scripts/check-workflow-evidence.py --available-manifests', ['05A.2', 'G1', 'G2']),
    'Fetch locked Rust dependencies': ('cargo fetch --locked', ['05A.2']),
    'Offline compilation': ('cargo check --locked --offline --all-targets', ['05A.4-offline']),
    'Clippy': ('cargo clippy --locked --all-targets -- -D warnings', ['05A.4-clippy']),
    'Migrations': ('SQLX_OFFLINE=false cargo sqlx migrate run', ['05A.1', '05A.4-migrations']),
    'SQLx metadata matches migrated schema': ('SQLX_OFFLINE=false cargo sqlx prepare --check -- --all-targets', ['05A.4-sqlx']),
    'Compile tests': ('cargo test --locked --offline --all-targets --no-run', ['05A.4-offline']),
    'Rust source module reachability': ('cargo test --locked --offline --test source_modules', ['05A.4-reachability']),
    'Test database for this run': ('create_check_database (scripts/check.sh)', ['05A.2']),
    'Database-backed test suite (isolated network)': ('scripts/test-network-isolation.sh cargo test --locked --offline --all-targets', ['05A.4-database', '05A.4-network']),
    'Stack budget': ('scripts/test-network-isolation.sh scripts/stack-budget.sh --offline', ['05A.4-stack', '05A.5']),
}
full = (logs / 'B1-check-resume-1.txt').read_text()
summary = full.split('\n==> Summary\n')[-1]
results = re.findall(r'^(PASS|FAIL)\s+\d+s\s+(.+)$', summary, re.M)
assert {name for _, name in results} == set(mapping), 'missing/full gate summary mismatch'
for status, name in results:
    command, criteria = mapping[name]
    note = 'Actual scripts/check.sh gate result.'
    if name == 'Retained workflow evidence': note += ' Ran before G2 repair; later aggregate validation supersedes it, without deleting this failure.'
    if name in ['Database-backed test suite (isolated network)', 'Stack budget']: note += ' Retain complete result; unrelated failures and any stack observation are documented separately in TASK.'
    add(criteria, command, status, ['B1-check-resume-1.txt'], note=note)
add(['05A.2'], 'scripts/check.sh', 'FAIL' if any(s=='FAIL' for s,_ in results) else 'PASS', ['B1-check-resume-1.txt'])
add(['G1','G2'], 'python3 scripts/check-workflow-evidence.py --available-manifests (after both authorized repairs)', 'PASS', ['G2-validation.txt','G1-repair.txt','G2-repair.txt','G1-original-p04-R1-manifest.json','G2-original-p04-R1-manifest.json'])
add(['05A.1'], 'SQLX_OFFLINE=false cargo sqlx prepare -- --all-targets (fresh disposable DB, checkpoint execution)', 'PASS', ['B1-sqlx-prepare.txt','B1-sqlx-cache-diff.txt'])
add(['05A.4-binary'], 'scripts/test-network-isolation.sh cargo test --locked --offline --bin open_bots -- --test-threads=4', 'PASS', ['B1-main-target.txt'])
add(['05A.4-inventory'], 'cargo test --locked --offline --all-targets -- --list', 'PASS', ['B1-full-test-inventory.txt'])
add(['05A.4-unrelated-failures'], 'scripts/test-network-isolation.sh cargo test --locked --offline --lib mcp_dropped_call_sends_protocol_cancellation_and_joins_cleanup -- --nocapture --test-threads=1', 'FAIL', ['B1-mcp-cancellation-rerun-1.txt','B1-unrelated-failures-source-identity.txt'], note='Confirmed unchanged MCP adapter/scenario/dependencies; shared stub diff is unrelated. No assertion skipped or weakened.')
assert re.search(r'test .*department_desk_relay_into_a_members_only_desk_stays_trusted \.\.\. ok', full)
add(['C001-r1'], 'full library suite: department_desk_relay_into_a_members_only_desk_stays_trusted', 'PASS', ['B1-check-resume-1.txt','B1-doc-contract-inspection.txt'])
add(['C004-r1'], 'inspect shipped docs, fixture, planner/writer/routing diffs against accepted p03', 'PASS', ['B1-doc-contract-inspection.txt'], note='Inspection claim only; automated dispatch, escalation and round-trip/loop checks remain deferred by C004.')
for file in ['B1-ci-results.json','B1-ci-browser-reruns.json']:
    entries=json.loads((logs/file).read_text())
    if file=='B1-ci-results.json': assert len(entries)==28
    for item in entries:
        name=item['name'].removesuffix('-rerun')
        extra={}
        if item['status']=='FAIL' and name in ['knowledge-browser','memory-browser']: extra['note']='Pinned Playwright shell absent on initial attempt; successful reruns retained separately.'
        add(['05A.3','CI-'+name], ' '.join(item['command']), item['status'], [item['log']], **extra)
add(['05A.3','CI-cloudroom'], 'npm ci; npm run typecheck; npm run build; npm test; npm run test:guards (integrations/cloudroom-plugin)', 'PASS', ['B1-cloudroom-npm-ci.txt','B1-cloudroom-plugin.txt'], fixture='checkpoint toolchain Node 26.5.0/npm 11.17.0; unchanged plugin sources; exact package runtime checked separately')
add(['05A.3','CI-cloudroom-package'], 'Node 24.21.0/npm 11.19.0: npm --prefix integrations/cloudroom-plugin run package; direct pinned npm ci --omit=dev in extracted tgz', 'PASS', ['B1-cloudroom-package-resume-2.txt','B1-cloudroom-runtime-install-2.txt'])
add(['05A.3','CI-node-extra'], 'node --test scripts/tests/workflow-runs.mjs scripts/tests/workflow-conversations.mjs scripts/tests/workflow-samples.mjs', 'PASS', ['B1-node-extra.txt'])
add(['05A.3','CI-run-counts'], 'npm run test:run-counts', 'PASS', ['B1-run-counts.txt'])
add(['05A.3','CI-knowledge-doop-unit'], 'python3 scripts/tests/knowledge-model-prepare.py; python3 scripts/tests/doop-test.py; node --test scripts/tests/doop-probe.mjs', 'PASS', ['B1-py-node-gates.txt'])
lease=json.loads((logs/'B1-lease-result.json').read_text())
add(['05A.4-lease','05A.5'], 'scripts/test-network-isolation.sh scripts/lease-budget.sh --offline', lease['status'], ['B1-lease-budget.txt','B1-lease-events.txt','B1-budget-environment.txt'], fixture='openbot_dept_p05_protocol; no competing phase checks; default 50 percent WORK budget; one test thread')
add(['05A.6-graph'], 'graft build', 'PASS', ['B1-graft-build.txt','B1-graft-build-final.txt'])
for criterion,command,reason in [
 ('CI-knowledge-image','docker build --tag openbot-knowledge-ci .; python3 scripts/check-knowledge-image.py --image openbot-knowledge-ci','Docker daemon unavailable; no production image built or tested.'),
 ('CI-doop-integration','complete .github/workflows/ci.yml doop-integration job','Docker daemon unavailable; real private deployment/probe acceptance not exercised.'),
 ('CI-memory-integration','scripts/memory-integration.sh','Local pgvector and pinned model absent; Docker daemon unavailable; real memory service acceptance not exercised.')]:
    add(['05A.3',criterion],command,'NOT EXERCISED',['B1-unavailable-prerequisites-final.txt'],reason=reason)
manifest=dict(version=1,round='01DEP-P05-B1-R1',artifact=artifact,checks=checks)
(logs/'01DEP-P05-B1-R1.evidence.json').write_text(json.dumps(manifest,indent=2)+'\n')
print('Manifest:',len(checks),'checks;', {s:sum(x['status']==s for x in checks) for s in ['PASS','FAIL','NOT EXERCISED']})
