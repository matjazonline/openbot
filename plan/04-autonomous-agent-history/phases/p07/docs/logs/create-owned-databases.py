from pathlib import Path
import json,subprocess
r=json.loads(Path(__file__).with_name('resources.json').read_text())
assert r['port']==65037 and r['database'].startswith('openbot_ah_p07_') and r['test_database']==r['database']+'_test'
for name in (r['database'],r['test_database']):
 subprocess.run(['createdb','-h','127.0.0.1','-p',str(r['port']),name],check=True)
