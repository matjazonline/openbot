from pathlib import Path
import json,os,subprocess,time
root=Path.cwd();logs=root/'plan/01-knowledge-access-levels/phases/p02/docs/logs'
env=os.environ.copy();env.update(CARGO_INCREMENTAL='0',SQLX_OFFLINE='true',DATABASE_URL='postgres://mac02@127.0.0.1:55502/open_bots',TEST_DATABASE_URL='postgres://mac02@127.0.0.1:55502/open_bots_test',KNOWLEDGE_BROWSER_FIXTURES=str(root/'target/knowledge-browser'))
command=['cargo','test','--locked','--offline','--all-targets','--no-run','--message-format=json']
with (logs/'author-build-r3.jsonl').open('w') as out,(logs/'author-build-r3.log').open('w') as error:
 subprocess.run(command,env=env,stdout=out,stderr=error,check=True)
artifacts=[json.loads(line) for line in (logs/'author-build-r3.jsonl').read_text().splitlines() if line.startswith('{')]
binaries={record['target']['kind'][0]:record['executable'] for record in artifacts if record.get('reason')=='compiler-artifact' and record.get('executable') and record.get('profile',{}).get('test')}
selectors=['knowledge_','adapters::harness::rig::','application::transport::dependency_tests::','application::services::harness::','access_level_','company_invite','scripted','onboarding_pages_render_each_step','icon_rail','every_workspace_rail_link']
names=[line[:-6] for line in subprocess.check_output([binaries['lib'],'--list'],env=env,text=True).splitlines() if line.endswith(': test')]
counts={selector:sum(selector in name for name in names) for selector in selectors};assert all(counts.values()),counts
selected=[name for name in names if any(selector in name for selector in selectors)]
identity={'commands':[command,[binaries['lib'],*selectors,'--include-ignored'],[binaries['bin']]],'selectors':counts,'selected_names':selected,'selected_count':len(selected),'known_unrelated_exclusion':'workflow_and_transport_depend_on_shared_ports_without_reverse_usecase_imports; Q001/r1 baselineproof; MUSTpassfinal06' ,'fixture':'Private PG18.6/55502/open_bots+open_bots_test; real own_database for competing writers; deterministic model, no weights loaded by native tests.'}
(logs/'author-focused-selection-r3.json').write_text(json.dumps(identity,indent=2)+'\n')
results=[]
for kind,arguments in [('lib',selectors+['--include-ignored','--skip','workflow_and_transport_depend_on_shared_ports_without_reverse_usecase_imports']),('bin',[])]:
 started=time.monotonic()
 with (logs/f'author-focused-r3-{kind}.log').open('w') as output:
  result=subprocess.run([binaries[kind],*arguments],env=env,stdout=output,stderr=subprocess.STDOUT)
 results.append({'kind':kind,'exit_code':result.returncode,'elapsed_seconds':time.monotonic()-started})
(logs/'author-focused-results-r3.json').write_text(json.dumps(results,indent=2)+'\n')
assert all(result['exit_code']==0 for result in results),results
print(json.dumps({'counts':counts,'selected':len(selected),'results':results}))
