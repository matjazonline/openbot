from pathlib import Path
import json,subprocess
r=json.loads(Path(__file__).with_name('resources.json').read_text())
assert r['port']==65037
assert (r['database'],r['test_database'])==('openbot_ah_p04','openbot_ah_p04_test')
base=['psql','-X','-v','ON_ERROR_STOP=1','-h','127.0.0.1','-p',str(r['port']),'-d','postgres']
for name in (r['database'],r['test_database']):
 subprocess.run([*base,'-c',f'DROP DATABASE IF EXISTS "{name}" WITH (FORCE)'],check=True)
 subprocess.run([*base,'-c',f'CREATE DATABASE "{name}"'],check=True)
