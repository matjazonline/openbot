"""Reduced PostgreSQL ownership/capture probes, not production admission/retention tests."""
import json,os,subprocess,time,uuid
from pathlib import Path
PSQL='/opt/homebrew/opt/postgresql@18/bin/psql'
URL='postgres://mac02@127.0.0.1:55406/open_bots_test'
S='p06_g1r3_review_'+uuid.uuid4().hex
records=[]
processes=[]
def sql(q):
 r=subprocess.run([PSQL,URL,'-XqAt','-v','ON_ERROR_STOP=1','-c',q],text=True,capture_output=True)
 records.append({'sql':q,'exit':r.returncode,'stdout':r.stdout,'stderr':r.stderr})
 return r
def checked(q):
 r=sql(q);assert r.returncode==0,r.stderr;return r.stdout.strip()
def create_temp():
 return 'CREATE TEMP TABLE retained_jobs(company int NOT NULL,id int NOT NULL,execution int NOT NULL,PRIMARY KEY(company,id)) ON COMMIT DROP;'
def capture(prefix,root=1):
 return f'''INSERT INTO pg_temp.retained_jobs(company,id,execution)
 SELECT job.company,job.id,job.execution FROM {S}.{prefix}jobs AS job
 JOIN {S}.{prefix}steps AS step ON step.company=job.company AND step.id=job.execution
 JOIN {S}.{prefix}links AS link ON link.company=step.company AND link.run=step.run
 WHERE link.company=1 AND link.root={root} ORDER BY job.id FOR UPDATE OF job NOWAIT;'''
def locks(prefix):
 return f'''SELECT count(*) FROM (SELECT run.id FROM {S}.{prefix}runs AS run
 JOIN {S}.{prefix}links AS link ON link.company=run.company AND link.run=run.id
 WHERE link.company=1 AND link.root=1 ORDER BY run.id FOR UPDATE OF run NOWAIT) AS locked;'''
def purge(fail=False):
 return f'''BEGIN; SET LOCAL statement_timeout='5s'; SET LOCAL lock_timeout='500ms';
 {locks('')} {create_temp()} {capture('')}
 INSERT INTO {S}.history SELECT * FROM {S}.usage WHERE company=1;
 WITH retained AS MATERIALIZED(SELECT company,run FROM {S}.links WHERE company=1 AND root=1)
 DELETE FROM {S}.runs AS run USING retained WHERE run.company=retained.company AND run.id=retained.run;
 {"DO $$ BEGIN RAISE EXCEPTION 'between run and job deletion'; END $$;" if fail else ''}
 DELETE FROM {S}.jobs AS job USING pg_temp.retained_jobs AS saved
 WHERE job.company=saved.company AND job.id=saved.id AND job.execution=saved.execution;
 COMMIT; SELECT to_regclass('pg_temp.retained_jobs') IS NULL;'''
