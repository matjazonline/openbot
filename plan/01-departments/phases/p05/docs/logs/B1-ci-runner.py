import json, os, subprocess, time, sys, re
from pathlib import Path
logs=Path(__file__).resolve().parent
root=Path.cwd()
fixture=Path((logs/'B1-fixture-root.txt').read_text().strip())
env=os.environ.copy()
env.update(SQLX_OFFLINE='true', RUST_TEST_THREADS='1', TEST_DATABASE_URL='postgres://mac02@localhost:5432/openbot_dept_p05_protocol', CHROME_PATH='/Applications/Google Chrome.app/Contents/MacOS/Google Chrome')
mode=sys.argv[1] if len(sys.argv)>1 else "all"
results=json.loads((logs/"B1-ci-results.json").read_text()) if mode=="protocols" else []
def run(name, command, updates=None):
    output=logs/('B1-ci-'+name+'.txt')
    effective=env.copy();effective.update(updates or {})
    started=time.monotonic()
    with output.open('w') as f:
        print('Command: '+' '.join(command),file=f,flush=True)
        result=subprocess.run(command,env=effective,stdout=f,stderr=subprocess.STDOUT)
    item=dict(name=name,command=command,status='PASS' if result.returncode==0 else 'FAIL',exit_code=result.returncode,seconds=round(time.monotonic()-started,2),log=output.name)
    if item['status']=='PASS' and command[:2]==['cargo','test'] and '--list' not in command and not re.search(r'test result: ok\. [1-9][0-9]* passed;', output.read_text()):
        item['status']='FAIL';item['reason']='no successful exercised Rust tests'
    results.append(item)
    (logs/'B1-ci-results.json').write_text(json.dumps(results,indent=2)+'\n')
    print(name+': '+item['status'],flush=True)
    return result.returncode==0
if mode != "protocols":
    for name in ['workflow-sample-boundary','integration-boundary']:
        run(name+'-fixtures',['python3','scripts/tests/'+name+'.py'])
        run(name,['python3','scripts/'+name+'.py'])
    run('integration-docs',['python3','scripts/check-integration-docs.py'])
    for family in ['integration_portability_','integration_service_contract_','vault_google_e2e_']:
        ok=run(family+'inventory',['cargo','test','--locked','--offline','--lib',family,'--','--list'])
        log=logs/('B1-ci-'+family+'inventory.txt')
        if ok and not any(line.endswith(': test') for line in log.read_text().splitlines()):
            results[-1]['status']='FAIL';results[-1]['reason']='no registered tests';(logs/'B1-ci-results.json').write_text(json.dumps(results,indent=2)+'\n')
if mode == "static": sys.exit(int(any(x["status"] != "PASS" for x in results)))
protocols=[
('vault', 'VAULT_AUTHORING_BROWSER_FIXTURES', [('vault_authoring_picker_is_owner_scoped_and_only_exposes_approved_metadata',False),('vault_authoring_owner_forms_and_reconnect_states_emit_real_browser_markup',False)], ['vault-authoring-browser']),
('html','HTML_INJECTION_BROWSER_FIXTURES',[('dump_html_injection_browser_fixtures',True)],['team-autocomplete-browser','html-injection-browser']),
('knowledge','KNOWLEDGE_BROWSER_FIXTURES',[('dump_knowledge_browser_fixtures',True)],['knowledge-browser']),
('memory','MEMORY_SETTINGS_BROWSER_FIXTURES',[('dump_memory_settings_browser_fixtures',True)],['memory-settings-browser']),
('agent','AGENT_AUTHORING_BROWSER_FIXTURES',[('dump_autonomous_agent_authoring_browser_fixtures',True)],['agent-authoring-browser']),
('local-agents','CLOUDROOM_MANAGEMENT_BROWSER_FIXTURES',[('dump_cloudroom_management_browser_fixtures',True)],['local-agents-browser']),
('agent-picker','WORKFLOW_AGENT_BROWSER_FIXTURES',[('dump_autonomous_workflow_agent_picker_browser_fixtures',True)],['workflow-agent-picker-browser']),
('session','SESSION_REVOCATION_BROWSER_FIXTURES',[('dump_session_revocation_browser_fixtures',True)],['session-revocation-browser']),
('decisions','WORKFLOW_DECISION_BROWSER_FIXTURES',[('decision_browser_markup_and_closed_draft_recovery',False),('source_message_outside_window_is_exact_and_authorized',False)],['workflow-decisions'])]
for name,key,tests,node_tests in protocols:
    directory=fixture/name;directory.mkdir(exist_ok=True)
    updates={key:str(directory)}
    if name=='html':updates['APP_JS_OUT']=str(directory/'app.js')
    for i,(test,ignored) in enumerate(tests):
        command=['cargo','test','--locked','--offline','--lib',test]
        if ignored:command+=['--','--ignored']
        run(name+'-rust-'+str(i+1),command,updates)
    run(name+'-browser',['node','--test']+['scripts/tests/'+n+'.mjs' for n in node_tests],updates)
print('Summary: '+str(sum(x['status']=='PASS' for x in results))+'/'+str(len(results))+' PASS',flush=True)
sys.exit(int(any(x["status"] != "PASS" for x in results)))
