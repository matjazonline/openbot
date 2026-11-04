"""Independent reduced checks of membership rollback and same-session temp cleanup."""
import json, os, subprocess, time, uuid
from pathlib import Path
PSQL='/opt/homebrew/opt/postgresql@18/bin/psql'
URL='postgres://mac02@127.0.0.1:55406/open_bots_test'
S='p06_r3_members_'+uuid.uuid4().hex
records=[]
owner=None

def sql(q):
    r=subprocess.run([PSQL,URL,'-XqAt','-v','ON_ERROR_STOP=1','-c',q],text=True,capture_output=True)
    records.append(dict(sql=q,exit=r.returncode,stdout=r.stdout,stderr=r.stderr))
    return r

def checked(q):
    r=sql(q)
    assert r.returncode==0,r.stderr
    return r.stdout.strip()

try:
    checked(f'''CREATE SCHEMA {S};
    CREATE TABLE {S}.runs(company int,id int,PRIMARY KEY(company,id));
    CREATE TABLE {S}.links(company int,run int,root int,PRIMARY KEY(company,run),
      FOREIGN KEY(company,run) REFERENCES {S}.runs ON DELETE CASCADE);
    CREATE TABLE {S}.usage(company int,run int,amount int,
      FOREIGN KEY(company,run) REFERENCES {S}.runs ON DELETE CASCADE);
    CREATE TABLE {S}.history(company int,run int,amount int);
    INSERT INTO {S}.runs VALUES(1,1),(1,2),(2,1);
    INSERT INTO {S}.links SELECT company,id,1 FROM {S}.runs;
    INSERT INTO {S}.usage SELECT company,id,80 FROM {S}.runs;''')
    # Deliberately permit a committed new member: native parent/root FK locks are
    # stronger. This isolates the proposed count-mismatch rollback safeguard.
    q=f'''BEGIN; SET LOCAL statement_timeout='5s'; SET LOCAL lock_timeout='500ms';
    SELECT set_config('p06.review_owned',count(*)::text,true) FROM (
      SELECT run.id FROM {S}.runs run JOIN {S}.links link
      ON link.company=run.company AND link.run=run.id
      WHERE link.company=1 AND link.root=1 ORDER BY run.id FOR UPDATE OF run NOWAIT) owned;
    SELECT pg_sleep(2);
    INSERT INTO {S}.history SELECT usage.* FROM {S}.usage usage JOIN {S}.links link
      ON link.company=usage.company AND link.run=usage.run WHERE link.company=1 AND link.root=1;
    DO $$ DECLARE removed bigint; BEGIN
      WITH retained AS MATERIALIZED(SELECT company,run FROM {S}.links WHERE company=1 AND root=1)
      DELETE FROM {S}.runs run USING retained WHERE run.company=retained.company AND run.id=retained.run;
      GET DIAGNOSTICS removed = ROW_COUNT;
      IF removed<>current_setting('p06.review_owned')::bigint THEN
        RAISE EXCEPTION 'review membership changed: locked %, removed %',current_setting('p06.review_owned'),removed;
      END IF;
    END $$;
    COMMIT;'''
    app='p06r3m_'+S[-10:]
    owner=subprocess.Popen([PSQL,URL,'-XqAt','-v','ON_ERROR_STOP=1','-c',q],text=True,
       stdout=subprocess.PIPE,stderr=subprocess.PIPE,env={**os.environ,'PGAPPNAME':app})
    end=time.monotonic()+5
    while time.monotonic()<end:
        if checked(f"SELECT coalesce(wait_event,'') FROM pg_stat_activity WHERE application_name='{app}'")=='PgSleep':break
        time.sleep(.02)
    else:raise AssertionError('ownership barrier not observed')
    checked(f'''BEGIN; INSERT INTO {S}.runs VALUES(1,3); INSERT INTO {S}.links VALUES(1,3,1);
      INSERT INTO {S}.usage VALUES(1,3,80); COMMIT;''')
    out,err=owner.communicate(timeout=8)
    records.append(dict(owner_sql=q,exit=owner.returncode,stdout=out,stderr=err))
    assert owner.returncode!=0 and 'review membership changed: locked 2, removed 3' in err,err
    assert checked(f'''SELECT (SELECT count(*) FROM {S}.runs WHERE company=1)||'|'||
      (SELECT count(*) FROM {S}.usage WHERE company=1)||'|'||
      (SELECT count(*) FROM {S}.history)||'|'||(SELECT count(*) FROM {S}.runs WHERE company=2)''')=='3|3|0|1'
    # -f keeps this single session alive through the expected aborted transaction.
    q='''BEGIN;
    CREATE TEMP TABLE workflow_retention_jobs(company_id uuid NOT NULL,job_id uuid NOT NULL,
      execution_id uuid NOT NULL,PRIMARY KEY(company_id,job_id)) ON COMMIT DROP;
    DO $$ BEGIN RAISE EXCEPTION 'review expected candidate failure'; END $$;
    ROLLBACK;
    SELECT to_regclass('pg_temp.workflow_retention_jobs') IS NULL;
    BEGIN;
    CREATE TEMP TABLE workflow_retention_jobs(company_id uuid NOT NULL,job_id uuid NOT NULL,
      execution_id uuid NOT NULL,PRIMARY KEY(company_id,job_id)) ON COMMIT DROP;
    COMMIT;
    SELECT to_regclass('pg_temp.workflow_retention_jobs') IS NULL;
    '''
    r=subprocess.run([PSQL,URL,'-XqAt','-v','ON_ERROR_STOP=0','-f','-'],input=q,text=True,capture_output=True)
    records.append(dict(same_session_sql=q,exit=r.returncode,stdout=r.stdout,stderr=r.stderr))
    assert r.returncode==0 and r.stdout.splitlines()==['t','t'],(r.stdout,r.stderr)
    assert r.stderr.count('ERROR:')==1 and 'review expected candidate failure' in r.stderr,r.stderr
    status='PASS'
except Exception as e:
    status='FAIL';records.append(dict(exception=repr(e)));raise
finally:
    if owner is not None and owner.poll() is None:owner.kill();owner.communicate()
    sql(f'DROP SCHEMA IF EXISTS {S} CASCADE')
    Path(__file__).with_suffix('.json').write_text(json.dumps(dict(status=locals().get('status','FAIL'),
      scope='reduced count-mismatch rollback and same-session temporary relation lifecycle; no production writer claim',records=records),indent=2)+'\n')
print('PASS: newly committed member causes rollback; error rollback and commit both remove temp relation in the same session.')
