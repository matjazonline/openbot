from pathlib import Path
import os, subprocess, json, hashlib, time, re
w=Path.cwd();p=w/'plan/cloudroom-harness/phases/p01/docs/logs';b=w.parent/'cloudroom-gate-baseline'
assert subprocess.check_output(['git','-C',str(b),'rev-parse','HEAD'],text=True).strip()=='abac7038a08a355156b7680a01d2f79f3de756fd'
assert not subprocess.check_output(['git','-C',str(b),'status','--porcelain=v1','--untracked-files=no'],text=True).strip()
match=re.search(r'Executable unittests src/lib.rs \(([^)]+)\)',(p/'B3-baseline-compile-R1.txt').read_text());assert match
binary=Path(match.group(1));digest=hashlib.sha256(binary.read_bytes()).hexdigest()
e=os.environ.copy();e.update(DATABASE_URL='postgresql://mac02@127.0.0.1:63451/openbot_cloudroom_harness_t1gfnhz1_baseline',TEST_DATABASE_URL='postgresql://mac02@127.0.0.1:63451/openbot_cloudroom_harness_t1gfnhz1_baseline',RUST_MIN_STACK='16777216',SQLX_OFFLINE='true');e['PATH']='/private/tmp/openbot-doop-main-k49qm1op/toolchains/node-v24.21.0-darwin-arm64/bin:'+e['PATH']
filters=['rendered_markup_carries_no_inline_javascript','mcp_dropped_call_sends_protocol_cancellation_and_joins_cleanup','jev_runtime_lost_lease_cancels_actual_http_and_prevents_settlement','doop_agent_human_edit_conflict_stops_without_blind_retry_or_false_completion','doop_agent_observe_policy_keeps_error_observation_available_to_the_model','doop_agent_published_editor_approvals_and_worker_restart_reuse_one_canvas']
results=[]
for n,name in enumerate(filters,1):
 for turn in range(1,4):
  log=f'B3-baseline-filter-{n}-R{turn}.txt';start=time.monotonic()
  with(p/log).open('w') as out:r=subprocess.run(['bash','scripts/test-network-isolation.sh',str(binary),name,'--nocapture'],cwd=b,env=e,stdout=out,stderr=subprocess.STDOUT,timeout=240)
  row={'filter':name,'round':turn,'exit':r.returncode,'seconds':round(time.monotonic()-start,2),'log':log};results.append(row)
  p.joinpath('B3-baseline-filter-results.json').write_text(json.dumps({'base':'abac7038a08a355156b7680a01d2f79f3de756fd','binary':str(binary),'binarySha256':digest,'environment':'PG18 owned baseline DB, Node24, 16MiB libtest stack, same network isolation','results':results},indent=2)+'\n');print(json.dumps(row),flush=True)
