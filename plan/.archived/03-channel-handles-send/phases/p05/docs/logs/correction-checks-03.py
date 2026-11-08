import json,subprocess,sys
from pathlib import Path
logs=Path(__file__).resolve().parent
checks=[('upstream-action-tests-02', ['scripts/test-network-isolation.sh', 'cargo', 'test', '--locked', '--offline', '--lib', '::action_tests::', '--', '--nocapture']), ('release-knowledge-tests-02', ['scripts/test-network-isolation.sh', 'cargo', 'test', '--locked', '--offline', '--lib', 'knowledge_run_level_', '--', '--nocapture']), ('upstream-message-agent-tests-01', ['scripts/test-network-isolation.sh', 'cargo', 'test', '--locked', '--offline', '--lib', 'workflow_agent_message_tool_', '--', '--nocapture']), ('upstream-knowledge-tests-01', ['scripts/test-network-isolation.sh', 'cargo', 'test', '--locked', '--offline', '--lib', 'knowledge_e2e_', '--', '--nocapture']), ('release-final-entry-tests-01', ['scripts/test-network-isolation.sh', 'cargo', 'test', '--locked', '--offline', '--lib', 'channel_reply_', '--', '--nocapture']), ('release-native-delivery-tests-01', ['scripts/test-network-isolation.sh', 'cargo', 'test', '--locked', '--offline', '--lib', 'adapters::persistence::delivery::', '--', '--nocapture']), ('clippy-03', ['cargo', 'clippy', '--locked', '--offline', '--all-targets', '--', '-D', 'warnings']), ('sqlx-prepare-04', ['cargo', 'sqlx', 'prepare', '--', '--all-targets']), ('sqlx-check-04', ['cargo', 'sqlx', 'prepare', '--check', '--', '--all-targets']), ('fmt-check-03', ['cargo', 'fmt', '--all', '--', '--check']), ('diff-check-03', ['git', 'diff', '--check']), ('graft-build-03', ['graft', 'build'])]
summary=[]
for name,command in checks:
 result=subprocess.run([sys.executable,str(logs/'run-check.py'),name,*command],capture_output=True,text=True)
 summary.append({'check':name,'exit':result.returncode})
 (logs/'correction-checks-03.json').write_text(json.dumps(summary,indent=2)+'\n')
 print(json.dumps(summary[-1]),flush=True)
 if 'test' in command and result.returncode == 0:
  output=(logs/(name+'.log')).read_text()
  import re
  count=re.search(r'test result: ok\. (\d+) passed;',output)
  if not count or int(count.group(1)) == 0:
   print('ERROR: focused test selection was empty',flush=True);sys.exit(2)
 if result.returncode:
  print(result.stdout[-9000:],flush=True);sys.exit(result.returncode)
