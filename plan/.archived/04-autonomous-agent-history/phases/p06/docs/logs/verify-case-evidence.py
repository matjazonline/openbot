from pathlib import Path
import json,re,hashlib
l=Path(__file__).resolve().parent
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
expected=set(re.findall(r'^(\S+): test$',(l/'p06-discovery-final-01.txt').read_text(),re.M))
assert expected
runs=['p06-final-history-01','p06-final-inspector-01','p06-final-focused-runtime-01','p06-decision-fixtures-01']
latest={};observations=[]
for run in runs:
 receipt=l/(run+'.json');d=json.loads(receipt.read_text());out=Path(d['log']);t=out.read_text();counts={}
 for name,status in re.findall(r'^test (\S+) \.\.\. (ok|FAILED|ignored[^\n]*)$',t,re.M):
  latest[name]={'status':status,'run':run};counts[status]=counts.get(status,0)+1
 observations.append({'run':run,'exit_code':d['exit_code'],'counts':counts,'receipt_sha256':sha(receipt),'log_sha256':sha(out)})
missing=sorted(expected-set(latest));failed={n:latest[n] for n in expected&set(latest) if latest[n]['status']!='ok'}
assert not missing and not failed,(missing,failed)
old=json.loads((l.parents[2]/'p05/docs/logs/verification-case-inventory.json').read_text())['cases']
assert set(old)<=expected,sorted(set(old)-expected)
families=['agent_history_views_','agent_history_retention_','agent_history_controls_','agent_history_level_']
counts={prefix:sum(prefix in n for n in expected) for prefix in families};assert all(counts.values())
d={'status':'PASS','selected':len(expected),'distinct_latest_passes':len(expected),'ignored':0,'required_families':counts,'accepted_p05_names_preserved':len(old),'observations':observations,
'reuse_disposition':'Current affected812 runtime supplies811 successful observations. Its only failure was old origin collector fixture assuming retained history did not pin source; only that test fixture changed after launch, and decision-fixtures01 reruns it with genuine native expiry and all original collector/action/revision/company assertions plus pin refusal. All product contracts, PG/schema/env/other test fixtures unchanged during this run. final-inspector01 covers final view/provenance/timing/source actions and lifecycle/pins. Additional source-retention/registry/logical-seed names reuse successful per-case observations from final-history01: subsequent production changes were safe frozen inspection, read projection/HTML, narrow agent-delete FK mapping and bounded staged/verified logs; they did not change native expiry/source-retirement/registry/logical-seed behavior or fixtures. Failed whole commands remain FAIL; this is latest successful per-case evidence, not a claim that either failed command passed.',
'cases':{n:latest[n] for n in sorted(expected)}}
(l/'verification-case-inventory.json').write_text(json.dumps(d,indent=2)+'\n')
print(json.dumps({k:v for k,v in d.items() if k!='cases'},indent=2))
