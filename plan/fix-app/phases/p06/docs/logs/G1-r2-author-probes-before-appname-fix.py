import json, os, subprocess, time, uuid
from pathlib import Path

PSQL='/opt/homebrew/opt/postgresql@18/bin/psql'
URL='postgres://mac02@127.0.0.1:55406/open_bots_test'
SCHEMA='p06_author_'+uuid.uuid4().hex
records=[]
processes=[]

def sql(statement):
    p=subprocess.run([PSQL,URL,'-X','-qAt','-v','ON_ERROR_STOP=1','-c',statement],text=True,capture_output=True)
    records.append({'sql':statement,'exit':p.returncode,'stdout':p.stdout,'stderr':p.stderr})
    return p

def checked(statement):
    p=sql(statement)
    assert p.returncode==0,p.stderr
    return p.stdout.strip()

def start(label,statement):
    app=SCHEMA+'_'+label
    p=subprocess.Popen([PSQL,URL,'-X','-qAt','-v','ON_ERROR_STOP=1','-c',statement],text=True,stdout=subprocess.PIPE,stderr=subprocess.PIPE,env={**os.environ,'PGAPPNAME':app})
    processes.append(p)
    return app,p,statement

def observed(app,event_type=None,event=None):
    deadline=time.monotonic()+5
    while time.monotonic()<deadline:
        output=checked(f"SELECT coalesce(wait_event_type,'')||'|'||coalesce(wait_event,'') FROM pg_stat_activity WHERE application_name='{app}'")
        if (not event_type or output.startswith(event_type+'|')) and (not event or output.endswith('|'+event)):
            records.append({'barrier':app,'observed':output})
            return
        time.sleep(.02)
    raise AssertionError('backend never reached required barrier '+app)

def finish(process):
    app,p,statement=process
    out,err=p.communicate(timeout=8)
    records.append({'application':app,'sql':statement,'exit':p.returncode,'stdout':out,'stderr':err})
    assert p.returncode==0,err

