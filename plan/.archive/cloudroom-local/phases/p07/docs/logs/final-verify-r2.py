import json, os, pathlib, subprocess, time, uuid
root=pathlib.Path('/Users/mac03/dev/openbot')
logs=root/'plan/cloudroom-local/phases/p07/docs/logs'
env=os.environ.copy(); env.update(CARGO_INCREMENTAL='0', SQLX_OFFLINE='true')
env.pop('ALLOW_MISSING_DATABASE_URL',None)
pg='/opt/homebrew/opt/postgresql@18/bin'
tag=uuid.uuid4().hex[:10]
build='openbot_cl_final_build_'+tag; tests='openbot_cl_final_test_'+tag
for db in (build,tests): subprocess.run([pg+'/createdb','-h','127.0.0.1','-p','56388',db],check=True)
env.update(DATABASE_URL='postgres://mac03@127.0.0.1:56388/'+build,TEST_DATABASE_URL='postgres://mac03@127.0.0.1:56388/'+tests)
(logs/'final-databases-r2.json').write_text(json.dumps({'owner':'root/p07/final-r2','port':56388,'build':build,'test':tests},indent=2)+'\n')
results=[]
def run(name,command,extra=None,timeout=1800):
    current=env.copy(); current.update(extra or {})
    started=time.monotonic()
    with (logs/('final-'+name+'-r2.txt')).open('w') as out:
        try: status=subprocess.run(command,cwd=root,env=current,stdout=out,stderr=subprocess.STDOUT,timeout=timeout).returncode
        except subprocess.TimeoutExpired: status=124; out.write('\nCHECK TIMEOUT\n')
    results.append(dict(name=name,command=command,status='PASS' if status==0 else 'FAIL',exit=status,seconds=round(time.monotonic()-started,2),file='logs/final-'+name+'-r2.txt'))
    (logs/'final-results-r2.json').write_text(json.dumps(results,indent=2)+'\n')
    print(name,results[-1]['status'],results[-1]['seconds'],flush=True)
    return status==0
run('versions',['bash','-c','rustc --version; cargo --version; node --version; npm --version; cargo sqlx --version; /opt/homebrew/opt/postgresql@18/bin/psql --version'])
for name,args in [('plugin-install',['ci']),('plugin-typecheck',['run','typecheck']),('plugin-build',['run','build']),('plugin-tests',['test']),('plugin-guards',['run','test:guards']),('plugin-package',['run','package','--','/tmp/openbot-cloudroom-p07-artifacts-r4'])]: run(name,['npm','--prefix','integrations/cloudroom-plugin',*args])
import tempfile
runtime=pathlib.Path(tempfile.mkdtemp(prefix='openbot-cloudroom-p07-final-package-')).resolve()
run('plugin-extract',['tar','-xzf','/tmp/openbot-cloudroom-p07-artifacts-r4/bb-plugin-openbot-local-0.2.0.tgz','-C',str(runtime),'--strip-components=1'])
run('plugin-runtime-install',['npm','--prefix',str(runtime),'ci','--omit=dev'])
run('format',['cargo','fmt','--all','--','--check'])
run('whitespace',['git','diff','--check'])
run('offline-check',['cargo','check','--locked','--offline','--all-targets'])
run('clippy',['cargo','clippy','--locked','--offline','--all-targets','--','-D','warnings'])
run('migrations',['cargo','sqlx','migrate','run'],{'SQLX_OFFLINE':'false'})
run('query-statistics-probe',[pg+'/psql',env['DATABASE_URL'],'-X','-v','ON_ERROR_STOP=1','-c','SELECT 1 AS ci_pg_stat_statements_probe'])
run('query-statistics',[pg+'/psql',env['DATABASE_URL'],'-X','-v','ON_ERROR_STOP=1','-c',"DO $$ BEGIN ASSERT current_setting('pg_stat_statements.track')='top'; ASSERT current_setting('pg_stat_statements.track_utility')='off'; ASSERT current_setting('log_parameter_max_length')='0'; ASSERT current_setting('log_parameter_max_length_on_error')='0'; ASSERT EXISTS (SELECT 1 FROM pg_stat_statements WHERE dbid=(SELECT oid FROM pg_database WHERE datname=current_database()) AND query LIKE '%ci_pg_stat_statements_probe%'); END $$"])
run('sqlx',['cargo','sqlx','prepare','--check','--','--all-targets'],{'SQLX_OFFLINE':'false'})
compiled=run('compile-tests',['cargo','test','--locked','--offline','--all-targets','--no-run'])
if compiled:
    run('inventory',['cargo','test','--locked','--offline','--all-targets','--','--list'])
    run('suite',['scripts/test-network-isolation.sh','cargo','test','--locked','--offline','--all-targets'])
    run('stack',['scripts/test-network-isolation.sh','scripts/stack-budget.sh','--offline'])
    run('lease',['scripts/test-network-isolation.sh','scripts/lease-budget.sh','--offline'],{'LEASE_BUDGET_RECORD':str(logs/'final-lease-events-r2.jsonl')})
