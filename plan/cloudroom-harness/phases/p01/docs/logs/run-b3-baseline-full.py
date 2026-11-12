#!/usr/bin/env python3
"""Run only after root's fullgateR3 has ended: serial, untouched full baseline context."""
from pathlib import Path
import hashlib,json,os,subprocess,time
W=Path('/private/tmp/openbot-cloudroom-harness-t1gfnhz1/worktree'); P=W.parent
B=P/'cloudroom-gate-baseline'; L=W/'plan/cloudroom-harness/phases/p01/docs/logs'
binary=P/'target/debug/deps/open_bots-c4f538027eed5c31'
expected='abac7038a08a355156b7680a01d2f79f3de756fd'
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=B,text=True).strip()==expected
assert not subprocess.check_output(['git','status','--porcelain','--untracked-files=normal'],cwd=B,text=True).strip()
old=json.loads((L/'B3-baseline-filter-results.json').read_text())
assert hashlib.sha256(binary.read_bytes()).hexdigest()==old['binarySha256']
# Identical root npm lock, so the baseline gets the same already-installed test dependencies.
assert (B/'package-lock.json').read_bytes()==(W/'package-lock.json').read_bytes()
if not (B/'node_modules').exists(): (B/'node_modules').symlink_to(W/'node_modules',target_is_directory=True)
assert (B/'scripts/test-network-isolation.sh').read_bytes()==(W/'scripts/test-network-isolation.sh').read_bytes()
assert (B/'integrations/cloudroom-plugin/dist/server.js').is_file()
assert (B/'integrations/cloudroom-plugin/build-tests').is_dir()
name='openbot_cloudroom_harness_t1gfnhz1_baseline_full_'+str(os.getpid())
base='postgresql://mac02@127.0.0.1:63451/'
env=dict(os.environ,PATH='/private/tmp/openbot-doop-main-k49qm1op/toolchains/node-v24.21.0-darwin-arm64/bin:'+os.environ['PATH'],DATABASE_URL=base+name,TEST_DATABASE_URL=base+name,SQLX_OFFLINE='true',RUST_MIN_STACK='16777216',CARGO_TARGET_DIR=str(P/'target'))
result=dict(base=expected,binary=str(binary),binarySha256=old['binarySha256'],command=[str(B/'scripts/test-network-isolation.sh'),'cargo','test','--locked','--offline','--lib'],cwd=str(B),fixture=name,environment='owned PG18 port63451, fresh original schema, Node24, baseline-rooted identical network guard and Cargo test environment,16MiB stack/defaultthreads; original plugin production/test outputs built and npm tests passed; no concurrent Cargo/PG',status='RUNNING')
result['fixtureIdentity']='B3-baseline-fixture-identity-R2.json'
result['guardRoot']=str(B)
result['comparisonRound']='R2 prepared baseline; R1-unprepared excluded'
manifest=L/'B3-baseline-full-result.json'; manifest.write_text(json.dumps(result,indent=2)+'\n')
subprocess.run(['psql',base+'postgres','-X','-v','ON_ERROR_STOP=1','-c','CREATE DATABASE "'+name+'"'],check=True,stdout=subprocess.DEVNULL)
try:
 with (L/'B3-baseline-full-migrate.txt').open('w') as f:
  m=subprocess.run(['cargo','sqlx','migrate','run'],cwd=B,env={**env,'SQLX_OFFLINE':'false'},stdout=f,stderr=subprocess.STDOUT)
 result['migrationExit']=m.returncode
 assert m.returncode==0,'baseline full schema migration failed'
 started=time.monotonic()
 with (L/'B3-baseline-full-suite.txt').open('w') as f:
  r=subprocess.run(result['command'],cwd=B,env=env,stdout=f,stderr=subprocess.STDOUT)
 result.update(exit=r.returncode,seconds=round(time.monotonic()-started,2),status='PASS' if r.returncode==0 else 'FAIL',log='B3-baseline-full-suite.txt',logSha256=hashlib.sha256((L/'B3-baseline-full-suite.txt').read_bytes()).hexdigest())
finally:
 subprocess.run(['psql',base+'postgres','-X','-q','-c','DROP DATABASE IF EXISTS "'+name+'" WITH (FORCE)'],check=True,stdout=subprocess.DEVNULL)
 manifest.write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps(result))
