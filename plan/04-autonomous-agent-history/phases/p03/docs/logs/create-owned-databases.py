from pathlib import Path
import json,subprocess
resources=json.loads(Path(__file__).with_name('resources.json').read_text())
base=['psql','-X','-v','ON_ERROR_STOP=1','-h','127.0.0.1','-p',str(resources['port']),'-d','postgres']
assert resources['port']==65037
for name in [resources['database'],resources['test_database']]:
 assert name in ('openbot_ah_p03','openbot_ah_p03_test')
 existing=subprocess.check_output([*base,'-At','-c',f"SELECT datname FROM pg_database WHERE datname='{name}'"]).decode().strip()
 assert not existing,existing
 subprocess.run([*base,'-c',f'CREATE DATABASE "{name}"'],check=True)
