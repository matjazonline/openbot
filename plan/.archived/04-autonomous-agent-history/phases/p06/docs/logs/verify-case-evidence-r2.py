from pathlib import Path
import json,re,hashlib
l=Path(__file__).resolve().parent
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
expected=set(re.findall(r'^(\S+): test$',(l/'p06-discovery-final-02.txt').read_text(),re.M))
assert expected
runs=['p06-final-history-01','p06-final-inspector-01','p06-final-focused-runtime-01','p06-decision-fixtures-01','p06-corrections-02','p06-correction-affected-runtime-01','p06-pressure-recheck-01']
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
prior=json.loads((l/'verification-case-inventory.json').read_text())['cases']
assert set(prior)<=expected,sorted(set(prior)-expected)
assert len(expected-set(prior))==4

families=['agent_history_views_','agent_history_retention_','agent_history_controls_','agent_history_level_']
counts={prefix:sum(prefix in n for n in expected) for prefix in families};assert all(counts.values())
d={'status':'PASS','selected':len(expected),'distinct_latest_passes':len(expected),'ignored':0,'required_families':counts,'accepted_p05_names_preserved':len(old),'prior_r1_names_preserved':len(prior),'new_cases':4,'observations':observations,

'reuse_disposition':'R2 native predicate correction changes only failed-own-job replay pin. Current affected188 cohort supplies186 successes and two truthful pressure-fixture corrections supply the remaining2 (ordinary complete_io127 source jobs; cancelled count-prefix original retry window elapsed while remaining younger than90days). All assertions/qualified names/bounds64/128/2048/1536 preserved. Added4 owner-race cases and original two-real-human-round case PASS in corrections02, also in affected188. Other R1 cases reuse exact successful retained observations: production authorization/read/API/UI/harness/tool rendering/CP fields/admission/source-retirement SQL are unchanged, and the new conjunct is exercised only by history expiry; all native history consumers are rerun in188. The unchanged100001-member source retirement fixture contains no history rows or expiry call, so its retained native PASS is reused rather than rerun. Existing R1 view/browser/run/sample/decision byte-rendering code and fixture builders remain behaviorally unchanged; current native reader cases PASS in188. Full discovery preserves862 R1 names plus4 new. Failed whole commands stay FAIL, including R2 affected186/188 and initial compile-only correction01; no single-command all-current cohort claim.',

'cases':{n:latest[n] for n in sorted(expected)}}
(l/'verification-case-inventory-r2.json').write_text(json.dumps(d,indent=2)+'\n')
print(json.dumps({k:v for k,v in d.items() if k!='cases'},indent=2))
