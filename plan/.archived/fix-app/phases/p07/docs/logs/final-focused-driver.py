from pathlib import Path
import subprocess,os,json,re,sys
p=Path('plan/fix-app/phases/p07/docs/logs')
env=os.environ.copy();env.update(CARGO_INCREMENTAL='0',SQLX_OFFLINE='true',DATABASE_URL='postgres://mac02@127.0.0.1:55407/open_bots',TEST_DATABASE_URL='postgres://mac02@127.0.0.1:55407/open_bots_test')
filters=['application::use_cases::user::','application::use_cases::channel::','application::use_cases::thread::','application::transport::','adapters::spam_scanner::','adapters::smtp::','adapters::protocols::email::','adapters::http::routes::channel','adapters::http::routes::webhooks::','adapters::http::routes::workflows::','adapters::http::routes::user','adapters::http::routes::ui_profile','adapters::http::session','adapters::persistence::user::','adapters::persistence::auth_attempts::','adapters::persistence::auth_key_rotation_tests::','adapters::persistence::account_identity::','adapters::persistence::channel::','adapters::persistence::thread::','adapters::persistence::integration::','application::workflow::tests','adapters::persistence::workflow::tests::message_tests::','message_ingress','::mcp_tests::','provisioning_runtime','infra::','notification','application::services::delivery_worker','application::services::inbound_event_worker','adapters::http::pages::tests::','stream_tests','onboarding']
results=[]
def run(name,cmd,live=False):
 run_env=env.copy()
 if live:run_env.pop('SQLX_OFFLINE')
 with (p/(name+'.log')).open('w') as f:r=subprocess.run(cmd,env=run_env,stdout=f,stderr=subprocess.STDOUT)
 results.append(dict(check=name,command=cmd,exit_code=r.returncode));(p/'final-check-results.json').write_text(json.dumps(results,indent=2));print(name,r.returncode,flush=True)
 if r.returncode:raise SystemExit(r.returncode)
run('final-lib-build',['cargo','test','--locked','--offline','--lib','--no-run','--message-format=json'])
artifacts=[]
for line in (p/'final-lib-build.log').read_text().splitlines():
 try:item=json.loads(line)
 except ValueError:continue
 if item.get('reason')=='compiler-artifact' and item.get('executable') and item.get('target',{}).get('kind')==['lib']:artifacts.append(item['executable'])
assert len(artifacts)==1,artifacts
exe=artifacts[0]
listed=subprocess.check_output([exe,'--list','--format','terse'],env=env,text=True);(p/'final-test-inventory.txt').write_text(listed)
names=[line.removesuffix(': test') for line in listed.splitlines() if line.endswith(': test')]
counts={f:sum(f in name for name in names) for f in filters};selected=[name for name in names if any(f in name for f in filters)]
(p/'final-filter-counts.json').write_text(json.dumps(dict(filters=counts,selected_unique=len(selected),inventory=len(names)),indent=2))
(p/'final-environment.json').write_text(json.dumps({k:env[k] for k in ('CARGO_INCREMENTAL','SQLX_OFFLINE','DATABASE_URL','TEST_DATABASE_URL')},indent=2))
assert all(counts.values()),counts
print('selected',len(selected),'of',len(names),'all filters nonzero',flush=True)
run('final-affected-union',[exe,*filters])
run('final-sqlx-live',['cargo','sqlx','prepare','--','--all-targets'],live=True)
run('final-clippy-offline',['cargo','clippy','--locked','--offline','--all-targets','--','-D','warnings'])
run('final-fmt',['cargo','fmt','--all','--check'])
run('final-whitespace',['git','diff','--check'])
