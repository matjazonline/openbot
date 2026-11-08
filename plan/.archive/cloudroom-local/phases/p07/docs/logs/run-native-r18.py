import os,subprocess,pathlib,json,time
root=pathlib.Path('/Users/mac03/dev/openbot'); logs=root/'plan/cloudroom-local/phases/p07/docs/logs'
env=os.environ.copy();env.update(OPENBOT_CLOUDROOM_NATIVE_SMOKE='1',CLOUDROOM_SOURCE='/tmp/openbot-cloudroom-d8b3cc3',CLOUDROOM_PLUGIN_ARCHIVE='/tmp/openbot-cloudroom-p07-artifacts-r3/bb-plugin-openbot-local-0.2.0.tgz',CLOUDROOM_NATIVE_EVIDENCE=str(logs/'native-r18'),CLOUDROOM_OPENCODE_BIN='/private/var/folders/n2/5y7x75r92zl0ll90154hxdzr0000gn/T/openbot-cloudroom-p07-opencode-_ztiw8qf/node_modules/.bin',DATABASE_URL='postgres://mac03@127.0.0.1:56388/openbot_cloudroom_build',TEST_DATABASE_URL='postgres://mac03@127.0.0.1:56388/openbot_cloudroom_test',RUST_MIN_STACK='16777216')
env.pop('ALLOW_MISSING_DATABASE_URL',None)
results=[]
for provider in ['codex','claude','opencode']:
 started=time.monotonic()
 with (logs/f'native-{provider}-r18.txt').open('w') as out:
  result=subprocess.run(['target/debug/deps/open_bots-50aeda0c9f5dbc3a','cloudroom_e2e_native_'+provider,'--ignored','--nocapture'],cwd=root,env=env,stdout=out,stderr=subprocess.STDOUT,timeout=600)
 results.append(dict(provider=provider,status='PASS' if result.returncode==0 else 'FAIL',seconds=round(time.monotonic()-started,2),file=f'logs/native-{provider}-r18.txt'))
 (logs/'native-results-r18.json').write_text(json.dumps(results,indent=2)+'\n')
 print(provider,results[-1]['status'],results[-1]['seconds'],flush=True)
