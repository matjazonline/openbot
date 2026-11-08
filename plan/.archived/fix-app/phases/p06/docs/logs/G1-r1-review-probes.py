import subprocess, time, uuid, json, os
from pathlib import Path
PSQL='/opt/homebrew/opt/postgresql@18/bin/psql'
URL='postgres://mac02@127.0.0.1:55406/open_bots_test'
schema='p06_review_'+uuid.uuid4().hex
app=schema+'_admission'
records=[]
def sql(text, extra=None):
 p=subprocess.run([PSQL, URL, '-X', '-qAt', '-v', 'ON_ERROR_STOP=1', '-c', text], text=True, capture_output=True, env={**os.environ,**(extra or {})})
 records.append({'sql':text,'exit':p.returncode,'stdout':p.stdout,'stderr':p.stderr})
 return p
try:
 assert sql(f'CREATE SCHEMA {schema}; CREATE TABLE {schema}.bundles (digest int PRIMARY KEY); CREATE TABLE {schema}.runs (id int PRIMARY KEY, digest int NOT NULL REFERENCES {schema}.bundles(digest) DEFERRABLE INITIALLY DEFERRED); INSERT INTO {schema}.bundles VALUES (1);').returncode==0
 statement=f'BEGIN; INSERT INTO {schema}.bundles VALUES (1) ON CONFLICT DO NOTHING; SELECT pg_sleep(4); INSERT INTO {schema}.runs VALUES (1,1); COMMIT;'
 a=subprocess.Popen([PSQL,URL,'-X','-qAt','-v','ON_ERROR_STOP=1','-c',statement], stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,env={**os.environ,'PGAPPNAME':app})
 observed=False
 for _ in range(100):
  status=sql(f"SELECT wait_event FROM pg_stat_activity WHERE application_name='{app}'")
  if 'PgSleep' in status.stdout:
   observed=True; break
  time.sleep(.02)
 assert observed, 'admission barrier not reached'
 result=sql(f'BEGIN; SET LOCAL lock_timeout=\'500ms\'; DELETE FROM {schema}.bundles AS bundle WHERE digest IN (SELECT digest FROM {schema}.bundles FOR UPDATE SKIP LOCKED LIMIT 1) AND NOT EXISTS (SELECT 1 FROM {schema}.runs AS run WHERE run.digest=bundle.digest) RETURNING digest; COMMIT;')
 out,err=a.communicate(timeout=10)
 records.append({'name':'admission after conflict reuse and concurrent GC','sql':statement,'exit':a.returncode,'stdout':out,'stderr':err})
 assert result.returncode==0 and '1' in result.stdout
 assert a.returncode!=0 and 'foreign key constraint' in err
 sql(f'CREATE TABLE {schema}.owners(id int PRIMARY KEY); CREATE TABLE {schema}.executions(id int PRIMARY KEY, run int REFERENCES {schema}.owners(id) ON DELETE CASCADE); CREATE TABLE {schema}.jobs(id int PRIMARY KEY, execution int REFERENCES {schema}.executions(id) DEFERRABLE INITIALLY DEFERRED); INSERT INTO {schema}.owners VALUES(1); INSERT INTO {schema}.executions VALUES(1,1); INSERT INTO {schema}.jobs VALUES(1,1);')
 p=sql(f'BEGIN; DELETE FROM {schema}.owners WHERE id=1; COMMIT;')
 assert p.returncode!=0 and 'foreign key constraint' in p.stderr
 sql("SELECT tgname,pg_get_triggerdef(oid) FROM pg_trigger WHERE tgrelid='workflow_executions'::regclass AND NOT tgisinternal ORDER BY tgname")
 sql("SELECT current_database(),current_user,inet_server_port(),version()")
finally:
 sql(f'DROP SCHEMA IF EXISTS {schema} CASCADE')
 Path('plan/fix-app/phases/p06/docs/logs/G1-r1-review-probes.json').write_text(json.dumps(records,indent=2)+'\n')
print(json.dumps({'isolated_schema':schema,'bundle_GC_probe':'reused bundle deleted; admission FK fails','deferred_job_probe':'run delete rolls back with surviving job FK','cleanup':'complete','log':'logs/G1-r1-review-probes.json'}))