run('css',['npm','run','check:css'])
run('run-counts',['npm','run','test:run-counts'])
run('js-protocols',['node','--test','scripts/tests/sse-navigation.mjs','scripts/tests/agent-runtime.mjs','scripts/tests/workflow-runs.mjs','scripts/tests/workflow-conversations.mjs','scripts/tests/workflow-samples.mjs'])
for name,path in [('deploy','scripts/tests/deploy.sh'),('key-rotation','scripts/tests/credential-key-rotation.sh'),('transport-tests','scripts/tests/transport-boundary.sh'),('transport','scripts/transport-boundary-check.sh')]: run(name,[path])
for name,path in [('sample-tests','scripts/tests/workflow-sample-boundary.py'),('sample','scripts/workflow-sample-boundary.py'),('knowledge-prepare','scripts/tests/knowledge-model-prepare.py')]: run(name,['python3',path])
fixtures=[
('html','HTML_INJECTION_BROWSER_FIXTURES','dump_html_injection_browser_fixtures',['scripts/tests/team-autocomplete-browser.mjs','scripts/tests/html-injection-browser.mjs']),
('knowledge','KNOWLEDGE_BROWSER_FIXTURES','dump_knowledge_browser_fixtures',['scripts/tests/knowledge-browser.mjs']),
('agent-authoring','AGENT_AUTHORING_BROWSER_FIXTURES','dump_autonomous_agent_authoring_browser_fixtures',['scripts/tests/agent-authoring-browser.mjs']),
('local-agents','CLOUDROOM_MANAGEMENT_BROWSER_FIXTURES','dump_cloudroom_management_browser_fixtures',['scripts/tests/local-agents-browser.mjs']),
('workflow-agent','WORKFLOW_AGENT_BROWSER_FIXTURES','dump_autonomous_workflow_agent_picker_browser_fixtures',['scripts/tests/workflow-agent-picker-browser.mjs']),
('session','SESSION_REVOCATION_BROWSER_FIXTURES','dump_session_revocation_browser_fixtures',['scripts/tests/session-revocation-browser.mjs'])]
if compiled:
    for name,key,filter,files in fixtures:
        folder=logs/('final-browser-'+name+'-r2'); folder.mkdir(exist_ok=True)
        values={key:str(folder)}
        if name=='html': values['APP_JS_OUT']=str(folder/'app.js')
        if run(name+'-fixtures',['cargo','test','--locked','--offline','--lib',filter,'--','--ignored'],values): run(name+'-browser',['node','--test',*files],values)
    folder=logs/'final-browser-decision-r2'; folder.mkdir(exist_ok=True)
    values={'WORKFLOW_DECISION_BROWSER_FIXTURES':str(folder)}
    for filter in ['decision_browser_markup_and_closed_draft_recovery','source_message_outside_window_is_exact_and_authorized']: run(filter,['cargo','test','--locked','--offline','--lib',filter],values)
    run('decision-browser',['node','--test','scripts/tests/workflow-decisions.mjs'],values)
results.append(dict(name='production-image-knowledge-memory',status='NOT EXERCISED',reason='Docker unavailable on this macOS workspace; CI retains required image and memory gate'))
results.append(dict(name='headless-linux-native',status='NOT EXERCISED',reason='User explicitly left Linux acceptance unverified; no Linux host/systemd here'))
(logs/'final-results-r2.json').write_text(json.dumps(results,indent=2)+'\n')
print('DONE',flush=True)
