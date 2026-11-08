#!/usr/bin/env python3
import json
from pathlib import Path
import subprocess
resources=json.loads(Path(__file__).with_name('resources.json').read_text())
for name in (resources['database'],resources['test_database']):
    assert name in ('openbot_ah_p02','openbot_ah_p02_test'), name
    for command in ('DROP DATABASE IF EXISTS','CREATE DATABASE'):
        subprocess.run(['psql','-X','-v','ON_ERROR_STOP=1','-h','127.0.0.1','-p',str(resources['port']),'-d','postgres','-c',f'{command} "{name}"'],check=True)
