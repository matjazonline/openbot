"""Clean only the p05 R3 owned fixtures; keep the daemon/images for reviewer inspection."""
from pathlib import Path
import json
import re
import subprocess

logs = Path(__file__).resolve().parent
root = Path((logs / 'B1-R3-resource-root.txt').read_text().strip())
acceptance = json.loads((root / 'doop-acceptance-4/acceptance.json').read_text())
assert acceptance['status'] == 'PASS'
for item in acceptance['runs']:
    receipt = json.loads((root / 'doop-acceptance-4' / item['name'] / 'cleanup-receipt.json').read_text())
    assert receipt['confirmed'] and receipt['runtime_removed'], item


def run(command):
    result = subprocess.run(command, capture_output=True, text=True, timeout=60)
    print('Command:', ' '.join(command), flush=True)
    print(result.stdout + result.stderr, end='', flush=True)
    result.check_returncode()
    return result.stdout.strip()


docker = str(root / 'docker-bin/docker')
owned = {'openbot-p05-r3-doop-pg', 'openbot-p05-r3-memory-pg'}
current = set(run([docker, 'ps', '-a', '--format', '{{.Names}}']).splitlines())
assert current <= owned, 'unexpected runtime: inspect ownership, do not remove'
if current:
    run([docker, 'rm', '-f', *sorted(current)])
r = json.loads((logs / 'B1-R3-native-pg-resource.json').read_text())
base = ['psql', '-h', '127.0.0.1', '-p', str(r['port']), '-U', 'mac02', '-d', 'postgres', '-X', '-v', 'ON_ERROR_STOP=1', '-Atqc']
databases = run(base + ["SELECT datname FROM pg_database WHERE datname LIKE 'open_bots_own_%'"]).splitlines()
for name in databases:
    assert re.fullmatch('open_bots_own_[0-9a-f]{32}', name), name
    run(['dropdb', '-h', '127.0.0.1', '-p', str(r['port']), '-U', 'mac02', '--force', name])
assert not run(base + ["SELECT datname FROM pg_database WHERE datname LIKE 'open_bots_own_%'"])
run(['pg_ctl', '-D', r['datadir'], '-m', 'fast', '-w', 'stop'])
assert not (Path(r['datadir']) / 'postmaster.pid').exists()
assert not run([docker, 'ps', '-a', '--format', '{{.Names}}'])
print('PASS: all four deployment runtimes removed; native cluster stopped; owned fixture containers removed.')
print('Owned Docker daemon/images and immutable helpers retained temporarily for independent review.')
print('No shared database/server, global Docker context or other Colima profile changed.')
