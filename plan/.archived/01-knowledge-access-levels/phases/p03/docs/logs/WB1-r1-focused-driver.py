from pathlib import Path
import os,subprocess,json
p=Path('plan/01-knowledge-access-levels/phases/p03/docs/logs')
env=dict(os.environ,CARGO_INCREMENTAL='0',SQLX_OFFLINE='true',DATABASE_URL='postgres://mac02@127.0.0.1:55503/open_bots',TEST_DATABASE_URL='postgres://mac02@127.0.0.1:55503/open_bots_test')
build=['cargo','test','--locked','--offline','--lib','--no-run','--message-format=json']
with (p/'WB1-r1-native-build.log').open('w') as f:result=subprocess.run(build,env=env,stdout=f,stderr=subprocess.STDOUT)
assert result.returncode==0
artifacts=[]
for line in (p/'WB1-r1-native-build.log').read_text().splitlines():
 try:obj=json.loads(line)
 except ValueError:continue
 if obj.get('reason')=='compiler-artifact' and obj.get('executable') and obj.get('target',{}).get('kind')==['lib']:artifacts.append(obj['executable'])
assert len(artifacts)==1
exe=artifacts[0];listing=subprocess.check_output([exe,'--list','--format','terse'],env=env,text=True)
(p/'WB1-r1-test-inventory.txt').write_text(listing)
names=[line.removesuffix(': test') for line in listing.splitlines() if line.endswith(': test')]
filters=['workflow_admit_', 'workflow_context_history_', 'workflow_action_', 'workflow_call_', 'workflow_message_', 'workflow_http_', 'workflow_mcp_', 'provisioning_', 'knowledge_run_level_', 'workflow_agent_', 'model_usage', 'access_level_', 'adapters::persistence::thread::']
counts={prefix:sum(prefix in name for name in names) for prefix in filters};assert all(counts.values()),counts
(p/'WB1-r1-native-filters.json').write_text(json.dumps({'filters':counts,'selected_unique':sum(any(prefix in name for prefix in filters) for name in names)},indent=2)+'\n')
print(counts,flush=True)
with (p/'WB1-r1-native-focused.log').open('w') as f:result=subprocess.run([exe,*filters],env=env,stdout=f,stderr=subprocess.STDOUT)
(p/'WB1-r1-native-result.json').write_text(json.dumps({'build_command':build,'command':[exe,*filters],'exit_code':result.returncode,'environment':{key:env[key] for key in ('CARGO_INCREMENTAL','SQLX_OFFLINE','DATABASE_URL','TEST_DATABASE_URL')}},indent=2)+'\n')
print('baseline native exit',result.returncode,flush=True)
raise SystemExit(result.returncode)
