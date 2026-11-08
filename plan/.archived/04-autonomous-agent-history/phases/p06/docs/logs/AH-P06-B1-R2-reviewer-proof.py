import json,hashlib,subprocess,os,tarfile,pathlib,datetime,sys,difflib
p=pathlib.Path('plan/04-autonomous-agent-history/phases/p06/docs/logs');i=json.loads((p/'AH-P06-B1-R2-input.json').read_text());h=lambda x:hashlib.sha256(pathlib.Path(x).read_bytes()).hexdigest() if pathlib.Path(x).is_file() else None
s=json.load(open(i['scope']));e=os.environ.copy();e['PATH']=str(pathlib.Path(i['snapshot_adapter']).resolve())+':'+e['PATH'];out=subprocess.check_output(['bash',i['skill']+'/scripts/snapshot.sh',*i['exclusions']],env=e,text=True);(p/('AH-P06-B1-R2-reviewer-'+sys.argv[1]+'-snapshot.txt')).write_text(out)
r={'digest':i['source_digest'] in out,'head':subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()==i['head'],'branch':subprocess.check_output(['git','branch','--show-current'],text=True).strip()==i['branch']}
for k in ['decision','scope','owned_archive','producer_archive','evidence','case_inventory','previous_review_input','correction_delta']:r[k]=h(i[k])==i[k+'_sha256']
r['skill']=h(i['skill']+'/SKILL.md')==i['skill_sha256'];r['coordination']=all(h(x)==i['coordination_sha256'] for x in [i['coordination_authority'],i['coordination_pin'],i['exclusions'][0]])
r['owned_mismatch']=[x for x,v in s['owned_hashes'].items() if h(x)!=v];r['external_mismatch']=[x for x,v in i['external_paths'].items() if h(x)!=v]
with tarfile.open(i['owned_archive']) as t:
 d={m.name:hashlib.sha256(t.extractfile(m).read()).hexdigest() for m in t.getmembers() if m.isfile()};r['archive_content']=d=={k:v for k,v in s['owned_hashes'].items() if v is not None}
assert all(v is True or v==[] for v in r.values()),r
r['checked_at']=datetime.datetime.now(datetime.timezone.utc).isoformat();(p/('AH-P06-B1-R2-reviewer-'+sys.argv[1]+'.json')).write_text(json.dumps(r,indent=2)+'\n');print(r)
