from pathlib import Path
import subprocess, uuid, select
PSQL=['/opt/homebrew/bin/psql','-X','-q','-A','-t','-h','127.0.0.1','-p','55601','-U','mac02','-v','ON_ERROR_STOP=1','-v','VERBOSITY=verbose']
name='auto_p01_g1r4_probe_'+uuid.uuid4().hex[:12]
log=[];held=[]
def sql(db,text,error=None):
    r=subprocess.run(PSQL+['-d',db],input=text,text=True,capture_output=True,timeout=10)
    log.append(f'exit={r.returncode} expected_error={error}\n{r.stdout}{r.stderr}')
    if error:
        assert r.returncode!=0 and error in r.stderr,(r.returncode,r.stderr)
    else:
        assert r.returncode==0,r.stderr
    return r.stdout.strip()
def hold(text):
    p=subprocess.Popen(PSQL+['-d',name],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,bufsize=1);held.append(p)
    p.stdin.write("BEGIN; SET LOCAL statement_timeout='5s'; "+text+"\n\\echo held\n");p.stdin.flush()
    ready,_,_=select.select([p.stdout],[],[],7)
    assert ready,'barrier timeout'
    assert p.stdout.readline().strip()=='held'
    return p
def commit(p):
    p.stdin.write('COMMIT;\n\\q\n');p.stdin.flush()
    out,err=p.communicate(timeout=7)
    log.append(f'held commit exit={p.returncode}\n{out}{err}')
    assert p.returncode==0,err
try:
    sql('postgres',f'CREATE DATABASE {name};')
    sql(name,"""
CREATE TABLE grant_key(id integer PRIMARY KEY,revoked_at timestamptz);
CREATE TABLE reviewed_operation(id integer PRIMARY KEY,grant_id integer REFERENCES grant_key(id) ON DELETE CASCADE,revoked_at timestamptz);
CREATE FUNCTION activation_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF NEW.revoked_at IS NULL THEN
   PERFORM 1 FROM grant_key WHERE id=NEW.grant_id AND revoked_at IS NULL FOR SHARE;
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
INSERT INTO grant_key VALUES(1,NULL),(2,NULL),(3,NULL);
INSERT INTO reviewed_operation VALUES(3,3,clock_timestamp());
""")
    log.append('CASE 1: new operation activation first')
    p=hold('INSERT INTO reviewed_operation VALUES(1,1,NULL);')
    sql(name,"BEGIN; SET LOCAL lock_timeout='150ms'; UPDATE grant_key SET revoked_at=clock_timestamp() WHERE id=1; COMMIT;",'55P03')
    commit(p)
    sql(name,'UPDATE grant_key SET revoked_at=clock_timestamp() WHERE id=1;')
    assert sql(name,'SELECT grant_key.revoked_at IS NOT NULL,reviewed_operation.revoked_at IS NOT NULL FROM grant_key JOIN reviewed_operation ON grant_key.id=reviewed_operation.grant_id WHERE grant_key.id=1;')=='t|t'
    log.append('CASE 2: revocation first')
    p=hold('UPDATE grant_key SET revoked_at=clock_timestamp() WHERE id=2;')
    sql(name,"BEGIN; SET LOCAL lock_timeout='150ms'; INSERT INTO reviewed_operation VALUES(2,2,NULL); COMMIT;",'55P03')
    commit(p)
    sql(name,'INSERT INTO reviewed_operation VALUES(2,2,NULL);','23503')
    log.append('CASE 3: retained-key operation reactivation first')
    p=hold('UPDATE reviewed_operation SET revoked_at=NULL WHERE id=3;')
    sql(name,"BEGIN; SET LOCAL lock_timeout='150ms'; UPDATE grant_key SET revoked_at=clock_timestamp() WHERE id=3; COMMIT;",'55P03')
    commit(p)
    sql(name,'UPDATE grant_key SET revoked_at=clock_timestamp() WHERE id=3;')
    assert sql(name,'SELECT grant_key.revoked_at IS NOT NULL,reviewed_operation.revoked_at IS NOT NULL FROM grant_key JOIN reviewed_operation ON grant_key.id=reviewed_operation.grant_id WHERE grant_key.id=3;')=='t|t'
    assert sql(name,'SELECT count(*) FROM reviewed_operation JOIN grant_key ON grant_key.id=reviewed_operation.grant_id WHERE reviewed_operation.revoked_at IS NULL AND grant_key.revoked_at IS NOT NULL;')=='0'
    log.append('PASS: 3 targeted concurrent ordering/reactivation cases; zero active-operation/revoked-grant rows.')
finally:
    for p in held:
        if p.poll() is None:p.kill();p.communicate(timeout=5)
    sql('postgres',f'DROP DATABASE IF EXISTS {name} WITH (FORCE);')
    Path('plan/02-autonomous-agent/phases/p01/docs/logs/AUTO-P01-G1-R4-share-probe.log').write_text('\n'.join(log)+'\n')
    print('\n'.join(log))
