import json,os,subprocess
url=os.environ['DATABASE_URL']
def query(sql):
 return subprocess.check_output(['psql',url,'-X','-v','ON_ERROR_STOP=1','-Atqc',sql],text=True).strip()
query('CREATE EXTENSION IF NOT EXISTS pg_stat_statements')
settings={k:query('SHOW '+k) for k in ['server_version','compute_query_id','pg_stat_statements.track','pg_stat_statements.track_utility','log_parameter_max_length','log_parameter_max_length_on_error']}
assert settings['server_version'].startswith('18.6'),settings
assert settings['compute_query_id'] in ('auto','on'),settings
assert settings['pg_stat_statements.track']=='top',settings
assert settings['pg_stat_statements.track_utility']=='off',settings
assert settings['log_parameter_max_length']==settings['log_parameter_max_length_on_error']=='0',settings
query('SELECT 1 AS history_ci_query_probe')
probe=query("SELECT queryid FROM pg_stat_statements WHERE dbid=(SELECT oid FROM pg_database WHERE datname=current_database()) AND query LIKE '%history_ci_query_probe%' LIMIT 1")
assert probe and probe.lstrip('-').isdigit(),probe
print(json.dumps({'status':'PASS','settings':settings,'actual_probe_query_id':probe},indent=2))
