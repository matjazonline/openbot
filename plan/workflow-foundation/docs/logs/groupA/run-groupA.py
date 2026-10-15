"""Group A runtime verification: run the prebuilt library test binary at stock 2 MiB.

Each filter must pass exactly as many tests as `--list` reports for it; zero is a failure.
Leftover `own_database` databases and foreign connections are inventoried after every filter.
"""
from pathlib import Path
import hashlib, json, os, re, subprocess, sys, time

ROOT = Path('/home/mh/dev/openbot')
LOGS = ROOT / 'plan/workflow-foundation/docs/logs/groupA'
BINARY = Path(os.environ.get('GROUPA_BINARY', ROOT / 'target/debug/deps/mail_agents-43f563ba6b6c24ac'))
EXPECTED_SHA = os.environ.get('GROUPA_SHA', 'f14fb5cda77c66735354f34141597cdc9ef718038eed2901c121d9340168517e')
LABEL = os.environ.get('GROUPA_LABEL', '')
CWD = Path(os.environ.get('GROUPA_CWD', ROOT))
URL = 'postgres://postgres:workflow048@127.0.0.1:55448/workflow048_test'
FILTERS = sys.argv[1:] or [
    'workflow_action_dispatch_',
    'workflow_action_receipt_return_authority_errors_propagate_after_commit',
    'workflow_action_receipt_refused_return_releases_run_before_returning',
    'workflow_action_receipt',
    'workflow_action_',
    'workflow_control_',
    'control_tests',
    '::controls::',
]

with BINARY.open('rb') as artifact:
    digest = hashlib.file_digest(artifact, 'sha256').hexdigest()
assert digest == EXPECTED_SHA, f'binary digest {digest} differs from {EXPECTED_SHA}'

env = os.environ.copy()
env.update(DATABASE_URL=URL, TEST_DATABASE_URL=URL, RUST_MIN_STACK='2097152')
env.pop('ALLOW_MISSING_DATABASE_URL', None)
env.pop('SQLX_OFFLINE', None)


def residue():
    query = ("SELECT (SELECT count(*) FROM pg_database WHERE datname LIKE 'mail\\_agents\\_own\\_%'),"
             " (SELECT count(*) FROM pg_stat_activity WHERE backend_type = 'client backend'"
             " AND pid <> pg_backend_pid())")
    out = subprocess.run(['psql', URL, '-X', '-At', '-c', query], capture_output=True, text=True,
                         env={**os.environ, 'PGPASSWORD': 'workflow048'})
    return out.stdout.strip() or out.stderr.strip()


results_path = LOGS / f'runtime-results{LABEL}.json'
results = json.loads(results_path.read_text()) if results_path.exists() else []
failed = False
for selected in FILTERS:
    listed = subprocess.run([str(BINARY), selected, '--list'], cwd=CWD, env=env,
                            capture_output=True, text=True).stdout
    expected = sum(1 for line in listed.splitlines() if line.endswith(': test'))
    slug = re.sub(r'[^A-Za-z0-9]+', '-', selected).strip('-')
    log = LOGS / f'runtime-{slug}{LABEL}.log'
    command = [str(BINARY), selected]
    started = time.time()
    with log.open('w') as stream:
        stream.write(f'# {time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(started))} '
                     f'binary={BINARY} sha256={digest} cwd={CWD} RUST_MIN_STACK=2097152 DATABASE_URL=TEST_DATABASE_URL={URL}\n# $ {" ".join(command)}\n')
        stream.flush()
        result = subprocess.run(command, cwd=CWD, env=env, stdout=stream, stderr=subprocess.STDOUT)
    elapsed = round(time.time() - started, 2)
    output = log.read_text()
    summary = re.search(r'test result: (\w+)\. (\d+) passed; (\d+) failed; (\d+) ignored; '
                        r'(\d+) measured; (\d+) filtered out', output)
    passed = int(summary[2]) if summary else 0
    success = (result.returncode == 0 and summary is not None and summary[1] == 'ok'
               and passed == expected and expected > 0 and int(summary[4]) == 0)
    entry = {'filter': selected, 'command': command, 'exit_code': result.returncode,
             'expected': expected, 'summary': summary[0] if summary else None,
             'elapsed_s': elapsed, 'success': success, 'log': str(log),
             'residue_own_dbs_and_foreign_clients_after': residue(), 'binary_sha256': digest}
    results.append(entry)
    results_path.write_text(json.dumps(results, indent=2) + '\n')
    print(json.dumps(entry), flush=True)
    failed |= not success
sys.exit(1 if failed else 0)
