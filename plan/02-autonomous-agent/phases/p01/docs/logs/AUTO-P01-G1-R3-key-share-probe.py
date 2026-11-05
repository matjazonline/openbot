from pathlib import Path
import subprocess, uuid, select
PSQL=['/opt/homebrew/bin/psql','-X','-q','-A','-t','-h','127.0.0.1','-p','55601','-U','mac02','-v','ON_ERROR_STOP=1']
name='auto_p01_g1r3_probe_'+uuid.uuid4().hex[:12]
log=[]
def sql(db, text):
    r=subprocess.run(PSQL+['-d',db],input=text,text=True,capture_output=True,timeout=10)
    log.append(f'database={db} exit={r.returncode}\n{r.stdout}{r.stderr}')
    if r.returncode: raise RuntimeError(r.stderr)
    return r.stdout.strip()
left=None
try:
    sql('postgres',f'CREATE DATABASE {name};')
    sql(name,"""
CREATE TABLE grant_key(id integer PRIMARY KEY,revoked_at timestamptz);
CREATE TABLE reviewed_operation(id integer PRIMARY KEY,grant_id integer REFERENCES grant_key(id) ON DELETE CASCADE,revoked_at timestamptz);
CREATE FUNCTION activation_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  IF NEW.revoked_at IS NULL THEN
    PERFORM 1 FROM grant_key WHERE id=NEW.grant_id AND revoked_at IS NULL FOR KEY SHARE;
    IF NOT FOUND THEN RAISE EXCEPTION 'inactive grant' USING ERRCODE='23503'; END IF;
  END IF;
  RETURN NEW;
END $$;
CREATE TRIGGER operation_activation BEFORE INSERT OR UPDATE ON reviewed_operation FOR EACH ROW EXECUTE FUNCTION activation_guard();
CREATE FUNCTION revoke_operation() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  IF OLD.revoked_at IS NULL AND NEW.revoked_at IS NOT NULL THEN
    UPDATE reviewed_operation SET revoked_at=clock_timestamp() WHERE grant_id=NEW.id AND revoked_at IS NULL;
  END IF;
  RETURN NEW;
END $$;
CREATE TRIGGER grant_revocation AFTER UPDATE ON grant_key FOR EACH ROW EXECUTE FUNCTION revoke_operation();
INSERT INTO grant_key VALUES(1,NULL);
""")
    left=subprocess.Popen(PSQL+['-d',name],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,bufsize=1)
    left.stdin.write("BEGIN; SET LOCAL statement_timeout='5s'; INSERT INTO reviewed_operation VALUES(1,1,NULL);\n\\echo activation_inserted_uncommitted\n")
    left.stdin.flush()
    ready,_,_=select.select([left.stdout],[],[],7)
    if not ready: raise RuntimeError('left barrier timeout')
    marker=left.stdout.readline().strip()
    log.append('left barrier: '+marker)
    assert marker=='activation_inserted_uncommitted',marker
    sql(name,"BEGIN; SET LOCAL lock_timeout='2s'; SET LOCAL statement_timeout='5s'; UPDATE grant_key SET revoked_at=clock_timestamp() WHERE id=1; COMMIT;")
    left.stdin.write('COMMIT;\n\\q\n')
    left.stdin.flush()
    out,err=left.communicate(timeout=7)
    log.append(f'left commit exit={left.returncode}\n{out}{err}')
    assert left.returncode==0
    state=sql(name,'SELECT grant_key.revoked_at IS NOT NULL,reviewed_operation.revoked_at IS NULL FROM grant_key JOIN reviewed_operation ON grant_key.id=reviewed_operation.grant_id;')
    assert state=='t|t',state
    log.append('REPRODUCED: both transactions committed; grant revoked=true, operation active=true. FOR KEY SHARE did not serialize status revocation.')
finally:
    if left is not None and left.poll() is None:
        left.kill();left.communicate(timeout=5)
    sql('postgres',f'DROP DATABASE IF EXISTS {name} WITH (FORCE);')
    path=Path('plan/02-autonomous-agent/phases/p01/docs/logs/AUTO-P01-G1-R3-key-share-probe.log')
    path.write_text('\n'.join(log)+'\n')
    print('\n'.join(log))
