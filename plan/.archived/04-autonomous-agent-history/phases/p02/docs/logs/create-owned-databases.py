#!/usr/bin/env python3
import json
from pathlib import Path
import subprocess
resources=json.loads(Path(__file__).with_name('resources.json').read_text())
for name in (resources['database'],resources['test_database']):
    assert name in ('openbot_ah_p02','openbot_ah_p02_test'),name
    base=['psql','-X','-v','ON_ERROR_STOP=1','-h','127.0.0.1','-p',str(resources['port']),'-d','postgres']
    existing=subprocess.check_output([*base,'-Atc',f"SELECT datname FROM pg_database WHERE datname='{name}'"],text=True).strip()
    assert not existing, f'Unexpected existing database; no reset: {name}'
    subprocess.run([*base,'-c',f'CREATE DATABASE "{name}"'],check=True)
