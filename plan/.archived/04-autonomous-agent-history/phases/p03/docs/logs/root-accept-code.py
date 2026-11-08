import os,json,hashlib,subprocess,tarfile,re
from pathlib import Path
from datetime import datetime,timezone
root=Path.cwd(); logs=root/'plan/04-autonomous-agent-history/phases/p03/docs/logs'
i=json.loads((logs/'AH-P03-B1-R1-input.json').read_text())
def sha(p): return hashlib.sha256(Path(p).read_bytes()).hexdigest()
env=os.environ.copy(); env['PATH']=i['snapshot_adapter']+os.pathsep+env['PATH']
s=subprocess.check_output(['bash',i['skill']+'/scripts/snapshot.sh',*i['exclusions']],env=env,text=True)
(logs/'AH-P03-root-code-snapshot.txt').write_text(s)
assert 'digest: '+i['source_digest'] in s
assert subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()==i['head']
assert subprocess.check_output(['git','branch','--show-current'],text=True).strip()==i['branch']
for k in ['decision','scope','owned_archive','producer_archive','coverage']:
 assert sha(i[k])==i[k+'_sha256'],k
for p in [i['coordination_authority'],i['coordination_pin'],root/'plan/04-autonomous-agent-history/COORDINATION.md']:
 assert sha(p)==i['coordination_sha256'],str(p)
scope=json.loads(Path(i['scope']).read_text())
assert len(scope['owned_hashes'])==100
for p,h in {**scope['owned_hashes'],**i['external_paths']}.items():
 assert (sha(root/p) if (root/p).exists() else None)==h,p
with tarfile.open(i['owned_archive']) as tar:
 members={m.name: m for m in tar.getmembers() if m.isfile()}
 for p,h in scope['owned_hashes'].items():
  assert hashlib.sha256(tar.extractfile(members[p]).read()).hexdigest()==h,p
passed=set()
for name,c in i['checks'].items():
 for k in ['receipt','log']: assert sha(c[k])==c[k+'_sha256'],name
 receipt=json.loads(Path(c['receipt']).read_text()); assert receipt['exit_code']==0,name
 if name.endswith('runtime-01'):
  content=Path(c['log']).read_text()
  names=set(re.findall(r'^test (\S+) \.\.\. ok$',content,re.M)); passed|=names
  assert not re.search(r'^test \S+ \.\.\. (FAILED|ignored)',content,re.M)
coverage=json.loads(Path(i['coverage']).read_text())
assert passed==set(coverage['test_names']) and len(passed)==460
assert sum('agent_history_' in n for n in passed)==77
for n in ['AH-P03-B1-R1-reviewer-entry.json','AH-P03-B1-R1-reviewer-exit.json']:
 d=json.loads((logs/n).read_text()); assert all(v for k,v in d.items() if k.endswith('_matches')); assert d['source_hash_errors']==d['archive_errors']==[]
assert 'PASS' in (logs.parent/'REVIEW.md').read_text().split('## AH-P03-B1-R1',1)[1]
out={'acceptance':'AH-P03-code-acceptance','at':datetime.now(timezone.utc).isoformat(),'result':'accepted in phase; not integrated/pushed','review':'AH-P03-B1-R1 PASS','source_digest':i['source_digest'],'head':i['head'],'coordination_revision':10,'coordination_sha256':i['coordination_sha256'],'owned':100,'phase_delta':43,'external':36,'archive_sha256':i['owned_archive_sha256'],'distinct_targeted_passes':460,'history_passes':77,'ignored':0,'checks':'all pinned receipts/log hashes and exit statuses verified; no runtime reruns','records':'complete new B1R1 review/notes/findings/entry/exit read; no actionable findings; prior architecture F1/F2 fixed','exclusions':i['exclusions'],'next':'publish accepted producer C004/r3 and start ordered P04'}
(logs/'AH-P03-code-acceptance.json').write_text(json.dumps(out,indent=2)+'\n'); print(json.dumps(out,indent=2))
