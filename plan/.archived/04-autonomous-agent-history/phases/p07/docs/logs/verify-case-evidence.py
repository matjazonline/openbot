from pathlib import Path
import json,re,hashlib
l=Path(__file__).resolve().parent
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
expected=set(re.findall(r'^(\S+): test$',(l/'p07-discovery-02.txt').read_text(),re.M))
assert expected
old=json.loads((l.parents[2]/'p06/docs/logs/verification-case-inventory-r2.json').read_text())['cases']
assert set(old)<=expected,sorted(set(old)-expected)
latest={};observations=[]
for run in ['p07-final-runtime-01','p07-e2e-completion-01','p07-inheritance-current-01','p07-process-current-01']:
 receipt=l/(run+'.json');d=json.loads(receipt.read_text());raw=Path(d['log']);counts={}
 for name,status in re.findall(r'^test (\S+) \.\.\. (ok|FAILED|ignored[^\n]*)$',raw.read_text(),re.M):
  latest[name]={'status':status,'run':run};counts[status]=counts.get(status,0)+1
 observations.append({'run':run,'exit_code':d['exit_code'],'counts':counts,'receipt_sha256':sha(receipt),'log_sha256':sha(raw)})
assert not expected-set(latest),sorted(expected-set(latest))
assert all(latest[n]['status']=='ok' for n in expected)
assert all(o['exit_code']==0 for o in observations)
families=['agent_history_identity_','agent_history_contract_','agent_history_storage_','agent_history_completion_','agent_history_ordering_','agent_history_recovery_','agent_history_selection_','agent_history_preparation_','agent_history_protocol_','agent_history_callers_','agent_history_children_','agent_history_samples_','agent_history_continuation_','agent_history_channel_','agent_history_views_','agent_history_retention_','agent_history_controls_','agent_history_level_','agent_history_e2e_']
counts={prefix:sum(prefix in name for name in expected) for prefix in families};assert all(counts.values()),counts
result={'status':'PASS','selected':len(expected),'distinct_latest_passes':len(expected),'ignored':0,'required_families':counts,'accepted_p06_names_preserved':len(old),'new_selection_names':len(expected-set(old)),'observations':observations,'reuse_disposition':'All current discovered names have actual current observations, no earlier-phase runtime reuse. Final-runtime01 exercised880 including actual100001-member source-retirement and four new journey parents; completion01 reruns direct changed inherited-listener consumers (all process/e2e parents plus foundation), adds genuine human+CC/reflection guard, and runs the separately selected capacity collector. Only inheritance MAC, bounded joined process-output fixture, and testmodule/new test changed after880: no ordinary model server/Rig/production path or earlier assertions changed; same native schema/SQL/currentreader/render/protocol. Technical ignored child helper is excluded from standalone discovery and genuinely invoked/killed six times by required process parents. No earlier tests moved/deleted; all866 P06 names retained.','cases':{n:latest[n] for n in sorted(expected)}}
(l/'verification-case-inventory.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps({k:v for k,v in result.items() if k!='cases'},indent=2))