try:
    checked(f'''CREATE SCHEMA {SCHEMA};
    CREATE TABLE {SCHEMA}.bundles(company int NOT NULL,digest bytea NOT NULL,bundle bytea NOT NULL,
      PRIMARY KEY(company,digest),CHECK(octet_length(digest)=32 AND digest=sha256(bundle)),
      CHECK(octet_length(bundle) BETWEEN 1 AND 4194304));
    CREATE FUNCTION {SCHEMA}.immutable_bundle() RETURNS trigger LANGUAGE plpgsql AS $$
      BEGIN RAISE EXCEPTION 'immutable bundle payload'; END $$;
    CREATE TRIGGER immutable_bundle BEFORE UPDATE ON {SCHEMA}.bundles FOR EACH ROW EXECUTE FUNCTION {SCHEMA}.immutable_bundle();
    CREATE TABLE {SCHEMA}.runs(id int PRIMARY KEY,company int NOT NULL,digest bytea NOT NULL,
      FOREIGN KEY(company,digest) REFERENCES {SCHEMA}.bundles(company,digest) DEFERRABLE INITIALLY DEFERRED);
    INSERT INTO {SCHEMA}.bundles VALUES(1,sha256('payload'::bytea),'payload'::bytea);''')
    upsert=f"INSERT INTO {SCHEMA}.bundles VALUES(1,sha256('payload'::bytea),'payload'::bytea) ON CONFLICT(company,digest) DO UPDATE SET bundle=EXCLUDED.bundle WHERE false;"
    gc=f'''BEGIN; SET LOCAL lock_timeout='500ms'; DELETE FROM {SCHEMA}.bundles AS blob
      WHERE (blob.company,blob.digest) IN (SELECT company,digest FROM {SCHEMA}.bundles FOR UPDATE SKIP LOCKED LIMIT 1)
      AND NOT EXISTS(SELECT 1 FROM {SCHEMA}.runs AS owner WHERE owner.company=blob.company AND owner.digest=blob.digest)
      RETURNING company; COMMIT;'''
    owner=start('admission_owns_reuse',f"BEGIN; {upsert} SELECT pg_sleep(3); INSERT INTO {SCHEMA}.runs VALUES(1,1,sha256('payload'::bytea)); COMMIT;")
    observed(owner[0],event='PgSleep')
    assert checked(gc)=='', 'GC must skip the protected reused tuple'
    finish(owner)
    assert checked(f'SELECT count(*) FROM {SCHEMA}.runs')=='1'
    checked(f'DELETE FROM {SCHEMA}.runs;')
    collector=start('gc_wins',f"BEGIN; DELETE FROM {SCHEMA}.bundles RETURNING company; SELECT pg_sleep(3); COMMIT;")
    observed(collector[0],event='PgSleep')
    waiter=start('admission_waits',f"BEGIN; SET LOCAL lock_timeout='5s'; {upsert} INSERT INTO {SCHEMA}.runs VALUES(2,1,sha256('payload'::bytea)); COMMIT;")
    observed(waiter[0],event_type='Lock')
    finish(collector);finish(waiter)
    assert checked(f"SELECT count(*) FROM {SCHEMA}.runs AS owner JOIN {SCHEMA}.bundles AS blob USING(company,digest) WHERE blob.bundle='payload'::bytea")=='1'
    checked(f'''CREATE TABLE {SCHEMA}.owners(id int PRIMARY KEY);
      CREATE TABLE {SCHEMA}.executions(id int PRIMARY KEY,owner int REFERENCES {SCHEMA}.owners(id) ON DELETE CASCADE);
      CREATE TABLE {SCHEMA}.jobs(id int PRIMARY KEY,execution int REFERENCES {SCHEMA}.executions(id) DEFERRABLE INITIALLY DEFERRED);
      CREATE TABLE {SCHEMA}.attempts(id int PRIMARY KEY,job int REFERENCES {SCHEMA}.jobs(id) ON DELETE CASCADE);
      CREATE TABLE {SCHEMA}.checkpoints(id int PRIMARY KEY,job int REFERENCES {SCHEMA}.jobs(id) ON DELETE CASCADE);
      CREATE TABLE {SCHEMA}.facts(id int PRIMARY KEY,job int REFERENCES {SCHEMA}.jobs(id) ON DELETE CASCADE,owner int NOT NULL);
      CREATE FUNCTION {SCHEMA}.immutable_fact() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN
        IF TG_OP='DELETE' AND pg_trigger_depth()>1 AND NOT EXISTS(SELECT 1 FROM {SCHEMA}.owners WHERE id=OLD.owner) THEN RETURN OLD; END IF;
        RAISE EXCEPTION 'append-only fact'; END $$;
      CREATE TRIGGER immutable_fact BEFORE DELETE OR UPDATE ON {SCHEMA}.facts FOR EACH ROW EXECUTE FUNCTION {SCHEMA}.immutable_fact();
      CREATE TABLE {SCHEMA}.usage(id int PRIMARY KEY,owner int REFERENCES {SCHEMA}.owners(id) ON DELETE CASCADE,job int REFERENCES {SCHEMA}.jobs(id) ON DELETE CASCADE,amount int);
      CREATE TABLE {SCHEMA}.archive(id int PRIMARY KEY,amount int);
      INSERT INTO {SCHEMA}.owners VALUES(1),(2); INSERT INTO {SCHEMA}.executions VALUES(1,1),(2,2);
      INSERT INTO {SCHEMA}.jobs VALUES(1,1),(2,2); INSERT INTO {SCHEMA}.attempts VALUES(1,1),(2,2);
      INSERT INTO {SCHEMA}.checkpoints VALUES(1,1),(2,2); INSERT INTO {SCHEMA}.facts VALUES(1,1,1),(2,2,2);
      INSERT INTO {SCHEMA}.usage VALUES(1,1,1,50),(2,2,2,80);
      BEGIN;
      CREATE TEMP TABLE retiring_jobs ON COMMIT DROP AS SELECT job.id FROM {SCHEMA}.jobs AS job JOIN {SCHEMA}.executions AS execution ON execution.id=job.execution WHERE execution.owner=1 FOR UPDATE OF job;
      INSERT INTO {SCHEMA}.archive SELECT id,amount FROM {SCHEMA}.usage WHERE owner=1;
      DELETE FROM {SCHEMA}.owners WHERE id=1;
      DELETE FROM {SCHEMA}.jobs AS job USING retiring_jobs AS selected WHERE job.id=selected.id;
      COMMIT;''')
    result=checked(f'''SELECT json_build_object('owners',(SELECT json_agg(id) FROM {SCHEMA}.owners),
      'executions',(SELECT json_agg(id) FROM {SCHEMA}.executions),'jobs',(SELECT json_agg(id) FROM {SCHEMA}.jobs),
      'attempts',(SELECT json_agg(id) FROM {SCHEMA}.attempts),'checkpoints',(SELECT json_agg(id) FROM {SCHEMA}.checkpoints),
      'facts',(SELECT json_agg(id) FROM {SCHEMA}.facts),'live_usage',(SELECT sum(amount) FROM {SCHEMA}.usage),
      'archived_usage',(SELECT sum(amount) FROM {SCHEMA}.archive))''')
    data=json.loads(result)
    assert all(data[name]==[2] for name in ['owners','executions','jobs','attempts','checkpoints','facts'])
    assert data['live_usage']==80 and data['archived_usage']==50
    records.append({'F1':'PASS explicit captured-job deletion AFTER owner/tree cascade preserves fact guards and unrelated jobs','F2':'PASS reused tuple protected; GC-first admission waits then inserts and commits; immutable UPDATE trigger not invoked','limitations':'Isolated relational protocols, not an implemented production retention writer. Real admitted-tree coverage remains required after implementation.'})
finally:
    for p in processes:
        if p.poll() is None:
            p.terminate();p.communicate(timeout=5)
    cleanup=sql(f'DROP SCHEMA IF EXISTS {SCHEMA} CASCADE')
    assert cleanup.returncode==0
    Path('plan/fix-app/phases/p06/docs/logs/G1-r2-author-probes.json').write_text(json.dumps(records,indent=2)+'\n')
print('F1/F2 corrected relational protocols PASS; scratch schema removed; source unchanged')
