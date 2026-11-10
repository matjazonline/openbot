from pathlib import Path
import json
import os
import shutil
import signal
import subprocess
import tempfile
import time
import uuid

root = Path('/Users/mac03/dev/openbot')
logs = root / 'plan/cloudroom-local/phases/p07/docs/logs'
environment = os.environ.copy()
environment.pop('ALLOW_MISSING_DATABASE_URL', None)
environment.update(CARGO_INCREMENTAL='0', SQLX_OFFLINE='true', PYTHONDONTWRITEBYTECODE='1')
databases = json.loads((logs / 'final-databases-r2.json').read_text())
database = 'openbot_cl_stock_' + uuid.uuid4().hex[:10]
subprocess.run(['/opt/homebrew/opt/postgresql@18/bin/createdb', '-h', '127.0.0.1', '-p', '56388', database], check=True)
environment['DATABASE_URL'] = 'postgres://mac03@127.0.0.1:56388/' + databases['build']
environment['TEST_DATABASE_URL'] = 'postgres://mac03@127.0.0.1:56388/' + database
real_cargo = shutil.which('cargo')
bash = shutil.which('bash')
assert real_cargo and bash
directory = Path(tempfile.mkdtemp(prefix='ob7-stack-', dir='/tmp')).resolve()
wrapper = '#!' + bash + '\nset -o pipefail\n"$OPENBOT_REAL_CARGO" "$@" 2>&1 | tee -a "$OPENBOT_STACK_PROGRESS"\n'
(directory / 'cargo').write_text(wrapper)
(directory / 'cargo').chmod(0o700)
(logs / 'cargo-stream-wrapper-r1.sh').write_text(wrapper)
environment['OPENBOT_REAL_CARGO'] = real_cargo
environment['PATH'] = str(directory) + ':' + environment['PATH']
metadata = dict(owner='root/cloudroom-local/p07/stock-r2', directory=str(directory),
                testDatabase=database, buildDatabase=databases['build'], port=56388,
                realCargo=real_cargo, wrapper='logs/cargo-stream-wrapper-r1.sh',
                timeoutSeconds=3600, sourceArtifact='bd92212d24d49b9d937eac2c52723eae45085cdb81abc36d12c262dcdf33cbe9')
(logs / 'stock-stream-resource-r1.json').write_text(json.dumps(metadata, indent=2) + '\n')
results = []
base = ['scripts/test-network-isolation.sh', 'scripts/stack-budget.sh', '--offline']
for name, arguments, timeout in [
    ('stock-stream-proof-r1', base + ['workflow_action_dispatch_resolution_stalled_local_effect_once'], 120),
    ('stock-correction-full-r2', base, 3600),
]:
    progress = logs / (name + '-progress.txt')
    progress.open('x').close()
    environment['OPENBOT_STACK_PROGRESS'] = str(progress)
    started = time.monotonic()
    with (logs / (name + '.txt')).open('x') as output:
        process = subprocess.Popen(arguments, cwd=root, env=environment,
                                   stdout=output, stderr=subprocess.STDOUT, start_new_session=True)
        metadata.update(activeName=name, activePid=process.pid)
        (logs / 'stock-stream-resource-r1.json').write_text(json.dumps(metadata, indent=2) + '\n')
        try:
            status = process.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGTERM)
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait()
            status = 124
            output.write('\nVerification supervisor timeout; process group terminated.\n')
    result = dict(name='stack' if name == 'stock-correction-full-r2' else name,
                  command=arguments, status='PASS' if status == 0 else 'FAIL', exit=status,
                  seconds=round(time.monotonic() - started, 2), file='logs/' + name + '.txt',
                  progress='logs/' + progress.name)
    results.append(result)
    (logs / 'final-correction-results-r2.json').write_text(json.dumps(results, indent=2) + '\n')
    print(result, flush=True)
    if status:
        raise SystemExit(status)
