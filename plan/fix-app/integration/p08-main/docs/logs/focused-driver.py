from pathlib import Path
import subprocess,os,json,re,sys
p=Path('plan/fix-app/integration/p08-main/docs/logs')
env=os.environ.copy();env.update(CARGO_INCREMENTAL='0',SQLX_OFFLINE='true',DATABASE_URL='postgres://mac02@127.0.0.1:55400/open_bots',TEST_DATABASE_URL='postgres://mac02@127.0.0.1:55400/open_bots_test')
filters=['application::use_cases::user::','application::use_cases::channel::','application::use_cases::thread::','application::transport::','adapters::spam_scanner::','adapters::smtp::','adapters::protocols::email::','adapters::http::routes::channel','adapters::http::routes::webhooks::','adapters::http::routes::workflows::','adapters::http::routes::user','adapters::http::routes::ui_profile','adapters::http::session','adapters::persistence::user::','adapters::persistence::auth_attempts::','adapters::persistence::auth_key_rotation_tests::','adapters::persistence::account_identity::','adapters::persistence::channel::','adapters::persistence::thread::','adapters::persistence::integration::','application::workflow::tests','adapters::persistence::workflow::tests::message_tests::','message_ingress','::mcp_tests::','provisioning_runtime','infra::','notification','application::services::delivery_worker','application::services::inbound_event_worker','adapters::http::pages::tests::','stream_tests','onboarding']
filters += ['schema_constraint_','jsonb_row_context_','adapters::persistence::dashboard::','adapters::http::pages::dashboard::','adapters::persistence::delivery::','model_usage','workflow_batch_','workflow_context_history_','workflow_control_competing_retry_preserves_identity_facts_and_remaining_allowance','workflow_agent_capture_refuses_unrunnable_agents','workflow_harness_owner_','workflow_io_','run_retention_complete_tree_jobs_usage_and_idempotent_retry','run_retention_rollback_after_runs_before_jobs_preserves_every_owner','run_retention_archive_guards_refuse_forgery_duplicates_and_mutation','workflow_maintenance_live_expired_and_spent_attempts_debit_once']
env['PATH']='/opt/homebrew/opt/postgresql@18/bin:'+env['PATH']
results=[]
def run(name,cmd,live=False):
 run_env=env.copy()
 if live:run_env.pop('SQLX_OFFLINE')
 with (p/(name+'.log')).open('w') as f:r=subprocess.run(cmd,env=run_env,stdout=f,stderr=subprocess.STDOUT)
 results.append(dict(check=name,command=cmd,exit_code=r.returncode));(p/'final-check-results.json').write_text(json.dumps(results,indent=2));print(name,r.returncode,flush=True)
 if r.returncode:raise SystemExit(r.returncode)
run('main-reset',['bash','scripts/reset-db.sh','--all','--yes'],live=True)
run('main-test-migrate',['cargo','sqlx','migrate','run','--database-url',env['TEST_DATABASE_URL']],live=True)
run('main-resource-identity',['python3',str(p/'verify-main-resources.py')])
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
run('main-lifecycle',['cargo','test','--locked','--offline','--bin','open_bots'])
run('deploy-regression',['bash','scripts/tests/deploy.sh'])
run('final-sqlx-live',['cargo','sqlx','prepare','--','--all-targets'],live=True)
run('final-clippy-offline',['cargo','clippy','--locked','--offline','--all-targets','--','-D','warnings'])
run('final-fmt',['cargo','fmt','--all','--check'])
run('final-whitespace',['git','diff','--check','77edd70862a88e2189587fda50fb2b0b913082b6'])
