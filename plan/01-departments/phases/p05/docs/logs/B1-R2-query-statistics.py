"""Run CI statistics assertions on a p05-owned PostgreSQL cluster, never the shared server."""
from pathlib import Path
import json
import os
import re
import socket
import subprocess

logs = Path(__file__).resolve().parent
scratch = Path((logs/'B1-fixture-root.txt').read_text().strip())
data = scratch/'query-statistics-cluster'
server_log = logs/'B1-R2-query-statistics-server.txt'
with socket.socket() as port_probe:
    port_probe.bind(('127.0.0.1', 0))
    port = port_probe.getsockname()[1]
url = f'postgres://mac02@127.0.0.1:{port}/openbot_dept_p05_stats'
resource = dict(datadir=str(data), port=port, database='openbot_dept_p05_stats', owner='p05', shared_server_modified=False)
(logs/'B1-R2-query-statistics-resource.json').write_text(json.dumps(resource,indent=2)+'\n')

def run(command, env=None):
    print('Command:', ' '.join(command), flush=True)
    result = subprocess.run(command, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, timeout=60)
    print(result.stdout, end='', flush=True)
    result.check_returncode()
    return result.stdout.strip()

def sql(statement):
    return run(['psql',url,'-X','-v','ON_ERROR_STOP=1','-Atqc',statement])

try:
    run(['postgres','--version'])
    run(['initdb','-D',str(data),'-U','mac02','-A','trust','--no-locale','-E','UTF8'])
    options = f'-p {port} -c listen_addresses=127.0.0.1 -c unix_socket_directories= -c shared_preload_libraries=pg_stat_statements -c compute_query_id=on -c pg_stat_statements.track=top -c pg_stat_statements.track_utility=off -c log_parameter_max_length=0 -c log_parameter_max_length_on_error=0'
    run(['pg_ctl','-D',str(data),'-l',str(server_log),'-o',options,'-w','start'])
    run(['createdb','-h','127.0.0.1','-p',str(port),'-U','mac02','openbot_dept_p05_stats'])
    env=os.environ.copy();env.update(DATABASE_URL=url,SQLX_OFFLINE='false')
    run(['cargo','sqlx','migrate','run'],env=env)
    for key, expected in [('shared_preload_libraries','pg_stat_statements'),('compute_query_id','on'),('pg_stat_statements.track','top'),('pg_stat_statements.track_utility','off'),('log_parameter_max_length','0'),('log_parameter_max_length_on_error','0')]:
        actual=sql('SHOW '+key)
        assert actual==expected,(key,actual,expected)
        print('PASS setting:',key,'=',expected,flush=True)
    assert sql('SELECT 1 AS ci_pg_stat_statements_probe')=='1'
    query="SELECT queryid FROM pg_stat_statements WHERE dbid = (SELECT oid FROM pg_database WHERE datname = current_database()) AND query LIKE '%ci_pg_stat_statements_probe%' LIMIT 1"
    queryid=sql(query)
    assert re.fullmatch(r'-?[0-9]+',queryid),queryid
    normalized=sql("SELECT query FROM pg_stat_statements WHERE queryid = "+queryid+" AND dbid = (SELECT oid FROM pg_database WHERE datname = current_database()) LIMIT 1")
    assert 'SELECT $1 AS ci_pg_stat_statements_probe' in normalized,normalized
    print('PASS: all CI query-statistics activation/privacy assertions and numeric normalized queryid probe exercised',flush=True)
finally:
    if (data/'postmaster.pid').exists():
        run(['pg_ctl','-D',str(data),'-m','fast','-w','stop'])
    print('Owned cluster stopped; shared server untouched',flush=True)
