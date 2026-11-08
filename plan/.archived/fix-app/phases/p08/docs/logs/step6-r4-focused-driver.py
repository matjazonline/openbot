from pathlib import Path
import os,subprocess,json
p=Path('plan/fix-app/phases/p08/docs/logs')
env=dict(os.environ,CARGO_INCREMENTAL='0',SQLX_OFFLINE='true',DATABASE_URL='postgres://mac02@127.0.0.1:55408/open_bots',TEST_DATABASE_URL='postgres://mac02@127.0.0.1:55408/open_bots_test')
build=['cargo','test','--locked','--offline','--lib','--no-run','--message-format=json']
with (p/'step6-r4-native-build.log').open('w') as f:r=subprocess.run(build,env=env,stdout=f,stderr=subprocess.STDOUT)
assert r.returncode==0
artifacts=[]
for line in (p/'step6-r4-native-build.log').read_text().splitlines():
 try:v=json.loads(line)
 except ValueError:continue
 if v.get('reason')=='compiler-artifact' and v.get('executable') and v.get('target',{}).get('kind')==['lib']:artifacts.append(v['executable'])
assert len(artifacts)==1
exe=artifacts[0]
listed=subprocess.check_output([exe,'--list','--format','terse'],env=env,text=True);(p/'step6-r4-test-inventory.txt').write_text(listed)
names=[s.removesuffix(': test') for s in listed.splitlines() if s.endswith(': test')]
filters=['jsonb_row_context_', 'schema_constraint_', 'adapters::persistence::dashboard::', 'workflow_job_schema_', 'workflow_io_']
counts={f:sum(f in n for n in names) for f in filters}
(p/'step6-r4-native-filters.json').write_text(json.dumps(dict(filters=counts,selected_unique=sum(any(f in n for f in filters) for n in names)),indent=2)+'\n')
assert all(counts.values()),counts
print('All',len(filters),'filters nonzero;',sum(any(f in n for f in filters) for n in names),'unique tests',flush=True)
with (p/'step6-r4-native-focused.log').open('w') as f:r=subprocess.run([exe,*filters],env=env,stdout=f,stderr=subprocess.STDOUT)
(p/'step6-r4-native-result.json').write_text(json.dumps(dict(build_command=build,command=[exe,*filters],exit_code=r.returncode,environment={k:env[k] for k in ('CARGO_INCREMENTAL','SQLX_OFFLINE','DATABASE_URL','TEST_DATABASE_URL')}),indent=2)+'\n')
print('Step4 native exit',r.returncode,flush=True)
raise SystemExit(r.returncode)
