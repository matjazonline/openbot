from pathlib import Path
import json,re,hashlib
l=Path(__file__).resolve().parent
expected=sorted(re.findall(r"^(\S+): test$",(l/'final-focused-list-02.txt').read_text(),re.M))
assert len(expected)==len(set(expected)) and expected
runs=['final-focused-runtime-02','final-affected-recheck01','knowledge-cc-final02','cc-owner-final01']
latest={};observations=[]
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
for run in runs:
 receipt=l/(run+'.json');data=json.loads(receipt.read_text());out=Path(data['log']);text=out.read_text()
 counts={}
 for name,status in re.findall(r"^test (\S+) \.\.\. (ok|FAILED|ignored[^\n]*)$",text,re.M):
  latest[name]={'status':status,'run':run};counts[status]=counts.get(status,0)+1
 observations.append({'run':run,'exit_code':data['exit_code'],'counts':counts,'receipt_sha256':sha(receipt),'log_sha256':sha(out)})
missing=[name for name in expected if name not in latest]
failed={name:latest[name] for name in expected if name in latest and latest[name]['status']!='ok'}
assert not missing and not failed,(missing,failed)
oldfile=l.parents[2]/'p04/docs/logs/final-focused-list-02.log'
# Accepted P04 inventory is retained as a raw .log (or .txt); preserve all existing discovery names.
candidates=list((l.parents[2]/'p04/docs/logs').glob('final-focused-list-02.*'))
oldtext=next(p.read_text() for p in candidates if p.suffix in ('.log','.txt'))
old=set(re.findall(r"^(\S+): test$",oldtext,re.M))
assert old and not (old-set(expected)),sorted(old-set(expected))
families=['agent_history_callers_','agent_history_children_','agent_history_samples_','agent_history_continuation_','agent_history_channel_','agent_history_level_']
counts={prefix:sum(prefix in n for n in expected) for prefix in families}
assert all(counts.values()),counts
result={'status':'PASS','selected':len(expected),'distinct_latest_passes':len(expected),'ignored':0,'required_families':counts,
 'accepted_p04_names_preserved':len(old),'observations':observations,
 'reuse_disposition':'Unchanged cases reuse runtime02 successful observations. Recheck01 covers every changed finalization/sample fixture caller, including synchronous saved-result and repeated fixture extractions plus permanent delivery case. Sole remaining CC test-only expectation/draining/script declaration corrections are covered by final02. Native producer/history/provider code, owned PG/schema/environment unchanged. Original whole-run failures remain FAIL, never relabelled. No single-command whole-cohort PASS claim.',
 'cases':{name:latest[name] for name in expected}}
(l/'verification-case-inventory.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps({k:v for k,v in result.items() if k!='cases'},indent=2))