try:
 checked(f'''CREATE SCHEMA {S};
 CREATE TABLE {S}.large_runs(company int,id int,PRIMARY KEY(company,id));
 CREATE TABLE {S}.large_links(company int,run int,root int,PRIMARY KEY(company,run));
 CREATE INDEX ON {S}.large_links(company,root,run);
 CREATE TABLE {S}.large_steps(company int,id int,run int,PRIMARY KEY(company,id));
 CREATE TABLE {S}.large_jobs(company int,id int,execution int,PRIMARY KEY(company,id));
 INSERT INTO {S}.large_runs SELECT 1,n FROM generate_series(1,100001) AS n;
 INSERT INTO {S}.large_links SELECT 1,n,1 FROM generate_series(1,100001) AS n;
 INSERT INTO {S}.large_steps SELECT 1,n,n FROM generate_series(1,100001) AS n;
 INSERT INTO {S}.large_jobs SELECT 1,n,n FROM generate_series(1,100001) AS n;
 ANALYZE {S}.large_runs; ANALYZE {S}.large_links; ANALYZE {S}.large_steps; ANALYZE {S}.large_jobs;''')
 app='p06r3_'+S[-12:]
 q=f'''BEGIN; SET LOCAL statement_timeout='5s'; SET LOCAL lock_timeout='500ms';
 {locks('large_')} {create_temp()} {capture('large_')}
 SELECT count(*) FROM pg_temp.retained_jobs; SELECT pg_sleep(2);
 ROLLBACK; SELECT to_regclass('pg_temp.retained_jobs') IS NULL;'''
 p=subprocess.Popen([PSQL,URL,'-XqAt','-v','ON_ERROR_STOP=1','-c',q],text=True,stdout=subprocess.PIPE,stderr=subprocess.PIPE,env={**os.environ,'PGAPPNAME':app});processes.append(p)
 deadline=time.monotonic()+5
 while time.monotonic()<deadline:
  if checked(f"SELECT coalesce(wait_event,'') FROM pg_stat_activity WHERE application_name='{app}'")=='PgSleep':break
  time.sleep(.02)
 else:raise AssertionError('large ownership/capture barrier not observed')
 for table in ['large_runs','large_jobs']:
  r=sql(f'SELECT id FROM {S}.{table} WHERE company=1 AND id=100001 FOR UPDATE NOWAIT')
  assert r.returncode!=0 and 'could not obtain lock' in r.stderr,(table,r.stderr)
 out,err=p.communicate(timeout=8);records.append({'large_owner_sql':q,'exit':p.returncode,'stdout':out,'stderr':err})
 assert p.returncode==0,err
 assert [line for line in out.splitlines() if line]==['100001','100001','t'],out
 checked(f'''CREATE TABLE {S}.runs(company int,id int,PRIMARY KEY(company,id));
 CREATE TABLE {S}.links(company int,run int,root int,PRIMARY KEY(company,run),
 FOREIGN KEY(company,run) REFERENCES {S}.runs(company,id) ON DELETE CASCADE);
 CREATE INDEX ON {S}.links(company,root,run);
 CREATE TABLE {S}.steps(company int,id int,run int,PRIMARY KEY(company,id),
 FOREIGN KEY(company,run) REFERENCES {S}.runs(company,id) ON DELETE CASCADE);
 CREATE INDEX ON {S}.steps(company,run);
 CREATE TABLE {S}.jobs(company int,id int,execution int,PRIMARY KEY(company,id),
 FOREIGN KEY(company,execution) REFERENCES {S}.steps(company,id) DEFERRABLE INITIALLY DEFERRED);
 CREATE INDEX ON {S}.jobs(company,execution);
 CREATE TABLE {S}.checkpoints(company int,job int,run int,amount int,
 FOREIGN KEY(company,job) REFERENCES {S}.jobs(company,id) ON DELETE CASCADE);
 CREATE FUNCTION {S}.fact_guard() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN
 IF pg_trigger_depth()>1 AND NOT EXISTS(SELECT 1 FROM {S}.runs WHERE company=OLD.company AND id=OLD.run) THEN RETURN OLD; END IF;
 RAISE EXCEPTION 'live run checkpoint is append only'; END $$;
 CREATE TRIGGER fact_guard BEFORE DELETE ON {S}.checkpoints FOR EACH ROW EXECUTE FUNCTION {S}.fact_guard();
 CREATE TABLE {S}.usage(company int,run int,amount int,FOREIGN KEY(company,run) REFERENCES {S}.runs(company,id) ON DELETE CASCADE);
 CREATE TABLE {S}.history(company int,run int,amount int);
 INSERT INTO {S}.runs SELECT 1,n FROM generate_series(1,4) AS n;
 INSERT INTO {S}.runs VALUES(2,1);
 INSERT INTO {S}.links SELECT company,id,1 FROM {S}.runs;
 INSERT INTO {S}.steps SELECT company,id,id FROM {S}.runs;
 INSERT INTO {S}.jobs SELECT company,id,id FROM {S}.runs;
 INSERT INTO {S}.checkpoints SELECT company,id,id,80 FROM {S}.runs;
 INSERT INTO {S}.usage SELECT company,id,80 FROM {S}.runs;''')
 failed=sql(purge(True));assert failed.returncode!=0 and 'between run and job deletion' in failed.stderr
 counts=f"SELECT (SELECT count(*) FROM {S}.runs WHERE company=1)||'|'||(SELECT count(*) FROM {S}.jobs WHERE company=1)||'|'||(SELECT count(*) FROM {S}.checkpoints WHERE company=1)||'|'||(SELECT count(*) FROM {S}.usage WHERE company=1)||'|'||(SELECT count(*) FROM {S}.history WHERE company=1)"
 assert checked(counts)=='4|4|4|4|0'
 assert checked(purge(False)).splitlines()==['4','t']
 assert checked(counts)=='0|0|0|0|4'
 assert checked(f'SELECT count(*) FROM {S}.checkpoints WHERE company=2')=='1'
 status='PASS'
except Exception as error:
 status='FAIL';records.append({'exception':repr(error)});raise
finally:
 for p in processes:
  if p.poll() is None:p.kill();p.communicate()
 sql(f'DROP SCHEMA IF EXISTS {S} CASCADE')
 Path(__file__).with_suffix('.json').write_text(json.dumps({'status':locals().get('status','FAIL'),'scope':'reduced relational/scaling protocol only; no production 100001-run admission or purge latency claimed','records':records},indent=2)+'\n')
print('PASS: 100001-row scalar ownership/temp capture, competing locks, guarded owner-before-job purge, rollback and temp cleanup.')
