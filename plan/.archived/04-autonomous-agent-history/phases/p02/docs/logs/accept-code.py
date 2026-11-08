from pathlib import Path
import datetime,hashlib,json,os,subprocess,tarfile
root=Path.cwd();logs=root/'plan/04-autonomous-agent-history/phases/p02/docs/logs';input=json.loads((logs/'AH-P02-B1-R2-input.json').read_text())
sha=lambda path:hashlib.sha256(Path(path).read_bytes()).hexdigest()
assert subprocess.check_output(['git','rev-parse','HEAD']).decode().strip()==input['head']
assert subprocess.check_output(['git','branch','--show-current']).decode().strip()==input['branch']
env=os.environ.copy();env['PATH']=input['snapshot_path_adapter']+os.pathsep+env['PATH']
snapshot=subprocess.check_output(['bash',input['skill']+'/scripts/snapshot.sh',*input['exclusions']],env=env).decode();digest=next(line.split(':',1)[1].strip() for line in snapshot.splitlines() if line.startswith('digest:'));assert digest==input['source_digest']
assert sha(input['source_archive'])==input['source_archive_sha256']
assert sha(input['producer_archive'])==input['producer_archive_sha256']
assert sha(input['prior_source_archive'])==input['prior_source_archive_sha256']
assert sha(input['author_scope'])==input['author_scope_sha256']
assert sha(input['correction_diff'])==input['correction_diff_sha256']
assert sha(logs/'AH-P02-D1-r2.md')==input['architecture_decision_sha256']
scope=json.loads(Path(input['author_scope']).read_text())
with tarfile.open(input['source_archive']) as archive:
 assert set(archive.getnames())==set(scope['owned_hashes'])
 for name,want in scope['owned_hashes'].items():
  assert sha(root/name)==want,name
  assert hashlib.sha256(archive.extractfile(name).read()).hexdigest()==want,name
for name,want in input['external_paths'].items():
 p=root/name;assert (sha(p) if p.exists() else None)==want,name
coord=Path(input['coordination_authority']);assert sha(coord)==input['coordination_sha256'];assert coord.read_bytes()==Path(input['coordination_pin']).read_bytes()==(root/'plan/04-autonomous-agent-history/COORDINATION.md').read_bytes()
for name,proof in input['checks'].items():
 assert sha(proof['receipt'])==proof['sha256'],name
 assert sha(proof['log'])==proof['log_sha256'],name
 assert json.loads(Path(proof['receipt']).read_text())['exit_code']==0,name
exit=json.loads((logs/'AH-P02-B1-R2-reviewer-exit.json').read_text());assert exit['HEAD']==input['head'];assert exit['branch']==input['branch'];assert all(v for k,v in exit.items() if k.endswith('_matches'));assert exit['source_hash_errors']==exit['archive_file_errors']==[]
evidence=json.loads(Path(input['targeted_evidence']).read_text());assert evidence['unique_passing_tests']==257 and evidence['history_passing_tests']==58
review=(logs.parent/'REVIEW.md').read_text().split('## AH-P02-B1-R2',1)[1];assert 'PASS' in review and input['source_digest'] in review
record={'at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'phase':'p02','round':'AH-P02-B1-R2','source':digest,'head':input['head'],'review_base':input['review_base'],'reviewer':'/root/channel_reply_review_recovery','verdict':'accepted in phase after independent PASS and root exact verification','findings':{'AH-P02-B1-F1':'FIXED','AH-P02-B1-F2':'FIXED'},'coordination_checked':5,'notices':input['notices'],'targeted_tests_passed':257,'history_tests':58,'checks_pass':list(input['checks']),'scope':input['author_scope'],'owned_count':82,'owned_archive':input['source_archive'],'owned_archive_sha256':input['source_archive_sha256'],'reviewer_runtime':'source and actual receipts inspected; no runtime reruns by B','full_suite':False,'integrated_to_primary':False,'history_pushed':False,'remaining':'p03–p07 required, ordered; source freeze lifted after acceptance and coordination publication'}
(logs/'AH-P02-code-acceptance.json').write_text(json.dumps(record,indent=2)+'\n')
(logs/'AH-P02-root-acceptance-snapshot.txt').write_text(snapshot)
print(json.dumps({'verdict':record['verdict'],'source':digest,'targeted':257,'history':58,'owned':82},indent=2))
