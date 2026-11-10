from pathlib import Path
import os,subprocess,json
p=Path('plan/fix-app/phases/p08/docs/logs')
env=dict(os.environ,CARGO_INCREMENTAL='0',SQLX_OFFLINE='true',DATABASE_URL='postgres://mac02@127.0.0.1:55408/open_bots',TEST_DATABASE_URL='postgres://mac02@127.0.0.1:55408/open_bots_test')
build=['cargo','test','--locked','--offline','--lib','--no-run','--message-format=json']
with (p/'step1-native-build.log').open('w') as f:r=subprocess.run(build,env=env,stdout=f,stderr=subprocess.STDOUT)
assert r.returncode==0
artifacts=[]
for line in (p/'step1-native-build.log').read_text().splitlines():
 try:v=json.loads(line)
 except ValueError:continue
 if v.get('reason')=='compiler-artifact' and v.get('executable') and v.get('target',{}).get('kind')==['lib']:artifacts.append(v['executable'])
assert len(artifacts)==1
exe=artifacts[0]
listed=subprocess.check_output([exe,'--list','--format','terse'],env=env,text=True);(p/'step1-test-inventory.txt').write_text(listed)
names=[s.removesuffix(': test') for s in listed.splitlines() if s.endswith(': test')]
filters=['schema_constraint_messages_require_declared_audience','dashboard','application::transport::','adapters::protocols::email::egress::','adapters::persistence::delivery::','application::services::delivery_worker','every_vocabulary_round_trips_through_its_stored_string','a_reply_only_interface_carries_every_purpose_but_new_conversations','workflow_fairness_','workflow_activation_','workflow_job_association_','workflow_job_projection_','workflow_decision_notice_rejects_non_notification_independently','workflow_action_reconciliation_claim_sql_deferred_greater_attempt_refusal']
counts={f:sum(f in n for n in names) for f in filters}
(p/'step1-native-filters.json').write_text(json.dumps(dict(filters=counts,selected_unique=sum(any(f in n for f in filters) for n in names)),indent=2)+'\n')
assert all(counts.values()),counts
print('All',len(filters),'filters nonzero;',sum(any(f in n for f in filters) for n in names),'unique tests',flush=True)
with (p/'step1-native-focused.log').open('w') as f:r=subprocess.run([exe,*filters],env=env,stdout=f,stderr=subprocess.STDOUT)
(p/'step1-native-result.json').write_text(json.dumps(dict(build_command=build,command=[exe,*filters],exit_code=r.returncode,environment={k:env[k] for k in ('CARGO_INCREMENTAL','SQLX_OFFLINE','DATABASE_URL','TEST_DATABASE_URL')}),indent=2)+'\n')
print('Step1 native exit',r.returncode,flush=True)
raise SystemExit(r.returncode)
